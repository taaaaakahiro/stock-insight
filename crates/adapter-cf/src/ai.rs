//! Workers AI を呼ぶ（Recommender の実装）。モデルと依頼の中身は recommend.rs。

use crate::recommend::{parse_response, request_body, MODEL};
use domain::recommend::RawPick;
use serde::Serialize;
use serde_json::Value;
use usecase::recommend::{Explain, Recommender};
use usecase::PortError;
use wasm_bindgen::JsValue;
use worker::Ai;

pub struct WorkersAi {
    ai: Ai,
}

impl WorkersAi {
    pub fn new(ai: Ai) -> Self {
        Self { ai }
    }
}

/// 組み立て済みの JS の値を、そのまま Ai::run に渡すための包み。
/// Ai::run は serde_wasm_bindgen の既定の変換を使い、serde_json::Value の
/// オブジェクトを JS の Map にしてしまう（Workers AI からは `messages` が無いように見える）
#[derive(Serialize)]
struct Input(#[serde(with = "serde_wasm_bindgen::preserve")] JsValue);

fn to_js_object(body: &Value) -> Result<JsValue, PortError> {
    body.serialize(&serde_wasm_bindgen::Serializer::json_compatible())
        .map_err(|e| PortError(format!("ai: input: {e}")))
}

impl Recommender for WorkersAi {
    fn model(&self) -> &str {
        MODEL
    }

    async fn explain(&self, picks: &[Explain]) -> Result<Vec<RawPick>, PortError> {
        let input = Input(to_js_object(&request_body(picks))?);
        let output: Value = self
            .ai
            .run(MODEL, input)
            .await
            .map_err(|e| PortError(format!("ai: {e}")))?;
        parse_response(&output)
    }
}
