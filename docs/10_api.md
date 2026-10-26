# 10 API仕様

実装元は [api-worker/src/lib.rs](../crates/api-worker/src/lib.rs)。全ルートでBearer JWTと利用許可が必要。管理権限は[権限仕様](user-permissions.md)、キャッシュは[共通処理](05_detailed_design.md)に従う。正常な変更応答もHTTP 200を返す。

## エンドポイント

| メソッド・パス | 入力 | 正常応答の構造 | 個別のエラー |
|---|---|---|---|
| `GET /api/me` | なし | `id, email, display_name, is_admin, role, permissions` | — |
| `GET /api/stocks` | 下記クエリ | `rows, total, page, page_size` | 不正条件400 |
| `GET /api/stocks/:code` | 銘柄コード | `stock, annual, quarters, yield_series, price_series, latest_disclosed` | 不正コード400、指標なし404 |
| `GET /api/stocks/:code/edinet` | 銘柄コード | `code, docs`（未取得は空配列） | 不正コード400 |
| `GET /api/stocks/:code/reit` | 銘柄コード | REIT要約・詳細・履歴 | 不正コード400、未取得404 |
| `GET /api/stocks/:code/reit-assessment` | 銘柄コード | REIT分析の公開版と `items` | 不正コード400、未生成・対象なしは空配列 |
| `GET /api/reit/sectors` | なし | REIT分析の公開版とセクター別 `items` | 未生成は空配列 |
| `GET /api/status` | なし | `version, latest_run`（未実行はnull） | — |
| `GET /api/portfolio` | なし | `codes, rows` | — |
| `PUT /api/portfolio/:code` | 銘柄コード、本文不要 | `ok: true` | 不正コード400、上限409 `too_many` |
| `POST /api/portfolio/:code/acquisition` | `acquisition_date`: YYYY-MM-DDまたはnull | `ok: true` | 不正コード・日付・未来日400 |
| `DELETE /api/portfolio/:code` | 銘柄コード、本文不要 | `ok: true` | 不正コード400 |
| `GET /api/recommendations` | なし | `day`（未生成はnull） | — |
| `GET /api/recommendations/history` | 任意のbefore（正の安全整数） | runs（最大20件）, next_before | 不正cursorは400 |
| `GET /api/recommendations/history/:id` | 正の安全整数ID | run, candidates（当時のstock）, outcomes | 不正ID400、未公開・存在なし404 |
| `GET /api/recommendations/comparison` | なし | report（未評価はnull）, as_of, evaluated_at, version | — |
| `POST /api/recommendations/regenerate` | 本文不要、管理者のみ | `date, picks, dropped` | 前提不成立409 `skipped` |
| `GET /api/themes` | なし | `themes` | — |
| `GET /api/themes/:id` | 整数ID | `theme, stocks` | 不正ID400、存在しない404 |
| `POST /api/themes` | `name, words`、管理者のみ | `id` | 不正入力400、重複409 |
| `PUT /api/themes/:id` | 整数IDと `name, words`、管理者のみ | `ok: true` | 不正入力400、存在しない404、重複409 |
| `DELETE /api/themes/:id` | 整数ID、管理者のみ | `ok: true` | 不正ID400、存在しない404 |

`permissions` は `manage_themes` と `regenerate_recommendations` の真偽値。`role` は `superuser` / `general`。`is_admin` は互換用。

共通エラーは `{"error":{"code":"...","message":"..."}}`。未認証・不正トークン401、利用未許可・管理権限なし403、認証設定不足・情報取得不能503、内部処理失敗500。認証は未知のAPIの404判定より先に行う。エラー応答は `no-store`。

## スクリーニングのクエリ

[screening.rs](../crates/domain/src/screening.rs)が入力検証の正本。

| キー | 形式・既定値 |
|---|---|
| `kind` | `stock`（既定）/ `reit` / `etf` |
| `q` | コードの前方一致、名称の部分一致。前後空白除去・全角英数字の半角化、最大50文字 |
| `sector` | カンマ区切り、最大40件、各30文字以内 |
| `market` | 市場名のカンマ区切り（プライム・スタンダード・グロース） |
| `size` | `large,mid,small,none` の組み合わせ |
| `reit_sector` | `office,logistics,residential,hotel,retail,healthcare,diversified,infra` の組み合わせ |
| `valuation` | `cheap` / `neutral` / `expensive` |
| `no_loss_years`, `profit_uptrend`, `is_value`, `dividend_every_year`, `no_quarterly_loss`, `dividend_policy` | `1` / `true` / `0` / `false` |
| `<metric>_min`, `<metric>_max` | 有限の数値。両端を含む。下限≤上限 |
| `sort` | `code` または下記metric。既定 `dividend_yield` |
| `order` | `asc` / `desc`。既定 `desc` |
| `page` | 1〜1000、既定1。ページサイズは50固定 |

metricは22種類:

