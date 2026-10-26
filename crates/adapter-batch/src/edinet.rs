//! EDINET API v2 から有価証券報告書・半期報告書を取り、配当政策などの文章だけを取り出す。
//!
//! 流れ: 書類一覧（1日1回の呼び出しで全社分）→ 銘柄・種類ごとに最新の1通を選ぶ（usecase::edinet）→
//! 書類を type=5（XBRL を CSV にした zip。PDF より軽い）で取る → jpcrp*.csv（UTF-16LE の TSV）から
//! 文章の項目（〜TextBlock）を抜く。
//! API キーは URL に載せず、ヘッダーで渡す。EDINET は Azure API Management の既定のヘッダー名
//! （Ocp-Apim-Subscription-Key）だけを受け付ける。クエリと同じ名前の Subscription-Key ヘッダーは 401 になる。

use crate::{http, ixbrl, jquants::code_from_jquants};
use domain::disclosure::{normalize_text, DocKind, Section};
use domain::reit::{
    self, Checks, DebtSchedule, DebtSummary, PortfolioSummary, Property, PropertyTotal, RateMix,
};
use domain::Date;
use serde_json::Value;
use std::collections::BTreeMap;
use std::io::Read;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;
use usecase::edinet::DocRef;
use usecase::PortError;

const BASE: &str = "https://api.edinet-fsa.go.jp/api/v2";

pub struct EdinetClient {
    client: reqwest::Client,
    key: String,
    interval: Duration,
    last: Mutex<Option<Instant>>,
}

impl EdinetClient {
    /// calls_per_minute: 呼び出しの上限（EDINET は公表の上限がないので、控えめに既定 60）
    pub fn new(client: reqwest::Client, key: String, calls_per_minute: u32) -> Self {
        Self {
            client,
            key,
            interval: Duration::from_millis(60_000 / calls_per_minute.max(1) as u64),
            last: Mutex::new(None),
        }
    }

    async fn pace(&self) {
        let mut last = self.last.lock().await;
        if let Some(t) = *last {
            let wait = self.interval.saturating_sub(t.elapsed());
            if !wait.is_zero() {
                tokio::time::sleep(wait).await;
            }
        }
        *last = Some(Instant::now());
    }

    async fn get(&self, url: &str) -> Result<Vec<u8>, PortError> {
        self.pace().await;
        let body = http::get_bytes_with_headers(
            &self.client,
            url,
            &[("Ocp-Apim-Subscription-Key", &self.key)],
        )
        .await?;
        // 認証の失敗なども HTTP 200 で JSON（{"StatusCode": 401, ...}）が返る
        if body.first() == Some(&b'{') {
            if let Ok(v) = serde_json::from_slice::<Value>(&body) {
                if let Some(code) = v["StatusCode"].as_i64().filter(|c| *c != 200) {
                    return Err(PortError(format!(
                        "edinet {code}: {} ({})",
                        v["message"].as_str().unwrap_or(""),
                        http::redact(url)
                    )));
                }
            }
        }
        Ok(body)
    }

    /// 1日分の書類一覧（その日に提出された有報・半期報告書だけ）
    pub async fn list_day(&self, date: Date) -> Result<Vec<DocRef>, PortError> {
        let body = self
            .get(&format!("{BASE}/documents.json?date={date}&type=2"))
            .await?;
        parse_document_list(&body, date)
    }

    /// 1日分の書類一覧のうち REIT（投資法人）の有報
    pub async fn list_reit_day(&self, date: Date) -> Result<Vec<ReitDocRef>, PortError> {
        let body = self
            .get(&format!("{BASE}/documents.json?date={date}&type=2"))
            .await?;
        parse_reit_list(&body, date)
    }

    /// RAG audit retains corrections and withdrawal/metadata events, including
    /// older reports appearing in today's list. Do not apply submitted==day here.
    pub async fn list_reit_events(&self, date: Date) -> Result<Vec<Value>, PortError> {
        let body = self
            .get(&format!("{BASE}/documents.json?date={date}&type=2"))
            .await?;
        parse_reit_events(&body)
    }

