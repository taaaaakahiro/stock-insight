//! Workers AI を Cloudflare REST API 経由で呼ぶ（Recommender の実装、Actions 用）。
//! Worker 側の adapter-cf::ai（wasm 専用）と同じモデル・依頼・解釈を使う。

use adapter_cf::recommend::{parse_response, request_body, MODEL};
use domain::recommend::RawPick;
use serde_json::Value;
use usecase::recommend::{Explain, Recommender};
use usecase::PortError;

/// 混み合い（429）とサーバー側の一時的なエラー（5xx）だけをやり直す
const AI_RETRIES: u32 = 3;
const AI_FIRST_WAIT: std::time::Duration = std::time::Duration::from_secs(3);

pub struct WorkersAiRest {
    client: reqwest::Client,
    url: String,
    token: String,
}

impl WorkersAiRest {
    pub fn new(client: reqwest::Client, account_id: &str, token: String) -> Self {
        Self {
            client,
            url: format!(
                "https://api.cloudflare.com/client/v4/accounts/{account_id}/ai/run/{MODEL}"
            ),
            token,
        }
    }
}

impl Recommender for WorkersAiRest {
    fn model(&self) -> &str {
        MODEL
    }

    fn request(&self, picks: &[Explain]) -> String {
        request_body(picks).to_string()
    }

    /// REST の応答は `{success, result: {response: ...}}`。保存する応答本文は
    /// Worker 側と同じくモデルの出力（result）だけにする。
    async fn explain_recorded(
        &self,
        _picks: &[Explain],
        request: &str,
    ) -> Result<(String, Result<Vec<RawPick>, PortError>), PortError> {
        let body: Value =
            serde_json::from_str(request).map_err(|e| PortError(format!("ai: input: {e}")))?;
        let mut wait = AI_FIRST_WAIT;
        let mut attempt = 0;
        let res = loop {
            let res = self
                .client
                .post(&self.url)
                .bearer_auth(&self.token)
                .json(&body)
                .send()
                .await
                .map_err(|e| PortError(format!("ai: {e}")))?;
            let status = res.status();
            if status.is_success() {
                break res;
            }
            let msg = res.text().await.unwrap_or_default();
            let retryable = status.as_u16() == 429 || status.is_server_error();
            if retryable && attempt < AI_RETRIES {
                attempt += 1;
                eprintln!("ai: HTTP {status} {msg}; retry {attempt}/{AI_RETRIES}");
                tokio::time::sleep(wait).await;
                wait *= 2;
                continue;
            }
            return Err(PortError(format!("ai: HTTP {status}: {msg}")));
        };
        let v: Value = res
            .json()
            .await
            .map_err(|e| PortError(format!("ai: {e}")))?;
        if v["success"].as_bool() != Some(true) {
            return Err(PortError(format!("ai: {}", v["errors"])));
        }
        let output = &v["result"];
        Ok((output.to_string(), parse_response(output)))
    }
}