`price`, `dividend_ttm`, `dividend_yield`, `yield_percentile`, `pbr`, `per`, `payout_ratio`, `doe`, `consecutive_increases`, `operating_margin`, `sales_growth`, `op_growth`, `floating_ratio`, `unknown_ratio`, `reprice_ratio`, `reprice_max_ratio`, `avg_rate`, `occupancy`, `doe_floor`, `doe_margin`, `op_growth_q`, `op_growth_ttm`。

`dividend_ttm` は検索キーで、応答では採用配当を `dividend` として返す。条件間はAND、同一分類の複数選択はOR。NULLは数値範囲に一致せず、並べ替えでは末尾、同順位はコード昇順。未定義のクエリキーに独自の効果を期待しない。

## 応答の主要型

銘柄行は以下を持つ。不明値はnull、金額は円、比率は小数（0.035 = 3.5%）。PBR・PERは倍。日付は `YYYY-MM-DD`、Unix時刻の列は秒。

| グループ | フィールド |
|---|---|
| 銘柄 | `code, name, kind, market, sector33, size_class` |
| 株価・配当 | `price, price_date, dividend, dividend_basis, dividend_yield, yield_percentile` |
| 指標 | `pbr, per, payout_ratio, doe, consecutive_increases, operating_margin, sales_growth, op_growth` |
| 判定 | `no_loss_years, profit_uptrend, valuation, dividend_every_year, no_quarterly_loss, is_value, dividend_cut` |
| 方針 | `progressive_dividend, no_dividend_cut, doe_floor, doe_goal, doe_forecast, payout_floor, payout_goal, payout_forecast, policy_gap` |
| 直近業績 | `op_growth_q, net_growth_q, op_growth_ttm, profit_one_off` |
| その他 | `fetched_at, data_years, reit` |

`policy_gap` は `basis, policy, now, upside`。`reit` は `sector, period_end, floating_ratio, unknown_ratio, reprice_ratio, reprice_max_ratio, avg_rate, occupancy`。計算式は[指標仕様](08_indicator_sources.md)。

- 銘柄詳細: `stock` は銘柄行。`annual` は新しい順で最大5期の財務と配当性向・営業利益率。`quarters` は古い順で最大20四半期の単独業績と前年同期比。系列は古い順の `[date, value]` 配列。`latest_disclosed` は数字のある最新本決算の開示日。
- EDINET: `docs` の各要素に書類種類・ID・提出日・期末、配当方針フラグ、下限・目標・根拠文、`phrases` を返す。
- REIT詳細: `code, doc_id, submitted, period_end, sector, balance, rate_mix, swap_notional, avg_rate, avg_years, reprice_1y, reprice_1y_max, property_count, leasable_m2, leased_m2, occupancy, checks, detail, history`。**このAPIの平均利率・稼働率は%値**であり、銘柄行の小数比率とは異なる。面積は㎡、金額は円。[REIT仕様](09_reit_debt.md)を参照。
- status: `latest_run` は `run_id, started_at, finished_at, status, latest_price_date, latest_fin_date`。
- ポートフォリオ: `codes` は登録コード、`rows` は銘柄行に `signals, reference_signals, acquisition_date, acquisition_price, acquisition_price_date` を追加した配列。
- レコメンド: `day` は `date, model, created_at, manual, rows`。行は銘柄行に `rank, grade, reason, themes` を追加。gradeは旧保存データではnullになりうる。`manual` は手動生成かどうか。
- テーマ: `id, name, words, count, updated_at`。詳細の `stocks` は `code, name, matched_word`（名称はnull可）。

テーマ入力の `words` は文字列配列またはカンマ・読点・改行区切りの文字列。配列の文字列以外の要素は除かれる。正規化・上限・再照合の仕様は[AI・テーマ](12_recommendations_and_themes.md)を参照。

推薦履歴の3APIは利用許可済みユーザー向け、Cache-Controlはno-store。履歴はpublishedとlegacy_incompleteのみ。入力・応答全文や失敗詳細は返さない。成績は完了バッチの最新結果のみ。価格収益率は小数、horizonは営業日数（0は最新）。状態・集計の意味は[成績仕様](16_recommendation_evaluation.md)を参照。

## REIT分析の応答

2つの分析APIは `private, no-store`。`status` は `ready` / `not_generated`、`update_status` は `not_started` / `running` / `ready` / `failed`、`attempted_at` は更新試行時刻。公開版がある場合は `run_id, generated_at, price_date, items` を返す。`generated_at` はrunの入力基準時刻で、処理終了時刻ではない。

銘柄の `items` は最大1件で `assessment, evidence, verified_metrics, document_issue` を含む。セクターの `items` は `sector, members, periods, ai_summary` を含む。期間の値は `sessions, return, direction, count, available, up_share, series, missing_codes`。欠損の騰落率はnull。定義は[REIT分析](17_reit_analysis.md)。
