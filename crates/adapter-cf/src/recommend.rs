//! Workers AI に渡す依頼（JSON）の組み立てと、答え（JSON）の解釈。ネイティブでテストできる部分。
//! 実際の呼び出しは ai.rs（wasm32 専用）。

use domain::indicators::CHEAP_PERCENTILE;
use domain::recommend::{RawPick, PICK_COUNT};
use serde::Deserialize;
use serde_json::{json, Value};
use usecase::screening::StockRow;
use usecase::PortError;

/// JSON モード（response_format の json_schema）に対応したテキスト生成モデル。
/// llama-3.1-8b-instruct-fp8 は JSON Schema に対応しておらず、5025 で断られる
pub const MODEL: &str = "@cf/meta/llama-3.3-70b-instruct-fp8-fast";

const SYSTEM_PROMPT: &str = "\
あなたは日本株の配当投資を手伝うアシスタントです。\
渡されるのは、ルールで絞り込んだ候補銘柄の最新の指標（JSON）です。\
各銘柄には「良い点」（こちらで判定済みの、値つきの短い文）がついています。\
「良い点」の多さと中身を見て、増配が期待できて割安な銘柄を上位10件選び、順位をつけてください。\
理由の書き方:\
(1) 日本語で1〜2文、全体で50〜110文字。銘柄名やコードから書き始めない（画面に銘柄名は出ている）。\
(2) その銘柄の「良い点」から2〜3個を選び、値と、括弧の中の説明の意味（なぜ良いのか）をそのまま使って、自然な日本語の文にする。値は書き換えない。括弧は使わなくてよい。\
書き方の例（値は例なので使わない）: 配当利回りが過去5年で上位10%の高さで、株価が過去と比べて割安。4年連続で増配しており、配当性向も30.0%と増配の余地がある。\
(3) 「良い点」に書かれていない評価（高い・低い・安い・余地がある など）を自分で付け足さない。「良い点」にないことには触れない。\
(4) 増配の根拠（連続増配・方針・配当性向・利益の伸び）が「良い点」にあれば、1つは入れる。\
(5) 10件で同じ書き出しや同じ項目の並びを使い回さない。\
「注意」がある銘柄は選ばないでください。\
候補にない銘柄や同じ銘柄を2回入れてはいけません。\
答えは指定した JSON だけを返し、他の文章は書かないでください。\
これは参考情報であり投資助言ではありません。";

/// 小数の比率を「4.8%」の形に（小数第1位まで）
fn pct(x: f64) -> String {
    format!("{:.1}%", x * 100.0)
}

/// これ以下の配当性向を「増配の余地がある」良い点にする
const GOOD_PAYOUT: f64 = 0.4;

/// 前年同期比を「+5.0%」の形に
fn growth(x: f64) -> String {
    format!("{:+.1}%", x * 100.0)
}

