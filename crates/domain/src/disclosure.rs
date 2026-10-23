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
    /// 事業の内容（注目テーマとの照合に使う。配当方針の手がかりは探さない）
    Business,
}

impl Section {
    pub const ALL: [Section; 4] = [
        Section::DividendPolicy,
        Section::BusinessPolicy,
        Section::Mdna,
        Section::Business,
    ];

    pub fn key(self) -> &'static str {
        match self {
            Section::DividendPolicy => "dividend_policy",
            Section::BusinessPolicy => "business_policy",
            Section::Mdna => "mdna",
            Section::Business => "business",
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
        } else if local_name.starts_with("DescriptionOfBusiness") {
            Some(Section::Business)
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
    /// DOE の下限（0.035 = 3.5%）。「下限」「最低」「を下回らない」と書かれた数字と、目標とはっきり書かずに
    /// 「X%以上」とした数字。引き上げなら新しい値
    pub doe_floor: Option<f64>,
    /// DOE の目標（「目標」「目途」「目安」「目指す」、または「以上」のない「程度」「水準」「方針」の数字）
    pub doe_goal: Option<f64>,
    /// DOE の下限（なければ目標）の数字を拾った文（MAX_PHRASE_CHARS 文字まで）
    pub doe_phrase: Option<String>,
    /// 配当性向の下限（なければ目標）（0.3 = 30%）。古い列との互換用で、中身は payout_floor.or(payout_goal)
    pub payout_target: Option<f64>,
    /// 配当性向の下限・目標・その数字を拾った文。言葉の分け方は DOE と同じ
    pub payout_floor: Option<f64>,
    pub payout_goal: Option<f64>,
    pub payout_phrase: Option<String>,
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
const PAYOUT: &[&str] = &["配当性向"];

/// 下限か目標かを分ける言葉（DOE・配当性向で共通）
const FLOOR_WORDS: &[&str] = &["下限", "最低", "下回らない", "下回ることなく", "確保"];
/// はっきり目標と書く言葉（「X%以上を目標」は目標）
const GOAL_WORDS: &[&str] = &["目標", "目途", "目処", "めど", "目安", "目指", "目線"];
/// 目標とみなす弱い言葉（「以上」があれば下限を優先する）
const LOOSE_GOAL_WORDS: &[&str] = &["程度", "水準", "方針", "基本", "基準"];
/// 方針ではない数字（特別配当の大きさ、当期の実績）
const NOT_POLICY: &[&str] = &["特別配当", "記念配当", "実績", "となりました", "でした"];

/// 数字つきの方針（DOE・配当性向）を文から拾うときの決まり
struct PolicyRule {
    /// 指標の言葉
    words: &'static [&'static str],
    /// 言葉の後ろのこの文字数以内の数字を、その指標の値とみなす
    window_chars: usize,
    /// 値を探すのを、ここで止める（ほかの指標の % を拾わないように）
    stop: &'static [&'static str],
    /// この指標として扱う上限。これより大きい数字は読み違いとみなす
    max: f64,
}

const DOE_RULE: PolicyRule = PolicyRule {
    words: DOE,
    window_chars: 40,
    stop: &["配当性向", "総還元", "ROE", "自己資本比率", "。"],
    max: 0.2,
};
/// 配当性向。「配当性向30%以上」のように数字が近いので窓は短め。「連結配当性向につきましては30%」も入る
const PAYOUT_RULE: PolicyRule = PolicyRule {
    words: PAYOUT,
    window_chars: 30,
    stop: &[
        "DOE",
        "株主資本配当率",
        "純資産配当率",
        "自己資本配当率",
        "総還元",
        "ROE",
        "。",
    ],
    max: 1.0,
};

const MAX_PHRASES: usize = 5;
const MAX_PHRASE_CHARS: usize = 160;

/// 文をまたいで、下限・目標のいちばん大きい値と、それを拾った文を集める
#[derive(Default)]
struct PolicyValues {
    floor: Option<f64>,
    goal: Option<f64>,
    floor_phrase: Option<String>,
    goal_phrase: Option<String>,
}

impl PolicyValues {
    fn take(&mut self, sentence: &str, (floor, goal): (Option<f64>, Option<f64>)) {
        if let Some(v) = floor.filter(|v| self.floor.is_none_or(|f| *v > f)) {
            self.floor = Some(v);
            self.floor_phrase = Some(cut_sentence(sentence));
        }
        if let Some(v) = goal.filter(|v| self.goal.is_none_or(|g| *v > g)) {
            self.goal = Some(v);
            self.goal_phrase = Some(cut_sentence(sentence));
        }
    }

