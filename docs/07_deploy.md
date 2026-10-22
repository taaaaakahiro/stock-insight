# 07 デプロイ手順

stock.insight.maa-un.com に API Worker と画面を公開する手順。初回の公開は 2026-10-21 に済んでいる。0.〜6. は初回の記録を兼ねた手順で、2回目以降は「[2回目以降のデプロイ](#2回目以降のデプロイ)」の流れで同じ手順を使う。本番への変更を含むため、組織の承認フロー（宣言 → 影響範囲 → バックアップ → ステージング相当での確認 → 人の承認 → 実行と報告）に沿って進める。

コマンドはリポジトリのルートで実行する。wrangler は `npx wrangler@4.151.3`（以下 `wrangler` と書く）。

## 0. 事前の確認

| 確認すること | コマンド・方法 | 期待する結果 |
|---|---|---|
| wrangler のログイン先 | `wrangler whoami` | maa'un japan の Cloudflare アカウント |
| main が最新で CI が通っている | `git pull` / GitHub の Actions | `ci` が成功 |
| 本番 D1 の現在のテーブル | `wrangler d1 execute maaun-stock-insight --remote --command "SELECT name FROM sqlite_master WHERE type='table'"` | 初回は `batch_runs`、`indicators_latest`、`financials_annual`。今は `migrations/0001`〜`0009` のテーブルすべて（[05](05_detailed_design.md#テーブル定義)） |
| Worker の状態 | `wrangler deployments list --name maaun-stock-insight` | 初回は見つからない。2回目以降は今のバージョン ID を控える（切り戻し先） |

## 1. 宣言と影響範囲

**やること**（初回の例）: 本番 D1 にユーザー用のテーブルを足し、Worker `maaun-stock-insight` を新しく作ってカスタムドメイン stock.insight.maa-un.com で公開する。2回目以降は、変えるテーブル・Worker・シークレットを同じ形の表にする。

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
2. **本番に切り替える前のプレビュー**: 公開先のない Worker にバージョンを上げ、プレビュー URL で確かめる（本番のドメインにも workers.dev にも出ない）

`versions upload` は Worker が存在しないと使えないため、**初回だけ** `routes` を外した一時的な設定で Worker を作る（公開先なし。`No targets deployed` と出る）。一時ファイルは Git に入れず、終わったら消す。

```bash
pnpm --dir web install --frozen-lockfile
VITE_CLERK_PUBLISHABLE_KEY=$(gh variable get CLERK_PUBLISHABLE_KEY --repo taaaaakahiro/stock-insight) pnpm --dir web build
grep -v '"routes"' wrangler.jsonc > wrangler.unrouted.jsonc   # 初回だけ
npx wrangler@4.151.3 deploy --config wrangler.unrouted.jsonc   # 初回だけ
npx wrangler@4.151.3 versions upload --config wrangler.unrouted.jsonc
rm wrangler.unrouted.jsonc
```

プレビュー URL（`https://<バージョン>-maaun-stock-insight.<サブドメイン>.workers.dev`）で確認すること:

- `/` と深いパスが 200 で、CSP・HSTS・`X-Frame-Options` などのヘッダーが付いている。`/assets/*` は `immutable`
- `/api/*` は 503（`authentication is not configured`）。シークレットを入れる前なので、設定不足では通さない動きのとおり
- `https://maaun-stock-insight.<サブドメイン>.workers.dev/` は 404、stock.insight.maa-un.com はまだ応答しない
- 画面は「読み込み中…」のまま。Clerk の本番キーは maa-un.com でしか使えないため（コンソールに `Production Keys are only allowed for domain "maa-un.com"`）。**ログインの確認は 6. で行う**

## 4. 人の承認

1〜3 の結果を見せて、承認を得てから次へ進む。

## 5. 本番への適用

### 5-1. D1 にテーブルを追加

初回は `0002_users.sql`。2回目以降は新しく足したマイグレーションのファイルを指定する。`ADD COLUMN` を含むファイルは2回目に「列がすでにある」で止まるので、まだ適用していないファイルだけを流す（適用済みかは `wrangler d1 execute maaun-stock-insight --remote --command "PRAGMA table_info(<テーブル>)"` で確かめる）。

```bash
npx wrangler@4.151.3 d1 execute maaun-stock-insight --remote --file migrations/0002_users.sql
```

日次バッチも毎回最初に `batch-cli migrate` で `migrations/` をすべて適用する。新しいテーブルを読む Worker を出すときは、デプロイより先にここで適用しておく（バッチを待つと、その間 API が 500 になる）。

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

マイグレーションはテーブル・列を足すだけなので、Worker を戻しても足したテーブルは残してよい（既存の処理に影響しない）。消す場合は DROP になるため、必ず承認を得る。

## 2回目以降のデプロイ

画面と Worker を変えたときは 0.（今のバージョン ID を控える）→ 3.（プレビュー。2回目以降は `wrangler.jsonc` のまま `versions upload` でよい）→ 4. 承認 → 5-3. `wrangler deploy` → 6. の確認。テーブル定義を変えるときは、デプロイの前に 2.（バックアップ）と 5-1.（マイグレーション）も行う。

```bash
VITE_CLERK_PUBLISHABLE_KEY=$(gh variable get CLERK_PUBLISHABLE_KEY --repo taaaaakahiro/stock-insight) pnpm --dir web build
npx wrangler@4.151.3 versions upload
```

デプロイは共有の作業ツリーではなく、main を切り出した clean な worktree から行う（他の作業中の変更をビルドに混ぜないため）。GitHub Actions からの自動デプロイは未設定。追加するときは API トークンに Workers 編集の権限を足す（[06](06_secrets_and_repository.md)）。

## 日次バッチを有効にする

日次バッチは GitHub Actions の `daily-batch`。Worker の Cron Trigger（`wrangler.jsonc` の `triggers.crons`、平日 09:30 UTC = 18:30 JST）が `workflow_dispatch` で起動する。中身は [05 日次バッチのステップ](05_detailed_design.md#日次バッチのステップ)。

0. GitHub Actions Secrets に `CLOUDFLARE_ACCOUNT_ID`・`D1_DATABASE_ID`・`CLOUDFLARE_API_TOKEN`（D1 編集・R2 編集）・`JQUANTS_API_KEY`・`EDINET_API_KEY` があることを `gh secret list` で確かめる（[06](06_secrets_and_repository.md)）。取得元だけを確かめたいときは、D1 に触れない `probe` で流せる: `gh workflow run daily-batch --repo taaaaakahiro/stock-insight -f probe=true`（結果の SQL は成果物 `probe-sql`）
1. GitHub で Fine-grained personal access token を作る（Repository access: `taaaaakahiro/stock-insight` のみ、Permissions: Actions = Read and write、有効期限を設定）
2. Worker Secret に登録する（値は対話入力。ダッシュボードの場合は Variables and Secrets に Secret として追加）

```bash
npx wrangler@4.151.3 secret put GITHUB_DISPATCH_TOKEN
```

3. `wrangler deploy` で Cron Trigger を反映する（手順 5-3 と同じ。画面は公開キー入りでビルドしてから）
4. 初回は手動で全銘柄を流して確かめる（休場日・当日実行済みの判定は `force=true` で飛ばせる）。初回は J-Quants の過去5年分で1時間以上、EDINET の過去400日分（数千通）で1〜2時間ほどかかる（2つは並列に動く）。時間切れ（`max_minutes`、EDINET は300分）になっても次回は続きから取る

```bash
gh workflow run daily-batch --repo taaaaakahiro/stock-insight -f force=true
```

5. 次の平日 18:30 以降に、`batch_runs` に `daily-...` の行が `succeeded` で増えていること、`wrangler tail` に `dispatched daily-batch` が出ていること、画面のヘッダーの株価の日付が進んでいることを確かめる

`persist_every` は小さくしない（保存のたびに全銘柄の終値の履歴を書き直し、書き込みが膨らむ）。J-Quants（`jquants` ジョブ）と EDINET（`edinet` ジョブ）は並列に動き、片方が失敗してももう片方は続く。

止めたいときは `wrangler.jsonc` の `triggers.crons` を空にして `wrangler deploy`（GitHub 側の予備の schedule は、ワークフローを無効化する: `gh workflow disable daily-batch`）。

## 株価の履歴を5年分に取り直す

v0.3.0 までは終値の履歴（D1 `price_history`）を直近3年に切り詰めていたため、利回りの過去5年比が実際は3年分で計算されていた。また、株式分割の記録（`splits`）は日足を読むときにしか残らないので、過去5年の分割は株価を読み直さないと集まらない。保存期間を5年＋1か月に直し、分割の記録を残す版（`0009_splits`）を main にマージしたあと、一度だけ過去5年分の株価を J-Quants から取り直す。本番の D1 を書き換えるので、承認フローに沿って進める。

先に初回の5年分の取り込み（`sync_state.jq_synced_until`）が今日まで追いついていること、新しいコードの日次バッチが1回動いて `splits` テーブルができていることを確かめてから行う。

**やること**: 日次バッチを `refill_prices=true` で1回流し、全銘柄の `price_history` を過去5年分で書き直し、過去5年の株式分割を `splits` に残す。

| 対象 | 変更 | 件数・範囲 | 既存への影響 |
|---|---|---|---|
| D1 `price_history` | 全銘柄の `closes` を5年分に書き直す（同じ日付は J-Quants の同じ値で上書き） | 約4,300行（Workers Paid の D1 の枠では気にならない量）。1行あたり約20KB、全体で約90MB | 削除なし。履歴が長くなるだけ |
| D1 `splits` | 過去5年の株式分割・併合を追加 | 数百行（今残っている終値だけでも、分割と見られる段差が500か所ほどある） | なし（新しいテーブル） |
| D1 `indicators_latest`・`batch_runs` | 続く `compute` で、分割で基準をそろえた5年分から計算し直される | 約4,300行・1行 | 分割した銘柄（約200）の利回り・PBR・PER などが正しい値に下がり、一覧の「過去5年比」とポートフォリオのサインも変わる |
| D1 の財務・`sync_state`、R2 | 変えない | — | なし |

1. この修正を入れた版を main にマージする（Worker のデプロイは不要。変わるのはバッチだけ）
2. バックアップ（手順 2. と同じ）

```bash
mkdir -p backups && npx wrangler@4.151.3 d1 export maaun-stock-insight --remote --table price_history --output backups/price_history-$(date +%Y%m%d-%H%M).sql
```

3. 先に D1 に触れない `probe` で、J-Quants から取れることを確かめる（ステージング相当）

```bash
gh workflow run daily-batch --repo taaaaakahiro/stock-insight -f probe=true
```

4. 1〜3 の結果を見せて承認を得る
5. 取り直す。休場日や当日の実行後でも動くよう `force=true` を付ける（通常の取り込み・指標計算・EDINET も続けて動く）

```bash
gh workflow run daily-batch --repo taaaaakahiro/stock-insight -f refill_prices=true -f force=true
```

6. 確認と報告

| 確認すること | 方法 | 期待する結果 |
|---|---|---|
| 取り直しの範囲 | Actions のログの `sync-jq: prices ... (refill from <5年前の日付>)` と最後の `persist: ... price histories, ... splits` | 約4,300銘柄と、数百件の分割を保存 |
| 分割の扱い | 一覧で東京海上（8766）の配当利回りを見る | 分割前の配当で割った約39%ではなく、数%に下がっている |
| 履歴の長さ | `wrangler d1 execute maaun-stock-insight --remote --command "SELECT code, substr(closes, 1, 8) AS first_day, last_date FROM price_history WHERE code = '7203'"` | `first_day` が約5年前 |
| 指標 | `batch_runs` の最新行が `succeeded`、画面の「過去5年比」が出ている | — |

所要は J-Quants の呼び出しが約1,250日分で30〜45分。`max_minutes` を超えて止まったときは、同じコマンドをもう一度流す（`sync_state` は変えていないので、取り直しは最初からやり直しになるが、同じ値の上書きなので問題ない）。うまくいかなかったときは 2. の書き出しから `price_history` を戻す（要承認）。

## バージョンとタグ

版の元は `Cargo.toml` の `workspace.package.version` だけ（semver: MAJOR.MINOR.PATCH）。画面のフッターと `/api/status` の `version` はビルド時にここから入る。

1. `Cargo.toml` と `web/package.json` の version を上げて main にマージする（画面の版は `web/vite.config.ts` が Cargo.toml から読む。`web/package.json` は揃えておくだけ）
2. 同じ番号でタグを押す（`git tag v0.3.0 && git push origin v0.3.0`）。`release.yml` が Cargo.toml と照合し、一致しなければ失敗する。一致すれば GitHub Release を作る
3. 本番への反映は 5-3 と同じ（`wrangler deploy`）。デプロイ後、フッターの `v0.3.0` と `/api/status` の `version` がタグと同じであることを確かめる

タグと画面の版がずれているときは、タグを打ち直すのではなく version を直して出し直す。
