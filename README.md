# stock-insight

日本株の「割安で増配が期待できる銘柄」を探すためのダッシュボード（stock.insight.maa-un.com）。
このリポジトリは公開前提です。キーやトークンの値は絶対にコミットしないでください。

## 現在の段階: 技術検証（PoC）

10銘柄で次の流れを通すことが目的です。

1. Yahoo Finance（非公式）から10年分の日次終値と配当履歴を取得
2. EDINET API v2 から最新の有価証券報告書（CSV版）を取得し、5期分の財務を抽出
3. 指標を計算（配当利回り・過去10年の利回りパーセンタイル・PBR・PER・配当性向・DOE・連続増配・赤字有無・営業利益の傾向）
4. Cloudflare D1 に保存（HTTP API）

## 構成（クリーンアーキテクチャ）

| crate | 役割 | 外部依存 |
| --- | --- | --- |
| `domain` | 銘柄・株価・財務のモデルと指標計算（純粋関数） | なし |
| `usecase` | 取り込みユースケースとポート（トレイト内 async fn） | なし |
| `adapter-batch` | Yahoo・EDINET・D1 との接続、XBRL CSV の解析 | reqwest, tokio(time), zip, serde_json |
| `batch-cli` | バッチの入口（並列実行・シャード分割） | tokio |

依存は最小限にしています。追加する場合は理由をこの表に書いてください。

## ドキュメント

仕様書は [docs/](docs/README.md) にあります（要件定義・非機能要件・システム構成図・基本設計・詳細設計・シークレット運用）。

## 使い方

```sh
cargo test                                   # 単体テスト + オフライン通し試験
python3 scripts/verify_sql.py target/poc_offline.sql   # 出力SQLをSQLiteで検証

# 実データ（ネットワークとキーが必要）
cargo run -p batch-cli -- ingest --codes 7203,8058 --dry-run out.sql
cargo run -p batch-cli -- migrate
cargo run -p batch-cli -- ingest --codes 7203,8058 --shard 0/2 --concurrency 4
```

GitHub Actions の `poc-ingest` ワークフロー（手動実行）でも同じことができます。

## シークレット（キー名のみ）

| キー名 | 設定先 | 用途 |
| --- | --- | --- |
| `EDINET_API_KEY` | GitHub Actions Secrets | EDINET API |
| `CLOUDFLARE_ACCOUNT_ID` | GitHub Actions Secrets | D1 HTTP API の宛先 |
| `D1_DATABASE_ID` | GitHub Actions Secrets | 書き込み先の D1 |
| `CLOUDFLARE_API_TOKEN` | GitHub Actions Secrets | D1 への書き込み（権限は D1 編集のみ） |

D1・R2 などのリソースは wrangler コマンドで作成します（一覧は [docs/03](docs/03_system_architecture.md#作成済みのリソース)）。シークレットは `gh secret set` / `wrangler secret put` で設定します（手順は [docs/06](docs/06_secrets_and_repository.md)）。

## 既知の制約（PoC時点）

- 営業利益は損益計算書から当期・前期の2期分のみ（経営指標の要約に営業利益が無いため）。3期以上は過去の有報を蓄積して補う
- 1株配当は有報の記載値のまま。株式分割の遡及調整は未対応
- DOE は簡易版（1株配当 ÷ BPS）
- 利回りパーセンタイルは配当履歴が1年以上・週次52点以上ある場合のみ算出
- D1 への書き込みは1文ずつ（全銘柄化の際にバッチ化する）
- Yahoo Finance は非公式のため、仕様変更で取得できなくなる可能性がある（代替候補: J-Quants 有料プラン）

## 免責

情報提供のみを目的としており、投資助言ではありません。投資判断はご自身の責任で行ってください。
