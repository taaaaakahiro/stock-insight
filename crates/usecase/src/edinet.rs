//! EDINET の取り込み計画: 書類一覧から、銘柄・種類ごとに最新の1通だけを選び、
//! まだ持っていないものだけを取る。新しい有報で古くなった半期報告書は取らずに消す。

use domain::disclosure::{semiannual_superseded, DividendSignals, DocKind};
use domain::Date;
use std::collections::{HashMap, HashSet};

/// 書類一覧の1件（または保存済みの1件）
#[derive(Debug, Clone, PartialEq)]
pub struct DocRef {
    /// 4桁の銘柄コード（StockCode の文字列）
    pub code: String,
    pub kind: DocKind,
    pub doc_id: String,
    pub submitted: Date,
    pub period_end: Option<Date>,
}

pub type DocKey = (String, DocKind);

/// 取り込んだ書類と、その文章から拾った配当方針の手がかり
#[derive(Debug, Clone, PartialEq)]
pub struct EdinetDoc {
    pub doc: DocRef,
    pub signals: DividendSignals,
}

/// 取り込みの計画
#[derive(Debug, Default, PartialEq)]
pub struct Plan {
    /// 取る書類（提出日の古い順。途中で止まっても、続きの日付から再開できるように）
    pub fetch: Vec<DocRef>,
    /// 消す保存済みの書類（新しい有報で古くなった半期報告書、上場していない銘柄）
    pub delete: Vec<DocKey>,
}

/// listed: 書類一覧で見つかった書類。stored: 保存済みの書類。current: いま上場している銘柄
pub fn plan(
    listed: Vec<DocRef>,
    stored: &HashMap<DocKey, DocRef>,
    current: &HashSet<String>,
) -> Plan {
    // 銘柄・種類ごとに、保存済みより新しい最新の1通
    let mut newest: HashMap<DocKey, DocRef> = HashMap::new();
    for d in listed {
        if !current.contains(&d.code) {
            continue;
        }
        let key = (d.code.clone(), d.kind);
        let known = stored.get(&key).into_iter().chain(newest.get(&key));
        if known.into_iter().any(|k| k.submitted >= d.submitted) {
            continue;
        }
        newest.insert(key, d);
    }
    // 取ったあとの状態（保存済み + 新しく取るもの）で、古くなった半期報告書を探す
    let after = |code: &str, kind| {
        let key = (code.to_string(), kind);
        newest.get(&key).or_else(|| stored.get(&key))
    };
    let superseded = |code: &str| -> bool {
        let semi = after(code, DocKind::Semiannual).and_then(|d| d.period_end);
        let annual = after(code, DocKind::Annual).and_then(|d| d.period_end);
        matches!((semi, annual), (Some(s), Some(a)) if semiannual_superseded(s, a))
    };
    let stale: HashSet<String> = newest
        .keys()
        .chain(stored.keys())
        .map(|(c, _)| c.clone())
        .filter(|c| superseded(c))
        .collect();

    let mut fetch: Vec<DocRef> = newest
        .into_values()
        .filter(|d| !(d.kind == DocKind::Semiannual && stale.contains(&d.code)))
        .collect();
    fetch.sort_by(|a, b| (a.submitted, &a.doc_id).cmp(&(b.submitted, &b.doc_id)));

    let mut delete: Vec<DocKey> = stored
        .keys()
        .filter(|(code, kind)| {
            !current.contains(code) || (*kind == DocKind::Semiannual && stale.contains(code))
        })
        .cloned()
        .collect();
    delete.sort();
    Plan { fetch, delete }
}

/// 次回どの日付の次から一覧を読み直すか。
/// 最後まで読んだ日、まだ取っていない最初の書類の提出日の前日、今日の前日のうち最も早い日。
/// 今日の一覧はバッチの後にも書類が増えるので、翌日もう一度読む（取得済みの書類は plan が飛ばす）
pub fn resume_cursor(listed_until: Date, first_unfetched: Option<&DocRef>, today: Date) -> Date {
    let c = listed_until.min(today.add_days(-1));
    match first_unfetched {
        Some(d) => c.min(d.submitted.add_days(-1)),
        None => c,
    }
}

