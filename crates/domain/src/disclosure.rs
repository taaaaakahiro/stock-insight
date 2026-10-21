//! 有価証券報告書・半期報告書の文章から、配当方針の手がかり（累進配当・減配しない・配当性向の目標など）を拾う。
//! 判定は言葉の有無だけ。最終的な判断は抜き出した文（phrases）を人が読んで行う。

use crate::Date;

/// 銘柄ごとに最新1通だけ持つ書類の種類
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum DocKind {
    /// 有価証券報告書（年1回）
    Annual,
    /// 半期報告書（年1回。2024年4月に四半期報告書から置き換わった）
    Semiannual,
}

impl DocKind {
    /// EDINET の書類種別コード。訂正報告書（130・170）は扱わない
    pub fn from_edinet(doc_type_code: &str) -> Option<Self> {
        match doc_type_code {
            "120" => Some(DocKind::Annual),
            "160" => Some(DocKind::Semiannual),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            DocKind::Annual => "annual",
            DocKind::Semiannual => "semiannual",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "annual" => Some(DocKind::Annual),
            "semiannual" => Some(DocKind::Semiannual),
            _ => None,
        }
    }
}

/// 抜き出す文章の項目。XBRL の要素名（局所名）に含まれる語で見分ける
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Section {
    /// 配当政策
    DividendPolicy,
    /// 経営方針、経営環境及び対処すべき課題等（中期経営計画の還元方針が書かれることが多い）
    BusinessPolicy,
    /// 経営者による財政状態、経営成績及びキャッシュ・フローの状況の分析
    Mdna,
}

impl Section {
    pub const ALL: [Section; 3] = [
        Section::DividendPolicy,
        Section::BusinessPolicy,
        Section::Mdna,
    ];

    pub fn key(self) -> &'static str {
        match self {
            Section::DividendPolicy => "dividend_policy",
            Section::BusinessPolicy => "business_policy",
            Section::Mdna => "mdna",
        }
    }

    /// 要素の局所名（例: DividendPolicyTextBlock）からどの項目か
    pub fn from_element(local_name: &str) -> Option<Self> {
        if !local_name.ends_with("TextBlock") {
            return None;
        }
        if local_name.starts_with("DividendPolicy") {
            Some(Section::DividendPolicy)
        } else if local_name.starts_with("BusinessPolicyBusinessEnvironmentIssuesToAddress") {
            Some(Section::BusinessPolicy)
        } else if local_name.starts_with("ManagementAnalysisOfFinancialPosition") {
            Some(Section::Mdna)
        } else {
            None
        }
    }
}

/// 配当方針の手がかり
#[derive(Debug, Clone, Default, PartialEq)]
pub struct DividendSignals {
    /// 累進配当（原則として減配せず、維持または増配）
    pub progressive: bool,
    /// 「減配しない」「維持または増配」など
    pub no_cut: bool,
    /// 増配の記述（実施・予定・方針のいずれも）
    pub raise: bool,
    /// DOE（株主資本配当率）を目標にしている
    pub doe_target: bool,
    /// 配当性向の目標（0.3 = 30%）。範囲なら下限
    pub payout_target: Option<f64>,
    /// 手がかりの言葉を含む文（最大 MAX_PHRASES、各 MAX_PHRASE_CHARS 文字まで）
    pub phrases: Vec<String>,
}

impl DividendSignals {
    /// 一覧の絞り込み用: 増配・非減配の方針がある
    pub fn shareholder_friendly(&self) -> bool {
        self.progressive || self.no_cut
    }
}

const PROGRESSIVE: &[&str] = &["累進配当", "累進的な配当", "累進的に"];
const NO_CUT: &[&str] = &[
    "減配しない",
    "減配を行わない",
    "減配は行わない",
    "減配を行わず",
    "減配せず",
    "非減配",
    "維持または増配",
    "維持もしくは増配",
    "維持・増配",
    "維持又は増配",
];
const RAISE: &[&str] = &["増配"];
const DOE: &[&str] = &["DOE", "株主資本配当率", "純資産配当率", "自己資本配当率"];
const PAYOUT: &str = "配当性向";

