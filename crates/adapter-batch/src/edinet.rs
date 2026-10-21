//! EDINET API v2 から有価証券報告書（XBRLをCSV化したもの）を取得し、年次財務を取り出す。
//!
//! 流れ: 書類一覧（日付ごと）を走査して銘柄ごとの最新の有報を索引化 →
//! 書類を type=5（CSV）でダウンロード → zip 内の jpcrp*.csv（UTF-16LE のTSV）を解析。

use crate::http;
use domain::{AnnualFinancial, Date, FinancialData, StockCode};
use serde_json::Value;
use std::collections::HashMap;
use std::io::Read;
use usecase::{FinancialSource, PortError};

const BASE: &str = "https://api.edinet-fsa.go.jp/api/v2";
/// 有価証券報告書
const DOC_TYPE_ANNUAL: &str = "120";

#[derive(Debug, Clone, PartialEq)]
pub struct DocRef {
    pub doc_id: String,
    pub submitted: Date,
    pub period_end: Option<Date>,
}

pub struct EdinetClient {
    client: reqwest::Client,
    key: String,
    /// 証券コード（5桁）→ 最新の有報
    index: HashMap<String, DocRef>,
}

impl EdinetClient {
    pub fn new(client: reqwest::Client, key: String) -> Self {
        Self {
            client,
            key,
            index: HashMap::new(),
        }
    }

    /// 1日分の書類一覧（有報のみ）
    pub async fn list_day(&self, date: Date) -> Result<Vec<(String, DocRef)>, PortError> {
        let url = format!(
            "{BASE}/documents.json?date={date}&type=2&Subscription-Key={}",
            self.key
        );
        let body = http::get_bytes(&self.client, &url).await?;
        parse_document_list(&body)
    }

    /// 一覧の結果を索引に入れる。銘柄ごとに提出日が最新の有報だけを残す。
    /// targets が空なら全銘柄。
    pub fn add_to_index(&mut self, list: Vec<(String, DocRef)>, targets: &[StockCode]) {
        for (sec, doc) in list {
            if !targets.is_empty() && !targets.iter().any(|c| c.edinet_sec_code() == sec) {
                continue;
            }
            let newer = self
                .index
                .get(&sec)
                .is_none_or(|d| doc.submitted > d.submitted);
            if newer {
                self.index.insert(sec, doc);
            }
        }
    }

    pub fn indexed(&self) -> usize {
        self.index.len()
    }
}

impl FinancialSource for EdinetClient {
    async fn fetch_financials(&self, code: &StockCode) -> Result<Option<FinancialData>, PortError> {
        let Some(doc) = self.index.get(&code.edinet_sec_code()) else {
            return Ok(None);
        };
        let url = format!(
            "{BASE}/documents/{}?type=5&Subscription-Key={}",
            doc.doc_id, self.key
        );
        let zip = http::get_bytes(&self.client, &url).await?;
        let rows = read_csv_from_zip(&zip)?;
        Ok(Some(FinancialData {
            doc_id: doc.doc_id.clone(),
            submitted: Some(doc.submitted),
            years: extract_years(&rows, doc.period_end),
        }))
    }
}

/// 書類一覧APIの応答から (証券コード, 有報) を取り出す。CSVがある有報だけ。
pub fn parse_document_list(body: &[u8]) -> Result<Vec<(String, DocRef)>, PortError> {
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
        .filter(|r| {
            r["docTypeCode"].as_str() == Some(DOC_TYPE_ANNUAL) && r["csvFlag"].as_str() == Some("1")
        })
        .filter(|r| r["withdrawalStatus"].as_str().is_none_or(|s| s == "0"))
        .filter_map(|r| {
            Some((
                r["secCode"].as_str()?.to_string(),
                DocRef {
                    doc_id: r["docID"].as_str()?.to_string(),
                    submitted: Date::parse(r["submitDateTime"].as_str()?)?,
                    period_end: r["periodEnd"].as_str().and_then(Date::parse),
                },
            ))
        })
        .collect())
}

/// CSVの1行（必要な列だけ）
#[derive(Debug, Clone, PartialEq)]
pub struct XbrlRow {
    pub element: String,
    pub context: String,
    pub value: String,
}

