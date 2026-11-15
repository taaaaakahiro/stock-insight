#!/usr/bin/env python3
"""仕様書（docs/*.md）とコードの記述が乖離していないかを機械的に検査する。

docs を正本として、次の記述がコードと一致することを確認する:
- docs/10_api.md のスクリーニング metric 一覧 = screening.rs の Metric::key()
- docs/13_batch_operations.md の CLI コマンド・オプション = batch-cli/src/main.rs
- docs/19_stock_recommendation_quality.md のルール版・プロンプト版 = usecase/recommend.rs
- docs/14_development_and_testing.md の検証コマンド = ci.yml で実行する scripts/test_*.py
"""
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
failures = []


def check(name, expected, actual):
    missing = sorted(expected - actual)
    extra = sorted(actual - expected)
    if missing or extra:
        failures.append(
            f"{name}: docs にない={missing or 'なし'} / docs にあるがコード側にない={extra or 'なし'}"
        )


# 1. docs/10 の metric 一覧 == screening.rs Metric::key()
screening = (ROOT / "crates/domain/src/screening.rs").read_text()
key_block = screening.split("pub fn key(self)")[1].split("pub fn is_reit")[0]
code_metrics = set(re.findall(r'=>\s*"([a-z_]+)",', key_block))
api_doc = (ROOT / "docs/10_api.md").read_text()
m = re.search(r"metricは(\d+)種類:\n\n([^。]+)。", api_doc)
doc_metrics = set(re.findall(r"`([a-z_]+)`", m.group(2))) if m else set()
check("10_api.md metric一覧", doc_metrics, code_metrics)
if m and int(m.group(1)) != len(doc_metrics):
    failures.append(f"10_api.md: 「{m.group(1)}種類」と実数 {len(doc_metrics)} が不一致")

# 2. docs/13 の CLI コマンド一覧 == main.rs のサブコマンド
main = (ROOT / "crates/batch-cli/src/main.rs").read_text()
code_cmds = set(re.findall(r'^\s+"([a-z][a-z-]*)"\s*=>', main, re.M)) | set(
    re.findall(r'"([a-z][a-z-]*)"\s+if\s', main)
)
doc13 = (ROOT / "docs/13_batch_operations.md").read_text()
m = re.search(r"コマンド: ([^。]+)。", doc13)
doc_cmds = set(re.findall(r"`([a-z-]+)`", m.group(1))) if m else set()
check("13_batch_operations.md CLIコマンド", doc_cmds, code_cmds)

# 3. docs/13 のオプション一覧 == main.rs の "--xxx" フラグ
code_opts = set(re.findall(r'"(--[a-z-]+)"', main))
m = re.search(r"オプション: ([^。]+)。", doc13)
doc_opts = set(re.findall(r"`(--[a-z-]+)", m.group(1))) if m else set()
check("13_batch_operations.md CLIオプション", doc_opts, code_opts)

# 4. docs/19 のルール版・プロンプト版 == recommend.rs
recommend = (ROOT / "crates/usecase/src/recommend.rs").read_text()
rule_v = re.search(r'RULE_VERSION:\s*&str\s*=\s*"([^"]+)"', recommend).group(1)
prompt_v = re.search(r'"(explain-grade-financial-v\d+)"', recommend).group(1)
doc19 = (ROOT / "docs/19_stock_recommendation_quality.md").read_text()
for label, v in (("ルール版", rule_v), ("プロンプト版", prompt_v)):
    if v not in doc19:
        failures.append(f"19_stock_recommendation_quality.md: {label} `{v}` が記載と不一致")

# 5. docs/14 の検証コマンド == ci.yml が実行する scripts/test_*.py
ci = (ROOT / ".github/workflows/ci.yml").read_text()
ci_tests = set(re.findall(r"python3 (scripts/test_[a-z_]+\.py)", ci))
doc14 = (ROOT / "docs/14_development_and_testing.md").read_text()
doc_tests = set(re.findall(r"python3 (scripts/test_[a-z_]+\.py)", doc14))
check("14_development_and_testing.md 検証コマンド", doc_tests, ci_tests)

if failures:
    for f in failures:
        print(f"FAIL: {f}")
    sys.exit(1)
print("docs consistency OK")