    /// 書類の iXBRL（type=1 の zip）。表を読むときに使う
    pub async fn fetch_ixbrl_zip(&self, doc_id: &str) -> Result<Vec<u8>, PortError> {
        self.get(&format!("{BASE}/documents/{doc_id}?type=1")).await
    }

    /// 書類の CSV（type=5 の zip）。中身は sections_from_zip で読む
    pub async fn fetch_csv_zip(&self, doc_id: &str) -> Result<Vec<u8>, PortError> {
        self.get(&format!("{BASE}/documents/{doc_id}?type=5")).await
    }
}

/// 書類一覧APIの応答から、有報・半期報告書（CSV があり、取下げでなく、証券コードがあるもの）。
/// 会社が信託受益証券などの特定有価証券の有報（府令 030、XBRL は jpsps）を出すと証券コードに会社のものが入るので、
/// 会社本体の書類（企業内容等の開示府令、010）だけに絞る。
/// 一覧には、その日に縦覧の状態などが変わった過去の書類も載るので、その日に提出されたものだけを残す
/// （古い書類の提出日で続きの日付が大きく戻らないように）
pub fn parse_document_list(body: &[u8], day: Date) -> Result<Vec<DocRef>, PortError> {
    let v: Value = serde_json::from_slice(body).map_err(|e| PortError(format!("json: {e}")))?;
    if let Some(code) = v["metadata"]["status"].as_str() {
        if code != "200" {
            return Err(PortError(format!("edinet status {code}")));
        }
    }
    let empty = vec![];
    let results = v["results"].as_array().unwrap_or(&empty);
    Ok(results
        .iter()
        .filter(|r| r["ordinanceCode"].as_str() == Some("010"))
        .filter(|r| r["csvFlag"].as_str() == Some("1"))
        .filter(|r| r["withdrawalStatus"].as_str().is_none_or(|s| s == "0"))
        .filter_map(|r| {
            Some(DocRef {
                code: code_from_jquants(r["secCode"].as_str()?)?
                    .as_str()
                    .to_string(),
                kind: DocKind::from_edinet(r["docTypeCode"].as_str()?)?,
                doc_id: r["docID"].as_str()?.to_string(),
                submitted: Date::parse(r["submitDateTime"].as_str()?)?,
                period_end: r["periodEnd"].as_str().and_then(Date::parse),
            })
        })
        .filter(|d| d.submitted == day)
        .collect())
}

pub fn parse_reit_events(body: &[u8]) -> Result<Vec<Value>, PortError> {
    let data: Value =
        serde_json::from_slice(body).map_err(|_| PortError("EDINET audit JSON".into()))?;
    if data["metadata"]["status"]
        .as_str()
        .is_some_and(|s| s != "200")
    {
        return Err(PortError("EDINET audit status".into()));
    }
    let rows = data["results"]
        .as_array()
        .ok_or_else(|| PortError("EDINET audit results missing".into()))?;
    Ok(rows
        .iter()
        .filter(|r| {
            r["ordinanceCode"] == "030" && matches!(r["docTypeCode"].as_str(), Some("120" | "130"))
        })
        .filter(|r| {
            r["filerName"]
                .as_str()
                .is_some_and(|name| name.contains("投資法人"))
        })
        .cloned()
        .collect())
}

/// REIT（投資法人）の有価証券報告書。書類一覧では secCode が空なので、証券コードは書類の表紙から読む
#[derive(Debug, Clone, PartialEq)]
pub struct ReitDocRef {
    pub doc_id: String,
    pub edinet_code: String,
    pub filer_name: String,
    pub submitted: Date,
    pub period_end: Option<Date>,
}

