//! EDINET の書類（type=1 の zip）に入っている iXBRL（XHTML）を読む。
//!
//! - 本文の HTML を「表の外の文章」と「表（rowspan / colspan を展開した格子）」に分ける（domain::reit が読む）
//! - 財務諸表の数値のタグ（ix:nonFraction）と、表紙の証券コード（ix:nonNumeric）を読む
//!
//! XBRL を CSV にした type=5 は、表のタグが消えてセルの文字がつながり、3万字で切れるので表には使えない。

use domain::disclosure::normalize_text;
use domain::reit::Block;
use std::collections::HashMap;
use std::io::Read;
use usecase::PortError;

/// zip の中の本文（XBRL/PublicDoc/*.htm）を、ファイル名の順に（名前, HTML）で返す
pub fn public_docs(zip_bytes: &[u8]) -> Result<Vec<(String, String)>, PortError> {
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(zip_bytes))
        .map_err(|e| PortError(format!("zip: {e}")))?;
    let mut out = vec![];
    for i in 0..zip.len() {
        let mut f = zip
            .by_index(i)
            .map_err(|e| PortError(format!("zip: {e}")))?;
        let name = f.name().to_string();
        if !(name.starts_with("XBRL/PublicDoc/") && name.ends_with(".htm")) {
            continue;
        }
        let mut s = String::new();
        f.read_to_string(&mut s)
            .map_err(|e| PortError(format!("{name}: {e}")))?;
        out.push((name.rsplit('/').next().unwrap_or(&name).to_string(), s));
    }
    if out.is_empty() {
        return Err(PortError("no PublicDoc htm in zip".into()));
    }
    out.sort();
    Ok(out)
}

/// 開始タグ・終了タグ（名前は小文字、属性は名前を小文字にした連想）
struct Tag<'a> {
    name: String,
    closing: bool,
    attrs: Vec<(String, &'a str)>,
}

impl Tag<'_> {
    fn attr(&self, k: &str) -> Option<&str> {
        self.attrs.iter().find(|(n, _)| n == k).map(|(_, v)| *v)
    }
}

fn parse_tag(inner: &str) -> Tag<'_> {
    let closing = inner.starts_with('/');
    let s = inner.trim_start_matches('/').trim_end_matches('/');
    let name_end = s.find(|c: char| c.is_whitespace()).unwrap_or(s.len());
    let name = s[..name_end].to_ascii_lowercase();
    let mut attrs = vec![];
    let mut rest = &s[name_end..];
    while let Some(eq) = rest.find('=') {
        let key = rest[..eq].trim().to_ascii_lowercase();
        let after = rest[eq + 1..].trim_start();
        let (val, next) = match after.chars().next() {
            Some(q @ ('"' | '\'')) => {
                let body = &after[1..];
                let end = body.find(q).unwrap_or(body.len());
                (&body[..end], &body[(end + 1).min(body.len())..])
            }
            _ => {
                let end = after.find(char::is_whitespace).unwrap_or(after.len());
                (&after[..end], &after[end..])
            }
        };
        attrs.push((key, val));
        rest = next;
    }
    Tag {
        name,
        closing,
        attrs,
    }
}

/// &amp; &lt; &#12345; &#x3042; などを戻す
fn decode_entities(s: &str) -> String {
    if !s.contains('&') {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]);
        let after = &rest[i + 1..];
        let Some(semi) = after.find(';').filter(|j| *j <= 10) else {
            out.push('&');
            rest = after;
            continue;
        };
        let ent = &after[..semi];
        let ch = match ent {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            "nbsp" => Some(' '),
            _ if ent.starts_with("#x") || ent.starts_with("#X") => {
                u32::from_str_radix(&ent[2..], 16)
                    .ok()
                    .and_then(char::from_u32)
            }
            _ if ent.starts_with('#') => ent[1..].parse().ok().and_then(char::from_u32),
            _ => None,
        };
        match ch {
            Some(c) => out.push(c),
            None => {
                out.push('&');
                out.push_str(ent);
                out.push(';');
            }
        }
        rest = &after[semi + 1..];
    }
    out.push_str(rest);
    out
}

