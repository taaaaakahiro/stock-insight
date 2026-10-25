# 11 データモデル

物理定義の正本は [migrations](../migrations)。以下は0001〜0018を番号順に適用したスキーマ。型・NOT NULL・主キー・既定値はSQLに従う。SQLiteではPRIMARY KEYと明示的NOT NULLの表記が一致しない場合がある。

## 更新と保存形式

- `batch-cli migrate` はmain.rs内の `MIGRATIONS` に列挙されたSQLを適用する。ファイル追加時はこの一覧も更新する。専用の適用履歴テーブルはない。CLI側で追加済み列を扱うが、WranglerからSQLを直接再実行する場合は別途確認が必要。
- 日付は主に `YYYY-MM-DD`、時刻はUnix秒。真偽値はINTEGERの0/1、判定不能はNULL。
- `price_history.closes` は `YYYYMMDD,終値;…` の圧縮文字列。価格・財務は原数値を保存し、計算時に分割調整する。
- `themes.words`、`edinet_docs.phrases`、REITの `checks` と `body` はJSON文字列。
- 金額は円、通常の比率は小数。REITの保存上の平均利率・稼働率は%値で、一覧APIで小数へ変換する。
- 個人データの分離は `portfolio_items.user_id` と認証済みIDを使うクエリで行う。
- EDINETは種類ごとに最新を置換。REIT最新値は古い期で上書きせず、期別要約を履歴へ保存。レコメンド再生成は最新表示キャッシュのみ置換し、生成試行と公開履歴を追記する。
- 履歴の一律削除期限を設ける定期処理はない。株価履歴は最後の株価日から1861日（5×366+31日）の範囲に切り詰める。EDINETの整理も取得処理が行う。

## `batch_runs`

指標計算の実行記録。

| 列 | 型 | NOT NULL | 主キー順 | 既定値 |
|---|---|---|---|---|
| `run_id` | TEXT | — | 1 | — |
| `started_at` | INTEGER | YES | — | — |
| `finished_at` | INTEGER | — | — | — |
| `target` | INTEGER | YES | — | `0` |
| `succeeded` | INTEGER | YES | — | `0` |
| `failed` | INTEGER | YES | — | `0` |
| `status` | TEXT | YES | — | — |

索引 `sqlite_autoindex_batch_runs_1`: (run_id)、UNIQUE。

外部キー: なし。

## `dividend_forecasts`

開示ごとの配当予想。

| 列 | 型 | NOT NULL | 主キー順 | 既定値 |
|---|---|---|---|---|
| `code` | TEXT | YES | 1 | — |
| `doc_id` | TEXT | YES | 2 | — |
| `disclosed` | TEXT | YES | — | — |
| `fiscal_year_end` | TEXT | — | — | — |
| `dps` | REAL | — | — | — |
| `fetched_at` | INTEGER | YES | — | — |

索引 `sqlite_autoindex_dividend_forecasts_1`: (code, doc_id)、UNIQUE。

外部キー: なし。

## `edinet_docs`

銘柄・種類ごとの配当方針と事業内容。

| 列 | 型 | NOT NULL | 主キー順 | 既定値 |
|---|---|---|---|---|
| `code` | TEXT | YES | 1 | — |
| `doc_kind` | TEXT | YES | 2 | — |
| `doc_id` | TEXT | YES | — | — |
| `submitted` | TEXT | YES | — | — |
| `period_end` | TEXT | — | — | — |
| `r2_key` | TEXT | YES | — | — |
| `progressive_dividend` | INTEGER | YES | — | — |
| `no_dividend_cut` | INTEGER | YES | — | — |
| `dividend_raise` | INTEGER | YES | — | — |
| `doe_target` | INTEGER | YES | — | — |
| `payout_target` | REAL | — | — | — |
| `phrases` | TEXT | YES | — | — |
| `fetched_at` | INTEGER | YES | — | — |
| `doe_floor` | REAL | — | — | — |
| `doe_goal` | REAL | — | — | — |
| `doe_phrase` | TEXT | — | — | — |
| `payout_floor` | REAL | — | — | — |
| `payout_goal` | REAL | — | — | — |
| `payout_phrase` | TEXT | — | — | — |
| `business_text` | TEXT | — | — | — |

索引 `sqlite_autoindex_edinet_docs_1`: (code, doc_kind)、UNIQUE。

外部キー: なし。

## `financials_annual`

通期財務の開示。

