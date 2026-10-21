//! HTTP の共通処理（タイムアウト・再試行）。リトライは自前で実装する。

use std::time::Duration;
use usecase::PortError;

pub use reqwest::Client;

pub fn client() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .user_agent("stock-insight-batch/0.1 (+https://stock.insight.maa-un.com)")
        .build()
        .expect("http client")
}

/// GET してボディを返す。429・5xx・通信エラーは指数バックオフで最大3回まで再試行する。
pub async fn get_bytes(client: &reqwest::Client, url: &str) -> Result<Vec<u8>, PortError> {
    get_bytes_with_headers(client, url, &[]).await
}

/// ヘッダー付きの GET（API キーをヘッダーで渡すときに使う。値はエラー文に出さない）
pub async fn get_bytes_with_headers(
    client: &reqwest::Client,
    url: &str,
    headers: &[(&str, &str)],
) -> Result<Vec<u8>, PortError> {
    let mut wait = Duration::from_millis(500);
    let mut last = String::new();
    for attempt in 0..4 {
        if attempt > 0 {
            tokio::time::sleep(wait).await;
            wait *= 2;
        }
        let mut req = client.get(url);
        for (k, v) in headers {
            req = req.header(*k, *v);
        }
        match req.send().await {
            Ok(res) if res.status().is_success() => {
                return res
                    .bytes()
                    .await
                    .map(|b| b.to_vec())
                    .map_err(|e| PortError(format!("{} ({})", e.without_url(), redact(url))));
            }
            Ok(res) => {
                let s = res.status();
                last = format!("HTTP {s} ({})", redact(url));
                if !(s.as_u16() == 429 || s.is_server_error()) {
                    break;
                }
            }
            // エラー文にURL（APIキーを含む）を出さない
            Err(e) => last = format!("{} ({})", e.without_url(), redact(url)),
        }
    }
    Err(PortError(last))
}

/// URLからクエリ文字列（APIキーを含みうる）を除いてログ用にする
pub fn redact(url: &str) -> &str {
    url.split('?').next().unwrap_or(url)
}

#[cfg(test)]
mod tests {
    #[test]
    fn redact_strips_query() {
        assert_eq!(
            super::redact("https://x/y?Subscription-Key=secret"),
            "https://x/y"
        );
    }
}