/// 書類一覧から、その日に提出された投資法人の有報（特定有価証券の府令 030・書類種別 120）
pub fn parse_reit_list(body: &[u8], day: Date) -> Result<Vec<ReitDocRef>, PortError> {
    let v: Value = serde_json::from_slice(body).map_err(|e| PortError(format!("json: {e}")))?;
    if let Some(code) = v["metadata"]["status"].as_str() {
        if code != "200" {
            return Err(PortError(format!("edinet status {code}")));
        }
    }
    let empty = vec![];
    Ok(v["results"]
        .as_array()
        .unwrap_or(&empty)
        .iter()
        .filter(|r| {
            r["docTypeCode"].as_str() == Some("120") && r["ordinanceCode"].as_str() == Some("030")
        })
        .filter(|r| r["xbrlFlag"].as_str() == Some("1"))
        .filter(|r| r["withdrawalStatus"].as_str().is_none_or(|s| s == "0"))
        .filter(|r| {
            r["filerName"]
                .as_str()
                .is_some_and(|n| n.contains("投資法人"))
        })
        .filter_map(|r| {
            Some(ReitDocRef {
                doc_id: r["docID"].as_str()?.to_string(),
                edinet_code: r["edinetCode"].as_str()?.to_string(),
                filer_name: r["filerName"].as_str()?.to_string(),
                submitted: Date::parse(r["submitDateTime"].as_str()?)?,
                period_end: r["periodEnd"].as_str().and_then(Date::parse),
            })
        })
        .filter(|d| d.submitted == day)
        .collect())
}

/// 当期末の貸借対照表の有利子負債（円）
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct ReitBalance {
    pub short_loans: i64,
    /// 1年内返済予定を含む
    pub long_loans: i64,
    /// 1年内償還予定を含む
    pub bonds: i64,
}

/// REIT の有報から読んだ借入金と物件
#[derive(Debug, Clone, PartialEq)]
pub struct ReitReport {
    /// 4桁の証券コード（表紙の SecurityCodeDEI）
    pub code: Option<String>,
    pub balance: ReitBalance,
    pub debt: DebtSchedule,
    pub properties: Vec<Property>,
    pub property_total: Option<PropertyTotal>,
}

/// 本文のファイル名の先頭2桁で分けたブロック（01 = ファンドの状況、02 = 経理の状況）
pub fn reit_blocks(zip_bytes: &[u8]) -> Result<(Vec<reit::Block>, Vec<reit::Block>), PortError> {
    let mut fund = vec![];
    let mut accounts = vec![];
    for (name, html) in ixbrl::public_docs(zip_bytes)? {
        if name.starts_with("01") {
            fund.extend(ixbrl::blocks(&html));
        } else if name.starts_with("02") {
            accounts.extend(ixbrl::blocks(&html));
        }
    }
    Ok((fund, accounts))
}

/// type=1 の zip から読む。本文のファイル名の先頭2桁で、01 = ファンドの状況（物件）、02 = 経理の状況（借入金）
pub fn read_reit_report(zip_bytes: &[u8]) -> Result<ReitReport, PortError> {
    let docs = ixbrl::public_docs(zip_bytes)?;
    let mut code = None;
    let mut fund = vec![];
    let mut accounts = vec![];
    let mut facts = std::collections::HashMap::new();
    for (name, html) in &docs {
        if name.contains("_header_") {
            code = ixbrl::non_numeric(html, "SecurityCodeDEI")
                .and_then(|c| code_from_jquants(&c))
                .map(|c| c.as_str().to_string());
            continue;
        }
        facts.extend(ixbrl::facts(html));
        if name.starts_with("01") {
            fund.extend(ixbrl::blocks(html));
        } else if name.starts_with("02") {
            accounts.extend(ixbrl::blocks(html));
        }
    }
    let bs = |n: &str| ixbrl::current_instant(&facts, n);
    let (properties, property_total) = reit::parse_properties(&fund);
    Ok(ReitReport {
        code,
        balance: ReitBalance {
            short_loans: bs("ShortTermLoansPayable"),
            // 1年内返済予定は銘柄独自の要素名のことがある（LongTermLoansRepayableWithinOneYearCL など）
            long_loans: bs("LongTermLoansPayable")
                + ixbrl::current_instant_like(
                    &facts,
                    &[
                        "CurrentPortionOfLongTermLoansPayable",
                        "LongTermLoansRepayableWithinOneYear",
                    ],
                ),
            bonds: bs("InvestmentCorporationBondINV")
                + bs("CurrentPortionOfInvestmentCorporationBondsINV"),
        },
        debt: reit::parse_debt(&accounts),
        properties,
        property_total,
    })
}