    /// 下限（なければ目標）を拾った文
    fn phrase(self) -> Option<String> {
        self.floor_phrase.or(self.goal_phrase)
    }
}

/// 項目ごとの文章から手がかりを拾う。配当政策の文を先に並べ、事業の内容は見ない（配当の方針ではない）。
/// DOE・配当性向の下限・目標は文ごとに違うこと（従来の計画と新しい計画）があるので、いちばん大きい値を取る
/// （引き上げのほうが多く、古い値は「従来の…から」と書かれることが多い）
pub fn detect(sections: &[(Section, &str)]) -> DividendSignals {
    let mut sorted: Vec<&(Section, &str)> = sections.iter().collect();
    sorted.sort_by_key(|(s, _)| *s);
    let mut sig = DividendSignals::default();
    let mut doe_values = PolicyValues::default();
    let mut payout_values = PolicyValues::default();
    for (_, text) in sorted.into_iter().filter(|(s, _)| *s != Section::Business) {
        for sentence in text.split('。').map(str::trim).filter(|s| !s.is_empty()) {
            let has = |words: &[&str]| words.iter().any(|w| sentence.contains(w));
            let progressive = has(PROGRESSIVE);
            let no_cut = has(NO_CUT);
            let raise = has(RAISE);
            let doe = has(DOE);
            let payout = has(PAYOUT);
            sig.progressive |= progressive;
            sig.no_cut |= no_cut;
            sig.raise |= raise;
            sig.doe_target |= doe;
            if doe {
                doe_values.take(sentence, policy_values(sentence, &DOE_RULE));
            }
            // 配当性向は数字があるときだけ手がかりの文に入れる（「配当性向を高める」だけでは方針の数字にならない）
            let payout_found = payout && {
                let found = policy_values(sentence, &PAYOUT_RULE);
                payout_values.take(sentence, found);
                found.0.is_some() || found.1.is_some()
            };
            if progressive || no_cut || raise || doe || payout_found {
                push_phrase(&mut sig.phrases, sentence);
            }
        }
    }
    sig.doe_floor = doe_values.floor;
    sig.doe_goal = doe_values.goal;
    sig.doe_phrase = doe_values.phrase();
    sig.payout_floor = payout_values.floor;
    sig.payout_goal = payout_values.goal;
    sig.payout_target = sig.payout_floor.or(sig.payout_goal);
    sig.payout_phrase = payout_values.phrase();
    sig
}

fn push_phrase(phrases: &mut Vec<String>, sentence: &str) {
    if phrases.len() >= MAX_PHRASES {
        return;
    }
    let s = cut_sentence(sentence);
    if !phrases.contains(&s) {
        phrases.push(s);
    }
}

/// MAX_PHRASE_CHARS 文字までにして、句点か「…」を付ける
fn cut_sentence(sentence: &str) -> String {
    let mut s: String = sentence.chars().take(MAX_PHRASE_CHARS).collect();
    if sentence.chars().count() > MAX_PHRASE_CHARS {
        s.push('…');
    } else {
        s.push('。');
    }
    s
}

/// 1文から指標（DOE・配当性向）の（下限, 目標）を拾う。指標の言葉ごとに、後ろの window_chars 文字（ほかの
/// 指標が出てきたらそこまで）にある % の数字を見る。「2.5%から3.5%へ」は新しい値、「3~4%」は下限の数字。
/// 下限の言葉があれば下限、目標とはっきり書いてあれば目標、「以上」なら下限、弱い目標の言葉なら目標。
/// どの言葉もない数字（当期の実績など）と、「従来の3.5%から」のように新しい値のない古い値は拾わない
fn policy_values(sentence: &str, rule: &PolicyRule) -> (Option<f64>, Option<f64>) {
    if NOT_POLICY.iter().any(|w| sentence.contains(w)) {
        return (None, None);
    }
    let mut floor = None;
    let mut goal = None;
    for word in rule.words {
        for (at, _) in sentence.match_indices(word) {
            let rest = &sentence[at + word.len()..];
            let mut window: String = rest.chars().take(rule.window_chars).collect();
            // ほかの指標か、同じ指標の次の記述（「配当性向30%以上を基本とし、配当性向40%を目指す」）が出てきたらそこまで
            if let Some(stop) = rule
                .stop
                .iter()
                .chain(rule.words)
                .filter_map(|w| window.find(w))
                .min()
            {
                window.truncate(stop);
            }
            let Some(v) = pct_value(&window, rule.max) else {
                continue;
            };
            let has = |words: &[&str]| words.iter().any(|w| window.contains(w));
            if has(FLOOR_WORDS) {
                floor = floor.or(Some(v));
            } else if has(GOAL_WORDS) {
                goal = goal.or(Some(v));
            } else if window.contains("以上") {
                floor = floor.or(Some(v));
            } else if has(LOOSE_GOAL_WORDS) {
                goal = goal.or(Some(v));
            }
        }
    }
    (floor, goal)
}