| 列 | 型 | NOT NULL | 主キー順 | 既定値 |
|---|---|---|---|---|
| `code` | TEXT | YES | 1 | — |
| `period_end` | TEXT | YES | 2 | — |
| `doc_id` | TEXT | YES | — | — |
| `net_sales` | REAL | — | — | — |
| `operating_income` | REAL | — | — | — |
| `ordinary_income` | REAL | — | — | — |
| `net_income` | REAL | — | — | — |
| `equity` | REAL | — | — | — |
| `eps` | REAL | — | — | — |
| `bps` | REAL | — | — | — |
| `dps` | REAL | — | — | — |
| `operating_cf` | REAL | — | — | — |
| `fetched_at` | INTEGER | YES | — | — |
| `disclosed` | TEXT | — | — | — |

索引 `sqlite_autoindex_financials_annual_1`: (code, period_end)、UNIQUE。

外部キー: なし。

## `financials_quarterly`

累計四半期財務の開示。

| 列 | 型 | NOT NULL | 主キー順 | 既定値 |
|---|---|---|---|---|
| `code` | TEXT | YES | 1 | — |
| `fiscal_year_end` | TEXT | YES | 2 | — |
| `period_type` | TEXT | YES | 3 | — |
| `period_end` | TEXT | YES | — | — |
| `doc_id` | TEXT | YES | — | — |
| `disclosed` | TEXT | — | — | — |
| `net_sales` | REAL | — | — | — |
| `operating_income` | REAL | — | — | — |
| `ordinary_income` | REAL | — | — | — |
| `net_income` | REAL | — | — | — |
| `fetched_at` | INTEGER | YES | — | — |

索引 `sqlite_autoindex_financials_quarterly_1`: (code, fiscal_year_end, period_type)、UNIQUE。

外部キー: なし。

## `indicators_latest`

銘柄ごとの最新計算結果。

| 列 | 型 | NOT NULL | 主キー順 | 既定値 |
|---|---|---|---|---|
| `code` | TEXT | — | 1 | — |
| `run_id` | TEXT | YES | — | — |
| `fetched_at` | INTEGER | YES | — | — |
| `price` | REAL | YES | — | — |
| `price_date` | TEXT | YES | — | — |
| `dividend_ttm` | REAL | YES | — | — |
| `dividend_yield` | REAL | — | — | — |
| `yield_percentile` | REAL | — | — | — |
| `pbr` | REAL | — | — | — |
| `per` | REAL | — | — | — |
| `payout_ratio` | REAL | — | — | — |
| `doe` | REAL | — | — | — |
| `consecutive_increases` | INTEGER | — | — | — |
| `operating_margin` | REAL | — | — | — |
| `no_loss_years` | INTEGER | — | — | — |
| `profit_uptrend` | INTEGER | — | — | — |
| `valuation` | TEXT | YES | — | — |
| `fin_doc_id` | TEXT | — | — | — |
| `fin_submitted` | TEXT | — | — | — |
| `dividend_every_year` | INTEGER | — | — | — |
| `no_quarterly_loss` | INTEGER | — | — | — |
| `is_value` | INTEGER | — | — | — |
| `data_years` | INTEGER | — | — | — |
| `sales_growth` | REAL | — | — | — |
| `op_growth` | REAL | — | — | — |
| `dividend_basis` | TEXT | — | — | — |
| `dividend_cut` | INTEGER | — | — | — |
| `op_growth_q` | REAL | — | — | — |
| `net_growth_q` | REAL | — | — | — |
| `op_growth_ttm` | REAL | — | — | — |
| `profit_one_off` | INTEGER | — | — | — |

索引 `idx_ind_val`: (valuation)。

索引 `idx_ind_payout`: (payout_ratio)。

索引 `idx_ind_pbr`: (pbr)。

索引 `idx_ind_yield`: (dividend_yield)。

索引 `sqlite_autoindex_indicators_latest_1`: (code)、UNIQUE。

外部キー: なし。

## `login_events`

認証情報再確認時の記録。

| 列 | 型 | NOT NULL | 主キー順 | 既定値 |
|---|---|---|---|---|
| `id` | INTEGER | — | 1 | — |
| `user_id` | TEXT | YES | — | — |
| `email` | TEXT | — | — | — |
| `occurred_at` | INTEGER | YES | — | — |
| `ip_address` | TEXT | — | — | — |
| `user_agent` | TEXT | — | — | — |
| `result` | TEXT | YES | — | — |

索引 `idx_login_events_at`: (occurred_at)。

外部キー: なし。

## `portfolio_items`

本人の登録銘柄。

| 列 | 型 | NOT NULL | 主キー順 | 既定値 |
|---|---|---|---|---|
| `user_id` | TEXT | YES | 1 | — |
| `code` | TEXT | YES | 2 | — |
| `added_at` | INTEGER | YES | — | — |