const MAX_PHRASES: usize = 5;
const MAX_PHRASE_CHARS: usize = 160;
/// 「配当性向」の後ろのこの文字数以内に「30%」などがあれば目標とみなす
const PAYOUT_WINDOW_CHARS: usize = 25;

/// 項目ごとの文章から手がかりを拾う。配当政策の文を先に並べる
pub fn detect(sections: &[(Section, &str)]) -> DividendSignals {
    let mut sorted: Vec<&(Section, &str)> = sections.iter().collect();
    sorted.sort_by_key(|(s, _)| *s);
    let mut sig = DividendSignals::default();
    for (_, text) in sorted {
        for sentence in text.split('。').map(str::trim).filter(|s| !s.is_empty()) {
            let has = |words: &[&str]| words.iter().any(|w| sentence.contains(w));
            let progressive = has(PROGRESSIVE);
            let no_cut = has(NO_CUT);
            let raise = has(RAISE);
            let doe = has(DOE);
            let payout = payout_target(sentence);
            sig.progressive |= progressive;
            sig.no_cut |= no_cut;
            sig.raise |= raise;
            sig.doe_target |= doe;
            if sig.payout_target.is_none() {
                sig.payout_target = payout;
            }
            if progressive || no_cut || raise || doe || payout.is_some() {
                push_phrase(&mut sig.phrases, sentence);
            }
        }
    }
    sig
}

fn push_phrase(phrases: &mut Vec<String>, sentence: &str) {
    if phrases.len() >= MAX_PHRASES {
        return;
    }
    let mut s: String = sentence.chars().take(MAX_PHRASE_CHARS).collect();
    if sentence.chars().count() > MAX_PHRASE_CHARS {
        s.push('…');
    } else {
        s.push('。');
    }
    if !phrases.contains(&s) {
        phrases.push(s);
    }
}

/// 「配当性向30%以上」「配当性向は35～40%」などの数値（下限）。文に「配当性向」がなければ None
fn payout_target(sentence: &str) -> Option<f64> {
    let start = sentence.find(PAYOUT)? + PAYOUT.len();
    let window: String = sentence[start..]
        .chars()
        .take(PAYOUT_WINDOW_CHARS)
        .collect();
    let pct = window.find('%')?;
    // % より前で最初に出てくる数字を下限とする（「30~40%」なら 30）
    let head = &window[..pct];
    let first: String = head
        .chars()
        .skip_while(|c| !c.is_ascii_digit())
        .take_while(|c| c.is_ascii_digit() || *c == '.')
        .collect();
    let v: f64 = first.parse().ok()?;
    (0.0 < v && v <= 100.0).then_some(v / 100.0)
}

/// XBRL の文章（HTML のタグを含むことがある）を判定しやすい形にする:
/// タグを除き、全角の英数字・記号を半角に、空白をまとめる
pub fn normalize_text(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut in_tag = false;
    let mut last_space = true;
    for c in raw.chars() {
        if c == '<' {
            in_tag = true;
            continue;
        }
        if in_tag {
            if c == '>' {
                in_tag = false;
                if !last_space {
                    out.push(' ');
                    last_space = true;
                }
            }
            continue;
        }
        let c = match c {
            '\u{FF01}'..='\u{FF5E}' => char::from_u32(c as u32 - 0xFEE0).unwrap_or(c),
            '\u{3000}' => ' ',
            _ => c,
        };
        if c.is_whitespace() {
            if !last_space {
                out.push(' ');
                last_space = true;
            }
        } else {
            out.push(c);
            last_space = false;
        }
    }
    out.replace("&nbsp;", " ")
        .replace("&amp;", "&")
        .trim()
        .to_string()
}