/// 窓の中の % の数字（小数、max まで）。「AからBへ」は B、範囲は下限の数字
fn pct_value(window: &str, max: f64) -> Option<f64> {
    let pcts: Vec<(usize, f64)> = window
        .match_indices('%')
        .filter_map(|(i, _)| {
            let head = &window[..i];
            let digits: String = head
                .chars()
                .rev()
                .take_while(|c| c.is_ascii_digit() || *c == '.')
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect();
            let v: f64 = digits.trim_start_matches('.').parse().ok()?;
            Some((i, v / 100.0))
        })
        .collect();
    let (first_at, first) = *pcts.first()?;
    // 「2.5%から3.5%へ」: から の後ろの数字を取る。後ろに数字がなければ古い値なので拾わない
    let v = match window[first_at..].find("から") {
        Some(k) => pcts.iter().find(|(i, _)| *i > first_at + k)?.1,
        // 「3~4%」「3〜4%」: % の前の範囲は最初の数字（下限）
        None => range_low(&window[..first_at]).unwrap_or(first),
    };
    // 0.018000000000000002 のような誤差を落とす（0.01% 単位）
    let v = (v * 10_000.0).round() / 10_000.0;
    (0.0 < v && v <= max).then_some(v)
}

/// 「3~4」「3〜4」「3-4」のように % の直前が範囲なら、下限の数字（小数）
fn range_low(head: &str) -> Option<f64> {
    let tail: String = head
        .chars()
        .rev()
        .take_while(|c| c.is_ascii_digit() || matches!(c, '.' | '~' | '〜' | '～' | '-' | '－'))
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    let low = tail.split(['~', '〜', '～', '-', '－']).collect::<Vec<_>>();
    if low.len() < 2 {
        return None;
    }
    low[0].parse::<f64>().ok().map(|v| v / 100.0)
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

/// 半期報告書が、同じ事業年度かそれより後の有報で古くなったか（不要なので消してよい）。
/// EDINET の periodEnd は、半期報告書でも半期の末日ではなく事業年度の末日
pub fn semiannual_superseded(semiannual_period_end: Date, annual_period_end: Date) -> bool {
    annual_period_end >= semiannual_period_end
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
        assert_eq!(s.payout_goal, Some(0.35), "「以上を目標」は目標");
        assert_eq!(s.payout_floor, None);
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
    fn doe_floor_and_goal_from_policy_sentences() {
        let cases: &[(&str, Option<f64>, Option<f64>)] = &[
            ("「株主資本配当率1.5%」を下限に設定し「連結配当性向20%以上」を目安とする方針としております", Some(0.015), None),
            ("DOEの下限を従来の2.5%から3.5%へ引き上げると共に、総還元性向について新たに40%", Some(0.035), None),
            ("DOE3.5%下限(注2)及び総還元性向40%程度を目標", Some(0.035), None),
            ("自己資本配当率(DOE)」3.0%以上(安定した利益配分の指標)かつ、配当性向30%以上", Some(0.03), None),
            ("株主資本配当率(DOE)4.0%を目途とすることに変更いたします", None, Some(0.04)),
            ("DOE目標を3.5%程度に引上げました", None, Some(0.035)),
            ("DOE 10%程度を目線に、持続的な累進配当を行うことを目標としています", None, Some(0.1)),
            ("純資産配当率(DOE)目標4%以上」に変更いたしました", None, Some(0.04)),
            ("DOE3~4%を目安とします", None, Some(0.03)),
            ("DOE2.0%以上を確保することを基本方針とし", Some(0.02), None),
            ("DOE(株主資本配当率)2.5%以上の配当を基本", Some(0.025), None),
            ("純資産配当率(DOE)の段階的な引き上げを実施し、3%以上を目指します", None, Some(0.03)),
            ("従来のDOE3.5%水準から、総還元性向100%(フルペイアウト)に", None, None),
            ("DOE1.8%をその下限として設定しております", Some(0.018), None),
            ("自己資本配当率につきましては3.5%、連結配当性向につきましては58.2%となります", None, None),
            ("DOE0.5%相当の特別配当を実施いたします", None, None),
            ("当期のDOEは3.2%となりました", None, None),
            ("DOE(株主資本配当率)を採用しております", None, None),
        ];
        for (text, floor, goal) in cases {
            assert_eq!(policy_values(text, &DOE_RULE), (*floor, *goal), "{text}");
        }
    }

    #[test]
    fn detect_takes_largest_doe_values_and_floor_sentence() {
        let policy = "中期経営計画2025ではDOE2.5%を下限の配当水準としました。\
                      中期経営計画2028ではDOE3.5%を下限とします。中期的にはDOE4%を目標とします。";
        let s = detect(&[(Section::DividendPolicy, policy)]);
        assert_eq!(s.doe_floor, Some(0.035), "新しい計画の引き上げた下限");
        assert_eq!(s.doe_goal, Some(0.04));
        assert_eq!(
            s.doe_phrase.as_deref(),
            Some("中期経営計画2028ではDOE3.5%を下限とします。"),
            "下限の文を出す"
        );
        let goal_only = detect(&[(Section::DividendPolicy, "DOE4%を目途とします。")]);
        assert_eq!(
            goal_only.doe_phrase.as_deref(),
            Some("DOE4%を目途とします。")
        );
    }

    #[test]
    fn payout_floor_and_goal_from_policy_sentences() {
        let cases: &[(&str, Option<f64>, Option<f64>)] = &[
            ("配当性向は30~40%を目安とします", None, Some(0.3)),
            ("連結配当性向40%を目標", None, Some(0.4)),
            ("連結配当性向30%以上を基本方針とします", Some(0.3), None),
            (
                "配当性向の下限を従来の30%から35%へ引き上げます",
                Some(0.35),
                None,
            ),
            (
                "配当性向50%を下限とし、DOE3%以上を目安とします",
                Some(0.5),
                None,
            ),
            (
                "DOE3%以上を下限とし、配当性向30%程度を目安とします",
                None,
                Some(0.3),
            ),
            ("配当性向を高めていきます", None, None),
            ("当期の配当性向は58.2%となりました", None, None),
            (
                "配当性向については、業績や財務の状況を総合的に勘案したうえで決定し、20%",
                None,
                None,
            ),
        ];
        for (text, floor, goal) in cases {
            assert_eq!(policy_values(text, &PAYOUT_RULE), (*floor, *goal), "{text}");
        }
    }

    #[test]
    fn detect_splits_payout_into_floor_and_goal() {
        let policy = "配当性向30%以上を基本とし、中期的には配当性向40%を目指します。\
                      配当性向の向上に努めます。";
        let s = detect(&[(Section::DividendPolicy, policy)]);
        assert_eq!(s.payout_floor, Some(0.3));
        assert_eq!(s.payout_goal, Some(0.4));
        assert_eq!(s.payout_target, Some(0.3), "古い列は下限（なければ目標）");
        assert_eq!(
            s.payout_phrase.as_deref(),
            Some("配当性向30%以上を基本とし、中期的には配当性向40%を目指します。")
        );
        assert_eq!(
            s.phrases.len(),
            1,
            "数字のない配当性向の文は手がかりにしない"
        );
    }

    #[test]
    fn no_signals_in_plain_text() {
        let s = detect(&[(Section::Mdna, "売上高は前期比5%増加しました。")]);
        assert_eq!(s, DividendSignals::default());
        let business = detect(&[(Section::Business, "累進配当型の商品を販売しています。")]);
        assert_eq!(
            business,
            DividendSignals::default(),
            "事業の内容は配当方針として読まない"
        );
        assert_eq!(
            Section::from_element("DescriptionOfBusinessTextBlock"),
            Some(Section::Business)
        );
    }

    #[test]
    fn long_sentences_are_cut() {
        let long = format!("累進配当{}", "あ".repeat(300));
        let s = detect(&[(Section::DividendPolicy, &long)]);
        assert_eq!(s.phrases[0].chars().count(), MAX_PHRASE_CHARS + 1);
        assert!(s.phrases[0].ends_with('…'));
    }

    #[test]
    fn semiannual_is_superseded_by_the_same_years_annual() {
        let d = |s| Date::parse(s).unwrap();
        // 2026年3月期の半期報告書（2025年11月提出）と有報（2026年6月提出）
        assert!(semiannual_superseded(d("2026-03-31"), d("2026-03-31")));
        // 2027年3月期の半期報告書は、2026年3月期の有報より新しい
        assert!(!semiannual_superseded(d("2027-03-31"), d("2026-03-31")));
    }
}