enum Ev<'a> {
    Tag(Tag<'a>),
    Text(&'a str),
}

/// HTML を走査して、タグと文字を出てくる順に並べる
fn scan(html: &str) -> Vec<Ev<'_>> {
    let mut evs = vec![];
    let mut rest = html;
    while let Some(i) = rest.find('<') {
        if i > 0 {
            evs.push(Ev::Text(&rest[..i]));
        }
        let after = &rest[i + 1..];
        if let Some(c) = after.strip_prefix("!--") {
            rest = c.find("-->").map(|j| &c[j + 3..]).unwrap_or("");
            continue;
        }
        // 属性値の中の '>' を飛ばす
        let mut quote = None;
        let mut end = after.len();
        for (j, ch) in after.char_indices() {
            match (quote, ch) {
                (None, '"' | '\'') => quote = Some(ch),
                (Some(q), _) if ch == q => quote = None,
                (None, '>') => {
                    end = j;
                    break;
                }
                _ => {}
            }
        }
        let inner = &after[..end];
        if !inner.starts_with('!') && !inner.starts_with('?') {
            evs.push(Ev::Tag(parse_tag(inner)));
        }
        rest = after.get(end + 1..).unwrap_or("");
    }
    if !rest.is_empty() {
        evs.push(Ev::Text(rest));
    }
    evs
}

struct RawCell {
    text: String,
    rowspan: usize,
    colspan: usize,
}

/// rowspan / colspan を展開する。結合セルは同じ文字列を各マスに入れ、空の行は捨てる。
/// あわせて、各マスが上の行からの縦の結合の続きかどうかの印を返す
fn grid(rows: Vec<Vec<RawCell>>) -> (Vec<Vec<String>>, Vec<Vec<bool>>) {
    let mut out = vec![];
    let mut flags = vec![];
    // 列 → (文字列, 残りの行数)
    let mut carry: HashMap<usize, (String, usize)> = HashMap::new();
    for r in rows {
        let mut line = vec![];
        let mut cont = vec![];
        let mut col = 0;
        let mut cells = r.into_iter();
        let mut next = cells.next();
        loop {
            if let Some((text, left)) = carry.get(&col).cloned() {
                line.push(text.clone());
                cont.push(true);
                if left > 1 {
                    carry.insert(col, (text, left - 1));
                } else {
                    carry.remove(&col);
                }
                col += 1;
                continue;
            }
            let Some(c) = next.take() else {
                if carry.keys().any(|k| *k >= col) {
                    // 右側にまだ結合セルが残っている
                    line.push(String::new());
                    cont.push(false);
                    col += 1;
                    continue;
                }
                break;
            };
            for k in 0..c.colspan {
                line.push(c.text.clone());
                cont.push(false);
                if c.rowspan > 1 {
                    carry.insert(col + k, (c.text.clone(), c.rowspan - 1));
                }
            }
            col += c.colspan;
            next = cells.next();
        }
        if line.iter().any(|s| !s.is_empty()) {
            out.push(line);
            flags.push(cont);
        }
    }
    (out, flags)
}