/// 取り込む1通: 書類・読んだ中身・一覧用の要約
#[derive(Debug, Clone, PartialEq)]
pub struct ReitRecord {
    pub code: String,
    pub doc: ReitDocRef,
    pub period_end: Date,
    pub report: ReitReport,
    pub mix: RateMix,
    pub debt: DebtSummary,
    pub portfolio: PortfolioSummary,
    pub checks: Checks,
}

/// 書類と読んだ中身から要約を作る。証券コードか期末日が分からない書類は None（取り込まない）
pub fn reit_record(doc: ReitDocRef, report: ReitReport) -> Option<ReitRecord> {
    let code = report.code.clone()?;
    let period_end = doc.period_end?;
    let mix = reit::rate_mix(&report.debt, report.balance.bonds);
    let debt = reit::debt_summary(&report.debt, &mix, period_end);
    let portfolio = reit::portfolio_summary(&report.properties);
    let checks = reit::checks(
        &report.debt,
        report.balance.short_loans + report.balance.long_loans,
        &report.properties,
        report.property_total.as_ref(),
    );
    Some(ReitRecord {
        code,
        doc,
        period_end,
        report,
        mix,
        debt,
        portfolio,
        checks,
    })
}

/// 詳細の画面に出す明細（reit_detail.body）
pub fn reit_detail_json(r: &ReitRecord) -> Value {
    let rp = &r.report;
    serde_json::json!({
        "doc_id": r.doc.doc_id,
        "period_end": r.period_end.to_string(),
        "loans": rp.debt.loans.iter().map(|l| serde_json::json!({
            "kind": l.kind.map(|k| match k { reit::LoanKind::Short => "short", reit::LoanKind::Long => "long" }),
            "lender": l.lender,
            "balance": l.balance_yen,
            "rate": l.rate_pct,
            "due": l.due.map(|d| d.to_string()),
            "rate_type": l.rate_type.map(|t| match t {
                reit::RateType::Fixed => "fixed",
                reit::RateType::SwapFixed => "swap_fixed",
                reit::RateType::Floating => "floating",
            }),
        })).collect::<Vec<_>>(),
        "ladder": rp.debt.ladder.iter().map(|(k, v)| serde_json::json!({ "item": k, "amounts": v })).collect::<Vec<_>>(),
        "rate_notes": rp.debt.rate_notes,
        "properties": rp.properties.iter().map(|p| serde_json::json!({
            "id": p.id,
            "name": p.name,
            "leasable": p.leasable_m2,
            "leased": p.leased_m2,
            "occupancy": p.occupancy_pct,
            "rent": p.annual_rent_yen,
            "rent_note": if p.annual_rent_yen.is_none() && !p.rent_raw.is_empty() { Some(&p.rent_raw) } else { None },
        })).collect::<Vec<_>>(),
    })
}

/// CSVの1行（必要な列だけ）
#[derive(Debug, Clone, PartialEq)]
pub struct XbrlRow {
    pub element: String,
    pub value: String,
}

/// 書類の文章（項目ごと、整形済み）。数値の項目は J-Quants で足りるので読まない
pub fn sections_from_zip(bytes: &[u8]) -> Result<Vec<(Section, String)>, PortError> {
    Ok(sections(&read_csv_from_zip(bytes)?))
}

/// zip の中から本体の CSV（XBRL_TO_CSV/jpcrp*.csv）を探して読む。監査報告書（jpaud）は読まない
pub fn read_csv_from_zip(bytes: &[u8]) -> Result<Vec<XbrlRow>, PortError> {
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes))
        .map_err(|e| PortError(format!("zip: {e}")))?;
    let mut rows = vec![];
    for i in 0..zip.len() {
        let mut f = zip
            .by_index(i)
            .map_err(|e| PortError(format!("zip: {e}")))?;
        let name = f.name().to_string();
        let file = name.rsplit('/').next().unwrap_or(&name);
        if !(name.starts_with("XBRL_TO_CSV/")
            && file.starts_with("jpcrp")
            && file.ends_with(".csv"))
        {
            continue;
        }
        let mut buf = vec![];
        f.read_to_end(&mut buf)
            .map_err(|e| PortError(e.to_string()))?;
        rows.extend(parse_xbrl_csv(&decode_utf16le(&buf)));
    }
    if rows.is_empty() {
        return Err(PortError("no jpcrp csv in zip".into()));
    }
    Ok(rows)
}

