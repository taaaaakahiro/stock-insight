//! EDINET API v2 から有価証券報告書・半期報告書を取り、配当政策などの文章だけを取り出す。
//!
//! 流れ: 書類一覧（1日1回の呼び出しで全社分）→ 銘柄・種類ごとに最新の1通を選ぶ（usecase::edinet）→
//! 書類を type=5（XBRL を CSV にした zip。PDF より軽い）で取る → jpcrp*.csv（UTF-16LE の TSV）から
//! 文章の項目（〜TextBlock）を抜く。
//! API キーは URL に載せず、ヘッダーで渡す。EDINET は Azure API Management の既定のヘッダー名
//! （Ocp-Apim-Subscription-Key）だけを受け付ける。クエリと同じ名前の Subscription-Key ヘッダーは 401 になる。

use crate::{http, jquants::code_from_jquants};
use domain::disclosure::{normalize_text, DocKind, Section};
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

    /// 1日分の書類一覧（有報・半期報告書だけ）
    pub async fn list_day(&self, date: Date) -> Result<Vec<DocRef>, PortError> {
        let body = self
            .get(&format!("{BASE}/documents.json?date={date}&type=2"))
            .await?;
        parse_document_list(&body)
    }

    /// 書類の文章（項目ごと、整形済み）。数値の項目は J-Quants で足りるので読まない
    pub async fn fetch_sections(&self, doc_id: &str) -> Result<Vec<(Section, String)>, PortError> {
        let zip = self
            .get(&format!("{BASE}/documents/{doc_id}?type=5"))
            .await?;
        let rows = read_csv_from_zip(&zip)?;
        Ok(sections(&rows))
    }
}

/// 書類一覧APIの応答から、有報・半期報告書（CSV があり、取下げでなく、証券コードがあるもの）
pub fn parse_document_list(body: &[u8]) -> Result<Vec<DocRef>, PortError> {
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
        .collect())
}

/// CSVの1行（必要な列だけ）
#[derive(Debug, Clone, PartialEq)]
pub struct XbrlRow {
    pub element: String,
    pub value: String,
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
        let list = parse_document_list(body).unwrap();
        let ids: Vec<(&str, DocKind)> = list.iter().map(|d| (d.doc_id.as_str(), d.kind)).collect();
        // 有報と半期報告書だけ。CSV なし・取下げ・証券コードなし・訂正は除く
        assert_eq!(
            ids,
            [
                ("S100AAAA", DocKind::Annual),
                ("S100BBBB", DocKind::Semiannual)
            ]
        );
        assert_eq!(list[0].code, "9999");
        assert_eq!(list[0].submitted.to_string(), "2026-06-20");
        assert_eq!(list[1].period_end.unwrap().to_string(), "2026-09-30");
    }

    #[test]
    fn error_status_in_list() {
        let body = br#"{"metadata":{"status":"404","message":"Not Found"}}"#;
        assert!(parse_document_list(body).is_err());
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