/// 本文の HTML を、表の外の文章（表と表の間ごとに1つ）と表に分ける。文字は全角英数字を半角にし、空白をまとめる
pub fn blocks(html: &str) -> Vec<Block> {
    let mut out = vec![];
    let mut buf = String::new();
    // 入れ子の表に備えたスタック。表ごとに行のリスト
    let mut tables: Vec<Vec<Vec<RawCell>>> = vec![];
    let mut cell: Option<RawCell> = None;
    let flush = |buf: &mut String, out: &mut Vec<Block>| {
        let t = normalize_text(&decode_entities(buf));
        if !t.is_empty() {
            out.push(Block::Text(t));
        }
        buf.clear();
    };
    for ev in scan(html) {
        match ev {
            Ev::Text(s) => {
                if let Some(c) = cell.as_mut() {
                    c.text.push_str(s);
                } else if tables.is_empty() {
                    buf.push_str(s);
                }
            }
            Ev::Tag(t) => {
                let span = |k: &str| {
                    t.attr(k)
                        .and_then(|v| v.trim().parse().ok())
                        .filter(|n: &usize| *n >= 1)
                        .unwrap_or(1)
                };
                match (t.name.as_str(), t.closing) {
                    ("table", false) => {
                        flush(&mut buf, &mut out);
                        tables.push(vec![]);
                    }
                    ("table", true) => {
                        if let Some(rows) = tables.pop() {
                            let (rows, flags) = grid(rows);
                            out.push(Block::Table(rows, flags));
                        }
                    }
                    ("tr", false) => {
                        if let Some(rows) = tables.last_mut() {
                            rows.push(vec![]);
                        }
                    }
                    ("td" | "th", false) if !tables.is_empty() => {
                        cell = Some(RawCell {
                            text: String::new(),
                            rowspan: span("rowspan"),
                            colspan: span("colspan"),
                        });
                    }
                    ("td" | "th", true) => {
                        if let (Some(c), Some(rows)) = (cell.take(), tables.last_mut()) {
                            if rows.is_empty() {
                                rows.push(vec![]);
                            }
                            let text = normalize_text(&decode_entities(&c.text));
                            rows.last_mut().unwrap().push(RawCell { text, ..c });
                        }
                    }
                    ("br" | "p" | "div", _) => {
                        if let Some(c) = cell.as_mut() {
                            c.text.push(' ');
                        } else if tables.is_empty() {
                            buf.push(' ');
                        }
                    }
                    _ => {}
                }
            }
        }
    }
    flush(&mut buf, &mut out);
    out
}

/// ix:nonFraction の数値（要素の局所名, contextRef）→ 値。scale と sign を反映する
pub fn facts(html: &str) -> HashMap<(String, String), f64> {
    let mut out = HashMap::new();
    let mut open: Option<(String, String, i32, bool)> = None;
    let mut text = String::new();
    for ev in scan(html) {
        match ev {
            Ev::Tag(t) if t.name == "ix:nonfraction" && !t.closing => {
                let name = t
                    .attr("name")
                    .unwrap_or("")
                    .rsplit(':')
                    .next()
                    .unwrap_or("")
                    .to_string();
                let ctx = t.attr("contextref").unwrap_or("").to_string();
                let scale = t.attr("scale").and_then(|s| s.parse().ok()).unwrap_or(0);
                open = Some((name, ctx, scale, t.attr("sign") == Some("-")));
                text.clear();
            }
            Ev::Tag(t) if t.name == "ix:nonfraction" && t.closing => {
                if let Some((name, ctx, scale, neg)) = open.take() {
                    let digits: String = normalize_text(&text)
                        .chars()
                        .filter(|c| c.is_ascii_digit() || *c == '.')
                        .collect();
                    // "－" などの値なしは 0 として扱う
                    let v: f64 = digits.parse().unwrap_or(0.0) * 10f64.powi(scale);
                    out.insert((name, ctx), if neg { -v } else { v });
                }
            }
            Ev::Text(s) if open.is_some() => text.push_str(s),
            _ => {}
        }
    }
    out
}

/// 当期末の貸借対照表の値（円）。なければ 0
pub fn current_instant(facts: &HashMap<(String, String), f64>, local_name: &str) -> i64 {
    facts
        .iter()
        .find(|((n, c), _)| n == local_name && c.starts_with("CurrentYearInstant"))
        .map(|(_, v)| v.round() as i64)
        .unwrap_or(0)
}

