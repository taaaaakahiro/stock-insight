# 05 詳細設計

> 出典: Claude Docs「増配期待株ダッシュボード 要件定義」（rev 56, 2026-10-21）

## 指標の定義

| 指標 | 計算式 | 種類 |
|---|---|---|
| PBR | 株価 ÷ BPS | 時価 |
| PER | 株価 ÷ EPS（直近実績） | 時価 |
| 配当利回り | 年間配当（直近12か月の実績）÷ 株価 | 時価 |
| 配当利回りパーセンタイル | 現在の配当利回りが、過去10年の週次配当利回りのうち下から何%の位置か。80%以上で「割安」、20%以下で「割高」 | 時価 |
| 配当性向 | 1株配当 ÷ EPS | 財務 |
| DOE | 配当金総額 ÷ 自己資本（期首・期末平均）。簡易版は1株配当 ÷ BPS | 財務 |
| 連続増配年数 | 1株配当（株式分割調整後）が前年を上回った年の連続数 | 財務 |
| 営業利益率 | 営業利益 ÷ 売上 | 財務 |
| 赤字なし | 直近5期の営業利益・純利益がすべてプラス | 財務 |
| 営業利益の増加傾向 | 直近5期の営業利益の回帰の傾きがプラス | 財務 |

株式分割は配当・株価の両方を調整してから計算する。指標はバッチで計算して保存し、画面では計算しない。

### 割安・中立・割高バッジ

| バッジ | 条件 | 色 |
|---|---|---|
| 割安 | 配当利回りパーセンタイル ≧ 80% | 緑 |
| 中立 | 20% < パーセンタイル < 80% | なし |
| 割高 | 配当利回りパーセンタイル ≦ 20% | 赤 |

色だけに意味を持たせず、文字（割安・中立・割高）を必ず併記する。

## 判定ロジック

判定は`domain`の純粋関数として実装し、判定Workflowから呼ぶ。

### 増配期待の警告（F4）

しきい値はユーザーが`alert_settings`で変更できる。下表は初期値。

| 警告 | 判定条件（初期値） | 重さ | 使うデータ |
|---|---|---|---|
| 減配 | 直近の配当実績が前年同期を下回った | 高 | 配当履歴、有報・半期報告書 |
| 配当方針の変更 | 有報の配当方針から「累進配当」「DOE目標」が消えた | 高 | dividend_policies（AIで前年と比較） |
| 赤字転落 | 最新期の営業利益または純利益がマイナス | 高 | financials_annual |
| 本業の弱体化 | 営業利益が2期連続で減少 | 中 | financials_annual |
| 配当余力の低下 | 配当性向が70%超 | 中 | indicators_latest |
| 現金の不足 | 営業キャッシュフローがマイナス | 中 | financials_annual |

### 売り時条件（F5）

- 使える指標: 株価、配当利回り、配当利回りパーセンタイル、PBR、PER、取得単価からの騰落率
- 演算子: `>=`、`<=`。条件はAND/ORで組み合わせる
- 対象: 銘柄ごと、または全保有株共通
- 再通知の抑止: 条件ごとに「前回当たっていたか」を持ち、外れた→当たったに変わったときだけ通知する

### 通知（F6）

- 判定Workflowはユーザーごとにステップを分け、警告・売り時の結果を`notifications`に書く
- 管理者（シークレット`ADMIN_NOTIFY_EMAIL`と一致するユーザー）にだけ、管理者の保有株の警告・売り時とバッチの結果をまとめて1通送る。該当がなければ送らない
- 本文: 銘柄名、当たった条件、現在値、データ取得日、銘柄詳細へのリンク、免責表示

### AIレコメンド（F7）

1. その日の株価で、ルールにより候補を約50銘柄に絞る（例: 割安バッジ、赤字なし、減配なし）
2. 候補の指標をWorkers AI（Llama 3.1 8B）に渡し、上位10銘柄と理由（1〜2文）を出させる
3. 根拠の数字と出典を併記して`recommendations`に保存（90日保持）
4. AIが失敗した日はルールによる候補だけを表示する
5. 配当方針の抽出（有報の「配当政策」節 → 累進配当の有無・DOE目標値）は無料枠に収まるよう数日に分ける

## 認証の処理（F1）

1. ブラウザはClerkのReact SDKでGoogleログインし、セッショントークン（JWT）を得る
2. APIは`Authorization`ヘッダーでトークンを受け取る（CSRF対策）
3. API WorkerはClerkの公開鍵（JWKS、1時間キャッシュ。検証失敗時は取り直す）でRS256の署名をWeb Cryptoで検証する
4. トークンのログイン方法がGoogle以外なら拒否する
5. 許可リスト（メールアドレス、またはClerkのユーザーメタデータの「stock-insight利用可」）になければ拒否する
6. 初回ログイン時は`users`を自動作成（ClerkのユーザーID、メールアドレス、表示名）
7. ClerkのWebhook（`session.created`）は署名をHMAC（Web Crypto）で検証してから`login_events`に記録する。許可リスト外の拒否も記録する

## バッチ

### バッチの並列実行

取得は「ジョブの分割」と「ジョブ内の同時取得」の2段で並列にする。取得先に負荷をかけすぎないよう、同時実行数には上限を設ける。

