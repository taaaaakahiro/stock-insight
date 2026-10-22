//! REIT（投資法人）の有価証券報告書の表から、借入金の明細と物件ごとの稼働率・賃料を読む。
//!
//! 有報の XBRL で数値のタグが付くのは財務諸表だけで、借入金明細表・物件の一覧は HTML の表しかない。
//! ここでは表を「結合セルを展開した文字列の格子」として受け取り、見出しの言葉で列を探す。
//! 書き方は銘柄ごとに違うので、読めたかどうかは BS の借入金・表の合計行と突き合わせて確かめる。

use crate::Date;

/// iXBRL の本文を、表の外の文章と表に分けたもの（文章は表と表の間ごとに1つ）
#[derive(Debug, Clone, PartialEq)]
pub enum Block {
    Text(String),
    /// 行ごとのセルと、各マスが上の行からの縦の結合（rowspan）の続きかどうか。
    /// rowspan / colspan は展開済みで、結合セルは同じ文字列が各マスに入る。印の表は空でもよい（すべて false）
    Table(Vec<Vec<String>>, Vec<Vec<bool>>),
}

/// 表のセル（行ごと）
pub type Grid = Vec<Vec<String>>;
/// 各マスが縦の結合の続きかどうか
pub type SpanFlags = Vec<Vec<bool>>;
/// 表とその結合の印
pub type TableRef<'a> = (&'a Grid, &'a SpanFlags);

/// 上の行から続く縦の結合セルか（協調融資で1つの金額が複数の借入先にまたがるときなど）
fn carried(flags: &[Vec<bool>], row: usize, col: Option<usize>) -> bool {
    col.and_then(|c| flags.get(row)?.get(c).copied())
        .unwrap_or(false)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoanKind {
    Short,
    /// 1年内返済予定の長期借入金を含む
    Long,
}

/// 金利の種類。分からないものは None（記載なし）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RateType {
    /// 固定金利と明記
    Fixed,
    /// 変動金利だが金利スワップで実質固定と明記
    SwapFixed,
    /// 変動金利と明記
    Floating,
}