索引 `sqlite_autoindex_portfolio_items_1`: (user_id, code)、UNIQUE。

外部キー: なし。

## `price_history`

銘柄ごとの終値履歴。

| 列 | 型 | NOT NULL | 主キー順 | 既定値 |
|---|---|---|---|---|
| `code` | TEXT | — | 1 | — |
| `closes` | TEXT | YES | — | — |
| `last_date` | TEXT | YES | — | — |
| `updated_at` | INTEGER | YES | — | — |

索引 `sqlite_autoindex_price_history_1`: (code)、UNIQUE。

外部キー: なし。

## `recommendations`

日別レコメンド結果。

| 列 | 型 | NOT NULL | 主キー順 | 既定値 |
|---|---|---|---|---|
| `date` | TEXT | YES | 1 | — |
| `rank` | INTEGER | YES | 2 | — |
| `code` | TEXT | YES | — | — |
| `reason` | TEXT | YES | — | — |
| `model` | TEXT | YES | — | — |
| `created_at` | INTEGER | YES | — | — |
| `grade` | TEXT | — | — | — |
| `source` | TEXT | — | — | — |

索引 `sqlite_autoindex_recommendations_1`: (date, rank)、UNIQUE。

外部キー: なし。

## `reit_detail`

REITの最新明細JSON。

| 列 | 型 | NOT NULL | 主キー順 | 既定値 |
|---|---|---|---|---|
| `code` | TEXT | — | 1 | — |
| `period_end` | TEXT | YES | — | — |
| `body` | TEXT | YES | — | — |
| `fetched_at` | INTEGER | YES | — | — |

索引 `sqlite_autoindex_reit_detail_1`: (code)、UNIQUE。

外部キー: なし。

## `reit_history`

REITの期別要約。

| 列 | 型 | NOT NULL | 主キー順 | 既定値 |
|---|---|---|---|---|
| `code` | TEXT | YES | 1 | — |
| `period_end` | TEXT | YES | 2 | — |
| `doc_id` | TEXT | YES | — | — |
| `loans_total` | INTEGER | YES | — | — |
| `bonds` | INTEGER | YES | — | — |
| `rate_floating` | INTEGER | YES | — | — |
| `rate_unknown` | INTEGER | YES | — | — |
| `swap_notional` | INTEGER | — | — | — |
| `avg_rate` | REAL | — | — | — |
| `property_count` | INTEGER | YES | — | — |
| `leasable_m2` | REAL | — | — | — |
| `occupancy` | REAL | — | — | — |
| `fetched_at` | INTEGER | YES | — | — |

索引 `sqlite_autoindex_reit_history_1`: (code, period_end)、UNIQUE。

外部キー: なし。

## `reit_profile`

REITの最新要約。

| 列 | 型 | NOT NULL | 主キー順 | 既定値 |
|---|---|---|---|---|
| `code` | TEXT | — | 1 | — |
| `doc_id` | TEXT | YES | — | — |
| `submitted` | TEXT | YES | — | — |
| `period_end` | TEXT | YES | — | — |
| `sector` | TEXT | — | — | — |
| `short_loans` | INTEGER | YES | — | — |
| `long_loans` | INTEGER | YES | — | — |
| `bonds` | INTEGER | YES | — | — |
| `rate_fixed` | INTEGER | YES | — | — |
| `rate_swap_fixed` | INTEGER | YES | — | — |
| `rate_swap_amount` | INTEGER | YES | — | — |
| `rate_floating` | INTEGER | YES | — | — |
| `rate_unknown` | INTEGER | YES | — | — |
| `swap_notional` | INTEGER | — | — | — |
| `avg_rate` | REAL | — | — | — |
| `avg_years` | REAL | — | — | — |
| `reprice_1y` | INTEGER | YES | — | — |
| `reprice_1y_max` | INTEGER | YES | — | — |
| `property_count` | INTEGER | YES | — | — |
| `leasable_m2` | REAL | — | — | — |
| `leased_m2` | REAL | — | — | — |
| `occupancy` | REAL | — | — | — |
| `checks` | TEXT | YES | — | — |
| `fetched_at` | INTEGER | YES | — | — |

索引 `sqlite_autoindex_reit_profile_1`: (code)、UNIQUE。

外部キー: なし。

## `splits`

分割・併合係数。

| 列 | 型 | NOT NULL | 主キー順 | 既定値 |
|---|---|---|---|---|
| `code` | TEXT | YES | 1 | — |
| `date` | TEXT | YES | 2 | — |
| `factor` | REAL | YES | — | — |
| `fetched_at` | INTEGER | YES | — | — |

