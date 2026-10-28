# Maa'un

「Maa'un」は、日本株の「割安で増配が期待できる銘柄」を探すためのダッシュボード（stock.insight.maa-un.com）。
このリポジトリは公開前提です。キーやトークンの値は絶対にコミットしないでください。

## 現在の段階: 技術検証（PoC）

10銘柄で次の流れを通すことが目的です。

1. J-Quants API（Light プラン）から日付ごとに全銘柄の終値と本決算の財務（年間配当を含む）を取得。初回だけ過去5年分、以後は前回の続きだけ
2. 終値の履歴は1銘柄1行にまとめて D1 に保存
3. 指標を計算（配当利回り・過去10年の利回りパーセンタイル・PBR・PER・配当性向・DOE・連続増配・赤字有無・営業利益の傾向）
4. Cloudflare D1 に保存（HTTP API）

## 構成（クリーンアーキテクチャ）

| crate | 役割 | 外部依存 |
| --- | --- | --- |
| `domain` | 銘柄・株価・財務のモデルと指標計算（純粋関数） | なし |
| `usecase` | 取り込みユースケースとポート（トレイト内 async fn） | なし |
| `adapter-batch` | J-Quants・JPX・D1 との接続、xlsx の解析 | reqwest, tokio(time), zip, serde_json |
| `batch-cli` | バッチの入口（並列実行・シャード分割） | tokio |
| `adapter-cf` | Worker 用: D1 の読み取り、スクリーニングの SQL 組み立て | worker（wasm32 のみ）, serde |
| `api-worker` | 画面向け API（workers-rs のルーター）。wasm32 専用 | worker, serde_json |

依存は最小限にしています。追加する場合は理由をこの表に書いてください。

## 画面（web/）

React + Vite + TypeScript、UI は shadcn/ui（使う部品だけ `web/src/components/ui` に取り込み）、ログインは Clerk の React SDK。画面は API Worker と同じオリジン（Workers の静的アセット）から配信し、`/api/*` だけ Worker を通します。セキュリティヘッダー（CSP など）は `web/public/_headers` にあります。

Node は 22.23.3（`.nvmrc`）。Volta を使っている場合、pnpm をこのバージョンで動かすには `VOLTA_FEATURE_PNPM=1` が必要です（古い Node だと Vite のネイティブ部品が入りません）。

```sh
cd web
pnpm install
pnpm lint && pnpm typecheck && pnpm test
pnpm build                       # web/dist を作る（wrangler はこれを配信する）
```

ローカルで画面ごと試す（偽の Clerk を使い、本物の Clerk・本番の D1 には触れない）:

```sh
node scripts/mock-clerk.mjs 9871 &                     # 偽の Clerk
npx wrangler dev --local --port 8787 &                 # API Worker（.dev.vars は上の API Worker の節）
cd web && VITE_MOCK_CLERK_URL=http://localhost:9871 pnpm dev   # http://localhost:5173
```

ローカルではログイン画面を経由せず、`user_member` として自動ログインします。Clerk の公開キーが未設定の開発サーバーでは、`VITE_MOCK_CLERK_URL` を省略しても `http://localhost:9871` の偽の Clerk を使います。`?mock_user=user_outsider` を付けて開くと、許可リストにないユーザーとして試せます。`VITE_MOCK_CLERK_URL` は開発サーバーでだけ有効で、本番のビルドには入りません。本番のビルドには `VITE_CLERK_PUBLISHABLE_KEY`（Clerk の公開キー）が必要です。

## コミット前のシークレット検査

