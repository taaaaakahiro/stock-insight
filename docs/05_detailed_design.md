# 05 詳細設計

> 出典: Claude Docs「増配期待株ダッシュボード 要件定義」（rev 56, 2026-10-21）。その後の実装（v0.3.0 まで）に合わせて更新

## 指標の定義

各指標の取得元の項目と計算式は [08 指標の取得元と計算式](08_indicator_sources.md#指標の計算式domainsrcindicatorsrs-analysisrs) にまとめた（そちらが正本）。指標はバッチで計算して保存し、画面では計算しない。比率は小数で保存し（0.035 = 3.5%）、画面で % にする。

株価と配当・EPS・BPS は J-Quants の分割調整前の値を保存し、計算のときに株式分割の記録（`splits`）で最新の株数の基準にそろえる（[08 株式分割の扱い](08_indicator_sources.md#株式分割の扱いdomainsrcsplitrsusecasesrcdailyrs-の-build_inputs)）。

### 利回りの過去5年比（valuation）

| 値 | 条件 | 画面の表示 |
|---|---|---|
| `cheap` | 利回りの過去5年比 ≧ 80% | 「高い（上位20%。株価は過去と比べて安い）」 |
| `neutral` | 20% < 過去5年比 < 80% | 「ふつう」 |
| `expensive` | 過去5年比 ≦ 20% | 「低い（下位20%。株価は過去と比べて高い）」。ポートフォリオでは「割高」のサイン |
| `unknown` | 週次の点が52に満たない、または配当がない | — |

基準は `domain::indicators::CHEAP_PERCENTILE`・`EXPENSIVE_PERCENTILE`。色だけに意味を持たせず、文字を必ず併記する。

## 判定ロジック

判定は`domain`の純粋関数として実装する。

### 売り時のサイン（F4、実装済み）

`domain::portfolio::sell_signals`。`/api/portfolio` を返すときに、最新の指標（indicators_latest）から毎回判定する。判定できない値（None）ではサインを出さない。

売るかどうかは配当性向と PBR で決める。主のサイン（`SellSignal::is_primary`）は API の `signals`、参考のサインは `reference_signals` に入れる。一覧は主のサインの多い順、同数なら参考のサインの多い順に並べる。主のサインは株式（`kind = stock`）だけで判定する（REIT は利益のほぼ全部を分配するので、配当性向が常に高く出る）。

| サイン（API の値） | 区分 | 条件 | 使う列 |
|---|---|---|---|
| `high_payout` | 主 | 配当性向 > 50%（`PAYOUT_WARN`。ちょうど50%は出さない） | payout_ratio |
| `high_pbr` | 主 | PBR > 1.5倍（`PBR_WARN`。ちょうど1.5倍は出さない） | pbr |
| `loss_dividend` | 主 | 最新の本決算の EPS ≤ 0 で、1株配当（予想か実績）> 0。赤字だと配当性向は出ないので、画面の配当性向は「赤字」と出す | financials_annual の最新の本決算の eps（数字のない行は飛ばす）、dividend_ttm |
| `expensive` | 参考 | valuation が `expensive` | valuation |
| `dividend_cut` | 参考 | 直近の1株配当の実績が前期より少ない、または会社予想が直近の実績より少ない（無配予想を含む。据え置きは出さない） | dividend_cut |
| `profit_downtrend` | 参考 | 営業利益の増加傾向が偽 | profit_uptrend |
| `loss` | 参考 | 通期で赤字なし、または四半期で赤字なしが偽 | no_loss_years, no_quarterly_loss |

1人が登録できる銘柄は100（`MAX_ITEMS`）。

### 増配期待の警告（F4、未実装）

しきい値はユーザーが`alert_settings`で変更できるようにする。下表は初期値。

| 警告 | 判定条件（初期値） | 重さ | 使うデータ |
|---|---|---|---|
| 減配 | 直近の配当実績が前年同期を下回った | 高 | financials_annual、financials_quarterly |
| 配当方針の変更 | 有報・半期報告書から「累進配当」「非減配」「DOE」の記述が消えた | 高 | edinet_docs（前の書類の値を残す必要がある） |
| 赤字転落 | 最新期の営業利益または純利益がマイナス | 高 | financials_annual |
| 本業の弱体化 | 営業利益が2期連続で減少 | 中 | financials_annual |
| 配当余力の低下 | 配当性向が70%超 | 中 | indicators_latest |
| 現金の不足 | 営業キャッシュフローがマイナス | 中 | financials_annual |

### 売り時条件（F5、未実装）

- 使える指標: 株価、配当利回り、利回りの過去5年比、PBR、PER、取得単価からの騰落率
- 演算子: `>=`、`<=`。条件はAND/ORで組み合わせる
- 対象: 銘柄ごと、または全保有株共通
- 再通知の抑止: 条件ごとに「前回当たっていたか」を持ち、外れた→当たったに変わったときだけ通知する

### 通知（F6、未実装）

- 判定Workflowはユーザーごとにステップを分け、警告・売り時の結果を`notifications`に書く
- 管理者（シークレット`ADMIN_NOTIFY_EMAIL`と一致するユーザー）にだけ、管理者の保有株の警告・売り時とバッチの結果をまとめて1通送る。該当がなければ送らない
- 本文: 銘柄名、当たった条件、現在値、データ取得日、銘柄詳細へのリンク、免責表示

### AIレコメンド（F7、実装済み）

`usecase::recommend::generate`。API Worker の Cron Trigger（平日21:00 JST、`0 12 * * 1-5`）から呼ぶ。候補の条件・モデル・検証の詳細は [08](08_indicator_sources.md#aiレコメンドf7の候補と使うモデル)。

1. 最新の batch_runs が今日終わった `succeeded` でなければ何もしない。同じ株価の日付の結果がすでにあっても何もしない
2. ルールで候補を50銘柄に絞る（株式・利回りが0より大きく8%未満・配当性向50%超・PBR1.5倍超（保有株の売りサイン）は除く・「バリュー株」または「5年配当かつ通期黒字」。満たす条件の多い順、同点は利回りの高い順）
3. 候補の指標を Workers AI に JSON で渡し、上位10銘柄と理由（1〜2文）を JSON Schema で返させる
4. 答えを1件ずつ確かめ、候補にない銘柄・2回目以降の重複・空か200文字を超える理由・10件を超えた分だけを捨てる。残りを AI の並び順のまま詰めて順位にし、5件以上あれば `recommendations` に保存する（捨てた分はログに出す）。5件に届かなければ保存せずログに出す

未実装: AIが失敗した日にルールの候補だけを出すこと、90日より古い行の削除。

## 認証の処理（F1）

`usecase::auth::authenticate`。

1. ブラウザはClerkのReact SDKでGoogleログインし、セッショントークン（JWT）を得る
2. APIは`Authorization: Bearer`ヘッダーでトークンを受け取る（CSRF対策）。なければ 401
3. API WorkerはClerkの公開鍵（JWKS、1時間キャッシュ。検証失敗時は取り直す）でRS256の署名をWeb Cryptoで検証する。発行元（`CLERK_ISSUER`）、発行先（`azp` が `AUTHORIZED_PARTIES` のどれか）、有効期限と `nbf`（ずれは5秒まで）も確かめる。失敗したら 401
4. `users` に1時間以内（`RECHECK_AFTER_SECS`）に確かめた結果があれば、それで許可・拒否を決める
5. なければ Clerk のユーザー API（`CLERK_SECRET_KEY`）でユーザーを取り、`domain::access::decide_access` で判定する
   - 確認済みの主メールアドレスと同じアドレスの Google アカウント（`oauth_google`）が確認済みでつながっていなければ拒否（`denied_not_google`）
   - 主メールアドレスが`ADMIN_NOTIFY_EMAIL`と同じなら管理者として許可
   - `public_metadata` に `{"stock_insight": true}` があれば許可。どちらでもなければ拒否
6. 結果を`users`に保存（初回は自動作成）し、`login_events`に許可・拒否を記録する（記録の失敗でログインは止めない）。拒否は 403
7. Clerk に問い合わせられないときは、前回の結果があればそれを使い、なければ 503
8. 設定（`CLERK_ISSUER`・`AUTHORIZED_PARTIES`・`CLERK_SECRET_KEY`）が足りなければ、すべての `/api/*` を 503 にする

未実装: ClerkのWebhook（`session.created`）の署名をHMAC（Web Crypto）で検証して`login_events`に記録する流れ。今は手順5のたびに記録するので、1時間以内の再ログインは記録されない。

## バッチ

### バッチの起動

GitHub Actionsにもcron（`on: schedule`）はあるが、起動はCloudflareのCron Triggerに任せ、GitHubの`workflow_dispatch`をAPIで呼ぶ。GitHubのcronは次の2点が日次運用に向かないため。

- 混雑時には開始が数十分遅れることがある（時刻指定はUTC）
- 公開リポジトリでは、60日間コミットなどの動きがないと定期実行が自動で止まる

CloudflareのCron Trigger（`30 9 * * 1-5` = 平日18:30 JST）は呼び出しを送るだけなのでCPU 10msに収まる。Workerには`GITHUB_DISPATCH_TOKEN`（対象リポジトリのActions実行権限のみ）をWorker Secretとして登録し、`vars.GITHUB_REPO` の `daily-batch.yml` を `main` で起動する。GitHub側の`schedule`（平日18:45 JST）は予備として残し、`batch-cli should-run` が休場日と当日成功済み（同じラベルで今日0:00 JST以降に `succeeded` の batch_runs がある）を弾く。

手動で動かすときの入力（`gh workflow run daily-batch -f ...`）:

| 入力 | 既定 | 意味 |
|---|---|---|
| `force` | false | 休場日・当日実行済みでも動かす |
| `probe` | false | D1 に触れず、直近の数日分を J-Quants（と EDINET を5件）から取って SQL を成果物に保存する（取得元の確認用） |
| `max_minutes` | 300 | 取り込みの時間の上限（分）。過ぎたらそこまでを保存し、次回は続きから |
| `persist_every` | 1500 | 何日分ごとに D1 へ保存するか |
| `refill_prices` | false | 取り込み済みの過去5年分の株価も取り直して `price_history` を埋める（`sync-jq --refill-prices`）。財務と `sync_state` は変えない |
| `refill_forecasts` | false | 取り込み済みの直近1年の決算短信を読み直し、会社予想の配当（`dividend_forecasts`）だけを埋める（`sync-jq --refill-forecasts`）。年次・四半期の財務、`sync_state`、R2 の生データは変えない |

### 日次バッチのステップ

ワークフロー `daily-batch.yml` は `build`（batch-cli を1回だけビルド）→ `guard`（`should-run`）→ `prepare` → `jquants` と `edinet` を並列、の順に動く。`jquants` と `edinet` は書くテーブルが別（`sync_state` もキーが別）なので、同時に動いてもぶつからない。片方が失敗しても、もう片方は止まらない。

`prepare`（タイムアウト20分）:

1. `batch-cli migrate`: `migrations/` の SQL を順に適用する。`ALTER TABLE ... ADD COLUMN` の「列がすでにある」は読み飛ばす
2. `batch-cli sync-master`: JPX の上場銘柄一覧（xlsx）から `stocks` を upsert（`compute` と `sync-edinet` が使う）

`jquants`（タイムアウト340分）:

3. `batch-cli sync-jq --rate 60 --max-minutes 300 --persist-every 1500 --raw-dir raw`: J-Quants を日付ごとに取り込む
   - 範囲は前回の続き（`sync_state.jq_synced_until` の翌日）から今日まで。初回は5年前から。財務は直近3日分を毎回取り直す
   - 呼び出しは1分あたり `--rate` 回まで（Light は60）。429・5xx・通信エラーは指数バックオフで最大3回再試行し、J-Quants の 429 はさらに1分待って2回まで試す
   - 日足の `AdjFactor` が 1 でない日を株式分割・併合として `splits` に残す（取り直しの日も含む）
   - `--refill-prices` のときは、`--from`（既定は5年前）から前回の続きの前日までの株価も取り直して履歴に足す（同じ日付は上書き）。取り直した日の生データは R2 に上げ直さず、財務も取り直さない
   - `--refill-forecasts` のときは、`--from`（既定は1年前）から財務の取り直しの前日までの決算短信を読み、予想の配当の行だけを `dividend_forecasts` に足す（同じ開示はキーで上書き）。予想を取り込み始める前（2026-10-19 より前）に開示した会社の予想を埋めるためのもの
   - 終値の履歴は直近5年＋1か月（`KEEP_DAYS`）を超えた分を捨てる
   - D1 への保存は `--persist-every` 日分ごと（既定では最後に1回）。保存のたびに全銘柄の `price_history` を書き直すので、回数は少なくする
   - 応答の全項目を `raw/bars/<日付>.json.gz`・`raw/fins/<日付>.json.gz` に書き出す
4. `batch-cli compute --label daily`: D1 に保存済みの終値と財務から、株式分割で基準をそろえたうえで全銘柄の指標を計算して `indicators_latest` に upsert し、`batch_runs` に記録する（run_id は `daily-<開始のUNIX秒>`）。株価のない銘柄は数えるだけで飛ばす。半数以上の銘柄を保存できたら `succeeded`、そうでなければ `failed`
5. R2 へのアップロード: `raw/bars`・`raw/fins` を `maaun-stock-insight-raw/jquants/` に置く（取り込みが途中で失敗しても、取れた分は置く）

`edinet`（タイムアウト340分）:

6. `batch-cli sync-edinet --rate 60 --max-minutes 300 --raw-dir raw`: EDINET の書類一覧（前回の続き。初回は過去400日分）から有報・半期報告書を選び、まだ持っていない書類の CSV を取って `edinet_docs` を更新、文章を `raw/edinet/<コード>/<種類>.json.gz` に書き出す。古くなった分の R2 のキーは `raw/edinet-delete.txt` に書く（詳細は [08](08_indicator_sources.md#取得元の-api)）。ワークフローの入力 `refetch_doe=true` のときは `--refetch-doe` を付け、新しい書類のあとに、DOE・配当性向の下限・目標の数字が欠けている保存済みの書類を取り直す（書類一覧の進み具合は変えない）
   - 続きの位置（`sync_state.edinet_listed_until`）は、読み終えた日・まだ取っていない最初の書類の提出日の前日・今日の前日のうち最も早い日にする。今日の一覧はバッチの後にも書類が増えうるので、翌日もう一度読む（取得済みの書類は飛ばす）
   - 株式の書類は府令が企業内容等の開示府令（`ordinanceCode` 010）のものだけ。会社が出す信託受益証券などの特定有価証券の有報（030、XBRL は jpsps）は、証券コードが会社のものでも読まない
   - 中身が読めない書類（本体の `jpcrp*.csv` がないなど）は、ログに `skip` を出して飛ばし、続ける（やり直しても読めないため）。通信の失敗などはそこで止め、次回その書類からやり直す
7. R2 へのアップロード: `raw/edinet/` のファイルを置き、`edinet-delete.txt` のキーを消す（途中で失敗しても、取れた分は置く。R2 の一時的なエラーに備えて1件ずつ3回までやり直す）

J-Quants を1ジョブで順に取り込むのは、J-Quants が日付指定で全銘柄をまとめて返すため銘柄ごとの並列取得が要らず、呼び出しの上限（60件/分）が律速になるから。EDINET は J-Quants と上限が別なので、別のジョブにして並列に動かす。

### 取得失敗とデータ品質

- 時間切れ（`--max-minutes`）や途中の失敗で止まっても、`sync_state` に保存した日付の続きから次回再開する
- D1 HTTP API への読み書きは、通信エラー・429・5xx（D1 がときどき返す code 7500 の internal error など）なら2秒・4秒・8秒・16秒と待って最大4回やり直す（`adapter-batch/src/d1.rs`）。書き込みはキーでの upsert・削除か `IF NOT EXISTS` なので、やり直しても結果は変わらない。SQL の誤りなどの 4xx はやり直さない
- 1銘柄の計算失敗でバッチ全体を止めず、その銘柄の `indicators_latest` は前回の値のまま残る
- EDINET の失敗は J-Quants の取り込みと一覧の更新を止めない（別のジョブ）。失敗は Actions の実行結果に出る
- 未実装: 当日中の自動再実行、管理者へのメール、前日比±50%超の株価・配当性向300%超・欠損率5%超の「要確認」の検知、画面の「古いデータ」表示

### マイグレーション

- `migrations/`に連番のSQL（`0001`〜`0009`）を置き、日次バッチの最初に`batch-cli migrate`（D1 HTTP API）で適用する。手で適用するときも同じコマンドか `wrangler d1 execute --remote --file`（[07](07_deploy.md)）
- `CREATE ... IF NOT EXISTS` と `ALTER TABLE ... ADD COLUMN` だけで後方互換を保つ。列の削除は2段階で行う

## テーブル定義

`migrations/` の実物。型はD1（SQLite）。日時はUNIX秒、日付は `YYYY-MM-DD` の文字列、比率は小数。

### 共通データ

| テーブル | 列 | 作成 |
|---|---|---|
| stocks | code (PK), name, kind（stock/reit/etf）, market, sector33, size_class（large/mid/small/none）, as_of, updated_at | 0003 |
| indicators_latest | code (PK), run_id, fetched_at, price, price_date, dividend_ttm, dividend_yield, yield_percentile, pbr, per, payout_ratio, doe, consecutive_increases, operating_margin, no_loss_years, profit_uptrend, valuation, fin_doc_id, fin_submitted, dividend_every_year, no_quarterly_loss, is_value, data_years, sales_growth, op_growth, dividend_basis, dividend_cut | 0001（0005・0007・0010・0012 で列を追加） |
| financials_annual | code, period_end (PK: code+period_end), doc_id, net_sales, operating_income, ordinary_income, net_income, equity, eps, bps, dps, operating_cf, fetched_at, disclosed | 0001（0004 で disclosed を追加） |
| financials_quarterly | code, fiscal_year_end, period_type (PK: code+fiscal_year_end+period_type), period_end, doc_id, disclosed, net_sales, operating_income, ordinary_income, net_income, fetched_at | 0005 |
| price_history | code (PK), closes（`YYYYMMDD,終値;…`）, last_date, updated_at | 0004 |
| edinet_docs | code, doc_kind (PK: code+doc_kind。annual/semiannual), doc_id, submitted, period_end, r2_key, progressive_dividend, no_dividend_cut, dividend_raise, doe_target, payout_target, phrases（JSON の文字列配列）, fetched_at, doe_floor, doe_goal, doe_phrase（DOE の下限・目標と、その数字を拾った文）, payout_floor, payout_goal, payout_phrase（配当性向の下限・目標と、その文。payout_target は下限（なければ目標）として残す） | 0007, 0013, 0014 |
| sync_state | key (PK。`jq_synced_until`・`edinet_listed_until`), value, updated_at | 0004 |
| splits | code, date（権利落ち日）(PK: code+date), factor（`AdjFactor`。1:2 の分割なら 0.5）, fetched_at | 0009 |
| recommendations | date（株価の日付）, rank (PK: date+rank), code, reason, model, created_at | 0008 |
| batch_runs | run_id (PK), started_at, finished_at, target, succeeded, failed, status（running/succeeded/failed） | 0001 |

索引: indicators_latest の dividend_yield・pbr・payout_ratio・valuation、stocks の kind。

### ユーザーデータ（すべて`user_id`を持つ）

| テーブル | 列 | 作成 |
|---|---|---|
| users | id (PK, ClerkのユーザーID), email, display_name, allowed, is_admin, checked_at, created_at | 0002 |
| login_events | id (PK), user_id, email, occurred_at, ip_address, user_agent, result（allowed / denied_not_google / denied_not_listed） | 0002（索引 occurred_at） |
| portfolio_items | user_id, code (PK: user_id+code), added_at | 0006 |

### 予定のテーブル（列の案）

| テーブル | 主な列 |
|---|---|
| holdings | user_id, code, shares, acquisition_price, memo, created_at, updated_at（portfolio_items に列を足す案もある） |
| sell_rules | id, user_id, code（NULLなら全保有株共通）, expression（AND/ORの条件をJSONで保持）, last_matched, updated_at |
| alert_settings | user_id (PK), payout_ratio_threshold, op_income_decline_periods, ほか各しきい値 |
| saved_filters | id, user_id, name, filter_json, is_exclusion, updated_at |
| notifications | id, user_id, code, kind（警告/売り時）, condition, value, data_date, created_at |

## キャッシュ

| 対象 | 実装 | 状態 |
|---|---|---|
| 一覧・配当方針・状態の API（`/api/stocks`・`/api/stocks/:code/edinet`・`/api/status`） | Cache API（TTL 24時間）。キーと ETag（弱い ETag、`If-None-Match` が一致すれば 304）は「アプリの版＋データの版」から作る。データの版は最新バッチの run_id・終了時刻と EDINET 取り込みの最終時刻（`sync_state` の `edinet_listed_until` の `updated_at`）で、Worker のメモリに60秒持つ。バッチの実行中は使わない（3時間以上「実行中」なら止まったとみなして使う）。ブラウザには `Cache-Control: private, max-age=300` | 済み |
| ユーザー固有データ（`/api/me`・`/api/portfolio`）とエラー | `Cache-Control: no-store` | 済み |
| 静的ファイル | `/assets/*`（ハッシュ付きJS・CSS）は`public, max-age=31536000, immutable`、それ以外は`no-cache`（`web/public/_headers`） | 済み |
| JWKS | Workerのメモリに1時間。検証失敗時は即時取り直し | 済み |
| 許可リストの判定 | D1 の `users` に1時間。Worker のメモリにも同じ1時間持ち、その間は D1 を読まない | 済み |
| 一覧の件数 | 一覧の SQL に `COUNT(*) OVER ()` を足し、同じスキャンで数える。最後のページより先を開いて0行のときだけ `COUNT(*)` を流す | 済み |

## エラー処理とログ

- エラー型は各crateの自前の型で定義する（anyhowは使わない）
- APIのエラーは `{"error": {"code": ..., "message": ...}}` とHTTPステータスで返す

| ステータス | code | 場面 |
|---|---|---|
| 400 | `bad_request` | クエリの値が不正（未知の並べ替え、min > max、不正な銘柄コードなど） |
| 401 | `unauthenticated` | トークンがない・検証できない（`WWW-Authenticate: Bearer` を付ける） |
| 403 | `forbidden` | Google 以外のログイン、許可リスト外 |
| 404 | `not_found` | 未知の `/api/*` |
| 409 | `too_many` | ポートフォリオが100銘柄を超える |
| 500 | `internal` | 内部エラー（詳細は応答に出さず Worker のログにだけ残す） |
| 503 | `unavailable` | 認証の設定が足りない、Clerk に問い合わせられず前回の結果もない |

- 429（レート制限）は未実装
- ログにはキー・トークン・メールアドレス・ポートフォリオは出さず、件数と銘柄コードだけを出す