pub fn decode_utf16le(buf: &[u8]) -> String {
    let units: Vec<u16> = buf
        .as_chunks::<2>()
        .0
        .iter()
        .map(|c| u16::from_le_bytes(*c))
        .collect();
    let s = String::from_utf16_lossy(&units);
    s.trim_start_matches('\u{feff}').to_string()
}

/// TSV（各列が "" で囲まれ、文章の値は改行を含む）。
/// 列: 要素ID, 項目名, コンテキストID, 相対年度, 連結・個別, 期間・時点, ユニットID, 単位, 値
pub fn parse_xbrl_csv(text: &str) -> Vec<XbrlRow> {
    split_tsv(text)
        .into_iter()
        .skip(1)
        .filter(|cols| cols.len() >= 9)
        .map(|cols| XbrlRow {
            element: cols[0].clone(),
            value: cols[8].clone(),
        })
        .collect()
}

/// 引用符の中の改行・タブ・"" を考慮して行と列に分ける
fn split_tsv(text: &str) -> Vec<Vec<String>> {
    let mut rows = vec![];
    let mut row = vec![];
    let mut field = String::new();
    let mut quoted = false;
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '"' if quoted && chars.peek() == Some(&'"') => {
                field.push('"');
                chars.next();
            }
            '"' => quoted = !quoted,
            '\t' if !quoted => row.push(std::mem::take(&mut field)),
            '\r' if !quoted => {}
            '\n' if !quoted => {
                row.push(std::mem::take(&mut field));
                rows.push(std::mem::take(&mut row));
            }
            _ => field.push(c),
        }
    }
    if !field.is_empty() || !row.is_empty() {
        row.push(field);
        rows.push(row);
    }
    rows
}