impl RateType {
    /// 画面に出す名前
    pub fn label(self) -> &'static str {
        match self {
            RateType::Fixed => "固定",
            RateType::SwapFixed => "スワップで固定",
            RateType::Floating => "変動",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Loan {
    pub kind: Option<LoanKind>,
    pub lender: String,
    pub balance_yen: i64,
    pub rate_pct: Option<f64>,
    pub due: Option<Date>,
    pub rate_type: Option<RateType>,
}

/// 借入金明細表と、その周りの注記・返済予定・金利スワップ
#[derive(Debug, Clone, PartialEq, Default)]
pub struct DebtSchedule {
    pub loans: Vec<Loan>,
    /// 明細表の「合計」行の当期末残高（読めた件数の確認用）
    pub table_total_yen: Option<i64>,
    /// 金利スワップ・キャップの契約額等（当期末）
    pub swap_yen: Option<i64>,
    /// 返済予定額（1年以内, 1〜2年, 2〜3年, 3〜4年, 4〜5年, 5年超）。科目ごと
    pub ladder: Vec<(String, [i64; 6])>,
    /// 金利の種類を言っている注記（"注4: floating" など。確認用）
    pub rate_notes: Vec<String>,
}

/// 有利子負債の金利の内訳（円）
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct RateMix {
    pub fixed: i64,
    /// 借入ごとにスワップで固定と書かれたもの
    pub swap_fixed: i64,
    /// スワップの契約額のうち、どの借入か書かれていない分（記載なしの借入から差し引く）
    pub swap_amount_only: i64,
    pub floating: i64,
    pub unknown: i64,
    /// 投資法人債（ほぼ固定だが、明記はされていない）
    pub bonds: i64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Property {
    pub id: String,
    pub name: String,
    pub leasable_m2: f64,
    pub leased_m2: Option<f64>,
    pub occupancy_pct: Option<f64>,
    /// 稼働率の列がなく、面積から計算した
    pub occupancy_computed: bool,
    pub annual_rent_yen: Option<i64>,
    /// 賃料の欄の文字（"非開示 (注11)" など。数値でないときの確認用）
    pub rent_raw: String,
}

/// 物件一覧の「合計」行（確認用）
#[derive(Debug, Clone, PartialEq)]
pub struct PropertyTotal {
    pub leasable_m2: f64,
    pub leased_m2: Option<f64>,
}

// ---- 文字列の小道具 ----

pub fn nospace(s: &str) -> String {
    s.chars().filter(|c| !c.is_whitespace()).collect()
}

/// "1,234" "0.495 (注3)" "△5" "98.4%" → 数値。数値でなければ None（"-" "非開示" など）
pub fn to_num(s: &str) -> Option<f64> {
    let mut t = s.replace(['%', '％'], "");
    for p in ["(注", "（注"] {
        if let Some(i) = t.find(p) {
            t.truncate(i);
        }
    }
    let t = t.trim();
    let first = t.chars().next()?;
    let (neg, body) = if matches!(first, '△' | '▲' | '-' | '−') {
        (true, &t[first.len_utf8()..])
    } else {
        (false, t)
    };
    let (int, frac) = match body.split_once('.') {
        Some((i, f)) => (i, Some(f)),
        None => (body, None),
    };
    if int.is_empty() || !int.starts_with(|c: char| c.is_ascii_digit()) {
        return None;
    }
    if let Some(f) = frac {
        if f.is_empty() || !f.chars().all(|c| c.is_ascii_digit()) {
            return None;
        }
    }
    if int.contains(',') {
        let mut groups = int.split(',');
        let head = groups.next()?;
        if head.is_empty() || head.len() > 3 || !head.chars().all(|c| c.is_ascii_digit()) {
            return None;
        }
        if !groups.all(|g| g.len() == 3 && g.chars().all(|c| c.is_ascii_digit())) {
            return None;
        }
    } else if !int.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let v: f64 = format!("{}.{}", int.replace(',', ""), frac.unwrap_or("0"))
        .parse()
        .ok()?;
    Some(if neg { -v } else { v })
}

/// セルの中で最初に数値として読める語（"変動金利 1.25018%" → 1.25018）
fn first_num(s: &str) -> Option<f64> {
    to_num(s).or_else(|| s.split_whitespace().find_map(to_num))
}

/// "（単位：千円）" など → 円への倍率
pub fn unit_of(s: &str) -> Option<i64> {
    if s.contains("百万円") {
        Some(1_000_000)
    } else if s.contains("千円") {
        Some(1_000)
    } else {
        None
    }
}

/// "1,234,000" のような桁区切りの整数だけのセル
fn is_grouped_int(c: &str) -> bool {
    c.contains(',')
        && !c.contains('.')
        && to_num(c).is_some()
        && c.chars().all(|ch| ch.is_ascii_digit() || ch == ',')
}

fn is_decimal(c: &str) -> bool {
    c.contains('.')
        && c.chars()
            .all(|ch| ch.is_ascii_digit() || ch == ',' || ch == '.')
        && to_num(c).is_some()
}

fn is_data_row(r: &[String]) -> bool {
    r.iter().any(|c| is_grouped_int(c))
}

/// "2026年 12月2日" "2029.6.1" "2026/6/22" → 日付
pub fn parse_jp_date(s: &str) -> Option<Date> {
    let t = nospace(s);
    let nums: Vec<(usize, u32)> = {
        let mut out = vec![];
        let mut cur = String::new();
        let mut start = 0;
        for (i, c) in t.char_indices() {
            if c.is_ascii_digit() {
                if cur.is_empty() {
                    start = i;
                }
                cur.push(c);
            } else if !cur.is_empty() {
                out.push((start, cur.parse().ok()?));
                cur.clear();
            }
        }
        if !cur.is_empty() {
            out.push((start, cur.parse().ok()?));
        }
        out
    };
    for w in nums.windows(3) {
        let (y, m, d) = (w[0].1, w[1].1, w[2].1);
        let sep = |a: (usize, u32), b: (usize, u32)| {
            t[a.0..b.0]
                .trim_start_matches(|c: char| c.is_ascii_digit())
                .to_string()
        };
        let ok = (1990..2100).contains(&y)
            && (1..=12).contains(&m)
            && (1..=31).contains(&d)
            && matches!(
                (sep(w[0], w[1]).as_str(), sep(w[1], w[2]).as_str()),
                ("年", "月") | (".", ".") | ("/", "/")
            );
        if ok {
            return Some(Date::new(y as i32, m, d));
        }
    }
    None
}

/// "(注3)" "（注2、3）" "(注1)(注4)" に出てくる注の番号
fn note_refs(s: &str) -> Vec<u32> {
    let mut out = vec![];
    let mut rest = s;
    while let Some(i) = rest.find('注') {
        let after = &rest[i + '注'.len_utf8()..];
        let mut cur = String::new();
        for c in after.chars() {
            match c {
                '0'..='9' => cur.push(c),
                '、' | ',' | '・' => {
                    if let Ok(n) = cur.parse() {
                        out.push(n);
                    }
                    cur.clear();
                }
                _ => break,
            }
        }
        if let Ok(n) = cur.parse() {
            out.push(n);
        }
        rest = after;
    }
    out
}

/// 見出しの行を列ごとに縦につなげ、空白を除いた文字列
fn header_cols(hdr: &[Vec<String>]) -> Vec<String> {
    let width = hdr.iter().map(|r| r.len()).max().unwrap_or(0);
    (0..width)
        .map(|i| {
            nospace(
                &hdr.iter()
                    .filter_map(|r| r.get(i))
                    .cloned()
                    .collect::<String>(),
            )
        })
        .collect()
}

fn find_col(cols: &[String], keys: &[&str], avoid: &[&str]) -> Option<usize> {
    cols.iter()
        .position(|c| keys.iter().any(|k| c.contains(k)) && !avoid.iter().any(|a| c.contains(a)))
}

fn cell(r: &[String], i: Option<usize>) -> &str {
    i.and_then(|i| r.get(i)).map(|s| s.as_str()).unwrap_or("")
}

fn yen(v: f64, unit: i64) -> i64 {
    (v * unit as f64).round() as i64
}

// ---- 借入金 ----

/// 区分のセルの文字から短期・長期を見分ける。縦書きのセルは文字の順番が崩れる
/// （「1年内返済予定の長期借入金」が「一の 年長 内期 返借 済入 予金 定」になる）ので、文字の集まりでも見る
fn loan_kind_of(cell: &str) -> Option<LoanKind> {
    let t = nospace(cell);
    let has = |cs: &str| cs.chars().all(|c| t.contains(c));
    if t.contains("短期借入金") {
        Some(LoanKind::Short)
    } else if t.contains("長期借入金") || (t.chars().count() <= 20 && has("返済予定長期"))
    {
        Some(LoanKind::Long)
    } else if t.chars().count() <= 12 && has("短期借入金") && !t.contains('長') {
        Some(LoanKind::Short)
    } else {
        None
    }
}

struct LoanCols {
    end: Option<usize>,
    rate: Option<usize>,
    due: Option<usize>,
    lender: Option<usize>,
    kind: Option<usize>,
    note: Option<usize>,
    usage: Option<usize>,
}

fn loan_cols(cols: &[String]) -> LoanCols {
    LoanCols {
        end: find_col(cols, &["当期末"], &[]),
        rate: find_col(cols, &["利率"], &[]),
        due: find_col(cols, &["返済期限", "返済期日", "元本弁済期日"], &[]),
        lender: find_col(cols, &["借入先"], &[]),
        kind: find_col(cols, &["区分"], &[]),
        note: find_col(cols, &["摘要"], &[]),
        usage: find_col(cols, &["使途"], &[]),
    }
}

fn is_loan_heading(text: &str) -> bool {
    let tail: String = text
        .chars()
        .rev()
        .take(200)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    tail.contains("借入金明細") || tail.contains("借入金等明細")
}

/// 附属明細表の借入金明細表（見出しの文の最後の出現）に続く表と、その後の注記と単位（調べる用に公開）
pub fn loan_tables(blocks: &[Block]) -> (Vec<TableRef<'_>>, String, Option<i64>) {
    let Some(start) = blocks
        .iter()
        .rposition(|b| matches!(b, Block::Text(t) if is_loan_heading(t)))
    else {
        return (vec![], String::new(), None);
    };
    let Block::Text(head) = &blocks[start] else {
        unreachable!()
    };
    let tail: String = head
        .chars()
        .rev()
        .take(60)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    let mut unit = unit_of(&tail);
    let mut tables = vec![];
    let mut notes = String::new();
    for b in &blocks[start + 1..] {
        match b {
            Block::Table(t, f) => tables.push((t, f)),
            Block::Text(t)
                if t.chars().count() < 40 && !t.starts_with("(注") && !t.starts_with("（注") =>
            {
                unit = unit.or(unit_of(t));
            }
            Block::Text(t) => {
                notes = t.clone();
                break;
            }
        }
    }
    (tables, notes, unit)
}

/// 注記の番号ごとの金利の種類と、「その他は全て固定」のような言い回し
struct RateNotes {
    by_note: Vec<(u32, RateType)>,
    /// 番号の付いた借入以外は全て固定（日本ビルファンドの書き方）
    rest_fixed: bool,
    /// 文書全体で「借入金は全て変動金利」「全て固定金利」と言っている
    whole: Option<RateType>,
}

fn rate_notes(notes: &str, doc_text: &str) -> RateNotes {
    let mut by_note = vec![];
    let mut rest_fixed = false;
    // "(注3) 本文 (注4) 本文" を番号ごとに切る
    let mut marks: Vec<(usize, usize, u32)> = vec![];
    for open in ["(注", "（注"] {
        let mut from = 0;
        while let Some(i) = notes[from..].find(open) {
            let s = from + i;
            let after = &notes[s + open.len()..];
            let digits: String = after.chars().take_while(|c| c.is_ascii_digit()).collect();
            let rest = &after[digits.len()..];
            if !digits.is_empty() && (rest.starts_with(')') || rest.starts_with('）')) {
                let close = rest.chars().next().unwrap().len_utf8();
                marks.push((
                    s,
                    s + open.len() + digits.len() + close,
                    digits.parse().unwrap(),
                ));
            }
            from = s + open.len();
        }
    }
    marks.sort();
    for (k, &(_, body_start, n)) in marks.iter().enumerate() {
        let body_end = marks.get(k + 1).map(|m| m.0).unwrap_or(notes.len());
        let body = &notes[body_start..body_end];
        let head: String = body.chars().take(30).collect();
        let t = if body.contains("スワップ") && body.contains("固定") {
            Some(RateType::SwapFixed)
        } else if body.contains("固定金利") && !head.contains("平均利率") {
            Some(RateType::Fixed)
        } else if body.contains("変動金利") && !head.contains("平均利率") {
            Some(RateType::Floating)
        } else {
            None
        };
        if let Some(t) = t {
            by_note.push((n, t));
        }
    }
    // 日本ビルファンド: 「４．変動金利による借入れです。その他は、全て固定金利による借入れです。」
    for (i, _) in notes.match_indices("変動金利による借入") {
        let before = notes[..i].trim_end();
        let before = before.strip_suffix('.').or(before.strip_suffix('．'));
        if let Some(b) = before {
            let digits: String = b
                .chars()
                .rev()
                .take_while(|c| c.is_ascii_digit())
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect();
            if let Ok(n) = digits.parse() {
                by_note.push((n, RateType::Floating));
                rest_fixed = notes[i..].contains("その他") && notes[i..].contains("全て固定金利");
            }
        }
    }
    let all = format!("{notes} {doc_text}");
    let says = |kind: &str| {
        [
            "借入金は全て",
            "借入金は、全て",
            "これらは全て",
            "これらは、全て",
        ]
        .iter()
        .any(|p| all.contains(&format!("{p}{kind}")))
    };
    let whole = if says("変動金利") {
        Some(RateType::Floating)
    } else if says("固定金利") {
        Some(RateType::Fixed)
    } else {
        None
    };
    RateNotes {
        by_note,
        rest_fixed,
        whole,
    }
}

fn parse_loans(
    tables: &[TableRef<'_>],
    unit: Option<i64>,
    notes: &RateNotes,
) -> (Vec<Loan>, Option<i64>) {
    let mut loans = vec![];
    let mut cols: Option<(Vec<String>, LoanCols)> = None;
    let mut kind = None;
    let mut total = None;
    for (t, flags) in tables {
        // 見出しの行は数値のセルがない行（最初の借入が "300" のように桁区切りのない額でも借入の行とみなす）
        let n_hdr = t
            .iter()
            .take_while(|r| r.iter().all(|c| to_num(c).is_none()))
            .count();
        if n_hdr > 0 {
            let hc = header_cols(&t[..n_hdr]);
            let c = loan_cols(&hc);
            if c.end.is_some() {
                cols = Some((hc, c));
            }
        }
        let Some((hc, c)) = &mut cols else { continue };
        // 「借入先」の見出しが複数の列にまたがるとき（「区分(借入先)」や colspan）は、借入先らしい文字
        // （数値でなく「借入金」「計」を含まない）がいちばん多く入っている列を使う
        let cands: Vec<usize> = (0..hc.len())
            .filter(|i| hc[*i].contains("借入先"))
            .collect();
        if cands.len() > 1 {
            let score = |i: usize| {
                t.iter()
                    .skip(n_hdr)
                    .filter(|r| {
                        let v = nospace(cell(r, Some(i)));
                        !v.is_empty()
                            && to_num(&v).is_none()
                            && !v.contains("借入金")
                            && !v.contains('計')
                    })
                    .count()
            };
            if let Some(best) = cands
                .iter()
                .copied()
                .max_by_key(|i| (score(*i), std::cmp::Reverse(*i)))
            {
                c.lender = Some(best);
            }
        }
        let u = unit.or(unit_of(&hc.join(" "))).unwrap_or(1_000);
        for (ri, r) in t.iter().enumerate().skip(n_hdr) {
            let lender = cell(r, c.lender);
            // 協調融資で金額のセルが複数の借入先にまたがるときは、最初の行でだけ数える
            let end = to_num(cell(r, c.end)).filter(|_| !carried(flags, ri, c.end));
            // 区分は列がずれることがあるので、行のどこかに「短期借入金」「長期借入金」があれば以降の行に引き継ぐ。
            // 借入先の列は、額のない見出しの行（「長期借入金(注1)」だけの行）のときだけ見る
            if let Some(k) = r
                .iter()
                .enumerate()
                .filter(|(i, _)| Some(*i) != c.lender || end.is_none())
                .find_map(|(_, s)| loan_kind_of(s))
            {
                kind = Some(k);
            }
            let ln = nospace(lender);
            if let Some(e) = end {
                if ln == "合計" || nospace(cell(r, Some(0))).contains("合計") {
                    total = Some(yen(e, u));
                }
            }
            if ln.contains("小計") && kind == Some(LoanKind::Short) {
                kind = Some(LoanKind::Long); // 短期の小計の後は長期（区分の列が空の表がある）
            }
            let Some(end) = end else { continue };
            if ln.is_empty() || ln.contains("合計") || ln.contains("小計") || ln.ends_with('計')
            {
                continue;
            }
            let marks = [
                cell(r, c.rate),
                cell(r, c.note),
                cell(r, c.usage),
                lender,
                cell(r, c.kind),
            ];
            let mut refs: Vec<u32> = marks.iter().flat_map(|m| note_refs(m)).collect();
            // 注の番号だけが書かれたセル（"4"）
            refs.extend(marks.iter().filter_map(|m| m.trim().parse::<u32>().ok()));
            let mut rate_type = notes.whole;
            // 利率・摘要の欄に「固定金利」「変動金利」と書く銘柄がある（オリックス不動産）
            let cell_txt = nospace(&marks.join(" "));
            if cell_txt.contains("変動金利") {
                rate_type = Some(RateType::Floating);
            } else if cell_txt.contains("固定金利") {
                rate_type = Some(RateType::Fixed);
            }
            for (n, t) in &notes.by_note {
                if refs.contains(n) {
                    rate_type = Some(*t);
                }
            }
            if rate_type.is_none() && notes.rest_fixed {
                rate_type = Some(RateType::Fixed);
            }
            loans.push(Loan {
                kind,
                lender: lender.to_string(),
                balance_yen: yen(end, u),
                rate_pct: first_num(cell(r, c.rate)),
                due: parse_jp_date(cell(r, c.due)),
                rate_type,
            });
        }
    }
    (loans, total)
}

/// 金融商品の注記の返済予定表（1年以内 … 5年超）のうち最後のもの（当期）
fn ladder(blocks: &[Block]) -> Vec<(String, [i64; 6])> {
    let mut best = None;
    let mut prev_text = "";
    for b in blocks {
        match b {
            Block::Text(t) => prev_text = t,
            Block::Table(t, _) => {
                let hdr: String = t.iter().take(2).flatten().map(|c| nospace(c)).collect();
                let body: String = t
                    .iter()
                    .filter_map(|r| r.first())
                    .map(|c| nospace(c))
                    .collect();
                if hdr.contains("1年以内")
                    && hdr.contains("5年超")
                    && (body.contains("借入金") || body.contains("投資法人債"))
                {
                    let tail: String = prev_text
                        .chars()
                        .rev()
                        .take(80)
                        .collect::<Vec<_>>()
                        .into_iter()
                        .rev()
                        .collect();
                    best = Some((t, unit_of(&format!("{tail}{hdr}")).unwrap_or(1_000)));
                }
            }
        }
    }
    let Some((t, u)) = best else { return vec![] };
    t.iter()
        .skip(1)
        .filter_map(|r| {
            let name = nospace(r.first()?);
            let nums: Vec<f64> = r.iter().skip(1).map(|c| to_num(c).unwrap_or(0.0)).collect();
            if name.is_empty()
                || name.contains("合計")
                || nums.len() < 6
                || nums.iter().all(|v| *v == 0.0)
            {
                return None;
            }
            let last6 = &nums[nums.len() - 6..];
            let mut a = [0i64; 6];
            for (i, v) in last6.iter().enumerate() {
                a[i] = yen(*v, u);
            }
            Some((name, a))
        })
        .collect()
}

/// 金利スワップ（・キャップ）の契約額等。附属明細表の「デリバティブ取引…の状況表」を優先し、なければ注記の最後の表
fn swaps(blocks: &[Block]) -> Option<i64> {
    let mut cand = vec![];
    let mut prev_text = "";
    for b in blocks {
        match b {
            Block::Text(t) => prev_text = t,
            Block::Table(t, _) => {
                let head: String = t.iter().take(2).flatten().map(|c| nospace(c)).collect();
                if !head.contains("契約額") {
                    continue;
                }
                let mut total = 0.0;
                for r in t {
                    if r.iter()
                        .any(|c| c.contains("金利スワップ") || c.contains("金利キャップ"))
                    {
                        if let Some(v) = r.iter().find_map(|c| to_num(c)) {
                            total += v;
                        }
                    }
                }
                let tail: String = prev_text
                    .chars()
                    .rev()
                    .take(200)
                    .collect::<Vec<_>>()
                    .into_iter()
                    .rev()
                    .collect();
                let tail80: String = tail
                    .chars()
                    .rev()
                    .take(80)
                    .collect::<Vec<_>>()
                    .into_iter()
                    .rev()
                    .collect();
                let unit = unit_of(&format!("{tail80}{head}")).unwrap_or(1_000);
                cand.push((yen(total, unit), tail.contains("状況表")));
            }
        }
    }
    let preferred: Vec<_> = cand.iter().filter(|c| c.1).collect();
    preferred
        .last()
        .or(cand.iter().collect::<Vec<_>>().last())
        .map(|c| c.0)
}

/// 財務諸表の部分（有報の「投資法人の経理状況」）の文章と表から借入金を読む
pub fn parse_debt(blocks: &[Block]) -> DebtSchedule {
    let (tables, notes_text, unit) = loan_tables(blocks);
    let doc_text: String = blocks
        .iter()
        .filter_map(|b| match b {
            Block::Text(t) => Some(t.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join(" ");
    let notes = rate_notes(&notes_text, &doc_text);
    let (loans, table_total_yen) = parse_loans(&tables, unit, &notes);
    let mut rate_notes: Vec<String> = notes
        .by_note
        .iter()
        .map(|(n, t)| format!("注{n}: {}", t.label()))
        .collect();
    if notes.rest_fixed {
        rate_notes.push("その他は全て固定".into());
    }
    if let Some(w) = notes.whole {
        rate_notes.push(format!("全て{}", w.label()));
    }
    DebtSchedule {
        loans,
        table_total_yen,
        swap_yen: swaps(blocks),
        ladder: ladder(blocks),
        rate_notes,
    }
}

/// 有利子負債の金利の内訳。どの借入か書かれていないスワップの契約額は、記載なしの借入のうち
/// スワップで固定された額とみなす（上限は記載なしの額）
pub fn rate_mix(debt: &DebtSchedule, bonds_yen: i64) -> RateMix {
    let sum = |t: Option<RateType>| {
        debt.loans
            .iter()
            .filter(|l| l.rate_type == t)
            .map(|l| l.balance_yen)
            .sum::<i64>()
    };
    let swap_fixed = sum(Some(RateType::SwapFixed));
    let unknown = sum(None);
    let swap_amount_only = (debt.swap_yen.unwrap_or(0) - swap_fixed).clamp(0, unknown);
    RateMix {
        fixed: sum(Some(RateType::Fixed)),
        swap_fixed,
        swap_amount_only,
        floating: sum(Some(RateType::Floating)),
        unknown: unknown - swap_amount_only,
        bonds: bonds_yen,
    }
}

// ---- 物件 ----

struct PropCols {
    id: Option<usize>,
    name: Option<usize>,
    leasable: Option<usize>,
    leased: Option<usize>,
    occ: Option<usize>,
    rent: Option<usize>,
}

fn prop_cols(cols: &[String]) -> PropCols {
    PropCols {
        id: find_col(cols, &["物件番号", "番号"], &[]),
        name: find_col(cols, &["名称", "物件名"], &[]),
        leasable: find_col(cols, &["賃貸可能面積"], &[]),
        leased: find_col(cols, &["賃貸面積"], &["可能"]),
        occ: find_col(cols, &["稼働率", "入居率"], &[]),
        rent: find_col(
            cols,
            &[
                "年間賃料",
                "賃貸事業収入",
                "総賃貸収入",
                "賃料収入",
                "不動産賃貸事業収益",
            ],
            &[],
        ),
    }
}

/// 期ごとの推移表（第11期 … 第15期、2025年11月 …）の見出しか。同じ期の「時点」と「期間」の2列は推移ではない
fn is_history_header(cols: &[String]) -> bool {
    let mut periods = std::collections::BTreeSet::new();
    let mut months = std::collections::BTreeSet::new();
    for c in cols {
        if let Some(i) = c.find('第') {
            let n: String = c[i + '第'.len_utf8()..]
                .chars()
                .take_while(|ch| ch.is_ascii_digit())
                .collect();
            if !n.is_empty() && c[i..].contains('期') {
                periods.insert(n);
            }
        }
        if let Some(d) = parse_year_month(c) {
            months.insert(d);
        }
    }
    periods.len() >= 3 || months.len() >= 3
}

/// "2025年11月" → (2025, 11)
fn parse_year_month(s: &str) -> Option<(u32, u32)> {
    let i = s.find('年')?;
    let y: String = s[..i]
        .chars()
        .rev()
        .take_while(|c| c.is_ascii_digit())
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    let m: String = s[i + '年'.len_utf8()..]
        .chars()
        .take_while(|c| c.is_ascii_digit())
        .collect();
    let rest = &s[i + '年'.len_utf8() + m.len()..];
    (y.len() == 4 && !m.is_empty() && rest.starts_with('月'))
        .then(|| (y.parse().ok(), m.parse().ok()))
        .and_then(|(a, b)| Some((a?, b?)))
}

/// 物件が列に、項目（名称・賃貸可能面積・稼働率…）が行に並ぶ表（日本プライムリアルティ）を、
/// 物件が行に並ぶ形に置き換える。そうでなければ None
fn transpose_if_sideways(t: &[Vec<String>]) -> Option<Vec<Vec<String>>> {
    let name_row = t
        .iter()
        .position(|r| r.first().is_some_and(|c| nospace(c).contains("名称")))?;
    let k = t[name_row]
        .iter()
        .take_while(|c| nospace(c).contains("名称"))
        .count();
    let width = t.iter().map(|r| r.len()).max()?;
    let label = |r: &Vec<String>| nospace(&r.iter().take(k).cloned().collect::<Vec<_>>().join(""));
    if width < k + 2 || !t.iter().any(|r| label(r).contains("賃貸可能面積")) {
        return None;
    }
    let mut out = vec![t.iter().map(label).collect::<Vec<_>>()];
    for j in k..width {
        out.push(
            t.iter()
                .map(|r| r.get(j).cloned().unwrap_or_default())
                .collect(),
        );
    }
    Some(out)
}

fn is_header_end(r: &[String]) -> bool {
    is_data_row(r) || r.iter().any(|c| is_decimal(c))
}

/// 物件の一覧の表（見出しに 名称 と 賃貸可能面積 を含む）。見出しのない続きの表は前の見出しを使う。
/// 縦横逆の表は置き換えてから使う
fn prop_tables(blocks: &[Block]) -> Vec<(Vec<String>, PropCols, Grid, SpanFlags)> {
    let mut out = vec![];
    let mut last: Option<Vec<String>> = None;
    for b in blocks {
        let (t, flags) = match b {
            Block::Text(t) => {
                if t.chars().count() > 60 {
                    last = None; // 長い文章を挟んだら続きの表ではない
                }
                continue;
            }
            Block::Table(t, f) => match transpose_if_sideways(t) {
                Some(tt) => (std::borrow::Cow::Owned(tt), std::borrow::Cow::Owned(vec![])),
                None => (std::borrow::Cow::Borrowed(t), std::borrow::Cow::Borrowed(f)),
            },
        };
        let n_hdr = t.iter().take_while(|r| !is_header_end(r)).count();
        if n_hdr > 0 {
            let hc = header_cols(&t[..n_hdr]);
            let c = prop_cols(&hc);
            if c.leasable.is_some() && c.name.is_some() {
                if is_history_header(&hc) {
                    last = None;
                    continue;
                }
                last = Some(hc);
            } else if c.occ.is_some() || c.leasable.is_some() {
                last = None;
            }
        }
        let Some(hc) = &last else { continue };
        if n_hdr == 0 && t.iter().map(|r| r.len()).max().unwrap_or(0) != hc.len() {
            last = None; // 列数の違う表は続きではない
            continue;
        }
        let from = n_hdr.min(t.len());
        out.push((
            hc.clone(),
            prop_cols(hc),
            t[from..].to_vec(),
            flags.get(from..).unwrap_or(&[]).to_vec(),
        ));
    }
    out
}

fn prop_key(name: &str) -> String {
    let mut n = nospace(name);
    for p in ["(注", "（注"] {
        if let Some(i) = n.find(p) {
            n.truncate(i);
        }
    }
    n.chars()
        .filter(|c| !matches!(c, '〈' | '〉' | '<' | '>'))
        .collect()
}

/// 物件ごとに覚えておくもの: props の位置, 表の番号, 足した行の面積（賃貸可能, 賃貸）
type Seen = (usize, usize, Vec<(f64, Option<f64>)>);

/// 「ファンドの状況」の部分の文章と表から物件の一覧を読む
pub fn parse_properties(blocks: &[Block]) -> (Vec<Property>, Option<PropertyTotal>) {
    let mut props: Vec<Property> = vec![];
    // 物件のキー → (props の位置, 表の番号, 足した行の面積)
    let mut seen: std::collections::HashMap<String, Seen> = Default::default();
    let mut total = None;
    for (ti, (hc, c, rows, flags)) in prop_tables(blocks).into_iter().enumerate() {
        let rent_unit = c.rent.and_then(|i| unit_of(&hc[i]));
        for (ri, r) in rows.iter().enumerate() {
            if carried(&flags, ri, c.leasable) {
                continue; // 1つの物件が縦に結合された複数の行にまたがる
            }
            let name = cell(r, c.name);
            // 底地は「48,932.00 (土地)」のように書かれる
            let leasable = first_num(cell(r, c.leasable));
            let leased = first_num(cell(r, c.leased));
            let occ = first_num(cell(r, c.occ));
            let nn = nospace(name);
            // 合計・小計（「物流施設 64物件 計」「コアアセット中計(41物件)」のような区分ごとの計を含む）・平均の行は物件ではない。
            // 合計行は、平均を除いた小計・合計のうち面積がいちばん大きいもの
            if nn.ends_with('計')
                || [
                    "合計",
                    "小計",
                    "中計",
                    "総計",
                    "計(",
                    "平均",
                    "ポートフォリオ",
                ]
                .iter()
                .any(|k| nn.contains(k))
            {
                let is_avg = nn.contains("平均") && !nn.contains('計');
                if let (Some(l), false) = (leasable, is_avg) {
                    if total
                        .as_ref()
                        .is_none_or(|t: &PropertyTotal| l > t.leasable_m2)
                    {
                        total = Some(PropertyTotal {
                            leasable_m2: l,
                            leased_m2: leased,
                        });
                    }
                }
                continue;
            }
            let Some(leasable) = leasable else { continue };
            if name.is_empty() {
                continue;
            }
            // 稼働率（%）の推移表の行を面積と取り違えない
            if leasable <= 100.0
                && (occ.is_some_and(|o| o <= 100.0) || leased.is_some_and(|l| l <= 100.0))
            {
                continue;
            }
            let id = cell(r, c.id).to_string();
            let key = prop_key(name);
            let hit = seen
                .get(&key)
                .or_else(|| (!id.is_empty()).then(|| seen.get(&id)).flatten())
                .cloned();
            if let Some((pi, pt, rows_seen)) = hit {
                // 別の表に同じ物件があれば、足りない項目（面積だけの表のあとの稼働率・賃料など）を埋める
                if pt != ti {
                    let p = &mut props[pi];
                    if p.leased_m2.is_none() {
                        p.leased_m2 = leased;
                    }
                    if p.occupancy_pct.is_none() || p.occupancy_computed {
                        if let Some(o) = occ {
                            p.occupancy_pct = Some(o);
                            p.occupancy_computed = false;
                        } else if let (None, Some(l)) = (p.occupancy_pct, p.leased_m2) {
                            p.occupancy_pct = Some((l / p.leasable_m2 * 1000.0).round() / 10.0);
                            p.occupancy_computed = true;
                        }
                    }
                    if p.annual_rent_yen.is_none() {
                        let raw = cell(r, c.rent);
                        if let Some(v) = rent_unit.and_then(|u| to_num(raw).map(|v| yen(v, u))) {
                            p.annual_rent_yen = Some(v);
                            p.rent_raw = raw.to_string();
                        }
                    }
                    continue;
                }
                // 同じ表で同じ物件の続きの行に別の面積があれば、建物が複数ある物件として足す
                // （結合セルの重複行は同じ値なので足さない）
                if !rows_seen.contains(&(leasable, leased)) {
                    let p = &mut props[pi];
                    p.leasable_m2 += leasable;
                    p.leased_m2 = Some(p.leased_m2.unwrap_or(0.0) + leased.unwrap_or(0.0));
                    p.occupancy_pct = p
                        .leased_m2
                        .map(|l| (l / p.leasable_m2 * 1000.0).round() / 10.0);
                    p.occupancy_computed = true;
                    for k in [&key, &id] {
                        if let Some(e) = seen.get_mut(k) {
                            e.2.push((leasable, leased));
                        }
                    }
                }
                continue;
            }
            let (occupancy_pct, occupancy_computed) = match (occ, leased) {
                (Some(o), _) => (Some(o), false),
                (None, Some(l)) => (Some((l / leasable * 1000.0).round() / 10.0), true),
                _ => (None, false),
            };
            let rent_raw = cell(r, c.rent).to_string();
            let entry = (props.len(), ti, vec![(leasable, leased)]);
            seen.insert(key.clone(), entry.clone());
            if !id.is_empty() {
                seen.insert(id.clone(), entry);
            }
            props.push(Property {
                id,
                name: name.to_string(),
                leasable_m2: leasable,
                leased_m2: leased,
                occupancy_pct,
                occupancy_computed,
                annual_rent_yen: rent_unit.and_then(|u| to_num(&rent_raw).map(|v| yen(v, u))),
                rent_raw,
            });
        }
    }
    (props, total)
}

// ---- 一覧に出す要約 ----

/// 借入金の要約（一覧で並べ替える数字）
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct DebtSummary {
    /// 残高加重の平均利率（%）
    pub avg_rate_pct: Option<f64>,
    /// 残高加重の平均残存年数（期末日から返済期限まで）
    pub avg_years: Option<f64>,
    /// 1年以内に金利が見直される額 = 変動（明記）＋ 1年以内に返済期限が来る借入（借り換えで新しい金利になる）
    /// ＋ 1年以内に償還が来る投資法人債。記載なしの借入は含めない（下限）
    pub reprice_1y: i64,
    /// 上の額に、記載なしの借入（スワップ額のみの分を除く）を全部変動とみなして足した上限
    pub reprice_1y_max: i64,
}

pub fn debt_summary(debt: &DebtSchedule, mix: &RateMix, period_end: Date) -> DebtSummary {
    let horizon = period_end.add_days(365);
    let with_rate: Vec<&Loan> = debt.loans.iter().filter(|l| l.rate_pct.is_some()).collect();
    let rate_base: i64 = with_rate.iter().map(|l| l.balance_yen).sum();
    let avg_rate_pct = (rate_base > 0).then(|| {
        with_rate
            .iter()
            .map(|l| l.balance_yen as f64 * l.rate_pct.unwrap_or(0.0))
            .sum::<f64>()
            / rate_base as f64
    });
    let with_due: Vec<(&Loan, Date)> = debt
        .loans
        .iter()
        .filter_map(|l| Some((l, l.due?)))
        .collect();
    let due_base: i64 = with_due.iter().map(|(l, _)| l.balance_yen).sum();
    let avg_years = (due_base > 0).then(|| {
        with_due
            .iter()
            .map(|(l, d)| {
                l.balance_yen as f64 * ((d.to_days() - period_end.to_days()).max(0) as f64 / 365.25)
            })
            .sum::<f64>()
            / due_base as f64
    });
    let due_soon = |l: &Loan| l.due.is_some_and(|d| d <= horizon);
    let floating: i64 = debt
        .loans
        .iter()
        .filter(|l| l.rate_type == Some(RateType::Floating))
        .map(|l| l.balance_yen)
        .sum();
    let other_due_1y: i64 = debt
        .loans
        .iter()
        .filter(|l| l.rate_type != Some(RateType::Floating) && due_soon(l))
        .map(|l| l.balance_yen)
        .sum();
    let unknown_due_1y: i64 = debt
        .loans
        .iter()
        .filter(|l| l.rate_type.is_none() && due_soon(l))
        .map(|l| l.balance_yen)
        .sum();
    // 返済予定表の「投資法人債」「1年内償還予定の投資法人債」の1年以内
    let bonds_1y: i64 = debt
        .ladder
        .iter()
        .filter(|(k, _)| k.contains("投資法人債"))
        .map(|(_, v)| v[0])
        .sum();
    let reprice_1y = floating + other_due_1y + bonds_1y;
    DebtSummary {
        avg_rate_pct,
        avg_years,
        reprice_1y,
        reprice_1y_max: reprice_1y + (mix.unknown - unknown_due_1y).max(0),
    }
}

/// 物件の要約
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct PortfolioSummary {
    pub property_count: usize,
    pub leasable_m2: f64,
    pub leased_m2: f64,
    /// 面積加重の稼働率（%）。稼働率の分かる物件だけで計算する
    pub occupancy_pct: Option<f64>,
}

pub fn portfolio_summary(props: &[Property]) -> PortfolioSummary {
    let mut leasable = 0.0;
    let mut leased = 0.0;
    let mut base = 0.0;
    for p in props {
        leasable += p.leasable_m2;
        let l = p
            .leased_m2
            .or(p.occupancy_pct.map(|o| p.leasable_m2 * o / 100.0));
        if let Some(l) = l {
            leased += l;
            base += p.leasable_m2;
        }
    }
    PortfolioSummary {
        property_count: props.len(),
        leasable_m2: leasable,
        leased_m2: leased,
        occupancy_pct: (base > 0.0).then(|| leased / base * 100.0),
    }
}

/// 読めた額と答え（BS・合計行）の比べ方: 一致 / 端数（tolerance 以内。千円・百万円未満の切り捨てが積もるなど）/ ずれ
pub fn agreement(got: f64, want: f64, tolerance: f64) -> &'static str {
    if (got - want).abs() < 0.5 {
        "一致"
    } else if want != 0.0 && ((got - want) / want).abs() <= tolerance {
        "端数"
    } else {
        "ずれ"
    }
}

/// 読み取りの確認結果
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Checks {
    /// 借入の合計と BS（短期＋長期）: 一致 / 端数 / ずれ / 明細なし
    pub loans: &'static str,
    /// 物件の面積の合計と表の合計行: 一致 / 端数 / ずれ / 合計行なし / 物件なし
    pub properties: &'static str,
}

pub fn checks(
    debt: &DebtSchedule,
    bs_loans: i64,
    props: &[Property],
    total: Option<&PropertyTotal>,
) -> Checks {
    let loans_total: i64 = debt.loans.iter().map(|l| l.balance_yen).sum();
    let loans = if debt.loans.is_empty() {
        "明細なし"
    } else {
        agreement(loans_total as f64, bs_loans as f64, 0.001)
    };
    let leasable: f64 = props.iter().map(|p| p.leasable_m2).sum();
    let properties = match (total, props.is_empty()) {
        (_, true) => "物件なし",
        (Some(t), false) => agreement(leasable, t.leasable_m2, 0.001),
        (None, false) => "合計行なし",
    };
    Checks { loans, properties }
}

/// 用途の区分（一覧の絞り込み用）。リポジトリの data/reit_sectors.csv が元
pub const SECTORS: [&str; 8] = [
    "office",
    "logistics",
    "residential",
    "hotel",
    "retail",
    "healthcare",
    "diversified",
    "infra",
];

/// data/reit_sectors.csv（code,sector,note）を読む。知らない用途の行は捨てる
pub fn parse_sectors(csv: &str) -> std::collections::HashMap<String, String> {
    csv.lines()
        .skip(1)
        .filter_map(|l| {
            let mut it = l.split(',');
            let code = it.next()?.trim();
            let sector = it.next()?.trim();
            (!code.is_empty() && SECTORS.contains(&sector))
                .then(|| (code.to_string(), sector.to_string()))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(cells: &[&str]) -> Vec<String> {
        cells.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn summaries() {
        let end = Date::new(2026, 5, 31);
        let loan = |bal: i64, rate: f64, due: Date, t: Option<RateType>| Loan {
            kind: Some(LoanKind::Long),
            lender: "A".into(),
            balance_yen: bal,
            rate_pct: Some(rate),
            due: Some(due),
            rate_type: t,
        };
        let debt = DebtSchedule {
            loans: vec![
                loan(100, 1.0, Date::new(2026, 12, 1), Some(RateType::Fixed)), // 1年以内に返済 → 見直し
                loan(200, 0.5, Date::new(2030, 5, 31), Some(RateType::Floating)), // 変動 → 見直し
                loan(300, 0.8, Date::new(2029, 5, 31), None), // 記載なし → 上限だけ
            ],
            ladder: vec![("投資法人債".into(), [50, 0, 0, 0, 0, 0])],
            ..Default::default()
        };
        let mix = rate_mix(&debt, 0);
        let s = debt_summary(&debt, &mix, end);
        assert_eq!(s.reprice_1y, 100 + 200 + 50);
        assert_eq!(s.reprice_1y_max, 350 + 300);
        assert!((s.avg_rate_pct.unwrap() - (100.0 + 100.0 + 240.0) / 600.0).abs() < 1e-9);
        let props = vec![
            Property {
                id: "1".into(),
                name: "a".into(),
                leasable_m2: 100.0,
                leased_m2: Some(90.0),
                occupancy_pct: Some(90.0),
                occupancy_computed: false,
                annual_rent_yen: None,
                rent_raw: String::new(),
            },
            Property {
                id: "2".into(),
                name: "b".into(),
                leasable_m2: 100.0,
                leased_m2: None,
                occupancy_pct: Some(100.0),
                occupancy_computed: false,
                annual_rent_yen: None,
                rent_raw: String::new(),
            },
        ];
        let p = portfolio_summary(&props);
        assert_eq!(p.property_count, 2);
        assert!((p.occupancy_pct.unwrap() - 95.0).abs() < 1e-9);
        let sec = parse_sectors("code,sector,note\n8951,office,\n3283,logistics,\n9999,unknown,\n");
        assert_eq!(sec.get("8951").map(|s| s.as_str()), Some("office"));
        assert!(!sec.contains_key("9999"));
    }

    #[test]
    fn numbers() {
        assert_eq!(to_num("1,234,000"), Some(1_234_000.0));
        assert_eq!(to_num("0.495 (注3)"), Some(0.495));
        assert_eq!(to_num("△5"), Some(-5.0));
        assert_eq!(to_num("98.4%"), Some(98.4));
        assert_eq!(to_num("-"), None);
        assert_eq!(to_num("非開示 (注11)"), None);
        assert_eq!(to_num("12,34"), None, "桁区切りの位置がおかしい");
        assert_eq!(to_num("2026年"), None);
    }

    #[test]
    fn dates() {
        assert_eq!(
            parse_jp_date("2026年 12月2日"),
            Some(Date::new(2026, 12, 2))
        );
        assert_eq!(parse_jp_date("2029.6.1"), Some(Date::new(2029, 6, 1)));
        assert_eq!(
            parse_jp_date("2026年6月22日 (注8)"),
            Some(Date::new(2026, 6, 22))
        );
        assert_eq!(parse_jp_date("期限一括"), None);
    }

    #[test]
    fn note_numbers() {
        assert_eq!(note_refs("0.495 (注3)"), vec![3]);
        assert_eq!(note_refs("（注2、3）"), vec![2, 3]);
        assert_eq!(note_refs("(注1)(注14)"), vec![1, 14]);
    }

    /// 区分の列が結合セル、利率・期限が複数の借入先にまたがる表（日本プロロジスの形）
    #[test]
    fn loans_with_merged_cells_and_notes() {
        let blocks = vec![
            Block::Text("⑥ 借入金明細表 （単位：千円）".into()),
            Block::Table(
                vec![
                row(&["区分", "区分", "当期首残高", "当期増加額", "当期減少額", "当期末残高", "平均利率", "返済期限", "使途", "摘要"]),
                row(&["区分", "借入先", "当期首残高", "当期増加額", "当期減少額", "当期末残高", "平均利率", "返済期限", "使途", "摘要"]),
                row(&["短期借入金", "A銀行", "-", "3,500,000", "-", "3,500,000", "1.00387%", "2026年 12月2日", "(注2)", "無担保"]),
                row(&["短期借入金", "合計", "-", "3,500,000", "-", "3,500,000", "", "", "", ""]),
                row(&["長期借入金", "B銀行", "2,070,000", "-", "-", "2,070,000", "0.71670% (注3)", "2028年3月13日", "(注2)", "無担保"]),
                row(&["長期借入金", "C銀行", "1,000,000", "-", "-", "1,000,000", "0.5%", "2030年3月13日", "(注2)", "無担保"]),
                row(&["合計", "合計", "", "", "", "6,570,000", "", "", "", ""]),
                ],
                vec![],
            ),
            Block::Text("(注1) 変動金利の平均利率は、期中の加重平均を記載しています。 (注2) 資金使途は借入金の返済です。 (注3) 変動金利借入ですが金利スワップ契約により実質的に金利が固定されています。".into()),
        ];
        let d = parse_debt(&blocks);
        assert_eq!(d.loans.len(), 3);
        assert_eq!(d.loans[0].kind, Some(LoanKind::Short));
        assert_eq!(d.loans[0].balance_yen, 3_500_000_000);
        assert_eq!(d.loans[0].due, Some(Date::new(2026, 12, 2)));
        assert_eq!(d.loans[1].kind, Some(LoanKind::Long));
        assert_eq!(d.loans[1].rate_type, Some(RateType::SwapFixed));
        assert_eq!(
            d.loans[2].rate_type, None,
            "注1 は平均利率の説明なので変動とはみなさない"
        );
        assert_eq!(d.table_total_yen, Some(6_570_000_000));
        let m = rate_mix(&d, 0);
        assert_eq!((m.swap_fixed, m.unknown), (2_070_000_000, 4_500_000_000));
    }

    /// 「その他は全て固定」の書き方（日本ビルファンド）と、区分の列が空の続きの表。文章は全角を半角にしたあとの形
    #[test]
    fn rest_fixed_and_kind_after_subtotal() {
        let blocks = vec![
            Block::Text("Ｇ．借入金等明細表".into()),
            Block::Table(
                vec![
                row(&["区分", "借入先", "当期首残高（千円）", "当期増加額（千円）", "当期減少額（千円）", "当期末残高（千円）", "平均利率（%）", "返済期限", "摘要"]),
                row(&["短期借入金", "A銀行", "100,000", "-", "-", "100,000", "1.0", "2026.12.1", ""]),
                row(&["", "小計", "100,000", "", "", "100,000", "", "", ""]),
                row(&["", "B銀行", "1,000,000", "-", "-", "1,000,000", "1.033", "2029.6.1", "4"]),
                row(&["", "C銀行", "2,000,000", "-", "-", "2,000,000", "0.45", "2031.1.7", ""]),
                ],
                vec![],
            ),
            Block::Text("(注)1.平均利率は期末の利率です。 4.変動金利による借入れです。その他は、全て固定金利による借入れです。".into()),
        ];
        let d = parse_debt(&blocks);
        assert_eq!(
            d.loans.iter().map(|l| l.kind).collect::<Vec<_>>(),
            [
                Some(LoanKind::Short),
                Some(LoanKind::Long),
                Some(LoanKind::Long)
            ]
        );
        assert_eq!(d.loans[1].rate_type, Some(RateType::Floating));
        assert_eq!(d.loans[2].rate_type, Some(RateType::Fixed));
    }

    #[test]
    fn loan_kind_from_vertical_text() {
        assert_eq!(
            loan_kind_of("一の 年長 内期 返借 済入 予金 定"),
            Some(LoanKind::Long)
        );
        assert_eq!(loan_kind_of("短 期 借 入 金"), Some(LoanKind::Short));
        assert_eq!(loan_kind_of("長期借入金(注1)"), Some(LoanKind::Long));
        assert_eq!(loan_kind_of("株式会社三井住友銀行"), None);
    }

    #[test]
    fn ladder_and_swaps() {
        let blocks = vec![
            Block::Text("（単位：千円）".into()),
            Block::Table(
                vec![
                    row(&[
                        "",
                        "1年以内",
                        "1年超2年以内",
                        "2年超3年以内",
                        "3年超4年以内",
                        "4年超5年以内",
                        "5年超",
                    ]),
                    row(&["投資法人債", "-", "8,500,000", "-", "-", "-", "16,000,000"]),
                    row(&["長期借入金", "44,100,000", "29,800,000", "-", "-", "-", "-"]),
                    row(&[
                        "合計",
                        "44,100,000",
                        "38,300,000",
                        "-",
                        "-",
                        "-",
                        "16,000,000",
                    ]),
                ],
                vec![],
            ),
            Block::Text(
                "② デリバティブ取引及び為替予約取引の契約額等及び時価の状況表 （単位：千円）"
                    .into(),
            ),
            Block::Table(
                vec![
                    row(&["区分", "種類", "契約額等", "契約額等", "時価"]),
                    row(&["区分", "種類", "", "うち1年超", "時価"]),
                    row(&[
                        "市場取引以外の取引",
                        "金利スワップ取引 変動受取・固定支払",
                        "268,661,000",
                        "224,561,000",
                        "15,157,162",
                    ]),
                ],
                vec![],
            ),
        ];
        let d = parse_debt(&blocks);
        assert_eq!(d.swap_yen, Some(268_661_000_000));
        assert_eq!(d.ladder.len(), 2);
        assert_eq!(d.ladder[1].0, "長期借入金");
        assert_eq!(d.ladder[1].1[0], 44_100_000_000);
    }

    #[test]
    fn sideways_property_table() {
        let blocks = vec![Block::Table(
            vec![
                row(&["用途", "事務所", "事務所"]),
                row(&["不動産の名称", "兼松ビル", "新麹町ビル"]),
                row(&["賃貸可能面積(㎡)", "8,090.42", "5,346.68"]),
                row(&["賃貸面積(㎡)", "8,090.42", "5,100.00"]),
                row(&["稼働率(%)", "100.0", "95.4"]),
            ],
            vec![],
        )];
        let (p, _) = parse_properties(&blocks);
        assert_eq!(p.len(), 2);
        assert_eq!(p[1].name, "新麹町ビル");
        assert_eq!(p[1].occupancy_pct, Some(95.4));
    }

    #[test]
    fn same_period_columns_are_not_history() {
        let cols: Vec<String> = [
            "物件名称",
            "第42期(2026年5月31日)賃貸可能面積",
            "第42期(2026年5月31日)稼働率",
            "第42期(自2025年12月1日至2026年5月31日)賃貸事業収入",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        assert!(!is_history_header(&cols));
        let hist: Vec<String> = ["物件名称", "第11期", "第12期", "第13期"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        assert!(is_history_header(&hist));
    }

    #[test]
    fn properties_merge_buildings_and_skip_history() {
        let blocks = vec![
            Block::Table(
                vec![
                    row(&[
                        "物件番号",
                        "物件名称",
                        "年間賃料（百万円）",
                        "賃貸可能面積（㎡）",
                        "賃貸面積（㎡）",
                    ]),
                    row(&["H-1", "ホテルA", "190", "3,706.56", "3,706.56"]),
                    row(&["S-1", "リゾートB", "629", "23,322.42", "23,322.42"]),
                    row(&["S-1", "リゾートB", "-", "40,515.83", "40,515.83"]),
                    row(&["S-2", "旅館C", "非開示 (注17)", "9,782.37", "9,000.00"]),
                    row(&["S-2", "旅館C", "非開示 (注17)", "9,782.37", "9,000.00"]),
                    row(&["合計", "合計", "3,900", "77,327.18", "76,544.81"]),
                ],
                vec![],
            ),
            Block::Table(
                vec![
                    row(&[
                        "物件番号",
                        "物件名称",
                        "稼働率（%）",
                        "稼働率（%）",
                        "稼働率（%）",
                    ]),
                    row(&["物件番号", "物件名称", "第11期", "第12期", "第13期"]),
                    row(&["H-1", "ホテルA", "100.0", "100.0", "100.0"]),
                ],
                vec![],
            ),
        ];
        let (p, total) = parse_properties(&blocks);
        assert_eq!(p.len(), 3);
        assert!(
            (p[1].leasable_m2 - 63_838.25).abs() < 0.001,
            "建物が2つの物件は足す"
        );
        assert_eq!(p[0].annual_rent_yen, Some(190_000_000));
        assert_eq!(p[2].annual_rent_yen, None);
        assert_eq!(p[2].occupancy_pct, Some(92.0));
        assert!(p[2].occupancy_computed);
        let sum: f64 = p.iter().map(|x| x.leasable_m2).sum();
        assert!((sum - total.unwrap().leasable_m2).abs() < 0.01);
    }
}
