# 06 シークレットと公開リポジトリの運用

> 出典: Claude Docs「増配期待株ダッシュボード 要件定義」（rev 56, 2026-10-21）

## 公開リポジトリとシークレット

リポジトリとこの仕様書は公開する前提で、キーの値はどこにも書かない。下表は設定するキー名だけを記す。

| キー名 | 設定先 | 用途 |
|---|---|---|
| CLOUDFLARE_ACCOUNT_ID | GitHub Actions Secrets | D1 HTTP API・wranglerの宛先 |
| CLOUDFLARE_API_TOKEN | GitHub Actions Secrets | `wrangler deploy`・D1のマイグレーションとバッチの書き込み（権限はWorkers編集・D1編集のみに絞る。Terraformで発行） |
| D1_DATABASE_ID | GitHub Actions Secrets | 書き込み先のD1 |
| R2_ACCESS_KEY_ID / R2_SECRET_ACCESS_KEY | GitHub Actions Secrets | R2への書き込み（対象バケットのみ） |
| EDINET_API_KEY | GitHub Actions Secrets | EDINET API |
| BATCH_TRIGGER_SECRET | GitHub Actions Secrets と Worker Secret | バッチ完了時に内部エンドポイントを呼ぶときの署名（HMAC） |
| GITHUB_DISPATCH_TOKEN | Worker Secret | Cron Triggerから日次バッチを起動（対象リポジトリのActions実行権限のみ） |
| CLERK_SECRET_KEY | Worker Secret | ClerkのAPI呼び出し |
| CLERK_WEBHOOK_SIGNING_SECRET | Worker Secret | Clerk Webhookの署名検証 |
| ADMIN_NOTIFY_EMAIL | Worker Secret | 管理者の判定と通知メールの宛先（Cloudflareで検証済みのアドレス） |
| VITE_CLERK_PUBLISHABLE_KEY | ビルド時の環境変数 | フロントのClerk初期化。公開される前提のキーだが、リポジトリには書かない |

- `.dev.vars`と`.env*`は`.gitignore`に入れ、値を抜いた`.dev.vars.example`だけを置く
- 設定ファイル（`wrangler.jsonc`）とTerraformにはシークレットの値を書かない
- 公開リポジトリではActionsのログも公開される。ログにはキー、メールアドレス、保有株などのユーザーデータを出さず、件数と銘柄コードだけを出す
- フォークからのプルリクエストではシークレットを使うジョブを動かさない（`pull_request_target`は使わない）
- シークレットの混入を防ぐため、GitHubのSecret scanningとPush protectionを有効にする

## 設定と取得の手順（すべてコマンド）

ダッシュボードは使わない。値は画面・ログ・ファイルに出さず、取得したコマンドの出力をパイプで直接設定コマンドの標準入力に渡す。

| キー名 | 値の取得・生成 | 設定コマンド |
|---|---|---|
| CLOUDFLARE_ACCOUNT_ID | maaun-terraformの`terraform output`（Cloudflareアカウントレイヤ） | `gh secret set CLOUDFLARE_ACCOUNT_ID` |
| CLOUDFLARE_API_TOKEN | maaun-terraformで`cloudflare_api_token`を発行し、`terraform output -raw`（sensitive） | `gh secret set CLOUDFLARE_API_TOKEN` |
| D1_DATABASE_ID | maaun-terraformの`terraform output -raw` | `gh secret set D1_DATABASE_ID` |
| R2_ACCESS_KEY_ID / R2_SECRET_ACCESS_KEY | R2用のAPIトークン（対象バケットのみ）をTerraformで発行し、`terraform output -raw` | `gh secret set` |
| EDINET_API_KEY | EDINETで発行（手元のパスワードマネージャーから渡す） | `gh secret set EDINET_API_KEY` |
| BATCH_TRIGGER_SECRET | `openssl rand -hex 32`で生成し、変数に入れて両方へ渡す | `gh secret set` と `wrangler secret put` |
| GITHUB_DISPATCH_TOKEN | GitHubのFine-grained token（対象リポジトリのActions書き込みのみ） | `wrangler secret put GITHUB_DISPATCH_TOKEN` |
| CLERK_SECRET_KEY / CLERK_WEBHOOK_SIGNING_SECRET | Clerkで発行（パスワードマネージャーから渡す） | `wrangler secret put` |
| ADMIN_NOTIFY_EMAIL | — | `wrangler secret put ADMIN_NOTIFY_EMAIL` |
| VITE_CLERK_PUBLISHABLE_KEY | Clerkで発行 | `gh variable set`（ビルド時に参照） |

例（値は表示しない）:

```bash
terraform -chdir=envs/global/cloudflare/account output -raw stock_insight_ci_token | gh secret set CLOUDFLARE_API_TOKEN --repo <owner>/stock-insight
```

```bash
openssl rand -hex 32 | tee >(gh secret set BATCH_TRIGGER_SECRET --repo <owner>/stock-insight) | wrangler secret put BATCH_TRIGGER_SECRET >/dev/null
```

- Terraformのoutputはすべて`sensitive = true`にする。シークレットの値（EDINET、Clerkなど外部で発行したもの）はTerraformの変数に入れない
- 手順はスクリプト（`scripts/set-secrets.sh`）にまとめ、値を引数や環境変数の表示に出さない
- 設定済みかどうかは`gh secret list`と`wrangler secret list`で名前だけを確認する

## 入れ替え

- 年1回と、漏洩の疑いがあるときにすべてのシークレットを入れ替える
- 入れ替えも上の手順のコマンドで行い、値をリポジトリ・Issue・PR・ログに書かない。Terraformで発行したトークンは`terraform apply -replace`で再発行する