/// 文章の項目を取り出して整形する。同じ項目が複数行あればつなげる
pub fn sections(rows: &[XbrlRow]) -> Vec<(Section, String)> {
    let mut by: BTreeMap<Section, Vec<String>> = BTreeMap::new();
    for r in rows {
        let local = r.element.rsplit(':').next().unwrap_or(&r.element);
        let Some(section) = Section::from_element(local) else {
            continue;
        };
        let text = normalize_text(&r.value);
        let texts = by.entry(section).or_default();
        if !text.is_empty() && !texts.contains(&text) {
            texts.push(text);
        }
    }
    by.into_iter()
        .filter(|(_, t)| !t.is_empty())
        .map(|(s, t)| (s, t.join(" ")))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn utf16(s: &str) -> Vec<u8> {
        let mut v = vec![0xFF, 0xFE];
        for u in s.encode_utf16() {
            v.extend_from_slice(&u.to_le_bytes());
        }
        v
    }

    #[test]
    fn document_list_fixture() {
        let body = include_bytes!("../../../fixtures/edinet_documents_sample.json");
        let list = parse_document_list(body, Date::new(2026, 11, 10)).unwrap();
        let ids: Vec<(&str, DocKind)> = list.iter().map(|d| (d.doc_id.as_str(), d.kind)).collect();
        // その日に提出された有報と半期報告書だけ。CSV なし・取下げ・証券コードなし・訂正・前に提出された書類・特定有価証券の書類は除く
        assert_eq!(
            ids,
            [
                ("S100AAAA", DocKind::Annual),
                ("S100BBBB", DocKind::Semiannual)
            ]
        );
        assert_eq!(list[0].code, "9999");
        assert_eq!(list[0].submitted.to_string(), "2026-11-10");
        assert_eq!(
            list[1].period_end.unwrap().to_string(),
            "2027-03-31",
            "事業年度の末日"
        );
    }

    #[test]
    fn reit_list_uses_filer_and_ordinance() {
        let body = r#"{"metadata":{"status":"200"},"results":[
          {"docID":"S1","edinetCode":"E27230","filerName":"日本プロロジスリート投資法人","docTypeCode":"120","ordinanceCode":"030","xbrlFlag":"1","withdrawalStatus":"0","secCode":null,"submitDateTime":"2026-08-28 15:00","periodEnd":"2026-05-31"},
          {"docID":"S2","edinetCode":"E00001","filerName":"何とか株式会社","docTypeCode":"120","ordinanceCode":"010","xbrlFlag":"1","secCode":"99990","submitDateTime":"2026-08-28 15:00"},
          {"docID":"S3","edinetCode":"E27230","filerName":"日本プロロジスリート投資法人","docTypeCode":"220","ordinanceCode":"030","xbrlFlag":"1","submitDateTime":"2026-08-28 15:00"},
          {"docID":"S4","edinetCode":"E13206","filerName":"日本ビルファンド投資法人","docTypeCode":"120","ordinanceCode":"030","xbrlFlag":"1","submitDateTime":"2026-03-30 15:00"}
        ]}"#;
        let list = parse_reit_list(body.as_bytes(), Date::new(2026, 8, 28)).unwrap();
        assert_eq!(
            list.iter().map(|d| d.doc_id.as_str()).collect::<Vec<_>>(),
            ["S1"],
            "secCode が空でも拾う。自己投資口の報告書・前の日の書類は除く"
        );
        assert_eq!(list[0].period_end, Some(Date::new(2026, 5, 31)));
    }

    fn zip_of(files: &[(&str, &[u8])]) -> Vec<u8> {
        use std::io::Write;
        let mut w = zip::ZipWriter::new(std::io::Cursor::new(vec![]));
        for (name, body) in files {
            w.start_file(*name, zip::write::SimpleFileOptions::default())
                .unwrap();
            w.write_all(body).unwrap();
        }
        w.finish().unwrap().into_inner()
    }

    #[test]
    fn zip_without_jpcrp_csv_is_unreadable() {
        let head = [
            "要素ID",
            "項目名",
            "コンテキストID",
            "相対年度",
            "連結・個別",
            "期間・時点",
            "ユニットID",
            "単位",
            "値",
        ];
        let row = [
            "jpcrp_cor:DividendPolicyTextBlock",
            "配当政策",
            "FilingDateInstant",
            "提出日時点",
            "その他",
            "時点",
            "",
            "",
            "安定的な配当を行う",
        ];
        let line = |cols: &[&str]| {
            cols.iter()
                .map(|c| format!("\"{c}\""))
                .collect::<Vec<_>>()
                .join("\t")
        };
        let csv = utf16(&format!("{}\n{}\n", line(&head), line(&row)));
        let trust = zip_of(&[(
            "XBRL_TO_CSV/jpsps090000-asr-001_G15497-000_2026-05-31_01_2026-08-28.csv",
            &csv,
        )]);
        assert!(
            sections_from_zip(&trust).is_err(),
            "特定有価証券（jpsps）の CSV しかない書類は読めない"
        );
        let company = zip_of(&[(
            "XBRL_TO_CSV/jpcrp030000-asr-001_E99999-000_2026-03-31_01_2026-06-20.csv",
            &csv,
        )]);
        assert!(sections_from_zip(&company).is_ok());
    }

    #[test]
    fn error_status_in_list() {
        let body = br#"{"metadata":{"status":"404","message":"Not Found"}}"#;
        assert!(parse_document_list(body, Date::new(2026, 11, 10)).is_err());
    }

    #[test]
    fn csv_with_multiline_text_blocks() {
        let text = std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../fixtures/edinet_text_sample.tsv"
        ))
        .unwrap();
        let rows = parse_xbrl_csv(&decode_utf16le(&utf16(&text)));
        assert_eq!(rows.len(), 4);
        let s = sections(&rows);
        assert_eq!(s.len(), 2);
        assert_eq!(s[0].0, Section::DividendPolicy);
        assert_eq!(
            s[0].1,
            "当社は累進配当を基本方針とし、配当性向30%以上を目標としております。 当期は1株当たり5円の増配といたしました。"
        );
        assert_eq!(s[1].0, Section::BusinessPolicy);
        assert!(
            s[1].1.contains(r#"「"中期経営計画"」"#),
            "\"\" は \" に戻す"
        );
    }
}
