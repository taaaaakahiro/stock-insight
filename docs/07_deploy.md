# 07 デプロイ手順（初回の本番公開）

stock.insight.maa-un.com に API Worker と画面を初めて公開する手順。本番への変更を含むため、組織の承認フロー（宣言 → 影響範囲 → バックアップ → ステージング相当での確認 → 人の承認 → 実行と報告）に沿って進める。

コマンドはリポジトリのルートで実行する。wrangler は `npx wrangler@4.151.3`（以下 `wrangler` と書く）。

## 0. 事前の確認

| 確認すること | コマンド・方法 | 期待する結果 |
|---|---|---|
| wrangler のログイン先 | `wrangler whoami` | maa'un japan の Cloudflare アカウント |
| main が最新で CI が通っている | `git pull` / GitHub の Actions | `ci` が成功 |
| 本番 D1 の現在のテーブル | `wrangler d1 execute maaun-stock-insight --remote --command "SELECT name FROM sqlite_master WHERE type='table'"` | `batch_runs`、`indicators_latest`、`financials_annual` |
| Worker がまだない | `wrangler deployments list --name maaun-stock-insight` | 見つからない（初回） |

## 1. 宣言と影響範囲

**やること**: 本番 D1 にユーザー用のテーブルを足し、Worker `maaun-stock-insight` を新しく作ってカスタムドメイン stock.insight.maa-un.com で公開する。

| 対象 | 変更 | 件数・範囲 | 既存への影響 |
|---|---|---|---|
| D1 `maaun-stock-insight` | `users`・`login_events` テーブルと索引を追加（`CREATE ... IF NOT EXISTS`） | 2テーブル・1索引、0行 | なし（既存3テーブルは変えない） |
| Worker `maaun-stock-insight` | 新規作成（画面の静的アセット + API） | 1 Worker | なし |
| Worker Secret | `CLERK_SECRET_KEY`、`ADMIN_NOTIFY_EMAIL` を登録 | 2件 | なし |
| DNS（maa-un.com） | `stock.insight.maa-un.com` を Workers のカスタムドメインとして追加（証明書も自動発行） | 1レコード | なし。maaun-terraform の管理外のレコードとして増える |
| Clerk（PropFolio と共通） | 使う人の `public_metadata` に `{"stock_insight": true}` を付ける | 利用者の人数分 | PropFolio の動作には影響しない（PropFolio はこのキーを読まない） |

## 2. バックアップ

D1 の中身を書き出す（`backups/` は Git に入れない）。

```bash
mkdir -p backups && npx wrangler@4.151.3 d1 export maaun-stock-insight --remote --output backups/maaun-stock-insight-$(date +%Y%m%d-%H%M).sql
```

D1 の Time Travel でも過去の時点に戻せる（`wrangler d1 time-travel info maaun-stock-insight` で今のブックマークを控えておく）。

## 3. ステージング相当での確認

専用のステージング環境はないため、次の2段で確認する。

1. **ローカル**（済み・PR #4〜#6）: `wrangler dev --local` と偽の Clerk（`scripts/mock-clerk.mjs`）で、認証・一覧・画面・ヘッダーを確認
2. **本番に切り替える前のプレビュー**: 新しいバージョンをアップロードだけして、プレビュー URL で確かめる（本番のドメインにはまだ出ない）

```bash
pnpm --dir web install --frozen-lockfile
VITE_CLERK_PUBLISHABLE_KEY=$(gh variable get CLERK_PUBLISHABLE_KEY --repo maa-un/area-insight) pnpm --dir web build
npx wrangler@4.151.3 versions upload
```

プレビュー URL（`*.workers.dev`）で確認すること:

- `/` が 200 で、CSP などのヘッダーが付いている
- `/api/me` がトークンなしで 401
- Clerk の本番インスタンスは maa-un.com のサブドメインでしか動かないため、**プレビュー URL ではログインできない**（ログインの確認は 5. で行う）

## 4. 人の承認

1〜3 の結果を見せて、承認を得てから次へ進む。

## 5. 本番への適用

### 5-1. D1 にテーブルを追加

```bash
npx wrangler@4.151.3 d1 execute maaun-stock-insight --remote --file migrations/0002_users.sql
```

### 5-2. Worker Secret を登録

値は対話入力で渡す（画面に出ない）。

```bash
npx wrangler@4.151.3 secret put CLERK_SECRET_KEY
```

```bash
npx wrangler@4.151.3 secret put ADMIN_NOTIFY_EMAIL
```

- `CLERK_SECRET_KEY`: Clerk の PropFolio アプリ（本番）の Secret key（`sk_live_...`）。Clerk ダッシュボードの API keys
- `ADMIN_NOTIFY_EMAIL`: 管理者のメールアドレス（Google アカウントのアドレス）
- Worker がまだないときは、作成するか聞かれるので yes で答える

### 5-3. デプロイ

3. で作った `web/dist`（公開キー入り）をそのまま使う。

```bash
npx wrangler@4.151.3 deploy
```

`wrangler.jsonc` の `routes`（`custom_domain: true`）により、stock.insight.maa-un.com の DNS レコードと証明書が作られる。証明書の発行には数分かかることがある。

### 5-4. Clerk で利用者に印を付ける

Clerk ダッシュボード → Users → 対象のユーザー → Metadata の Public に次を入れる（管理者は不要）。

```json
{ "stock_insight": true }
```

## 6. 確認と報告

| 確認すること | 方法 | 期待する結果 |
|---|---|---|
| 画面 | https://stock.insight.maa-un.com を開く | 「Google でログイン」だけが出る |
| ヘッダー | `curl -sI https://stock.insight.maa-un.com/` | CSP・HSTS・`X-Frame-Options: DENY` |
| 認証なしの API | `curl -s https://stock.insight.maa-un.com/api/me` | 401 |
| 管理者でログイン | ブラウザ | 一覧が出て、ヘッダーに株価の取得日時 |
| 印のない人 | 別の Google アカウント | 「このアカウントは利用できません」 |
| ログイン記録 | `wrangler d1 execute maaun-stock-insight --remote --command "SELECT user_id, result, occurred_at FROM login_events ORDER BY id DESC LIMIT 5"` | 許可・拒否が残っている |
| ログ | `wrangler tail maaun-stock-insight` | `internal error` や `auth unavailable` が出ていない |

結果（デプロイしたバージョン ID、確認の結果）を報告する。

## 切り戻し

| 問題 | 戻し方 |
|---|---|
| 新しいバージョンに不具合 | `npx wrangler@4.151.3 rollback`（直前のバージョンに戻す。初回は戻す先がないので、下の公開停止） |
| 公開を止めたい | `wrangler.jsonc` の `routes` を外して `wrangler deploy`（カスタムドメインが外れる）。または Worker を削除（要承認） |
| D1 を戻したい | `wrangler d1 time-travel restore maaun-stock-insight --bookmark <控えたブックマーク>`、または 2. の書き出しから復元（どちらも要承認） |

`users`・`login_events` は新しく足すだけなので、残しても既存の処理に影響しない。消す場合は DROP になるため、必ず承認を得る。

## 2回目以降のデプロイ

画面と Worker を変えたときは 3.（プレビュー）→ 承認 → `wrangler deploy` → 6. の確認。テーブル定義を変えるときは 2.（バックアップ）と `batch-cli migrate` も行う。GitHub Actions からの自動デプロイは、デプロイ用の API トークン（[06](06_secrets_and_repository.md) のデプロイ用の権限）を用意してから追加する。
