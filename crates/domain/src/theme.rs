//! 注目テーマ（半導体・防衛・データセンターなど）。管理者がテーマ名と言葉を登録し、
//! 有価証券報告書の「事業の内容」の文にその言葉があれば、その銘柄をテーマ株とみなす。
//! 流行りは人が決め、対応づけ（照合）は機械でやる。

use crate::disclosure::normalize_text;

/// D1 に持つ「事業の内容」の文字数の上限。先頭に主な事業が書かれるので、ここまでで足りる
pub const BUSINESS_TEXT_CHARS: usize = 3000;
/// テーマ名の長さ、言葉の数と長さの上限
pub const MAX_NAME_CHARS: usize = 30;
pub const MAX_WORDS: usize = 20;
pub const MAX_WORD_CHARS: usize = 30;

/// 「事業の内容」の文を D1 に持つ形にする（整形済みの文の先頭 BUSINESS_TEXT_CHARS 文字）
pub fn business_excerpt(text: &str) -> String {
    text.chars().take(BUSINESS_TEXT_CHARS).collect()
}

/// 登録できる形に整えたテーマ
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThemeInput {
    pub name: String,
    /// 照合に使う言葉（整形済み・重複なし・入力の順）
    pub words: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ThemeError {
    EmptyName,
    NameTooLong,
    NoWords,
    TooManyWords,
    WordTooLong(String),
}

impl std::fmt::Display for ThemeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ThemeError::EmptyName => write!(f, "name is empty"),
            ThemeError::NameTooLong => write!(f, "name is longer than {MAX_NAME_CHARS} chars"),
            ThemeError::NoWords => write!(f, "no words"),
            ThemeError::TooManyWords => write!(f, "more than {MAX_WORDS} words"),
            ThemeError::WordTooLong(w) => {
                write!(f, "word {w:?} is longer than {MAX_WORD_CHARS} chars")
            }
        }
    }
}

/// 管理者の入力を整える。言葉は「事業の内容」の文と同じ整形（全角英数字を半角に、空白をまとめる）をして
/// 突き合わせが合うようにし、空のものと重複は除く
pub fn validate_theme(name: &str, words: &[String]) -> Result<ThemeInput, ThemeError> {
    let name = normalize_text(name).trim().to_string();
    if name.is_empty() {
        return Err(ThemeError::EmptyName);
    }
    if name.chars().count() > MAX_NAME_CHARS {
        return Err(ThemeError::NameTooLong);
    }
    let mut out: Vec<String> = Vec::new();
    for w in words {
        let w = normalize_text(w).trim().to_string();
        if w.is_empty() || out.contains(&w) {
            continue;
        }
        if w.chars().count() > MAX_WORD_CHARS {
            return Err(ThemeError::WordTooLong(w));
        }
        out.push(w);
    }
    if out.is_empty() {
        return Err(ThemeError::NoWords);
    }
    if out.len() > MAX_WORDS {
        return Err(ThemeError::TooManyWords);
    }
    Ok(ThemeInput { name, words: out })
}

/// 「事業の内容」の文に言葉のどれかがあるか。あれば最初に見つかった言葉（D1 の instr と同じ、単純な部分一致）
pub fn matched_word<'a>(business_text: &str, words: &'a [String]) -> Option<&'a str> {
    words
        .iter()
        .find(|w| business_text.contains(w.as_str()))
        .map(String::as_str)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(ws: &[&str]) -> Vec<String> {
        ws.iter().map(|w| w.to_string()).collect()
    }

    #[test]
    fn validates_and_normalizes_the_input() {
        let t = validate_theme(
            " 半導体 ",
            &words(&["半導体", "ＳｉＣ", " ", "半導体", "パワー半導体"]),
        )
        .unwrap();
        assert_eq!(t.name, "半導体");
        assert_eq!(
            t.words,
            words(&["半導体", "SiC", "パワー半導体"]),
            "全角は半角に、空と重複は除く"
        );
        assert_eq!(
            validate_theme("", &words(&["a"])),
            Err(ThemeError::EmptyName)
        );
        assert_eq!(
            validate_theme(&"あ".repeat(31), &words(&["a"])),
            Err(ThemeError::NameTooLong)
        );
        assert_eq!(
            validate_theme("x", &words(&[" ", ""])),
            Err(ThemeError::NoWords)
        );
        let many: Vec<String> = (0..21).map(|i| format!("w{i}")).collect();
        assert_eq!(validate_theme("x", &many), Err(ThemeError::TooManyWords));
        assert_eq!(
            validate_theme("x", &words(&[&"あ".repeat(31)])),
            Err(ThemeError::WordTooLong("あ".repeat(31)))
        );
    }

    #[test]
    fn matches_the_first_word_found() {
        let text = "当社グループは、パワー半導体の製造装置と検査装置を主な事業としております";
        assert_eq!(
            matched_word(text, &words(&["データセンター", "半導体", "検査装置"])),
            Some("半導体")
        );
        assert_eq!(matched_word(text, &words(&["防衛"])), None);
    }

    #[test]
    fn excerpt_is_cut_at_the_limit() {
        let long = "あ".repeat(BUSINESS_TEXT_CHARS + 10);
        assert_eq!(business_excerpt(&long).chars().count(), BUSINESS_TEXT_CHARS);
        assert_eq!(business_excerpt("短い"), "短い");
    }
}
