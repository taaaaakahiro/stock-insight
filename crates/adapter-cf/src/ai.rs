//! Workers AI を呼ぶ（Recommender の実装）。モデルと依頼の中身は recommend.rs。

use crate::recommend::{parse_response, request_body, MODEL};
use domain::recommend::RawPick;
use serde_json::Value;
use usecase::recommend::Recommender;
use usecase::screening::StockRow;
use usecase::PortError;
use worker::Ai;

pub struct WorkersAi {
    ai: Ai,
}

impl WorkersAi {
    pub fn new(ai: Ai) -> Self {
        Self { ai }
    }
}

impl Recommender for WorkersAi {
    fn model(&self) -> &str {
        MODEL
    }

    async fn rank(&self, candidates: &[StockRow]) -> Result<Vec<RawPick>, PortError> {
        let output: Value = self
            .ai
            .run(MODEL, request_body(candidates))
            .await
            .map_err(|e| PortError(format!("ai: {e}")))?;
        parse_response(&output)
    }
}
