//! AIレコメンド（F7）: 日次バッチのあとにルールで候補を絞り、Workers AI に上位10銘柄と理由を出させて保存する。
//! AI は説明と優先順位付けだけに使い、答えが壊れていれば保存しない。

use crate::screening::{DataStatus, StockRow};
use crate::PortError;
use domain::recommend::{select_candidates, validate_picks, Candidate, Pick, RawPick};
use domain::Date;

/// 保存した1日分
#[derive(Debug, Clone, PartialEq)]
pub struct Recommendations {
    /// 株価の日付（指標の price_date）
    pub date: String,
    pub model: String,
    pub created_at: i64,
    /// 順位順
    pub rows: Vec<RecommendationRow>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RecommendationRow {
    pub rank: u32,
    pub reason: String,
    pub stock: StockRow,
}

pub trait RecommendRepository {
    /// 直近のバッチ（F8 と同じ）
    async fn latest_run(&self) -> Result<Option<DataStatus>, PortError>;
    /// ルールに合いそうな株式（種類が株式で、バリュー株か「5年連続配当かつ通期黒字」）。最終判定は domain で行う
    async fn candidates(&self) -> Result<Vec<StockRow>, PortError>;
    /// その日の結果がすでにあるか
    async fn exists(&self, date: &str) -> Result<bool, PortError>;
    /// 1日分をまとめて保存する（PICK_COUNT 行）
    async fn save(
        &self,
        date: &str,
        model: &str,
        picks: &[Pick],
        now: i64,
    ) -> Result<(), PortError>;
    /// 最新日の結果（銘柄名と最新の指標つき）
    async fn latest(&self) -> Result<Option<Recommendations>, PortError>;
}

/// AI（Workers AI）。候補の指標を渡して、上位の銘柄コードと理由を受け取る
pub trait Recommender {
    fn model(&self) -> &str;
    /// 答えの JSON を解釈できなければ Err。中身の検証（候補にあるか等）は呼び出し側で行う
    async fn rank(&self, candidates: &[StockRow]) -> Result<Vec<RawPick>, PortError>;
}

/// AI に渡す前の、ルールによる候補の絞り込み
pub fn candidates_of(rows: &[StockRow]) -> Vec<Candidate> {
    let all: Vec<Candidate> = rows
        .iter()
        .filter(|r| r.kind == "stock")
        .map(|r| Candidate {
            code: r.code.clone(),
            dividend_yield: r.dividend_yield,
            is_value: r.is_value,
            dividend_every_year: r.dividend_every_year,
            no_loss_years: r.no_loss_years,
            no_quarterly_loss: r.no_quarterly_loss,
            profit_uptrend: r.profit_uptrend,
        })
        .collect();
    select_candidates(&all)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// 何もしなかった理由
    Skipped(String),
    Saved {
        date: String,
        picks: usize,
    },
}

/// その日のバッチが成功していて、まだ結果がなければ生成して保存する
pub async fn generate<R: RecommendRepository, A: Recommender>(
    repo: &R,
    ai: &A,
    now: i64,
) -> Result<Outcome, PortError> {
    let Some(run) = repo.latest_run().await? else {
        return Ok(Outcome::Skipped("no batch run yet".into()));
    };
    if run.status != "succeeded" {
        return Ok(Outcome::Skipped(format!(
            "latest batch {} is {}",
            run.run_id, run.status
        )));
    }
    let today = Date::from_unix_jst(now);
    if run.finished_at.map(Date::from_unix_jst) != Some(today) {
        return Ok(Outcome::Skipped(format!(
            "latest batch {} did not finish today",
            run.run_id
        )));
    }
    let Some(date) = run.latest_price_date else {
        return Ok(Outcome::Skipped("no prices yet".into()));
    };
    if repo.exists(&date).await? {
        return Ok(Outcome::Skipped(format!("already generated for {date}")));
    }
    let rows = repo.candidates().await?;
    let candidates = candidates_of(&rows);
    if candidates.is_empty() {
        return Ok(Outcome::Skipped("no candidates".into()));
    }
    let shortlist: Vec<StockRow> = candidates
        .iter()
        .filter_map(|c| rows.iter().find(|r| r.code == c.code).cloned())
        .collect();
    let raw = ai.rank(&shortlist).await?;
    let picks = validate_picks(&raw, &candidates).map_err(|e| PortError(format!("ai: {e}")))?;
    repo.save(&date, ai.model(), &picks, now).await?;
    Ok(Outcome::Saved {
        date,
        picks: picks.len(),
    })
}

pub async fn latest<R: RecommendRepository>(
    repo: &R,
) -> Result<Option<Recommendations>, PortError> {
    repo.latest().await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests_support::block_on;
    use std::cell::RefCell;

    struct Fake {
        run: Option<DataStatus>,
        rows: Vec<StockRow>,
        existing: Vec<String>,
        saved: RefCell<Vec<(String, String, Vec<Pick>)>>,
    }

    impl RecommendRepository for Fake {
        async fn latest_run(&self) -> Result<Option<DataStatus>, PortError> {
            Ok(self.run.clone())
        }
        async fn candidates(&self) -> Result<Vec<StockRow>, PortError> {
            Ok(self.rows.clone())
        }
        async fn exists(&self, date: &str) -> Result<bool, PortError> {
            Ok(self.existing.iter().any(|d| d == date))
        }
        async fn save(
            &self,
            date: &str,
            model: &str,
            picks: &[Pick],
            _now: i64,
        ) -> Result<(), PortError> {
            self.saved
                .borrow_mut()
                .push((date.into(), model.into(), picks.to_vec()));
            Ok(())
        }
        async fn latest(&self) -> Result<Option<Recommendations>, PortError> {
            Ok(None)
        }
    }

    /// 渡された候補をそのまま上から返す偽の AI
    struct EchoAi {
        broken: bool,
    }

    impl Recommender for EchoAi {
        fn model(&self) -> &str {
            "fake-model"
        }
        async fn rank(&self, candidates: &[StockRow]) -> Result<Vec<RawPick>, PortError> {
            if self.broken {
                return Err(PortError("ai: bad json".into()));
            }
            Ok(candidates
                .iter()
                .take(10)
                .map(|r| RawPick {
                    code: r.code.clone(),
                    reason: format!("{} は割安", r.code),
                })
                .collect())
        }
    }

    // 2026-10-21 12:00 UTC（21:00 JST）
    const NOW: i64 = 1_792_584_000;

    fn run(status: &str, finished_at: i64) -> Option<DataStatus> {
        Some(DataStatus {
            run_id: "r1".into(),
            started_at: finished_at - 600,
            finished_at: Some(finished_at),
            status: status.into(),
            latest_price_date: Some("2026-10-21".into()),
            latest_fin_date: None,
        })
    }

    fn row(code: &str, y: f64, value: bool) -> StockRow {
        StockRow {
            code: code.into(),
            price: 1000.0,
            price_date: "2026-10-21".into(),
            dividend_yield: Some(y),
            yield_percentile: None,
            pbr: None,
            per: None,
            payout_ratio: None,
            doe: None,
            consecutive_increases: None,
            operating_margin: None,
            no_loss_years: Some(true),
            profit_uptrend: None,
            valuation: "cheap".into(),
            fetched_at: 0,
            name: None,
            kind: "stock".into(),
            market: None,
            sector33: None,
            size_class: None,
            dividend_every_year: Some(true),
            no_quarterly_loss: None,
            is_value: Some(value),
            data_years: Some(5),
            sales_growth: None,
            op_growth: None,
            progressive_dividend: None,
            no_dividend_cut: None,
        }
    }

    fn fake(run: Option<DataStatus>, n: usize) -> Fake {
        Fake {
            run,
            rows: (0..n)
                .map(|i| row(&format!("{}", 5000 + i), 0.03, i % 2 == 0))
                .collect(),
            existing: vec![],
            saved: RefCell::new(vec![]),
        }
    }

    #[test]
    fn saves_ten_picks_after_a_successful_batch() {
        let repo = fake(run("succeeded", NOW - 3600), 30);
        let out = block_on(generate(&repo, &EchoAi { broken: false }, NOW)).unwrap();
        assert_eq!(
            out,
            Outcome::Saved {
                date: "2026-10-21".into(),
                picks: 10
            }
        );
        let saved = repo.saved.borrow();
        assert_eq!(saved.len(), 1);
        assert_eq!(saved[0].1, "fake-model");
        assert_eq!(saved[0].2[0].rank, 1);
        assert_eq!(saved[0].2[0].code, "5000", "条件を多く満たす銘柄が先");
    }

    #[test]
    fn skips_when_batch_failed_or_old_or_done() {
        let skipped = |repo: &Fake| {
            matches!(
                block_on(generate(repo, &EchoAi { broken: false }, NOW)).unwrap(),
                Outcome::Skipped(_)
            )
        };
        assert!(skipped(&fake(None, 30)));
        assert!(skipped(&fake(run("failed", NOW - 3600), 30)));
        assert!(skipped(&fake(run("partial", NOW - 3600), 30)));
        assert!(
            skipped(&fake(run("succeeded", NOW - 86_400), 30)),
            "前日のバッチ"
        );
        let mut done = fake(run("succeeded", NOW - 3600), 30);
        done.existing.push("2026-10-21".into());
        assert!(skipped(&done), "同じ日の結果がある");
        assert!(skipped(&fake(run("succeeded", NOW - 3600), 0)), "候補なし");
    }

    #[test]
    fn broken_ai_answer_is_not_saved() {
        let repo = fake(run("succeeded", NOW - 3600), 30);
        assert!(block_on(generate(&repo, &EchoAi { broken: true }, NOW)).is_err());
        assert!(repo.saved.borrow().is_empty());

        // 候補が10未満なら AI の答えも10件に足りず、保存しない
        let few = fake(run("succeeded", NOW - 3600), 5);
        assert!(block_on(generate(&few, &EchoAi { broken: false }, NOW)).is_err());
        assert!(few.saved.borrow().is_empty());
    }

    #[test]
    fn candidates_ignore_non_stocks() {
        let mut etf = row("1306", 0.02, true);
        etf.kind = "etf".into();
        let c = candidates_of(&[etf, row("7203", 0.03, true)]);
        assert_eq!(c.len(), 1);
        assert_eq!(c[0].code, "7203");
    }
}