/// 半期報告書が、それより後の期末の有報で古くなったか（不要なので消してよい）
pub fn semiannual_superseded(semiannual_period_end: Date, annual_period_end: Date) -> bool {
    annual_period_end > semiannual_period_end
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn edinet_doc_types() {
        assert_eq!(DocKind::from_edinet("120"), Some(DocKind::Annual));
        assert_eq!(DocKind::from_edinet("160"), Some(DocKind::Semiannual));
        assert_eq!(DocKind::from_edinet("130"), None, "訂正は扱わない");
        assert_eq!(DocKind::from_edinet("140"), None, "四半期報告書は廃止済み");
        assert_eq!(DocKind::parse("semiannual"), Some(DocKind::Semiannual));
    }

    #[test]
    fn sections_from_element_names() {
        assert_eq!(
            Section::from_element("DividendPolicyTextBlock"),
            Some(Section::DividendPolicy)
        );
        assert_eq!(
            Section::from_element("BusinessPolicyBusinessEnvironmentIssuesToAddressEtcTextBlock"),
            Some(Section::BusinessPolicy)
        );
        assert_eq!(
            Section::from_element(
                "ManagementAnalysisOfFinancialPositionOperatingResultsAndCashFlowsTextBlock"
            ),
            Some(Section::Mdna)
        );
        assert_eq!(Section::from_element("DividendPolicy"), None);
        assert_eq!(Section::from_element("BusinessRisksTextBlock"), None);
    }

    #[test]
    fn normalizes_html_and_full_width() {
        assert_eq!(
            normalize_text("<p>配当性向　３０％以上を</p>\n<p>目標と&nbsp;します。</p>"),
            "配当性向 30%以上を 目標と します。"
        );
    }

    #[test]
    fn detects_progressive_dividend_policy() {
        let policy = normalize_text(
            "当社は、株主還元を重要な経営課題と位置付けております。\
             中期経営計画期間中は累進配当を基本方針とし、配当性向３５％以上を目標とします。\
             当期の配当は前期から５円の増配となりました。",
        );
        let s = detect(&[(Section::DividendPolicy, &policy)]);
        assert!(s.progressive);
        assert!(s.raise);
        assert!(!s.no_cut);
        assert!(!s.doe_target);
        assert_eq!(s.payout_target, Some(0.35));
        assert_eq!(s.phrases.len(), 2, "手がかりのない最初の文は拾わない");
        assert!(s.phrases[0].starts_with("中期経営計画期間中は累進配当"));
        assert!(s.shareholder_friendly());
    }

    #[test]
    fn detects_no_cut_and_doe() {
        let policy = "DOE3%を下限とし、原則として減配は行わない方針です。";
        let business = "株主資本配当率の向上に努めます。";
        let s = detect(&[
            (Section::BusinessPolicy, business),
            (Section::DividendPolicy, policy),
        ]);
        assert!(s.no_cut);
        assert!(s.doe_target);
        assert_eq!(s.payout_target, None, "DOE の % は配当性向ではない");
        assert!(
            s.phrases[0].starts_with("DOE3%"),
            "配当政策の文を先に並べる"
        );
    }

    #[test]
    fn payout_target_takes_lower_bound() {
        assert_eq!(payout_target("配当性向は30~40%を目安とします"), Some(0.3));
        assert_eq!(payout_target("連結配当性向40%を目標"), Some(0.4));
        assert_eq!(payout_target("配当性向を高めていきます"), None);
        assert_eq!(
            payout_target(
                "配当性向については、業績や財務の状況を総合的に勘案したうえで決定し、20%"
            ),
            None,
            "遠すぎる数字は目標とみなさない"
        );
    }

    #[test]
    fn no_signals_in_plain_text() {
        let s = detect(&[(Section::Mdna, "売上高は前期比5%増加しました。")]);
        assert_eq!(s, DividendSignals::default());
    }

    #[test]
    fn long_sentences_are_cut() {
        let long = format!("累進配当{}", "あ".repeat(300));
        let s = detect(&[(Section::DividendPolicy, &long)]);
        assert_eq!(s.phrases[0].chars().count(), MAX_PHRASE_CHARS + 1);
        assert!(s.phrases[0].ends_with('…'));
    }

    #[test]
    fn semiannual_is_superseded_by_later_annual() {
        let d = |s| Date::parse(s).unwrap();
        assert!(semiannual_superseded(d("2025-09-30"), d("2026-03-31")));
        assert!(!semiannual_superseded(d("2026-09-30"), d("2026-03-31")));
    }
}