索引 `sqlite_autoindex_splits_1`: (code, date)、UNIQUE。

外部キー: なし。

## `stock_themes`

保存済みテーマ一致。

| 列 | 型 | NOT NULL | 主キー順 | 既定値 |
|---|---|---|---|---|
| `theme_id` | INTEGER | YES | 1 | — |
| `code` | TEXT | YES | 2 | — |
| `matched_word` | TEXT | YES | — | — |

索引 `idx_stock_themes_code`: (code)。

索引 `sqlite_autoindex_stock_themes_1`: (theme_id, code)、UNIQUE。

外部キー: なし。

## `stocks`

JPX銘柄マスタ。

| 列 | 型 | NOT NULL | 主キー順 | 既定値 |
|---|---|---|---|---|
| `code` | TEXT | — | 1 | — |
| `name` | TEXT | YES | — | — |
| `kind` | TEXT | YES | — | — |
| `market` | TEXT | YES | — | — |
| `sector33` | TEXT | — | — | — |
| `size_class` | TEXT | YES | — | — |
| `as_of` | TEXT | YES | — | — |
| `updated_at` | INTEGER | YES | — | — |

索引 `idx_stocks_kind`: (kind)。

索引 `sqlite_autoindex_stocks_1`: (code)、UNIQUE。

外部キー: なし。

## `sync_state`

取得カーソル・更新時刻。

| 列 | 型 | NOT NULL | 主キー順 | 既定値 |
|---|---|---|---|---|
| `key` | TEXT | — | 1 | — |
| `value` | TEXT | YES | — | — |
| `updated_at` | INTEGER | YES | — | — |

索引 `sqlite_autoindex_sync_state_1`: (key)、UNIQUE。

外部キー: なし。

## `themes`

共通テーマ定義。

| 列 | 型 | NOT NULL | 主キー順 | 既定値 |
|---|---|---|---|---|
| `id` | INTEGER | — | 1 | — |
| `name` | TEXT | YES | — | — |
| `words` | TEXT | YES | — | — |
| `created_at` | INTEGER | YES | — | — |
| `updated_at` | INTEGER | YES | — | — |

索引 `sqlite_autoindex_themes_1`: (name)、UNIQUE。

外部キー: なし。

## `users`

Clerkユーザーの許可判定キャッシュ。

| 列 | 型 | NOT NULL | 主キー順 | 既定値 |
|---|---|---|---|---|
| `id` | TEXT | — | 1 | — |
| `email` | TEXT | YES | — | — |
| `display_name` | TEXT | YES | — | — |
| `allowed` | INTEGER | YES | — | — |
| `is_admin` | INTEGER | YES | — | — |
| `checked_at` | INTEGER | YES | — | — |
| `created_at` | INTEGER | YES | — | — |

索引 `sqlite_autoindex_users_1`: (id)、UNIQUE。

外部キー: なし。

## 原本保存

R2の `jquants/bars/<日付>.json.gz` と `jquants/fins/<日付>.json.gz` に取得原本、`edinet/<コード>/annual.json.gz`・`semiannual.json.gz` に抽出文章を保存する。REIT詳細はD1のJSONから配信する。R2はAPI Workerから直接参照しない。

## 推薦追跡（0021）

DDLの正本は [0021_recommendation_tracking.sql](../migrations/0021_recommendation_tracking.sql)。

| テーブル | キー・保存内容 |
|---|---|
| recommendation_runs | 自動採番id、legacy_key一意。基準日・バッチ・版・入力・応答・状態・生成元・生成/公開時刻・エラー。is_primary=1はprice_dateごとに一意 |
| recommendation_candidates | (run_id,code)主キー、(run_id,rank)一意。rule_rank、snapshot_json、公開rank/grade/reason |
| recommendation_tracking_meta | key主キー。旧キャッシュ移行済みマーカー。繰り返しmigrateしても再移行しない |
| evaluation_market_data | id、code、trade_date、close、split_factor、observed_at。観測値の組に一意制約。訂正は追記 |
| evaluation_batches | id、created_at、as_of、source_revision、calendar_json、report_json、version、status。completeのみ閲覧対象 |
| recommendation_outcomes | (run_id,code,horizon,batch_id)主キー、result_json。codeは銘柄または集計キー__ai10（公開推薦）、__rule10、__candidates |
| recommendation_fundamentals | id、code、source、observed_at、payload。直前から内容が変わった観測を追記 |

外部キーは候補→run、成績→run/batch。自動削除なし。raw JSONはDB内の監査情報であり、履歴APIは候補指標と計算結果だけを返す。現在の選定はルール方式で、内部キー__ai10は互換名として公開推薦を表す。