/// 当期末の値のうち、局所名に指定の語のどれかを含むものの合計（銘柄独自の要素名に備える）
pub fn current_instant_like(facts: &HashMap<(String, String), f64>, parts: &[&str]) -> i64 {
    facts
        .iter()
        .filter(|((n, c), _)| {
            c.starts_with("CurrentYearInstant") && parts.iter().any(|p| n.contains(p))
        })
        .map(|(_, v)| v.round() as i64)
        .sum()
}

/// 表紙の ix:nonNumeric（証券コードなど）の文字
pub fn non_numeric(html: &str, local_name: &str) -> Option<String> {
    let mut found = None;
    let mut inside = false;
    let mut text = String::new();
    for ev in scan(html) {
        match ev {
            Ev::Tag(t) if t.name == "ix:nonnumeric" && !t.closing && found.is_none() => {
                inside = t.attr("name").and_then(|n| n.rsplit(':').next()) == Some(local_name);
                text.clear();
            }
            Ev::Tag(t) if t.name == "ix:nonnumeric" && t.closing && inside => {
                found = Some(normalize_text(&decode_entities(&text)));
                inside = false;
            }
            Ev::Text(s) if inside => text.push_str(s),
            _ => {}
        }
    }
    found
}

/// Strict monetary facts for RAG. Unlike the legacy debt parser, missing/dash/nil
/// values never become zero and context dates and currency units are mandatory.
#[derive(Debug, Clone, PartialEq)]
pub struct VerifiedInstantFact {
    pub name: String,
    pub context: String,
    pub value: f64,
    pub raw: String,
    pub location: String,
}

