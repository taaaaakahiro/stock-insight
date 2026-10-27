# 06 設定・シークレット

## 設定一覧

| 名前 | 設定先 | 用途 |
|---|---|---|
| `CLERK_ISSUER` | Worker vars | JWT issuer・JWKS取得先 |
| `CLERK_API_URL` | Worker vars | Clerk Backend API。既定 `https://api.clerk.com/v1` |
| `AUTHORIZED_PARTIES` | Worker vars | 許可するJWTのazp（カンマ区切り） |
| `GITHUB_REPO` | Worker vars | 日次バッチを起動するリポジトリ |
| `CLERK_SECRET_KEY` | Worker Secret | ユーザー情報の取得 |
| `ADMIN_NOTIFY_EMAIL` | Worker Secret | 管理者の主メールアドレス。現行ではメール送信には使わない |
| `GITHUB_DISPATCH_TOKEN` | Worker Secret | 対象リポジトリのActions起動 |
| `CLOUDFLARE_ACCOUNT_ID` | Actions Secret / CLI環境 | D1・R2のアカウント |
| `D1_DATABASE_ID` | Actions Secret / CLI環境 | バッチのD1接続先 |
| `CLOUDFLARE_API_TOKEN` | Actions Secret / CLI環境 | バッチのD1編集・R2操作・Workers AI Read/Edit（両方）。意味検索使用時はVectorizeも必要 |
| `JQUANTS_API_KEY` | Actions Secret / CLI環境 | 株価・財務取得 |
| `EDINET_API_KEY` | Actions Secret / CLI環境 | EDINET取得 |
| `REIT_RAG_ENABLED` | Actions Variable | `true` のとき日次REIT分析を実行 |
| `REIT_RAG_BUCKET` | workflow / CLI環境 | 原本・AI入出力の保存先。workflowは `maaun-stock-insight-raw` |
| `REIT_RAG_VECTOR_INDEX` | Actions Variable / CLI環境 | 任意。設定時にVectorize検索を使用 |
| `REIT_RAG_EMBEDDING_MODEL` | Actions Variable / CLI環境 | Vectorize使用時に必須。indexの次元と一致させる |
| `CLERK_PUBLISHABLE_KEY` | GitHub Actions Variable | フロントエンドのビルド用公開キー |
| `VITE_CLERK_PUBLISHABLE_KEY` | webビルド環境 | 上の公開キーをViteへ渡す。本番ビルドで必須 |
| `VITE_MOCK_CLERK_URL` | ローカルweb環境 | 開発用モック認証。詳細は開発手順 |
| `MOCK_AZP` | モックサーバー環境 | 開発用JWTのazp。既定 `http://localhost:5173` |

D1・AIのbinding、静的アセット、Cron、公開ドメインは [wrangler.jsonc](../wrangler.jsonc) で管理する。バッチの設定元は [daily-batch.yml](../.github/workflows/daily-batch.yml)。

生成モデルはレコメンドとREIT分析で[共通定義](../crates/usecase/src/ai_config.rs)を使う。`REIT_RAG_MODEL` の設定は不要。埋め込みモデルは生成モデルとは別。

## 登録と管理

Workerの値は `wrangler secret put <NAME>`、Actionsの値は `gh secret set <NAME>` の対話入力などで登録し、値をログ・PR・Issueへ出さない。公開キーは秘密ではないためActions Variableを使う。

設定の有無は `wrangler secret list`、`gh secret list`、`gh variable list` で名前を確認する。トークンの権限は用途に限定し、有効期限前または漏洩時に入れ替える。日次起動用トークンは対象リポジトリのActions書き込み権限を使う。

`.dev.vars`、ローカル環境ファイル、取得原本、DBバックアップをコミットしない。公開リポジトリのため、個人情報を含むデータや実トークンをテストfixtureへ入れない。

利用許可はClerkの対象ユーザーのPublic Metadataに `stock_insight: true` を設定する。管理者の判定にこのフラグは不要。反映時間と障害時の挙動は[認証仕様](05_detailed_design.md)を参照。
