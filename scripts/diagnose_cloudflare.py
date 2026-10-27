"""Cloudflare auth probes (read-only unless --ai). Never log credentials, URLs, or raw bodies."""
import datetime
import json
import os
import re
import sys
from pathlib import Path
import time
import urllib.error
import urllib.request

BASE = "https://api.cloudflare.com/client/v4"


def request(label, path, payload=None):
    token = os.environ["CLOUDFLARE_API_TOKEN"]
    if re.match(r"^/accounts/[A-Za-z0-9]+/ai/", path):
        token = os.environ.get("CLOUDFLARE_AI_API_TOKEN", "").strip() or token
    req = urllib.request.Request(
        BASE + path,
        data=None if payload is None else json.dumps(payload).encode(),
        headers={"Authorization": "Bearer " + token,
                 "Content-Type": "application/json", "User-Agent": "stock-insight-diagnostics/1"},
    )
    started = time.monotonic()
    try:
        response = urllib.request.urlopen(req, timeout=30)
    except urllib.error.HTTPError as exc:
        response = exc
    except (urllib.error.URLError, TimeoutError):
        print(json.dumps({"probe": label, "transport_error": True}), flush=True)
        return None
    with response:
        try:
            body = json.load(response)
        except (ValueError, UnicodeDecodeError):
            body = {}
        print(json.dumps({
            "at": datetime.datetime.now(datetime.timezone.utc).isoformat(),
            "probe": label, "http": response.code,
            "success": body.get("success"),
            "codes": [e.get("code") for e in body.get("errors", []) if isinstance(e, dict)],
            "cf_ray": response.headers.get("cf-ray"),
            "ratelimit": response.headers.get("ratelimit"),
            "retry_after": response.headers.get("retry-after"),
            "elapsed_ms": round((time.monotonic()-started)*1000),
        }), flush=True)
        return body


def main(ai=False):
    for key in ["CLOUDFLARE_API_TOKEN", "CLOUDFLARE_ACCOUNT_ID", "D1_DATABASE_ID"]:
        if not os.environ.get(key):
            raise SystemExit(f"missing {key}")
    account = os.environ["CLOUDFLARE_ACCOUNT_ID"]
    database = os.environ["D1_DATABASE_ID"]
    verified = request("user-token-verify", "/user/tokens/verify")
    if not verified or not verified.get("success"):
        verified = request("account-token-verify", f"/accounts/{account}/tokens/verify")
    if verified and isinstance(verified.get("result"), dict):
        result = verified["result"]
        print(json.dumps({"token_status": result.get("status"), "expires_on": result.get("expires_on")}), flush=True)
    path = f"/accounts/{account}/d1/database/{database}/query"
    if ai:
        model = re.search(r'pub const TEXT_MODEL: &str = "([^"]+)"',
                          (Path(__file__).resolve().parents[1] / "crates/usecase/src/ai_config.rs").read_text()).group(1)
        db = request("d1-select", path, {"sql": "SELECT 1 AS ok", "params": []})
        result = request("workers-ai-inference", f"/accounts/{account}/ai/run/{model}",
                         {"messages": [{"role": "user", "content": "Reply OK."}], "max_tokens": 1})
        if not db or not db.get("success") or not result or not result.get("success"):
            raise SystemExit(1)
        return
    successes = 0
    for i in range(8):
        if i:
            time.sleep(5)
        result = request(f"d1-select-{i+1}", path, {"sql": "SELECT 1 AS ok", "params": []})
        successes += bool(result and result.get("success"))
    # Aggregate counts only; no documents, holdings, user data or financial rows are logged.
    result = request("reit-storage-counts", path, {"sql": "SELECT (SELECT COUNT(*) FROM reit_rag_documents WHERE state='ready') AS ready_documents, (SELECT COUNT(*) FROM reit_rag_documents WHERE state='staging') AS staging_documents, (SELECT COUNT(*) FROM reit_rag_chunks) AS chunks, (SELECT COUNT(*) FROM reit_rag_runs) AS analysis_runs, (SELECT COUNT(*) FROM reit_rag_publication) AS publications", "params": []})
    if result and result.get("success"):
        rows = result.get("result", [{}])[0].get("results", [])
        if rows:
            print(json.dumps({"reit_counts": {k: v for k, v in rows[0].items() if isinstance(v, int)}}), flush=True)
    print(json.dumps({"d1_successful_probes": successes, "d1_total_probes": 8}), flush=True)
    if successes != 8:
        raise SystemExit(1)


if __name__ == "__main__":
    main(ai="--ai" in sys.argv[1:])