pub fn verified_instant_facts(
    documents: &[(String, String)],
    period_end: domain::Date,
) -> Vec<VerifiedInstantFact> {
    let mut contexts: HashMap<String, (String, bool)> = HashMap::new();
    let mut units: HashMap<String, String> = HashMap::new();
    for (_, html) in documents {
        let mut context: Option<(String, String, bool)> = None;
        let mut instant = false;
        let mut unit: Option<(String, String, bool)> = None;
        let mut measure = false;
        for event in scan(html) {
            match event {
                Ev::Tag(t) => {
                    let local = t.name.rsplit(':').next().unwrap_or("");
                    match (local, t.closing) {
                        ("context", false) => {
                            context =
                                Some((t.attr("id").unwrap_or("").into(), String::new(), false))
                        }
                        ("instant", false) => instant = true,
                        ("instant", true) => instant = false,
                        ("segment" | "scenario" | "explicitmember" | "typedmember", false) => {
                            if let Some(c) = context.as_mut() {
                                c.2 = true;
                            }
                        }
                        ("context", true) => {
                            if let Some((id, date, dimension)) = context.take() {
                                // A contradictory resource definition invalidates the context.
                                contexts
                                    .entry(id)
                                    .and_modify(|old| {
                                        if *old != (date.clone(), dimension) {
                                            old.1 = true;
                                        }
                                    })
                                    .or_insert((date, dimension));
                            }
                        }
                        ("unit", false) => {
                            unit = Some((t.attr("id").unwrap_or("").into(), String::new(), false))
                        }
                        ("divide", false) => {
                            if let Some(u) = unit.as_mut() {
                                u.2 = true;
                            }
                        }
                        ("measure", false) => measure = true,
                        ("measure", true) => measure = false,
                        ("unit", true) => {
                            if let Some((id, value, divided)) = unit.take() {
                                let value = if divided {
                                    String::new()
                                } else {
                                    value.trim().to_string()
                                };
                                units
                                    .entry(id)
                                    .and_modify(|old| {
                                        if *old != value {
                                            old.clear();
                                        }
                                    })
                                    .or_insert(value);
                            }
                        }
                        _ => {}
                    }
                }
                Ev::Text(text) => {
                    if instant {
                        if let Some(c) = context.as_mut() {
                            c.1.push_str(text);
                        }
                    }
                    if measure {
                        if let Some(u) = unit.as_mut() {
                            u.1.push_str(text);
                        }
                    }
                }
            }
        }
    }
    let mut out = vec![];
    for (file, html) in documents {
        // name, context, scale, negative, valid
        let mut open: Option<(String, String, i32, bool, bool)> = None;
        let mut raw = String::new();
        let mut ordinal = 0;
        for event in scan(html) {
            match event {
                Ev::Tag(t) if t.name == "ix:nonfraction" && !t.closing => {
                    ordinal += 1;
                    let name = t
                        .attr("name")
                        .unwrap_or("")
                        .rsplit(':')
                        .next()
                        .unwrap_or("")
                        .to_string();
                    let ctx = t.attr("contextref").unwrap_or("").to_string();
                    let scale = t.attr("scale").unwrap_or("0").parse::<i32>().ok();
                    let format = t
                        .attr("format")
                        .unwrap_or("")
                        .rsplit(':')
                        .next()
                        .unwrap_or("");
                    let currency = units.get(t.attr("unitref").unwrap_or(""));
                    let valid = contexts
                        .get(&ctx)
                        .is_some_and(|(date, dim)| !dim && date.trim() == period_end.to_string())
                        && currency.is_some_and(|u| u == "iso4217:JPY")
                        && scale.is_some_and(|v| (-6..=12).contains(&v))
                        && matches!(format, "" | "numdotdecimal" | "num-dot-decimal")
                        && !matches!(t.attr("xsi:nil"), Some("true" | "1"));
                    open = Some((
                        name,
                        ctx,
                        scale.unwrap_or(0),
                        t.attr("sign") == Some("-"),
                        valid,
                    ));
                    raw.clear();
                }
                Ev::Tag(t) if t.name == "ix:nonfraction" && t.closing => {
                    if let Some((name, context, scale, negative, true)) = open.take() {
                        let normalized = normalize_text(&raw);
                        let digits: String = normalized
                            .chars()
                            .filter(|c| !c.is_whitespace() && *c != ',')
                            .collect();
                        if !digits.is_empty()
                            && digits.chars().all(|c| c.is_ascii_digit() || c == '.')
                        {
                            if let Ok(number) = digits.parse::<f64>() {
                                let value =
                                    number * 10f64.powi(scale) * if negative { -1.0 } else { 1.0 };
                                if value.is_finite() {
                                    out.push(VerifiedInstantFact {
                                        name,
                                        context,
                                        value,
                                        raw: raw.clone(),
                                        location: format!("{file}#nonFraction:{ordinal}"),
                                    });
                                }
                            }
                        }
                    }
                }
                Ev::Text(s) if open.is_some() => raw.push_str(s),
                _ => {}
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tables_with_spans_and_text_between() {
        let html = r#"<html><body><p>⑥　借入金明細表</p><p>（単位：千円）</p>
<table><tr><td rowspan="2">区分</td><td colspan="2">当期末残高</td></tr>
<tr><td>A&amp;B</td><td>１，０００</td></tr>
<tr><td>短期</td><td colspan="2">合計</td></tr></table>
<p>（注１）&#x5909;動金利</p></body></html>"#;
        let b = blocks(html);
        assert_eq!(b[0], Block::Text("⑥ 借入金明細表 (単位:千円)".into()));
        let Block::Table(t, f) = &b[1] else { panic!() };
        assert_eq!(f[1], [true, false, false], "rowspan の続きの印");
        assert_eq!(t[0], ["区分", "当期末残高", "当期末残高"]);
        assert_eq!(
            t[1],
            ["区分", "A&B", "1,000"],
            "rowspan は下の行にも入る。全角は半角に"
        );
        assert_eq!(t[2], ["短期", "合計", "合計"]);
        assert_eq!(b[2], Block::Text("(注1)変動金利".into()));
    }

    #[test]
    fn rowspan_on_the_right() {
        let html = "<table><tr><td>a</td><td rowspan='2'>r</td></tr><tr><td>b</td></tr></table>";
        let Block::Table(t, _) = &blocks(html)[0] else {
            panic!()
        };
        assert_eq!(t[1], ["b", "r"]);
    }

    #[test]
    fn numeric_facts_and_cover() {
        let html = r#"<ix:nonFraction name="jppfs_cor:ShortTermLoansPayable" contextRef="CurrentYearInstant_NonConsolidatedMember" unitRef="JPY" scale="3" decimals="-3" format="ixt:num-dot-decimal">5,000,000</ix:nonFraction>
<ix:nonFraction name="jppfs_cor:ShortTermLoansPayable" contextRef="Prior1YearInstant_NonConsolidatedMember" scale="3">1,000</ix:nonFraction>
<ix:nonFraction name="jppfs_cor:X" contextRef="CurrentYearInstant" sign="-" scale="3">12</ix:nonFraction>
<ix:nonNumeric name="jpdei_cor:SecurityCodeDEI" contextRef="FilingDateInstant">32830</ix:nonNumeric>"#;
        let f = facts(html);
        assert_eq!(current_instant(&f, "ShortTermLoansPayable"), 5_000_000_000);
        assert_eq!(current_instant(&f, "X"), -12_000);
        assert_eq!(current_instant(&f, "Missing"), 0);
        assert_eq!(
            non_numeric(html, "SecurityCodeDEI").as_deref(),
            Some("32830")
        );
    }
    #[test]
    fn strict_facts_require_date_currency_and_real_number() {
        let html = r#"<xbrli:context id="current"><xbrli:period><xbrli:instant>2026-06-30</xbrli:instant></xbrli:period></xbrli:context>
<xbrli:context id="prior"><xbrli:period><xbrli:instant>2025-06-30</xbrli:instant></xbrli:period></xbrli:context>
<xbrli:unit id="JPY"><xbrli:measure>iso4217:JPY</xbrli:measure></xbrli:unit>
<xbrli:unit id="USD"><xbrli:measure>iso4217:USD</xbrli:measure></xbrli:unit>
<ix:nonFraction name="x:Assets" contextRef="current" unitRef="JPY" scale="3">1,000</ix:nonFraction>
<ix:nonFraction name="x:Assets" contextRef="prior" unitRef="JPY">800</ix:nonFraction>
<ix:nonFraction name="x:Assets" contextRef="current" unitRef="USD">900</ix:nonFraction>
<ix:nonFraction name="x:ShortTermLoansPayable" contextRef="current" unitRef="JPY">－</ix:nonFraction>
<ix:nonFraction name="x:Liabilities" contextRef="current" unitRef="JPY" xsi:nil="true">0</ix:nonFraction>
<ix:nonFraction name="x:Zero" contextRef="current" unitRef="JPY">0</ix:nonFraction>"#;
        let facts = verified_instant_facts(
            &[("report.htm".into(), html.into())],
            domain::Date::parse("2026-06-30").unwrap(),
        );
        assert_eq!(facts.len(), 2);
        assert_eq!(facts[0].name, "Assets");
        assert_eq!(facts[0].value, 1_000_000.0);
        assert_eq!(facts[1].name, "Zero");
        assert_eq!(facts[1].value, 0.0);
    }
    #[test]
    fn contradictory_contexts_are_rejected() {
        let html = r#"<xbrli:context id="same"><xbrli:instant>2026-06-30</xbrli:instant></xbrli:context><xbrli:context id="same"><xbrli:instant>2025-06-30</xbrli:instant></xbrli:context><xbrli:unit id="JPY"><xbrli:measure>iso4217:JPY</xbrli:measure></xbrli:unit><ix:nonFraction name="x:Assets" contextRef="same" unitRef="JPY">100</ix:nonFraction>"#;
        assert!(verified_instant_facts(
            &[("report.htm".into(), html.into())],
            domain::Date::parse("2026-06-30").unwrap()
        )
        .is_empty());
    }
}
