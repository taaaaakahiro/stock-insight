//! Workers AI に渡す依頼（JSON）の組み立てと、答え（JSON）の解釈。ネイティブでテストできる部分。
//! 実際の呼び出しは ai.rs（wasm32 専用）。

use domain::recommend::{RawPick, PICK_COUNT};
use serde::Deserialize;
use serde_json::{json, Value};
use usecase::screening::StockRow;
use usecase::PortError;

/// 無料枠で動く軽いテキスト生成モデル（JSON モード対応）
pub const MODEL: &str = "@cf/meta/llama-3.1-8b-instruct-fp8";

const SYSTEM_PROMPT: &str = "\
あなたは日本株の配当投資を手伝うアシスタントです。\
渡されるのは、ルールで絞り込んだ候補銘柄の最新の指標（JSON）です。\
数字の計算はせず、与えられた指標だけを根拠に、増配が期待できて割安な銘柄を上位10件選び、順位をつけてください。\
各銘柄の理由は日本語で1〜2文、指標の値に触れて具体的に書いてください。\
候補にない銘柄や同じ銘柄を2回入れてはいけません。\
答えは指定した JSON だけを返し、他の文章は書かないでください。\
これは参考情報であり投資助言ではありません。";

/// 候補の1銘柄分。比率は % にし、桁を落としてトークンを節約する
fn candidate_json(r: &StockRow) -> Value {
    let pct = |x: Option<f64>| x.map(|v| (v * 1000.0).round() / 10.0);
    let times = |x: Option<f64>| x.map(|v| (v * 100.0).round() / 100.0);
    json!({
        "code": r.code,
        "name": r.name,
        "sector": r.sector33,
        "price": r.price,
        "dividend_yield_pct": pct(r.dividend_yield),
        "yield_percentile_5y_pct": pct(r.yield_percentile),
        "pbr": times(r.pbr),
        "per": times(r.per),
        "payout_ratio_pct": pct(r.payout_ratio),
        "doe_pct": pct(r.doe),
        "consecutive_dividend_increases": r.consecutive_increases,
        "operating_margin_pct": pct(r.operating_margin),
        "is_value": r.is_value,
        "dividend_every_year_5y": r.dividend_every_year,
        "no_loss_5y": r.no_loss_years,
        "no_quarterly_loss_5y": r.no_quarterly_loss,
        "profit_uptrend": r.profit_uptrend,
    })
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
            progressive_dividend: None,
            no_dividend_cut: None,
        }
    }

    #[test]
    fn request_carries_candidates_as_json_and_asks_for_json() {
        let body = request_body(&[row("7203")]);
        assert_eq!(body["messages"][0]["role"], "system");
        let user: Value = serde_json::from_str(body["messages"][1]["content"].as_str().unwrap())
            .expect("user content is json");
        let c = &user["candidates"][0];
        assert_eq!(c["code"], "7203");
        assert_eq!(c["name"], "テスト工業");
        assert_eq!(c["dividend_yield_pct"], 4.1);
        assert_eq!(c["pbr"], 0.88);
        assert_eq!(c["per"], 9.88);
        assert_eq!(c["doe_pct"], Value::Null);
        assert_eq!(body["response_format"]["type"], "json_schema");
        assert_eq!(
            body["response_format"]["json_schema"]["properties"]["picks"]["maxItems"],
            10
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
