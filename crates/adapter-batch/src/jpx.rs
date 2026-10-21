//! JPX「東証上場銘柄一覧」（xlsx）から銘柄マスタを作る。
//! xlsx は zip の中の XML。必要なのは共有文字列（sharedStrings.xml）と1枚目のシート（sheet1.xml）だけなので、
//! 汎用の Excel ライブラリは使わず、この2つの単純な形だけを読む。

use crate::http;
use domain::stock_master::{listed_stock_from_jpx, ListedStock};
use std::collections::HashMap;
use std::io::Read;
use usecase::master::StockMasterSource;
use usecase::PortError;

pub const LIST_URL: &str =
    "https://www.jpx.co.jp/markets/statistics-equities/misc/tvdivq0000001vg2-att/data_j.xlsx";

/// JPX のサイトから取得する。`from_file` を渡すと手元のファイルを読む（ローカル検証用）
pub struct JpxSource {
    pub client: reqwest::Client,
    pub from_file: Option<String>,
}

impl StockMasterSource for JpxSource {
    async fn fetch_master(&self) -> Result<(String, Vec<ListedStock>), PortError> {
        let bytes = match &self.from_file {
            Some(path) => std::fs::read(path).map_err(|e| PortError(format!("jpx: {e}")))?,
            None => http::get_bytes(&self.client, LIST_URL).await?,
        };
        parse_xlsx(&bytes)
    }
}

/// (基準日 "YYYYMMDD", 銘柄)
pub fn parse_xlsx(bytes: &[u8]) -> Result<(String, Vec<ListedStock>), PortError> {
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes))
        .map_err(|e| PortError(format!("jpx zip: {e}")))?;
    let mut read = |name: &str| -> Result<String, PortError> {
        let mut f = zip
            .by_name(name)
            .map_err(|e| PortError(format!("jpx {name}: {e}")))?;
        let mut s = String::new();
        f.read_to_string(&mut s)
            .map_err(|e| PortError(format!("jpx {name}: {e}")))?;
        Ok(s)
    };
    let strings = parse_shared_strings(&read("xl/sharedStrings.xml")?);
    let rows = parse_sheet(&read("xl/worksheets/sheet1.xml")?, &strings);
    rows_to_stocks(&rows)
}

/// 見出し行の名前で列を探し、国内株式・REIT 等・ETF/ETN を取り出す
pub fn rows_to_stocks(
    rows: &[HashMap<String, String>],
) -> Result<(String, Vec<ListedStock>), PortError> {
    let header = rows
        .first()
        .ok_or_else(|| PortError("jpx: empty sheet".into()))?;
    let col = |title: &str| -> Result<String, PortError> {
        header
            .iter()
            .find(|(_, v)| v.as_str() == title)
            .map(|(k, _)| k.clone())
            .ok_or_else(|| PortError(format!("jpx: column {title} not found")))
    };
    let (c_date, c_code, c_name, c_market, c_sector, c_size) = (
        col("日付")?,
        col("コード")?,
        col("銘柄名")?,
        col("市場・商品区分")?,
        col("33業種区分")?,
        col("規模区分")?,
    );
    let get = |r: &HashMap<String, String>, c: &str| r.get(c).cloned().unwrap_or_default();
    let as_of = rows.get(1).map(|r| get(r, &c_date)).unwrap_or_default();
    let stocks: Vec<ListedStock> = rows[1..]
        .iter()
        .filter_map(|r| {
            listed_stock_from_jpx(
                &get(r, &c_code),
                &get(r, &c_name),
                &get(r, &c_market),
                &get(r, &c_sector),
                &get(r, &c_size),
            )
        })
        .collect();
    if stocks.is_empty() {
        return Err(PortError("jpx: no stocks found".into()));
    }
    Ok((as_of, stocks))
}

/// `<si>` ごとに、中の `<t>` をつなげた文字列（書式付きの文字列は `<r><t>` に分かれる）
pub fn parse_shared_strings(xml: &str) -> Vec<String> {
    elements(xml, "si")
        .map(|si| elements(si, "t").map(unescape).collect::<String>())
        .collect()
}

/// 行ごとに「列の文字（A, B, ...）→ 値」
pub fn parse_sheet(xml: &str, strings: &[String]) -> Vec<HashMap<String, String>> {
    elements_with_attrs(xml, "row")
        .map(|(_, row)| {
            elements_with_attrs(row, "c")
                .filter_map(|(attrs, cell)| {
                    let r = attr(attrs, "r")?;
                    let column: String =
                        r.chars().take_while(|c| c.is_ascii_alphabetic()).collect();
                    let value = match attr(attrs, "t") {
                        Some("s") => {
                            let i: usize = elements(cell, "v").next()?.trim().parse().ok()?;
                            strings.get(i)?.clone()
                        }
                        Some("inlineStr") => elements(cell, "t").map(unescape).collect(),
                        _ => unescape(elements(cell, "v").next()?),
                    };
                    Some((column, value))
                })
                .collect()
        })
        .collect()
}

/// `<tag ...>中身</tag>` の中身を順に返す（`<tag/>` は空文字）。入れ子の同名タグはない前提
fn elements<'a>(xml: &'a str, tag: &'a str) -> impl Iterator<Item = &'a str> + 'a {
    elements_with_attrs(xml, tag).map(|(_, body)| body)
}

