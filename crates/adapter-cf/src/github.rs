//! GitHub Actions の workflow_dispatch を呼ぶ（Cron Trigger から日次バッチを起動する）。

use worker::wasm_bindgen::JsValue;
use worker::{Fetch, Headers, Method, Request, RequestInit};

/// `repo` は "owner/name"。成功（204）なら Ok。トークンはヘッダーで渡し、エラー文に含めない
pub async fn dispatch_workflow(
    repo: &str,
    workflow_file: &str,
    git_ref: &str,
    token: &str,
) -> Result<(), String> {
    let url =
        format!("https://api.github.com/repos/{repo}/actions/workflows/{workflow_file}/dispatches");
    let headers = Headers::new();
    let set = |k: &str, v: &str| headers.set(k, v).map_err(|e| e.to_string());
    set("Authorization", &format!("Bearer {token}"))?;
    set("Accept", "application/vnd.github+json")?;
    set("X-GitHub-Api-Version", "2022-11-28")?;
    set("Content-Type", "application/json")?;
    // GitHub の API は User-Agent が必須
    set("User-Agent", "stock-insight-cron")?;
    let body = serde_json::json!({ "ref": git_ref }).to_string();
    let mut init = RequestInit::new();
    init.with_method(Method::Post)
        .with_headers(headers)
        .with_body(Some(JsValue::from_str(&body)));
    let req = Request::new_with_init(&url, &init).map_err(|e| e.to_string())?;
    let res = Fetch::Request(req)
        .send()
        .await
        .map_err(|e| e.to_string())?;
    match res.status_code() {
        204 => Ok(()),
        s => Err(format!("workflow_dispatch returned HTTP {s}")),
    }
}