/// zip の中から本体の CSV（XBRL_TO_CSV/jpcrp*.csv）を探して読む
pub fn read_csv_from_zip(bytes: &[u8]) -> Result<Vec<XbrlRow>, PortError> {
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes))
        .map_err(|e| PortError(format!("zip: {e}")))?;
    let mut rows = vec![];
    for i in 0..zip.len() {
        let mut f = zip
            .by_index(i)
            .map_err(|e| PortError(format!("zip: {e}")))?;
        let name = f.name().to_string();
        let is_target =
            name.starts_with("XBRL_TO_CSV/") && name.ends_with(".csv") && name.contains("jp");
        if !is_target {
            continue;
        }
        let mut buf = vec![];
        f.read_to_end(&mut buf)
            .map_err(|e| PortError(e.to_string()))?;
        rows.extend(parse_xbrl_csv(&decode_utf16le(&buf)));
    }
    if rows.is_empty() {
        return Err(PortError("no xbrl csv in zip".into()));
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

/// TSV（各列が "" で囲まれる）。列: 要素ID, 項目名, コンテキストID, 相対年度, 連結・個別, 期間・時点, ユニットID, 単位, 値
pub fn parse_xbrl_csv(text: &str) -> Vec<XbrlRow> {
    text.lines()
        .skip(1)
        .filter_map(|line| {
            let cols: Vec<&str> = line
                .split('\t')
                .map(|c| c.trim().trim_matches('"'))
                .collect();
            if cols.len() < 9 {
                return None;
            }
            Some(XbrlRow {
                element: cols[0].to_string(),
                context: cols[2].to_string(),
                value: cols[8].to_string(),
            })
        })
        .collect()
}

/// コンテキストIDから (何期前か, 個別か) を得る。セグメント等の内訳は除外。
fn context_year(ctx: &str) -> Option<(usize, bool)> {
    let (base, non_consolidated) = match ctx.strip_suffix("_NonConsolidatedMember") {
        Some(b) => (b, true),
        None => (ctx, false),
    };
    if base.contains('_') {
        return None;
    }
    let base = base
        .strip_suffix("Duration")
        .or_else(|| base.strip_suffix("Instant"))?;
    let n = match base {
        "CurrentYear" => 0,
        _ => base
            .strip_prefix("Prior")?
            .strip_suffix("Year")?
            .parse()
            .ok()?,
    };
    Some((n, non_consolidated))
}

fn parse_number(s: &str) -> Option<f64> {
    let t = s.replace(',', "");
    t.parse::<f64>().ok()
}

/// 項目ごとの候補要素名（日本基準・IFRS・米国基準・個別）。先にあるものを優先。
const NET_SALES: &[&str] = &[
    "NetSalesSummaryOfBusinessResults",
    "RevenueIFRSSummaryOfBusinessResults",
    "RevenuesUSGAAPSummaryOfBusinessResults",
    "OperatingRevenue1SummaryOfBusinessResults",
    "NetSales",
    "Revenue",
];
const OPERATING_INCOME: &[&str] = &[
    "OperatingIncome",
    "OperatingProfitLossIFRS",
    "OperatingIncomeLossUSGAAPSummaryOfBusinessResults",
    "OperatingProfitLossIFRSSummaryOfBusinessResults",
];
const ORDINARY_INCOME: &[&str] = &["OrdinaryIncomeLossSummaryOfBusinessResults"];
const NET_INCOME: &[&str] = &[
    "ProfitLossAttributableToOwnersOfParentSummaryOfBusinessResults",
    "ProfitLossAttributableToOwnersOfParentIFRSSummaryOfBusinessResults",
    "NetIncomeLossAttributableToOwnersOfParentUSGAAPSummaryOfBusinessResults",
    "NetIncomeLossSummaryOfBusinessResults",
];
const EQUITY: &[&str] = &[
    "EquityAttributableToOwnersOfParentIFRSSummaryOfBusinessResults",
    "NetAssetsSummaryOfBusinessResults",
];
const EPS: &[&str] = &[
    "BasicEarningsLossPerShareSummaryOfBusinessResults",
    "BasicEarningsLossPerShareIFRSSummaryOfBusinessResults",
    "BasicEarningsLossPerShareUSGAAPSummaryOfBusinessResults",
];
const BPS: &[&str] = &[
    "NetAssetsPerShareSummaryOfBusinessResults",
    "EquityAttributableToOwnersOfParentPerShareIFRSSummaryOfBusinessResults",
    "EquityAttributableToOwnersOfParentPerShareUSGAAPSummaryOfBusinessResults",
];
const DPS: &[&str] = &["DividendPaidPerShareSummaryOfBusinessResults"];
const OPERATING_CF: &[&str] = &[
    "NetCashProvidedByUsedInOperatingActivitiesSummaryOfBusinessResults",
    "CashFlowsFromUsedInOperatingActivitiesIFRSSummaryOfBusinessResults",
    "CashFlowsFromUsedInOperatingActivitiesUSGAAPSummaryOfBusinessResults",
];

/// (連結の値, 個別の値)
type ConsAndParent = (Option<f64>, Option<f64>);

/// 最大5期分（新しい順）の年次財務を組み立てる
pub fn extract_years(rows: &[XbrlRow], period_end: Option<Date>) -> Vec<AnnualFinancial> {
    // (局所名, 何期前) -> (連結の値, 個別の値)
    let mut map: HashMap<(&str, usize), ConsAndParent> = HashMap::new();
    for r in rows {
        let local = r.element.rsplit(':').next().unwrap_or(&r.element);
        let Some((n, non_cons)) = context_year(&r.context) else {
            continue;
        };
        let Some(v) = parse_number(&r.value) else {
            continue;
        };
        let e = map.entry((local, n)).or_default();
        if non_cons {
            e.1.get_or_insert(v);
        } else {
            e.0.get_or_insert(v);
        }
    }
    // 連結を優先し、無ければ個別（個別決算のみの会社や、配当は個別で開示されるため）
    let pick = |names: &[&str], n: usize| -> Option<f64> {
        names
            .iter()
            .find_map(|k| map.get(&(*k, n)).and_then(|(c, _)| *c))
            .or_else(|| {
                names
                    .iter()
                    .find_map(|k| map.get(&(*k, n)).and_then(|(_, p)| *p))
            })
    };
    let mut years = vec![];
    for n in 0..5 {
        let y = AnnualFinancial {
            period_end: period_end.map(|p| Date::new(p.y - n as i32, p.m, p.d)),
            net_sales: pick(NET_SALES, n),
            operating_income: pick(OPERATING_INCOME, n),
            ordinary_income: pick(ORDINARY_INCOME, n),
            net_income: pick(NET_INCOME, n),
            equity: pick(EQUITY, n),
            eps: pick(EPS, n),
            bps: pick(BPS, n),
            dps: pick(DPS, n),
            operating_cf: pick(OPERATING_CF, n),
        };
        let empty =
            y.net_sales.is_none() && y.net_income.is_none() && y.eps.is_none() && y.dps.is_none();
        if empty {
            break;
        }
        years.push(y);
    }
    years
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
    fn context_parsing() {
        assert_eq!(context_year("CurrentYearDuration"), Some((0, false)));
        assert_eq!(
            context_year("Prior3YearInstant_NonConsolidatedMember"),
            Some((3, true))
        );
        assert_eq!(
            context_year("CurrentYearDuration_jpcrp030000-asr_E00001-000FooMember"),
            None
        );
        assert_eq!(context_year("FilingDateInstant"), None);
    }

    #[test]
    fn document_list_fixture() {
        let body = include_bytes!("../../../fixtures/edinet_documents_sample.json");
        let list = parse_document_list(body).unwrap();
        // 有報かつCSVありの1件だけが残る（半期報告書・CSVなしは除外）
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].0, "99990");
        assert_eq!(list[0].1.doc_id, "S100TEST");
        assert_eq!(list[0].1.period_end.unwrap().to_string(), "2026-03-31");
    }

    #[test]
    fn csv_fixture_extracts_five_years() {
        let text = std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../fixtures/edinet_xbrl_sample.tsv"
        ))
        .unwrap();
        let rows = parse_xbrl_csv(&decode_utf16le(&utf16(&text)));
        let years = extract_years(&rows, Date::parse("2026-03-31"));
        assert_eq!(years.len(), 5);
        let y0 = &years[0];
        assert_eq!(y0.net_sales, Some(120_000_000_000.0));
        assert_eq!(y0.operating_income, Some(12_000_000_000.0)); // PLの営業利益
        assert_eq!(y0.dps, Some(60.0)); // 個別コンテキストから補完
        assert_eq!(y0.eps, Some(150.0));
        assert_eq!(years[1].operating_income, Some(11_000_000_000.0));
        assert_eq!(years[2].operating_income, None); // PLは2期分しかない
        assert_eq!(years[4].dps, Some(40.0));
        assert_eq!(years[4].period_end.unwrap().to_string(), "2022-03-31");
    }
}