1. GitHub Actionsのmatrixで銘柄コードを4分割し、4ジョブを同時に実行する（分割数は設定で変更可）
2. 各ジョブ内はtokioで並列に取得する。取得先ごとにSemaphoreで同時実行数を制限（初期値: Yahoo 8、EDINET 4）
3. 失敗は指数バックオフで最大3回再試行し、それでも失敗した銘柄は前回の値を残す
4. D1へは100行単位のバッチで書き込む
5. 全ジョブの完了後に集約ジョブが全銘柄の指標とパーセンタイルを確定し、batch_runsに結果を記録してから判定Workflowを起動する
6. 判定Workflowはユーザーごとにステップを分け、警告・売り時の判定とメール作成を並列に処理する

### バッチの起動

GitHub Actionsにもcron（`on: schedule`）はあるが、起動はCloudflareのCron Triggerに任せ、GitHubの`workflow_dispatch`をAPIで呼ぶ。GitHubのcronは次の2点が日次運用に向かないため。

- 混雑時には開始が数十分遅れることがある（時刻指定はUTC）
- 公開リポジトリでは、60日間コミットなどの動きがないと定期実行が自動で止まる

CloudflareのCron Trigger（平日18:30 JST）は呼び出しを送るだけなのでCPU 10msに収まる。GitHub側の`schedule`は予備として残し、同じ日に2回動かないようbatch_runsで重複を弾く。Workerにはキー名GITHUB_DISPATCH_TOKEN（対象リポジトリのActions実行権限のみ）をWorker Secretとして登録する。

### 取得失敗とデータ品質

- 1銘柄の取得失敗でバッチ全体を止めず、前回の値を残して「古いデータ」と表示する
- 失敗は指数バックオフで最大3回再試行する
- 前日比±50%超の株価、配当性向300%超、欠損率5%超を「要確認」として管理画面に出す
- バッチが失敗したら当日中に自動で1回再実行し、それでも失敗したら管理者にメールする

### マイグレーション

- `batch-cli migrate`でD1 HTTP API経由で適用する（`wrangler d1 migrations`は使わない）
- 後方互換を保ち、列の削除は2段階で行う

## テーブル定義（案）

要件から導いた列の案。型はD1（SQLite）に合わせる。実装時にマイグレーションで確定する。

### 共通データ

| テーブル | 主な列 |
|---|---|
| stocks | code (PK), name, sector33, market, size_class（大型/中型/小型/なし）, updated_at |
| indicators_latest | code (PK), price, price_date, pbr, per, dividend_yield, yield_percentile, payout_ratio, doe, consecutive_increase_years, operating_margin, equity_ratio, market_cap, no_deficit_5y, op_income_uptrend, no_dividend_cut, fetched_at, source, batch_run_id, is_stale |
| financials_annual | code, fiscal_year (PK: code+fiscal_year), revenue, operating_income, net_income, operating_cf, net_assets, eps, bps, dps, total_dividends, filed_at, source, fetched_at |
| dividend_policies | code, fiscal_year, progressive (bool), doe_target, summary, source_doc_id, extracted_at |
| recommendations | date, rank, code, reason, created_at |
| batch_runs | id (PK), started_at, finished_at, target_count, success_count, failure_count, status, trigger（cron/schedule/manual） |

### ユーザーデータ（すべて`user_id`を持つ）

| テーブル | 主な列 |
|---|---|
| users | id (PK, ClerkのユーザーID), email, display_name, created_at |
| login_events | id, user_id, email, occurred_at, ip_address, user_agent, result（許可/拒否） |
| holdings | id, user_id, code, shares, acquisition_price, memo, created_at, updated_at |
| sell_rules | id, user_id, code（NULLなら全保有株共通）, expression（AND/ORの条件をJSONで保持）, last_matched, updated_at |
| alert_settings | user_id (PK), payout_ratio_threshold, op_income_decline_periods, ほか各しきい値 |
| saved_filters | id, user_id, name, filter_json, is_exclusion, updated_at |
| notifications | id, user_id, code, kind（警告/売り時）, condition, value, data_date, created_at |

索引: 一覧の絞り込みに使う`indicators_latest`の列、ユーザーデータの`user_id`。

## キャッシュ

| 対象 | 実装 |
|---|---|
| 銘柄一覧・詳細API | Cache API。キャッシュキーに最新バッチIDを含め、TTL 24時間 |
| APIのレスポンス | `Cache-Control: private, max-age=300` + `ETag`。ユーザー固有データは`no-store` |
| 静的ファイル | ハッシュ付きJS・CSSは`max-age=31536000, immutable`、`index.html`は`no-cache` |
| JWKS | Workerのメモリに1時間。検証失敗時は即時取り直し |

## エラー処理とログ

- エラー型は各crateの自前の`enum`で定義する（anyhowは使わない）
- APIのエラーはHTTPステータスと機械可読なコードで返す（400 入力不正、401 未認証、403 許可リスト外・権限なし、404、429 レート制限、500）
- ログは構造化ログ（JSON）。キー・メールアドレス・保有株は出さず、件数と銘柄コードだけを出す
