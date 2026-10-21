# 06 シークレットと公開リポジトリの運用

> 出典: Claude Docs「増配期待株ダッシュボード 要件定義」（rev 56, 2026-10-21）

## 公開リポジトリとシークレット

リポジトリとこの仕様書は公開する前提で、キーの値はどこにも書かない。下表は設定するキー名だけを記す。

| キー名 | 設定先 | 用途 |
|---|---|---|
| CLOUDFLARE_ACCOUNT_ID | GitHub Actions Secrets | D1 HTTP API・wranglerの宛先 |
| CLOUDFLARE_API_TOKEN | GitHub Actions Secrets | `wrangler deploy`・D1のマイグレーションとバッチの書き込み（権限はWorkers編集・D1編集のみに絞る） |
| D1_DATABASE_ID | GitHub Actions Secrets | 書き込み先のD1 |
| R2_ACCESS_KEY_ID / R2_SECRET_ACCESS_KEY | GitHub Actions Secrets | R2への書き込み（対象バケットのみ） |
| JQUANTS_API_KEY | GitHub Actions Secrets | J-Quants API（Light プラン。ダッシュボードで発行） |
| BATCH_TRIGGER_SECRET | GitHub Actions Secrets と Worker Secret | バッチ完了時に内部エンドポイントを呼ぶときの署名（HMAC） |
| GITHUB_DISPATCH_TOKEN | Worker Secret | Cron Triggerから日次バッチを起動（対象リポジトリのActions実行権限のみ） |
| CLERK_SECRET_KEY | Worker Secret | ClerkのAPI呼び出し |
| CLERK_WEBHOOK_SIGNING_SECRET | Worker Secret | Clerk Webhookの署名検証 |
| ADMIN_NOTIFY_EMAIL | Worker Secret | 管理者の判定と通知メールの宛先（Cloudflareで検証済みのアドレス） |
| VITE_CLERK_PUBLISHABLE_KEY | ビルド時の環境変数 | フロントのClerk初期化。公開される前提のキーだが、リポジトリには書かない |

- `.dev.vars`と`.env*`は`.gitignore`に入れ、値を抜いた`.dev.vars.example`だけを置く
- 設定ファイル（`wrangler.jsonc`）にはシークレットの値を書かない
- 公開リポジトリではActionsのログも公開される。ログにはキー、メールアドレス、保有株などのユーザーデータを出さず、件数と銘柄コードだけを出す
- フォークからのプルリクエストではシークレットを使うジョブを動かさない（`pull_request_target`は使わない）
- シークレットの混入を防ぐため、GitHubのSecret scanningとPush protectionを有効にする

## 設定と取得の手順（コマンド）

値は画面・ログ・ファイルに出さず、取得したコマンドの出力をパイプで直接設定コマンドの標準入力に渡す。手で入力する値は`gh secret set <名前>`の対話入力（表示されない）で渡す。

| キー名 | 値の取得・生成 | 設定コマンド |
|---|---|---|
| CLOUDFLARE_ACCOUNT_ID | `wrangler whoami --json` | `gh secret set CLOUDFLARE_ACCOUNT_ID` |
| D1_DATABASE_ID | `wrangler d1 info maaun-stock-insight --json`の`uuid` | `gh secret set D1_DATABASE_ID` |
| CLOUDFLARE_API_TOKEN | ダッシュボードで発行（権限はD1編集。デプロイを始めたらWorkers編集を追加） | `gh secret set CLOUDFLARE_API_TOKEN`（対話入力） |
| R2_ACCESS_KEY_ID / R2_SECRET_ACCESS_KEY | ダッシュボードでR2のAPIトークンを発行（対象バケットのみ） | `gh secret set`（対話入力） |
| JQUANTS_API_KEY | J-Quantsのダッシュボードで発行 | `gh secret set JQUANTS_API_KEY`（対話入力） |
| BATCH_TRIGGER_SECRET | `openssl rand -hex 32`で生成し、両方へ渡す | `gh secret set` と `wrangler secret put` |
| GITHUB_DISPATCH_TOKEN | GitHubのFine-grained token（対象リポジトリのActions書き込みのみ） | `wrangler secret put GITHUB_DISPATCH_TOKEN` |
| CLERK_SECRET_KEY / CLERK_WEBHOOK_SIGNING_SECRET | Clerkで発行 | `wrangler secret put` |
| ADMIN_NOTIFY_EMAIL | — | `wrangler secret put ADMIN_NOTIFY_EMAIL` |
| VITE_CLERK_PUBLISHABLE_KEY | Clerkで発行 | `gh variable set`（ビルド時に参照） |

例（値は表示しない）:

```bash
npx wrangler d1 info maaun-stock-insight --json | python3 -c 'import json,sys;sys.stdout.write(json.load(sys.stdin)["uuid"])' | gh secret set D1_DATABASE_ID --repo taaaaakahiro/stock-insight
```

```bash
openssl rand -hex 32 | tee >(gh secret set BATCH_TRIGGER_SECRET --repo taaaaakahiro/stock-insight) | npx wrangler secret put BATCH_TRIGGER_SECRET >/dev/null
```

- 設定済みかどうかは`gh secret list`と`wrangler secret list`で名前だけを確認する

## 入れ替え

- 年1回と、漏洩の疑いがあるときにすべてのシークレットを入れ替える
- 入れ替えも上の手順のコマンドで行い、値をリポジトリ・Issue・PR・ログに書かない。APIトークンは再発行してから同じコマンドで登録し直す