/// R2 に置く場所（銘柄・種類ごとに1つ。新しい書類で上書きする）
pub fn r2_key(code: &str, kind: DocKind) -> String {
    format!("edinet/{code}/{}.json.gz", kind.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(s: &str) -> Date {
        Date::parse(s).unwrap()
    }

    fn doc(code: &str, kind: DocKind, id: &str, submitted: &str, period_end: &str) -> DocRef {
        DocRef {
            code: code.into(),
            kind,
            doc_id: id.into(),
            submitted: d(submitted),
            period_end: Some(d(period_end)),
        }
    }

    fn current(codes: &[&str]) -> HashSet<String> {
        codes.iter().map(|c| c.to_string()).collect()
    }

    #[test]
    fn keeps_only_the_newest_unseen_doc() {
        let listed = vec![
            doc("7203", DocKind::Annual, "A2", "2026-06-20", "2026-03-31"),
            doc("7203", DocKind::Annual, "A1", "2025-06-20", "2025-03-31"),
            doc("9999", DocKind::Annual, "X", "2026-06-20", "2026-03-31"),
            doc("8058", DocKind::Annual, "B1", "2026-06-21", "2026-03-31"),
        ];
        let mut stored = HashMap::new();
        stored.insert(
            ("8058".to_string(), DocKind::Annual),
            doc("8058", DocKind::Annual, "B1", "2026-06-21", "2026-03-31"),
        );
        let p = plan(listed, &stored, &current(&["7203", "8058"]));
        let ids: Vec<&str> = p.fetch.iter().map(|d| d.doc_id.as_str()).collect();
        assert_eq!(
            ids,
            ["A2"],
            "古い有報・上場していない銘柄・取得済みは取らない"
        );
        assert!(p.delete.is_empty());
    }

    #[test]
    fn new_annual_drops_the_older_semiannual() {
        let mut stored = HashMap::new();
        stored.insert(
            ("7203".to_string(), DocKind::Semiannual),
            doc(
                "7203",
                DocKind::Semiannual,
                "S1",
                "2025-11-10",
                "2026-03-31",
            ),
        );
        let listed = vec![doc(
            "7203",
            DocKind::Annual,
            "A2",
            "2026-06-20",
            "2026-03-31",
        )];
        let p = plan(listed, &stored, &current(&["7203"]));
        assert_eq!(p.fetch.len(), 1);
        assert_eq!(p.delete, vec![("7203".to_string(), DocKind::Semiannual)]);
    }

    #[test]
    fn skips_semiannual_already_superseded_in_the_backfill() {
        // 初回の取り込み: 1年分の一覧に、半期報告書とその後の有報の両方がある
        let listed = vec![
            doc(
                "7203",
                DocKind::Semiannual,
                "S1",
                "2025-11-10",
                "2026-03-31",
            ),
            doc("7203", DocKind::Annual, "A2", "2026-06-20", "2026-03-31"),
            doc(
                "8058",
                DocKind::Semiannual,
                "S2",
                "2026-11-10",
                "2027-03-31",
            ),
            doc("8058", DocKind::Annual, "B2", "2026-06-21", "2026-03-31"),
        ];
        let p = plan(listed, &HashMap::new(), &current(&["7203", "8058"]));
        let ids: Vec<&str> = p.fetch.iter().map(|d| d.doc_id.as_str()).collect();
        assert_eq!(ids, ["A2", "B2", "S2"], "提出日の古い順");
    }

    #[test]
    fn delisted_codes_are_deleted() {
        let mut stored = HashMap::new();
        stored.insert(
            ("1111".to_string(), DocKind::Annual),
            doc("1111", DocKind::Annual, "Z", "2026-06-20", "2026-03-31"),
        );
        let p = plan(vec![], &stored, &current(&["7203"]));
        assert_eq!(p.delete, vec![("1111".to_string(), DocKind::Annual)]);
    }

    #[test]
    fn cursor_resumes_before_the_first_unfetched_doc() {
        let first = doc("7203", DocKind::Annual, "A", "2026-06-20", "2026-03-31");
        let today = d("2026-10-22");
        assert_eq!(
            resume_cursor(d("2026-10-20"), Some(&first), today),
            d("2026-06-19")
        );
        assert_eq!(resume_cursor(d("2026-10-20"), None, today), d("2026-10-20"));
        assert_eq!(
            resume_cursor(d("2026-10-22"), None, today),
            d("2026-10-21"),
            "今日の一覧は翌日にもう一度読む"
        );
        assert_eq!(
            r2_key("130A", DocKind::Semiannual),
            "edinet/130A/semiannual.json.gz"
        );
    }
}
