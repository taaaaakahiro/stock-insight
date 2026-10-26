# 11 データモデル

物理定義は [migrations](../migrations) が正本。型・列・索引をこの文書へ重複転記せず、用途と更新単位を示す。

## 保存形式と適用

- 日付は `YYYY-MM-DD`、時刻はUnix秒。真偽値は0/1、判定不能はNULL。
- 金額は円、比率は小数。REITの保存上の平均利率・稼働率は%値で、一覧APIで小数へ変換する。
- `price_history.closes` は `YYYYMMDD,終値;…`。原数値を保存し、計算時に分割調整する。
- JSONの列もD1では文字列として保存する。原本・AI入出力は公開APIへそのまま返さない。
- `batch-cli migrate` は[main.rs](../crates/batch-cli/src/main.rs)の `MIGRATIONS` にあるSQLを実行し、追加済み列のエラーを無視する。専用の適用履歴は管理しない。
- `0019_acquisition.sql`・`0020_acquisition_close.sql` はCLIの一覧に含まれない。スキーマを確認して別途適用する。Wranglerでの再適用時も追加済み列を確認する。

## 基本テーブル

| テーブル | 保存単位・用途 |
|---|---|
| `stocks` | 銘柄コード。JPXの名称・種類・市場・業種・規模 |
| `price_history` | 銘柄コード。日足終値履歴 |
| `splits` | 銘柄コード・日付。分割・併合係数 |
| `financials_annual` | 銘柄コード・期末。通期財務 |
| `financials_quarterly` | 銘柄コード・年度末・期種別。累計四半期財務 |
| `dividend_forecasts` | 銘柄コード・開示ID。配当予想 |
| `indicators_latest` | 銘柄コード。最新の計算指標 |
| `batch_runs` | 実行ID。指標計算の日時・件数・成否 |
| `sync_state` | キー。取得カーソル・更新時刻・実行状態 |
| `edinet_docs` | 銘柄コード・書類種類。最新の配当方針・事業内容 |
| `users` | ClerkユーザーID。利用許可・管理者判定のキャッシュ |
| `login_events` | 自動採番ID。認証情報の再確認記録 |
| `portfolio_items` | ユーザーID・銘柄コード。登録日時、取得日、取得価格、価格基準日 |
| `themes` | テーマID。名前・キーワード。名前は一意 |
| `stock_themes` | テーマID・銘柄コード。一致キーワード |
| `reit_profile` | 銘柄コード。最新期の借入・金利・物件要約 |
| `reit_detail` | 銘柄コード。最新期の借入・返済予定・物件のJSON |
| `reit_history` | 銘柄コード・期末。決算ごとのREIT要約 |

基本定義は0001〜0020。ポートフォリオの3取得列は [0019](../migrations/0019_acquisition.sql)・[0020](../migrations/0020_acquisition_close.sql)、REIT明細は [0011](../migrations/0011_reit.sql) を参照。

## レコメンドと成績

[0021_recommendation_tracking.sql](../migrations/0021_recommendation_tracking.sql)が履歴・追跡の定義。

| テーブル | 保存単位・用途 |
|---|---|
| `recommendations` | 株価日・順位。最新表示用の結果キャッシュ |
| `recommendation_runs` | 試行ID。基準日・元バッチ・版・入力・応答・状態・公開時刻 |
| `recommendation_candidates` | 試行ID・銘柄コード。当時の指標とルール順位、公開順位・評価・理由 |
| `recommendation_tracking_meta` | キー。旧結果の移行済みマーカー |
| `evaluation_market_data` | 観測ID。終値・分割係数・観測時刻。訂正は追記 |
| `evaluation_batches` | 評価ID。価格基準日・暦・レポート・状態 |
| `recommendation_outcomes` | 試行ID・銘柄/集計キー・期間・評価ID。成績JSON |
| `recommendation_fundamentals` | 観測ID。財務・予想・配当方針の変更時点の内容 |

同じ株価日の初回公開版は一意。再生成は新しい履歴を残して最新キャッシュを置換する。候補はrun、成績はrunと評価batchを外部キーで参照する。`__ai10` は公開レコメンドの互換用内部名で、AI選別を意味しない。計算と公開条件は[成績仕様](16_recommendation_evaluation.md)。

## REIT分析

定義: [0022_reit_rag.sql](../migrations/0022_reit_rag.sql)、[0023_reit_rag_facts.sql](../migrations/0023_reit_rag_facts.sql)。

| テーブル | 保存単位・用途 |
|---|---|
| `reit_rag_documents` | 文書版ID。銘柄・書類ID・期末・提出日・原本ハッシュ・R2キー・状態 |
| `reit_rag_chunks` | 分割片ID。文書版・原文位置・本文・検索文字列 |
| `reit_rag_fts` | FTS5検索索引。付随する内部テーブルはSQLiteが管理 |
| `reit_rag_numbers` | 文書版ID。数値観測JSON |
| `reit_rag_document_events` | イベントID。訂正・撤回等の情報と親書類ID |
| `reit_rag_vector_documents` | 文書版・index・埋め込みモデル。索引の準備状態 |
| `reit_rag_extraction_cache` | 入力ハッシュ。検証対象の抽出応答・モデル・版 |
| `reit_rag_runs` | 実行ID。基準時刻・株価日・モデル・版・入出力キー・状態 |
| `reit_rag_results` | 実行ID・種類・対象。銘柄評価／セクター結果のJSON |
| `reit_rag_publication` | 単一行。公開する実行IDと基準時刻 |

公開版が参照するreadyなrunだけをAPIが読む。途中失敗の結果は公開しない。`sync_state.reit_rag_attempt_status` は更新試行の状態で、前回公開版とは別。処理の詳細は[REIT分析](17_reit_analysis.md)。

## 原本と保持

R2の `jquants/bars/`・`jquants/fins/` に取得原本、`edinet/<code>/` に最新の抽出文章、`rag/` にREIT分析の原本・解析・入出力を保存する。API WorkerにR2 bindingはなく、公開結果はD1から返す。

株価履歴は最後の価格日から1861日以内に切り詰める。EDINET配当方針は種類ごとの最新を置換・整理する。REIT最新値は古い期で上書きしない。その他の履歴・分析結果に一律の自動削除期限はない。