/// 候補の1銘柄分。AI に値の良し悪しを判断させると「PBR 1.20倍と低い」のような誤りが出るので、
/// こちらで判定した良い点だけを、値つきの日本語の短い文にして渡す（基準は一覧のバッジと同じ）。
/// 分からない値・当てはまらない点は入れない
fn candidate_json(r: &StockRow) -> Value {
    let yes = |b: Option<bool>| b == Some(true);
    let mut good: Vec<String> = Vec::new();
    if let Some(p) = r.yield_percentile.filter(|p| *p >= CHEAP_PERCENTILE) {
        // yield_percentile は「過去5年の中で今の利回りが下から何割か」。上位の割合に直す
        let top = ((1.0 - p) * 100.0).round().max(1.0) as i64;
        good.push(format!(
            "配当利回りが過去5年で上位{top}%の高さ（株価が過去と比べて割安）"
        ));
    }
    if let Some(pbr) = r.pbr.filter(|v| *v > 0.0 && *v <= 1.0) {
        good.push(format!(
            "PBR {pbr:.2}倍（1倍を下回り、純資産から見て株価が割安）"
        ));
    }
    if let Some(per) = r.per.filter(|v| *v > 0.0 && *v <= 15.0) {
        good.push(format!(
            "PER {per:.1}倍（15倍以下で、利益から見て株価が割安）"
        ));
    }
    if let Some(n) = r.consecutive_increases.filter(|n| *n > 0) {
        good.push(format!("{n}年連続で増配している"));
    }
    if yes(r.progressive_dividend) {
        good.push("有報で累進配当を掲げている".into());
    }
    if yes(r.no_dividend_cut) {
        good.push("有報で減配しない方針を掲げている".into());
    }
    // 配当性向は低いほど増配の余地がある。4割を超えると余地が大きいとは言えないので良い点にしない
    // （5割を超える銘柄は売りサインなので候補にしていない）
    if let Some(p) = r.payout_ratio.filter(|v| *v > 0.0 && *v <= GOOD_PAYOUT) {
        good.push(format!(
            "配当性向 {}（利益のうち配当に回す割合が4割以下で、増配の余地がある）",
            pct(p)
        ));
    }
    if let Some(g) = r.op_growth.filter(|g| *g > 0.0) {
        good.push(format!("営業利益が前年同期比 {}", growth(g)));
    }
    if let Some(g) = r.sales_growth.filter(|g| *g > 0.0) {
        good.push(format!("売上高が前年同期比 {}", growth(g)));
    }
    if yes(r.profit_uptrend) {
        good.push("利益が増える傾向".into());
    }
    if yes(r.dividend_every_year) {
        good.push("5年続けて配当を出している".into());
    }
    if yes(r.no_loss_years) {
        good.push("通期で5年赤字がない".into());
    }
    if yes(r.no_quarterly_loss) {
        good.push("四半期でも5年赤字がない".into());
    }

    let mut c = serde_json::Map::new();
    c.insert("コード".into(), json!(r.code));
    if let Some(n) = &r.name {
        c.insert("銘柄名".into(), json!(n));
    }
    if let Some(s) = &r.sector33 {
        c.insert("業種".into(), json!(s));
    }
    if let Some(y) = r.dividend_yield {
        let basis = match r.dividend_basis.as_deref() {
            Some("forecast") => "（会社予想の配当で計算）",
            Some("actual") => "（実績の配当で計算）",
            _ => "",
        };
        c.insert("配当利回り".into(), json!(format!("{}{basis}", pct(y))));
    }
    c.insert("良い点".into(), json!(good));
    if yes(r.dividend_cut) {
        c.insert(
            "注意".into(),
            json!(["直近で減配した、または減配の予想を出している"]),
        );
    }
    Value::Object(c)
}

/// Workers AI（テキスト生成、messages 形式）への入力
pub fn request_body(candidates: &[StockRow]) -> Value {
    let user = json!({ "candidates": candidates.iter().map(candidate_json).collect::<Vec<_>>() });
    json!({
        "messages": [
            { "role": "system", "content": SYSTEM_PROMPT },
            { "role": "user", "content": user.to_string() },
        ],
        "response_format": {
            "type": "json_schema",
            "json_schema": {
                "type": "object",
                "properties": {
                    "picks": {
                        "type": "array",
                        "minItems": PICK_COUNT,
                        "maxItems": PICK_COUNT,
                        "items": {
                            "type": "object",
                            "properties": {
                                "code": { "type": "string" },
                                "reason": { "type": "string" },
                            },
                            "required": ["code", "reason"],
                        },
                    },
                },
                "required": ["picks"],
            },
        },
        "max_tokens": 2048,
        "temperature": 0.2,
    })
}

#[derive(Deserialize)]
struct Answer {
    picks: Vec<AnswerPick>,
}

#[derive(Deserialize)]
struct AnswerPick {
    code: String,
    reason: String,
}

/// Workers AI の応答（`{"response": ...}`）から picks を取り出す。
/// response は JSON の文字列のことも、すでに JSON のこともある。壊れていれば Err
pub fn parse_response(output: &Value) -> Result<Vec<RawPick>, PortError> {
    let bad = |what: &str| PortError(format!("ai: {what}"));
    let response = output
        .get("response")
        .ok_or_else(|| bad("no response field"))?;
    let answer: Answer = match response {
        Value::String(s) => serde_json::from_str(strip_fence(s))
            .map_err(|e| bad(&format!("response is not the expected json: {e}")))?,
        other => serde_json::from_value(other.clone())
            .map_err(|e| bad(&format!("response is not the expected json: {e}")))?,
    };
    Ok(answer
        .picks
        .into_iter()
        .map(|p| RawPick {
            code: p.code,
            reason: p.reason,
        })
        .collect())
}

