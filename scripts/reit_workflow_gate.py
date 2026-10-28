"""Gate the independent REIT workflow without running or modifying the daily batch."""
import datetime
import json
import os
import re
import urllib.request
from pathlib import Path


def select_run(event_name, event, repository, enabled, fetch_jobs):
    if event_name == "workflow_dispatch":
        inputs = event.get("inputs") or {}
        mode = inputs.get("mode", "full")
        run_id = inputs.get("rag_run_id", "")
        refill_date = inputs.get("refill_price_date", "")
        if refill_date:
            if mode != "full" or not re.fullmatch(r"[0-9]{4}-[0-9]{2}-[0-9]{2}", refill_date):
                raise ValueError("refill_price_date requires full mode and YYYY-MM-DD")
            datetime.date.fromisoformat(refill_date)
        if mode not in ("full", "analyze", "resume", "diagnose", "ai-diagnose"):
            raise ValueError("invalid REIT mode")
        if mode == "resume":
            if not re.fullmatch(r"[A-Za-z0-9-]{1,128}", run_id):
                raise ValueError("resume requires a valid rag_run_id")
        elif run_id:
            raise ValueError("rag_run_id is only supported in resume mode")
        if mode in ("analyze", "resume") and not enabled:
            raise ValueError("REIT_RAG_ENABLED must be true for analysis")
        return True, mode, run_id
    if event_name != "workflow_run":
        return False, "full", ""
    source = event.get("workflow_run") or {}
    if (source.get("name") != "daily-batch" or source.get("conclusion") != "success"
            or source.get("head_branch") != "main"
            or (source.get("head_repository") or {}).get("full_name") != repository):
        return False, "full", ""
    # A successful no-op (holiday/already processed) or dry-run must not trigger REIT work.
    jobs = fetch_jobs(int(source["id"]), int(source["run_attempt"]))
    run = any(job.get("name") == "jquants" and job.get("conclusion") == "success" for job in jobs)
    return run, "full", ""


def main():
    repository = os.environ["GITHUB_REPOSITORY"]
    event = json.loads(Path(os.environ["GITHUB_EVENT_PATH"]).read_text())

    def fetch_jobs(run_id, attempt):
        url = f"https://api.github.com/repos/{repository}/actions/runs/{run_id}/attempts/{attempt}/jobs?per_page=100"
        req = urllib.request.Request(url, headers={
            "Authorization": "Bearer " + os.environ["GH_TOKEN"],
            "Accept": "application/vnd.github+json", "User-Agent": "stock-insight-reit-gate",
        })
        with urllib.request.urlopen(req, timeout=30) as response:
            body = json.load(response)
        if body.get("total_count", 0) > 100:
            raise ValueError("too many upstream jobs to verify")
        return body.get("jobs", [])

    run, mode, run_id = select_run(os.environ["GITHUB_EVENT_NAME"], event, repository,
                                   os.environ.get("REIT_RAG_ENABLED") == "true", fetch_jobs)
    with open(os.environ["GITHUB_OUTPUT"], "a") as output:
        output.write(f"run={str(run).lower()}\nmode={mode}\nrun_id={run_id}\n")
    print(f"REIT workflow: mode={mode}, run={str(run).lower()}")


if __name__ == "__main__":
    main()
