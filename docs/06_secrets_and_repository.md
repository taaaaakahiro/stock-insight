# 06 シークレットと公開リポジトリの運用

> 出典: Claude Docs「増配期待株ダッシュボード 要件定義」（rev 56, 2026-10-21）

## 公開リポジトリとシークレット

リポジトリとこの仕様書は公開する前提で、キーの値はどこにも書かない。下表は設定するキー名だけを記す。

| キー名 | 設定先 | 用途 |
|---|---|---|
| CLOUDFLARE_ACCOUNT_ID | GitHub Actions Secrets | D1 HTTP APIの宛先 |
| CLOUDFLARE_API_TOKEN | GitHub Actions Secrets | D1への書き込み（権限はD1編集のみに絞る） |
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
- 設定ファイル（`wrangler.jsonc`）にはシークレットを書かない（Worker SecretはCloudflareダッシュボードで登録）
- 公開リポジトリではActionsのログも公開される。ログにはキー、メールアドレス、保有株などのユーザーデータを出さず、件数と銘柄コードだけを出す
- フォークからのプルリクエストではシークレットを使うジョブを動かさない（`pull_request_target`は使わない）
- シークレットの混入を防ぐため、GitHubのSecret scanningとPush protectionを有効にする

## 入れ替え

- 年1回と、漏洩の疑いがあるときにすべてのシークレットを入れ替える
- 入れ替えはGitHub Actions SecretsとCloudflareダッシュボード（Worker Secret）で行い、値をリポジトリ・Issue・PR・ログに書かない