/// ```json ... ``` で囲んで返してきたときに中身だけにする
fn strip_fence(s: &str) -> &str {
    let t = s.trim();
    let Some(rest) = t.strip_prefix("```") else {
        return t;
    };
    let rest = rest.strip_prefix("json").unwrap_or(rest);
    rest.strip_suffix("```").unwrap_or(rest).trim()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(code: &str) -> StockRow {
        StockRow {
            code: code.into(),
            price: 1234.5,
            price_date: "2026-10-21".into(),
            dividend: None,
            dividend_basis: None,
            dividend_yield: Some(0.04123),
            yield_percentile: Some(0.8),
            pbr: Some(0.876),
            per: Some(9.87654),
            payout_ratio: Some(0.3),
            doe: None,
            consecutive_increases: Some(3),
            operating_margin: Some(0.123),
            no_loss_years: Some(true),
            profit_uptrend: Some(true),
            valuation: "cheap".into(),
            fetched_at: 0,
            name: Some("テスト工業".into()),
            kind: "stock".into(),
            market: Some("プライム".into()),
            sector33: Some("機械".into()),
            size_class: Some("mid".into()),
            dividend_every_year: Some(true),
            no_quarterly_loss: None,
            is_value: Some(true),
            data_years: Some(5),
            sales_growth: None,
            op_growth: None,
            dividend_cut: None,
            progressive_dividend: None,
            no_dividend_cut: None,
            doe_floor: None,
            doe_goal: None,
            doe_forecast: None,
            reit: None,
        }
    }

    #[test]
    fn request_carries_candidates_as_json_and_asks_for_json() {
        let body = request_body(&[row("7203")]);
        assert_eq!(body["messages"][0]["role"], "system");
        let user: Value = serde_json::from_str(body["messages"][1]["content"].as_str().unwrap())
            .expect("user content is json");
        let c = &user["candidates"][0];
        assert_eq!(c["コード"], "7203");
        assert_eq!(c["銘柄名"], "テスト工業");
        assert_eq!(c["配当利回り"], "4.1%");
        assert_eq!(
            c["良い点"],
            json!([
                "配当利回りが過去5年で上位20%の高さ（株価が過去と比べて割安）",
                "PBR 0.88倍（1倍を下回り、純資産から見て株価が割安）",
                "PER 9.9倍（15倍以下で、利益から見て株価が割安）",
                "3年連続で増配している",
                "配当性向 30.0%（利益のうち配当に回す割合が4割以下で、増配の余地がある）",
                "利益が増える傾向",
                "5年続けて配当を出している",
                "通期で5年赤字がない"
            ])
        );
        assert!(c.get("注意").is_none());
        assert_eq!(body["response_format"]["type"], "json_schema");
        assert_eq!(
            body["response_format"]["json_schema"]["properties"]["picks"]["maxItems"],
            10
        );
    }

    #[test]
    fn candidate_lists_only_what_passes_the_bar() {
        let mut r = row("8306");
        r.consecutive_increases = Some(0);
        r.yield_percentile = Some(0.5);
        r.pbr = Some(1.2);
        r.per = Some(16.0);
        r.payout_ratio = Some(0.45);
        r.profit_uptrend = Some(false);
        r.op_growth = Some(-0.05);
        r.sales_growth = Some(0.031);
        r.dividend_basis = Some("forecast".into());
        r.dividend_cut = Some(true);
        let c = candidate_json(&r);
        assert_eq!(c["配当利回り"], "4.1%（会社予想の配当で計算）");
        assert_eq!(
            c["良い点"],
            json!([
                "売上高が前年同期比 +3.1%",
                "5年続けて配当を出している",
                "通期で5年赤字がない"
            ]),
            "0年の増配・基準に届かない値・減益は良い点にしない"
        );
        assert_eq!(
            c["注意"],
            json!(["直近で減配した、または減配の予想を出している"])
        );

        r.yield_percentile = Some(0.996);
        assert_eq!(
            candidate_json(&r)["良い点"][0],
            "配当利回りが過去5年で上位1%の高さ（株価が過去と比べて割安）",
            "上位0%とは書かない"
        );
    }

    #[test]
    fn parses_string_or_object_response() {
        let as_string = json!({
            "response": "```json\n{\"picks\":[{\"code\":\"7203\",\"reason\":\"割安\"}]}\n```"
        });
        let picks = parse_response(&as_string).unwrap();
        assert_eq!(
            picks,
            vec![RawPick {
                code: "7203".into(),
                reason: "割安".into()
            }]
        );

        let as_object = json!({ "response": { "picks": [{ "code": "8306", "reason": "増配" }] } });
        assert_eq!(parse_response(&as_object).unwrap()[0].code, "8306");
    }

    #[test]
    fn broken_response_is_an_error() {
        assert!(parse_response(&json!({ "result": "x" })).is_err());
        assert!(parse_response(&json!({ "response": "上位10銘柄は…" })).is_err());
        assert!(parse_response(&json!({ "response": { "items": [] } })).is_err());
        assert!(
            parse_response(&json!({ "response": "{\"picks\":[{\"code\":\"7203\"}]}" })).is_err()
        );
    }
}