キー・トークンを誤ってコミットしないよう、[gitleaks](https://github.com/gitleaks/gitleaks) をコミット前に動かします。clone したら一度だけ有効にしてください（CI の `secrets` ジョブでも全履歴を調べます）。

```sh
brew install gitleaks
git config core.hooksPath .githooks
```

誤検知は `.gitleaks.toml` の allowlist に足します。

## ドキュメント

仕様書は [docs/](docs/README.md) にあります（機能・権限・画面・API・データモデル・計算式・AI・運用・開発手順）。

版は `Cargo.toml` の `workspace.package.version` が元で、GitHub のタグ `v<version>` と一致させます（画面のフッターと `/api/status` に出ます）。

## 使い方

```sh
cargo test                                   # 単体テスト + オフライン通し試験
python3 scripts/verify_sql.py target/poc_offline.sql   # 出力SQLをSQLiteで検証

# 実データ（ネットワークとキーが必要）
cargo run -p batch-cli -- sync-master --dry-run master.sql   # JPX の上場銘柄一覧 → 銘柄マスタ
cargo run -p batch-cli -- sync-edinet --dry-run edinet.sql --from 2026-06-22 --until 2026-06-26 --limit 5   # EDINET（D1 に触れない）
cargo run -p batch-cli -- ingest --codes 7203,8058 --dry-run out.sql
cargo run -p batch-cli -- migrate
cargo run -p batch-cli -- ingest --codes 7203,8058 --shard 0/2 --concurrency 4
```

GitHub Actions の `daily-batch` ワークフローが日次バッチです。平日 18:30 JST に Cloudflare の Cron Trigger から起動され、定期実行では休場日（土日祝・12/31〜1/3）をスキップします。同日も毎回最新データを取得し、手動実行は休場日も動きます。手動で動かすときは銘柄コードを指定できます（`gh workflow run daily-batch -f codes=7203,8306`）。

### API Worker（ローカル）

```sh
cargo install worker-build --locked                       # 初回のみ
npx wrangler d1 execute maaun-stock-insight --local --file migrations/0001_init.sql
npx wrangler d1 execute maaun-stock-insight --local --file target/poc_offline.sql   # テストデータ
npx wrangler dev --local                                   # http://localhost:8787
curl "http://localhost:8787/api/stocks?dividend_yield_min=0.03&sort=pbr&order=asc"
curl "http://localhost:8787/api/status"
```

`--local` はローカルの D1 だけを使い、本番には触れません。

すべての `/api/*` は Clerk のセッショントークン（`Authorization: Bearer`）が必要です。ローカルでは偽の Clerk（`scripts/mock-clerk.mjs`）で本物と同じ流れ（RS256 の署名検証・JWKS・ユーザー取得）を試せます。

```sh
cat > .dev.vars <<'VARS'
CLERK_ISSUER=http://localhost:9871
CLERK_API_URL=http://localhost:9871/v1
AUTHORIZED_PARTIES=http://localhost:5173
CLERK_SECRET_KEY=sk_test_local_mock
ADMIN_NOTIFY_EMAIL=admin@example.test
VARS
npx wrangler d1 execute maaun-stock-insight --local --file migrations/0002_users.sql
node scripts/mock-clerk.mjs 9871 &
npx wrangler dev --local
curl -H "Authorization: Bearer $(curl -s localhost:9871/token/user_member)" localhost:8787/api/me
```

使えるのは、Google アカウントで確認済みで、かつ Clerk の `public_metadata` に `{"stock_insight": true}` があるユーザーと、`ADMIN_NOTIFY_EMAIL` と同じアドレスの管理者だけです。Clerk への問い合わせ結果は `users` に保存し、1時間ごとに確認し直します。

| エンドポイント | 内容 |
| --- | --- |
| `GET /api/me` | ログイン中のユーザー（管理者かどうかを含む） |
| `GET /api/stocks` | スクリーニング一覧。`<指標>_min` / `<指標>_max`（price, dividend_yield, yield_percentile, pbr, per, payout_ratio, doe, consecutive_increases, operating_margin, sales_growth, op_growth）、`valuation`、`no_loss_years`、`profit_uptrend`、`dividend_policy`（累進配当・非減配の記述あり）、`sort`、`order`、`page`（50件ずつ） |
| `GET /api/stocks/:code/edinet` | 有報・半期報告書（EDINET、最新1通ずつ）から拾った配当方針の手がかりと該当の文 |
| `GET /api/status` | 直近のバッチの状態と最新の株価日付 |

## シークレット（キー名のみ）

| キー名 | 設定先 | 用途 |
| --- | --- | --- |
| `JQUANTS_API_KEY` | GitHub Actions Secrets | J-Quants API（Light プラン） |
| `EDINET_API_KEY` | GitHub Actions Secrets | EDINET API v2（有報・半期報告書の配当方針の文章） |
| `CLOUDFLARE_ACCOUNT_ID` | GitHub Actions Secrets | D1 HTTP API の宛先 |
| `D1_DATABASE_ID` | GitHub Actions Secrets | 書き込み先の D1 |
| `CLOUDFLARE_API_TOKEN` | GitHub Actions Secrets | D1 への書き込み（権限は D1 編集のみ） |

D1・R2 などのリソースは wrangler コマンドで作成します（一覧は [docs/03](docs/03_system_architecture.md#配置とバインディング)）。シークレットは `gh secret set` / `wrangler secret put` で設定します（手順は [docs/06](docs/06_secrets_and_repository.md)）。

## 既知の制約（PoC時点）

- 営業利益は損益計算書から当期・前期の2期分のみ（経営指標の要約に営業利益が無いため）。3期以上は過去の有報を蓄積して補う
- 1株配当は有報の記載値のまま。株式分割の遡及調整は未対応
- DOE は簡易版（1株配当 ÷ BPS）
- 利回りパーセンタイルは配当履歴が1年以上・週次52点以上ある場合のみ算出
- D1 への書き込みは1文ずつ（全銘柄化の際にバッチ化する）
- J-Quants の規約により、データの配布と分析結果の継続提供は不可（当面は管理者1人で使う）

## 免責

情報提供のみを目的としており、投資助言ではありません。投資判断はご自身の責任で行ってください。