/// (属性部分, 中身)
fn elements_with_attrs<'a>(
    xml: &'a str,
    tag: &'a str,
) -> impl Iterator<Item = (&'a str, &'a str)> + 'a {
    let open = format!("<{tag}");
    let close = format!("</{tag}>");
    let mut rest = xml;
    std::iter::from_fn(move || loop {
        let start = rest.find(&open)?;
        let after = &rest[start + open.len()..];
        // <tag> / <tag attr> / <tag/> だけを拾い、<tagX> のような別名は飛ばす
        match after.chars().next()? {
            '>' | ' ' | '/' | '\t' | '\r' | '\n' => {}
            _ => {
                rest = after;
                continue;
            }
        }
        let gt = after.find('>')?;
        let attrs = &after[..gt];
        if attrs.ends_with('/') {
            rest = &after[gt + 1..];
            return Some((attrs.trim_end_matches('/'), ""));
        }
        let body_start = &after[gt + 1..];
        let end = body_start.find(&close)?;
        rest = &body_start[end + close.len()..];
        return Some((attrs, &body_start[..end]));
    })
}

fn attr<'a>(attrs: &'a str, name: &str) -> Option<&'a str> {
    let key = format!(" {name}=\"");
    let i = format!(" {attrs}").find(&key)?;
    let v = &attrs[i + key.len() - 1..];
    v.split('"').next()
}

fn unescape(s: &str) -> String {
    if !s.contains('&') {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]);
        let tail = &rest[i..];
        let Some(semi) = tail.find(';') else {
            out.push_str(tail);
            return out;
        };
        let entity = &tail[1..semi];
        let decoded = match entity {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            _ => entity
                .strip_prefix("#x")
                .and_then(|h| u32::from_str_radix(h, 16).ok())
                .or_else(|| entity.strip_prefix('#').and_then(|d| d.parse().ok()))
                .and_then(char::from_u32),
        };
        match decoded {
            Some(c) => out.push(c),
            None => out.push_str(&tail[..=semi]),
        }
        rest = &tail[semi + 1..];
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use domain::stock_master::SizeClass;

    const STRINGS: &str = r#"<?xml version="1.0"?><sst count="9"><si><t>日付</t></si><si><t>コード</t></si><si><t>銘柄名</t></si><si><t>市場・商品区分</t></si><si><t>33業種区分</t></si><si><t>規模区分</t></si><si><r><rPr/><t>極</t></r><r><t>洋</t></r></si><si><t>プライム（内国株式）</t></si><si><t>水産・農林業</t></si><si><t>TOPIX Small 1</t></si><si><t>ETF・ETN</t></si><si><t>A&amp;B ETF</t></si><si><t xml:space="preserve">-</t></si></sst>"#;

    const SHEET: &str = r#"<worksheet><sheetData>
<row r="1"><c r="A1" t="s"><v>0</v></c><c r="B1" t="s"><v>1</v></c><c r="C1" t="s"><v>2</v></c><c r="D1" t="s"><v>3</v></c><c r="F1" t="s"><v>4</v></c><c r="J1" t="s"><v>5</v></c></row>
<row r="2"><c r="A2"><v>20260930</v></c><c r="B2" t="inlineStr"><is><t>1301</t></is></c><c r="C2" t="s"><v>6</v></c><c r="D2" t="s"><v>7</v></c><c r="F2" t="s"><v>8</v></c><c r="J2" t="s"><v>9</v></c></row>
<row r="3"><c r="A3"><v>20260930</v></c><c r="B3" t="inlineStr"><is><t>1305</t></is></c><c r="C3" t="s"><v>11</v></c><c r="D3" t="s"><v>10</v></c><c r="F3" t="s"><v>12</v></c><c r="J3" t="s"><v>12</v></c><c r="K3"/></row>
</sheetData></worksheet>"#;

    #[test]
    fn parses_shared_strings_with_runs_and_entities() {
        let s = parse_shared_strings(STRINGS);
        assert_eq!(s[6], "極洋");
        assert_eq!(s[11], "A&B ETF");
        assert_eq!(s[12], "-");
    }

    #[test]
    fn extracts_domestic_stocks_by_header_names() {
        let strings = parse_shared_strings(STRINGS);
        let rows = parse_sheet(SHEET, &strings);
        assert_eq!(rows.len(), 3);
        let (as_of, stocks) = rows_to_stocks(&rows).unwrap();
        assert_eq!(as_of, "20260930");
        assert_eq!(stocks.len(), 2);
        assert_eq!(stocks[0].name, "極洋");
        assert_eq!(stocks[1].kind, domain::stock_master::AssetKind::Etf);
        assert_eq!(stocks[1].name, "A&B ETF");
        assert_eq!(stocks[0].market, "プライム");
        assert_eq!(stocks[0].size_class, SizeClass::Small);
    }

    #[test]
    fn unescapes_entities() {
        assert_eq!(
            unescape("a&lt;b&gt;&amp;&#12354;&#x3042;&unknown;"),
            "a<b>&ああ&unknown;"
        );
    }

    #[test]
    fn missing_column_is_an_error() {
        let rows = parse_sheet(
            SHEET
                .replace(r#"<c r="J1" t="s"><v>5</v></c>"#, "")
                .as_str(),
            &parse_shared_strings(STRINGS),
        );
        assert!(rows_to_stocks(&rows).is_err());
    }
}
