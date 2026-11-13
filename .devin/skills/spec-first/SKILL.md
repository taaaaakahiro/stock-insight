---
name: spec-first
description: stock-insightリポジトリでは仕様書（docs/配下のmd）を先に変更してからコードを修正する。仕様とコードが乖離しないようにするための手順と確認ハーネス。
---

# 仕様ファースト（spec-first）

このリポジトリでは `docs/` 配下のMarkdownが現行仕様の正本です。

## ルール

1. **必ず仕様書を変更してからコードを修正する。** 実装変更・機能追加・挙動変更を行うときは、まず該当する `docs/*.md` の記述を更新し、その後にコードを直す。コードだけ先に変えて docs を後追いにしない。
2. docs とコードが食い違っている場合は、より良い方に統一する。コードの現行挙動が正しい場合は docs を直し、docs の仕様が正しい場合はコードを直す。どちらを正としたかはPR本文で一言説明する。
3. 既存の確認コマンド（`scripts/test_*.py`・cargo test）を回し、`python3 scripts/test_docs_consistency.py` で仕様書とコードの記述の乖離がないことを確認する。

## 確認ハーネス

CI とローカルで `python3 scripts/test_docs_consistency.py` を実行する。このスクリプトは以下を機械的に検査する:

- `docs/10_api.md` のスクリーニングmetric一覧 = `crates/domain/src/screening.rs` の `Metric::key()`
- `docs/13_batch_operations.md` のCLIコマンド・オプション一覧 = `crates/batch-cli/src/main.rs` のサブコマンド・フラグ
- `docs/19_stock_recommendation_quality.md` のルール版・プロンプト版 = `crates/usecase/src/recommend.rs` の `RULE_VERSION` / `prompt_version()`
- `docs/14_development_and_testing.md` の検証コマンド一覧 = `.github/workflows/ci.yml` が実行する `scripts/test_*.py`

検査が失敗したら先に仕様書かコードのどちらが正かを確認し、正しいほうを残してもう一方を直してから作業を進める。
