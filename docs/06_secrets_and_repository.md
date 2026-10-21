# 06 シークレットと公開リポジトリの運用

> 出典: Claude Docs「増配期待株ダッシュボード 要件定義」（rev 56, 2026-10-21）。その後の実装（v0.3.0 まで）に合わせて更新

## 公開リポジトリとシークレット

リポジトリとこの仕様書は公開する前提で、キーの値はどこにも書かない。下表は設定するキー名だけを記す。

### 使っているもの

| キー名 | 設定先 | 用途 |
|---|---|---|
| CLOUDFLARE_ACCOUNT_ID | GitHub Actions Secrets | D1 HTTP API・wranglerの宛先 |
| CLOUDFLARE_API_TOKEN | GitHub Actions Secrets | 日次バッチの D1 のマイグレーションと書き込み、R2 へのファイルの書き込み・削除（権限は D1 編集と R2 編集に絞る。CI から `wrangler deploy` するようになったら Workers 編集を足す） |
| D1_DATABASE_ID | GitHub Actions Secrets | 書き込み先のD1 |
| JQUANTS_API_KEY | GitHub Actions Secrets | J-Quants API（Light プラン。ダッシュボードで発行） |
| EDINET_API_KEY | GitHub Actions Secrets | EDINET API v2（無料。EDINET のサイトで発行。期限なし）。ヘッダー `Ocp-Apim-Subscription-Key` で渡し、URL には載せない |
| GITHUB_DISPATCH_TOKEN | Worker Secret | Cron Triggerから日次バッチを起動（対象リポジトリのActions実行権限のみ） |
| CLERK_SECRET_KEY | Worker Secret | ClerkのユーザーAPIの呼び出し（Googleのつながりと`public_metadata`の確認） |
| ADMIN_NOTIFY_EMAIL | Worker Secret | 管理者の判定（将来は通知メールの宛先。Cloudflareで検証済みのアドレス） |
| CLERK_PUBLISHABLE_KEY | GitHub Actions Variables | CI の画面のビルドで `VITE_CLERK_PUBLISHABLE_KEY` として渡す。公開される前提のキーだが、リポジトリには書かない |
| VITE_CLERK_PUBLISHABLE_KEY | ビルド時の環境変数 | フロントのClerk初期化。手元でビルドするときは上の Variable から取って渡す（[07](07_deploy.md#3-ステージング相当での確認)）。ないと本番ビルドは失敗する |

秘密ではない設定（`CLERK_ISSUER`・`CLERK_API_URL`・`AUTHORIZED_PARTIES`・`GITHUB_REPO`）は `wrangler.jsonc` の `vars` に書く。

### 予定（まだ設定しない）

| キー名 | 設定先 | 用途 |
|---|---|---|
| BATCH_TRIGGER_SECRET | GitHub Actions Secrets と Worker Secret | バッチ完了時に内部エンドポイントを呼ぶときの署名（HMAC）。判定Workflowを作るときに設定 |
| CLERK_WEBHOOK_SIGNING_SECRET | Worker Secret | Clerk Webhookの署名検証。Webhook でログインを記録するときに設定 |

R2 には `wrangler r2 object put` で書くため、S3 互換 API のキー（R2_ACCESS_KEY_ID / R2_SECRET_ACCESS_KEY）は使わない。

### 公開リポジトリでの注意

- `.dev.vars`と`.env*`は`.gitignore`に入れる。ローカルの `.dev.vars` の例（偽の Clerk 用の値）はリポジトリの README にある
- 設定ファイル（`wrangler.jsonc`）にはシークレットの値を書かない
- 公開リポジトリではActionsのログも公開される。ログにはキー、メールアドレス、ポートフォリオなどのユーザーデータを出さず、件数と銘柄コードだけを出す
- フォークからのプルリクエストではシークレットを使うジョブを動かさない（`ci` はシークレットを使わず、`pull_request_target`は使わない）
- シークレットの混入を防ぐため、GitHubのSecret scanningとPush protectionを有効にする

## 設定と取得の手順（コマンド）

値は画面・ログ・ファイルに出さず、取得したコマンドの出力をパイプで直接設定コマンドの標準入力に渡す。手で入力する値は`gh secret set <名前>` / `wrangler secret put <名前>` の対話入力（表示されない）で渡す。

| キー名 | 値の取得・生成 | 設定コマンド |
|---|---|---|
| CLOUDFLARE_ACCOUNT_ID | `wrangler whoami --json` | `gh secret set CLOUDFLARE_ACCOUNT_ID` |
| D1_DATABASE_ID | `wrangler d1 info maaun-stock-insight --json`の`uuid` | `gh secret set D1_DATABASE_ID` |
| CLOUDFLARE_API_TOKEN | ダッシュボードで発行（権限は D1 編集・R2 編集。デプロイを CI に移したら Workers 編集を追加） | `gh secret set CLOUDFLARE_API_TOKEN`（対話入力） |
| JQUANTS_API_KEY | J-Quantsのダッシュボードで発行 | `gh secret set JQUANTS_API_KEY`（対話入力） |
| EDINET_API_KEY | EDINETのAPI利用登録で発行 | `gh secret set EDINET_API_KEY`（対話入力） |
| GITHUB_DISPATCH_TOKEN | GitHubのFine-grained token（対象リポジトリのActions書き込みのみ、有効期限つき） | `wrangler secret put GITHUB_DISPATCH_TOKEN` |
| CLERK_SECRET_KEY | Clerk の PropFolio アプリ（本番）の Secret key | `wrangler secret put CLERK_SECRET_KEY` |
| ADMIN_NOTIFY_EMAIL | 管理者の Google アカウントのアドレス | `wrangler secret put ADMIN_NOTIFY_EMAIL` |
| CLERK_PUBLISHABLE_KEY | `gh variable get CLERK_PUBLISHABLE_KEY --repo maa-un/area-insight`（Clerk は共通の本番インスタンス） | `gh variable set CLERK_PUBLISHABLE_KEY --repo taaaaakahiro/stock-insight` |
| BATCH_TRIGGER_SECRET（予定） | `openssl rand -hex 32`で生成し、両方へ渡す | `gh secret set` と `wrangler secret put` |
| CLERK_WEBHOOK_SIGNING_SECRET（予定） | Clerkで発行 | `wrangler secret put` |

例（値は表示しない）:

```bash
npx wrangler d1 info maaun-stock-insight --json | python3 -c 'import json,sys;sys.stdout.write(json.load(sys.stdin)["uuid"])' | gh secret set D1_DATABASE_ID --repo taaaaakahiro/stock-insight
```

```bash
gh variable get CLERK_PUBLISHABLE_KEY --repo maa-un/area-insight | gh variable set CLERK_PUBLISHABLE_KEY --repo taaaaakahiro/stock-insight
```

```bash
openssl rand -hex 32 | tee >(gh secret set BATCH_TRIGGER_SECRET --repo taaaaakahiro/stock-insight) | npx wrangler secret put BATCH_TRIGGER_SECRET >/dev/null
```

- 設定済みかどうかは`gh secret list` / `gh variable list` と`wrangler secret list`で名前だけを確認する

## 入れ替え

- 年1回と、漏洩の疑いがあるときにすべてのシークレットを入れ替える。`GITHUB_DISPATCH_TOKEN` は有効期限が切れる前に入れ替える（切れると日次バッチが Cron から起動しなくなり、予備の schedule だけで動く）
- 入れ替えも上の手順のコマンドで行い、値をリポジトリ・Issue・PR・ログに書かない。APIトークンは再発行してから同じコマンドで登録し直す
