//! AIレコメンド（F7）: 日次バッチのあとにルールで候補を絞り、3段階の評価と順位をこちらで決め、
//! Workers AI に各銘柄の理由だけを書かせて保存する。答えのうち壊れた分（理由がない・空など）は捨て、
//! 正しい分が MIN_PICKS に届かなければ保存しない。

use crate::screening::{DataStatus, StockRow};
use crate::PortError;
use domain::dividend_policy::{PolicyGap, PolicyLevel};
use domain::recommend::{
    select_candidates, validate_picks, Candidate, Grade, Pick, RawPick, PICK_COUNT,
};
use domain::Date;

/// 保存した1日分
#[derive(Debug, Clone, PartialEq)]
pub struct Recommendations {
    /// 株価の日付（指標の price_date）
    pub date: String,
    pub model: String,
    pub created_at: i64,
    /// 画面のボタンで作り直した分なら true（Cron で作った分と、列を足す前の分は false）
    pub manual: bool,
    /// 順位順
    pub rows: Vec<RecommendationRow>,
}

/// 保存の元: 21:00 の Cron か、画面のボタン
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    Cron,
    Manual,
}

impl Source {
    pub fn as_str(self) -> &'static str {
        match self {
            Source::Cron => "cron",
            Source::Manual => "manual",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct RecommendationRow {
    pub rank: u32,
    /// 3段階の評価（評価を付ける前に保存した日の分は None）
    pub grade: Option<Grade>,
    pub reason: String,
    /// 当たっている注目テーマの名前（今の照合結果）
    pub themes: Vec<String>,
    pub stock: StockRow,
}

/// AI に理由を書かせる1銘柄: こちらで決めた順位・評価と、その根拠
#[derive(Debug, Clone, PartialEq)]
pub struct Explain {
    pub rank: u32,
    pub grade: Grade,
    pub stock: StockRow,
    /// 会社の配当方針との乖離（方針がないか満たしていれば None）
    pub gap: Option<PolicyGap>,
    /// 有報・半期報告書の配当方針の文（DOE・配当性向の数字を拾った文）
    pub phrases: Vec<String>,
    /// 当たっている注目テーマの名前
    pub themes: Vec<String>,
}

pub trait RecommendRepository {
    /// 直近のバッチ（F8 と同じ）
    async fn latest_run(&self) -> Result<Option<DataStatus>, PortError>;
    /// ルールに合いそうな株式（種類が株式で、バリュー株か「5年連続配当かつ通期黒字」）。最終判定は domain で行う
    async fn candidates(&self) -> Result<Vec<StockRow>, PortError>;
    /// 銘柄ごとの配当方針の文（有報・半期報告書の DOE・配当性向の数字を拾った文）。(コード, 文) の列
    async fn policy_phrases(&self, codes: &[String]) -> Result<Vec<(String, String)>, PortError>;
    /// 注目テーマに当たっている銘柄（コード, テーマ名）の列（全銘柄分。表は小さい）
    async fn themes(&self) -> Result<Vec<(String, String)>, PortError>;
    /// その日の結果がすでにあるか
    async fn exists(&self, date: &str) -> Result<bool, PortError>;
    /// AI を呼ぶ前に全入力を固定保存する。
    async fn begin(&self, attempt: &Attempt<'_>) -> Result<i64, PortError>;
    /// 検証済みの結果を公開する。入力と旧runは変更しない。
    async fn publish(&self, id: i64, response: &str, picks: &[Pick]) -> Result<(), PortError>;
    async fn fail(&self, id: i64, response: Option<&str>, error: &str) -> Result<(), PortError>;
    /// 最新日の結果（銘柄名と最新の指標つき）
    async fn latest(&self) -> Result<Option<Recommendations>, PortError>;
}

pub struct Attempt<'a> {
    pub date: &'a str,
    pub batch_id: &'a str,
    pub model: &'a str,
    pub prompt_version: &'a str,
    pub request: &'a str,
    pub candidates: &'a [StockRow],
    pub themes: &'a [(String, String)],
    pub now: i64,
    pub manual: bool,
}

pub const RULE_VERSION: &str = "graded-dividend-theme-v2";

/// AI（Workers AI）。こちらで選んだ銘柄と根拠を渡して、各銘柄の理由の文章を受け取る
pub trait Recommender {
    fn model(&self) -> &str;
    fn prompt_version(&self) -> &str {
        "explain-grade-v1"
    }
    fn request(&self, picks: &[Explain]) -> String;
    /// 保存済みの入力を使い、解析に失敗しても応答本文を返す。
    async fn explain_recorded(
        &self,
        picks: &[Explain],
        request: &str,
    ) -> Result<(String, Result<Vec<RawPick>, PortError>), PortError>;
}

/// 一覧の1行から、候補の判定に使う値だけを取り出す。`themed` は注目テーマに当たっているか
pub fn candidate_of(r: &StockRow, themed: bool) -> Candidate {
    let gap = r.policy_gap();
    Candidate {
        code: r.code.clone(),
        dividend_yield: r.dividend_yield,
        is_value: r.is_value,
        yield_cheap: r.valuation == "cheap",
        dividend_every_year: r.dividend_every_year,
        no_loss_years: r.no_loss_years,
        no_quarterly_loss: r.no_quarterly_loss,
        profit_uptrend: r.profit_uptrend,
        payout_ratio: r.payout_ratio,
        pbr: r.pbr,
        dividend_cut: r.dividend_cut,
        consecutive_increases: r.consecutive_increases,
        progressive_dividend: r.progressive_dividend,
        policy_upside: gap.as_ref().map(|g| g.upside),
        policy_floor_breach: gap.is_some_and(|g| g.level == PolicyLevel::Floor),
        op_growth_q: r.op_growth_q,
        op_growth_ttm: r.op_growth_ttm,
        profit_one_off: r.profit_one_off,
        themed,
    }
}

/// ルールによる候補の絞り込みと順位づけ（株式だけ）。先頭から PICK_COUNT 件が画面に出る分。
/// `themes` は (コード, テーマ名) の列
pub fn candidates_of(rows: &[StockRow], themes: &[(String, String)]) -> Vec<Candidate> {
    let all: Vec<Candidate> = rows
        .iter()
        .filter(|r| r.kind == "stock")
        .map(|r| candidate_of(r, themes.iter().any(|(code, _)| *code == r.code)))
        .collect();
    select_candidates(&all)
}

/// 1銘柄に当たっているテーマ名
fn themes_of(themes: &[(String, String)], code: &str) -> Vec<String> {
    themes
        .iter()
        .filter(|(c, _)| c == code)
        .map(|(_, name)| name.clone())
        .collect()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// 何もしなかった理由
    Skipped(String),
    Saved {
        date: String,
        picks: usize,
        /// AI の答えのうち捨てた分（ログに出す）
        dropped: Vec<String>,
    },
}

/// その日のバッチが成功していて、まだ結果がなければ生成して保存する（21:00 の Cron）
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
    build_and_save(repo, ai, &date, &run.run_id, now, Source::Cron).await
}

/// 画面のボタン: 直近のバッチが成功していれば（今日でなくてもよい）、その株価の日付の結果を作り直す。
/// その日の行があれば消して入れ替える。回数の上限は設けない（押せるのは管理者だけ）
pub async fn regenerate<R: RecommendRepository, A: Recommender>(
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
    let Some(date) = run.latest_price_date else {
        return Ok(Outcome::Skipped("no prices yet".into()));
    };
    build_and_save(repo, ai, &date, &run.run_id, now, Source::Manual).await
}

/// 候補を絞って順位と評価を決め、AI に理由を書かせて保存する（generate と regenerate の共通部分）
async fn build_and_save<R: RecommendRepository, A: Recommender>(
    repo: &R,
    ai: &A,
    date: &str,
    batch_id: &str,
    now: i64,
    source: Source,
) -> Result<Outcome, PortError> {
    let rows = repo.candidates().await?;
    let themes = repo.themes().await?;
    let mut picks = candidates_of(&rows, &themes);
    if picks.is_empty() {
        return Ok(Outcome::Skipped("no candidates".into()));
    }
    // 比較に使う上位50候補を、ルール順位と当日の指標ごと固定する。
    let snapshots: Vec<StockRow> = picks
        .iter()
        .take(50)
        .filter_map(|c| rows.iter().find(|r| r.code == c.code).cloned())
        .collect();
    picks.truncate(PICK_COUNT);
    let codes: Vec<String> = picks.iter().map(|c| c.code.clone()).collect();
    let phrases = repo.policy_phrases(&codes).await?;
    let explains: Vec<Explain> = picks
        .iter()
        .enumerate()
        .filter_map(|(i, c)| {
            let stock = rows.iter().find(|r| r.code == c.code)?.clone();
            Some(Explain {
                rank: i as u32 + 1,
                grade: c.grade(),
                gap: stock.policy_gap(),
                phrases: phrases
                    .iter()
                    .filter(|(code, _)| *code == c.code)
                    .map(|(_, p)| p.clone())
                    .collect(),
                themes: themes_of(&themes, &c.code),
                stock,
            })
        })
        .collect();
    let request = ai.request(&explains);
    let id = repo
        .begin(&Attempt {
            date,
            batch_id,
            model: ai.model(),
            prompt_version: ai.prompt_version(),
            request: &request,
            candidates: &snapshots,
            themes: &themes,
            now,
            manual: source == Source::Manual,
        })
        .await?;
    let (response, raw) = match ai.explain_recorded(&explains, &request).await {
        Ok(value) => value,
        Err(e) => {
            repo.fail(id, None, &e.0).await?;
            return Err(e);
        }
    };
    let v = match raw
        .and_then(|raw| validate_picks(&raw, &picks).map_err(|e| PortError(format!("ai: {e}"))))
    {
        Ok(value) => value,
        Err(e) => {
            repo.fail(id, Some(&response), &e.0).await?;
            return Err(e);
        }
    };
    if let Err(e) = repo.publish(id, &response, &v.picks).await {
        repo.fail(id, Some(&response), &e.0).await?;
        return Err(e);
    }
    Ok(Outcome::Saved {
        date: date.to_string(),
        picks: v.picks.len(),
        dropped: v.dropped.iter().map(|d| d.to_string()).collect(),
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
        asked_phrases: RefCell<Vec<String>>,
        themes: Vec<(String, String)>,
        saved_how: RefCell<Vec<(Source, bool)>>,
        attempts: RefCell<Vec<(String, String, bool)>>,
        failures: RefCell<Vec<Option<String>>>,
    }

    impl RecommendRepository for Fake {
        async fn latest_run(&self) -> Result<Option<DataStatus>, PortError> {
            Ok(self.run.clone())
        }
        async fn candidates(&self) -> Result<Vec<StockRow>, PortError> {
            Ok(self.rows.clone())
        }
        async fn policy_phrases(
            &self,
            codes: &[String],
        ) -> Result<Vec<(String, String)>, PortError> {
            self.asked_phrases
                .borrow_mut()
                .extend(codes.iter().cloned());
            Ok(vec![(codes[0].clone(), "DOE3.5%を下限とします。".into())])
        }
        async fn themes(&self) -> Result<Vec<(String, String)>, PortError> {
            Ok(self.themes.clone())
        }
        async fn exists(&self, date: &str) -> Result<bool, PortError> {
            Ok(self.existing.iter().any(|d| d == date))
        }
        async fn begin(&self, a: &Attempt<'_>) -> Result<i64, PortError> {
            self.attempts
                .borrow_mut()
                .push((a.date.into(), a.model.into(), a.manual));
            Ok(self.attempts.borrow().len() as i64)
        }
        async fn publish(&self, id: i64, _response: &str, picks: &[Pick]) -> Result<(), PortError> {
            let a = self.attempts.borrow()[id as usize - 1].clone();
            self.saved.borrow_mut().push((a.0, a.1, picks.to_vec()));
            self.saved_how
                .borrow_mut()
                .push((if a.2 { Source::Manual } else { Source::Cron }, a.2));
            Ok(())
        }
        async fn fail(
            &self,
            _id: i64,
            response: Option<&str>,
            _error: &str,
        ) -> Result<(), PortError> {
            self.failures.borrow_mut().push(response.map(str::to_owned));
            Ok(())
        }
        async fn latest(&self) -> Result<Option<Recommendations>, PortError> {
            Ok(None)
        }
    }

    /// 渡された銘柄すべてに理由を返す偽の AI。渡されたものを覚えておく
    struct EchoAi {
        broken: bool,
        seen: RefCell<Vec<Explain>>,
    }

    fn echo() -> EchoAi {
        EchoAi {
            broken: false,
            seen: RefCell::new(vec![]),
        }
    }

    /// 決まった答えを返す偽の AI
    struct FixedAi(Vec<RawPick>);

    impl Recommender for FixedAi {
        fn model(&self) -> &str {
            "fake-model"
        }
        fn request(&self, _picks: &[Explain]) -> String {
            "fixed-input".into()
        }
        async fn explain_recorded(
            &self,
            _picks: &[Explain],
            request: &str,
        ) -> Result<(String, Result<Vec<RawPick>, PortError>), PortError> {
            assert_eq!(request, "fixed-input");
            Ok(("fixed-response".into(), Ok(self.0.clone())))
        }
    }

    impl Recommender for EchoAi {
        fn model(&self) -> &str {
            "fake-model"
        }
        fn request(&self, picks: &[Explain]) -> String {
            picks
                .iter()
                .map(|p| p.stock.code.as_str())
                .collect::<Vec<_>>()
                .join(",")
        }
        async fn explain_recorded(
            &self,
            picks: &[Explain],
            request: &str,
        ) -> Result<(String, Result<Vec<RawPick>, PortError>), PortError> {
            assert_eq!(request, self.request(picks));
            if self.broken {
                return Ok((
                    "broken-response".into(),
                    Err(PortError("ai: bad json".into())),
                ));
            }
            self.seen.borrow_mut().extend(picks.iter().cloned());
            Ok((
                "echo-response".into(),
                Ok(picks
                    .iter()
                    .map(|p| RawPick {
                        code: p.stock.code.clone(),
                        reason: format!("{} は割安", p.stock.code),
                    })
                    .collect()),
            ))
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
            dividend: None,
            dividend_basis: None,
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
            dividend_cut: None,
            progressive_dividend: None,
            no_dividend_cut: None,
            doe_floor: None,
            doe_goal: None,
            doe_forecast: None,
            payout_floor: None,
            payout_goal: None,
            payout_forecast: None,
            op_growth_q: None,
            net_growth_q: None,
            op_growth_ttm: None,
            profit_one_off: None,
            reit: None,
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
            asked_phrases: RefCell::new(vec![]),
            themes: vec![],
            saved_how: RefCell::new(vec![]),
            attempts: RefCell::new(vec![]),
            failures: RefCell::new(vec![]),
        }
    }

    #[test]
    fn saves_ten_picks_after_a_successful_batch() {
        let mut repo = fake(run("succeeded", NOW - 3600), 30);
        // 5003 だけ方針の下限を割っていて業績も良い → 強く推奨で1位
        let strong = &mut repo.rows[3];
        strong.doe = Some(0.028);
        strong.doe_floor = Some(0.035);
        strong.op_growth_q = Some(0.12);
        strong.op_growth_ttm = Some(0.08);
        strong.profit_one_off = Some(false);
        let ai = echo();
        let out = block_on(generate(&repo, &ai, NOW)).unwrap();
        assert_eq!(
            out,
            Outcome::Saved {
                date: "2026-10-21".into(),
                picks: 10,
                dropped: vec![],
            }
        );
        let saved = repo.saved.borrow();
        assert_eq!(saved.len(), 1);
        assert_eq!(saved[0].1, "fake-model");
        assert_eq!(saved[0].2[0].rank, 1);
        assert_eq!(saved[0].2[0].code, "5003", "評価の高い銘柄が先");
        assert_eq!(repo.saved_how.borrow()[0], (Source::Cron, false));
        assert_eq!(saved[0].2[0].grade, Grade::Strong);
        assert_eq!(saved[0].2[1].grade, Grade::Watch);

        let seen = ai.seen.borrow();
        assert_eq!(seen.len(), 10, "画面に出す10件だけ AI に渡す");
        assert_eq!(seen[0].rank, 1);
        assert_eq!(seen[0].gap.as_ref().map(|g| g.key()), Some("doe_floor"));
        assert_eq!(seen[0].phrases, vec!["DOE3.5%を下限とします。".to_string()]);
        assert!(seen[1].phrases.is_empty());
        assert!(seen[0].themes.is_empty());
        assert_eq!(
            repo.asked_phrases.borrow().len(),
            10,
            "方針の文は選んだ銘柄の分だけ読む"
        );
    }

    #[test]
    fn skips_when_batch_failed_or_old_or_done() {
        let skipped = |repo: &Fake| {
            matches!(
                block_on(generate(repo, &echo(), NOW)).unwrap(),
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
        let broken = EchoAi {
            broken: true,
            seen: RefCell::new(vec![]),
        };
        assert!(block_on(generate(&repo, &broken, NOW)).is_err());
        assert!(repo.saved.borrow().is_empty());

        // 候補が MIN_PICKS 未満なら AI の答えも足りず、保存しない
        let few = fake(run("succeeded", NOW - 3600), 4);
        assert!(block_on(generate(&few, &echo(), NOW)).is_err());
        assert!(few.saved.borrow().is_empty());
    }

    #[test]
    fn saves_the_valid_picks_when_some_are_broken() {
        let repo = fake(run("succeeded", NOW - 3600), 30);
        let pick = |code: &str| RawPick {
            code: code.into(),
            reason: format!("{code} は割安"),
        };
        // 同じ評価なら条件を多く満たす順（偶数コードがバリュー）→ コード順なので、5000, 5002, …, 5018
        let mut answer: Vec<RawPick> = (0..10)
            .map(|i| pick(&format!("{}", 5000 + 2 * i)))
            .collect();
        answer[4] = pick("5000"); // 5008 の理由がなく、5000 が2回（2回目は無視）
        answer[7] = pick("9999"); // 5014 の理由がなく、候補にない銘柄の理由（無視）
        let out = block_on(generate(&repo, &FixedAi(answer), NOW)).unwrap();
        assert_eq!(
            out,
            Outcome::Saved {
                date: "2026-10-21".into(),
                picks: 8,
                dropped: vec!["no reason for 5008".into(), "no reason for 5014".into()],
            }
        );
        let saved = repo.saved.borrow();
        assert_eq!(saved[0].2.len(), 8);
        assert_eq!(saved[0].2[7].rank, 8);
        assert_eq!(saved[0].2[7].code, "5018");
    }

    #[test]
    fn regenerate_replaces_the_day() {
        // 前日に終わったバッチでも、成功していれば作り直せる。その日の行は入れ替える
        let mut repo = fake(run("succeeded", NOW - 86_400), 30);
        repo.existing.push("2026-10-21".into());
        let out = block_on(regenerate(&repo, &echo(), NOW)).unwrap();
        assert!(matches!(out, Outcome::Saved { picks: 10, .. }), "{out:?}");
        assert_eq!(repo.saved_how.borrow()[0], (Source::Manual, true));

        // 何度でも作り直せる（押せるのは管理者だけ）
        assert!(matches!(
            block_on(regenerate(&repo, &echo(), NOW + 60)).unwrap(),
            Outcome::Saved { .. }
        ));
        assert_eq!(repo.saved.borrow().len(), 2);

        // バッチが走っている・失敗していれば作れない
        assert!(matches!(
            block_on(regenerate(
                &fake(run("running", NOW - 600), 30),
                &echo(),
                NOW
            ))
            .unwrap(),
            Outcome::Skipped(_)
        ));
    }

    #[test]
    fn candidates_ignore_non_stocks() {
        let mut etf = row("1306", 0.02, true);
        etf.kind = "etf".into();
        let c = candidates_of(&[etf, row("7203", 0.03, true)], &[]);
        assert_eq!(c.len(), 1);
        assert_eq!(c[0].code, "7203");
    }

    #[test]
    fn themed_stocks_come_first_and_reach_the_ai() {
        let mut repo = fake(run("succeeded", NOW - 3600), 30);
        // 5017 は奇数コード（バリューでない）で本来は後ろのほうだが、テーマに当たっているので同じ評価の先頭
        repo.themes = vec![
            ("5017".into(), "半導体".into()),
            ("5017".into(), "データセンター".into()),
        ];
        let ai = echo();
        block_on(generate(&repo, &ai, NOW)).unwrap();
        let seen = ai.seen.borrow();
        assert_eq!(seen[0].stock.code, "5017");
        assert_eq!(
            seen[0].themes,
            vec!["半導体".to_string(), "データセンター".to_string()]
        );
        assert!(seen[1].themes.is_empty());
    }
    #[test]
    fn invalid_answer_is_retained_without_publishing() {
        let repo = fake(run("succeeded", NOW - 3600), 30);
        let mut ai = echo();
        ai.broken = true;
        assert!(block_on(generate(&repo, &ai, NOW)).is_err());
        assert_eq!(repo.attempts.borrow().len(), 1);
        assert_eq!(&*repo.failures.borrow(), &[Some("broken-response".into())]);
        assert!(repo.saved.borrow().is_empty());
    }
}
