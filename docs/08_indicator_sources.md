# 08 指標の取得元と計算式

一覧・詳細に出す各指標が、どの API のどの項目から、どう計算されているかの一覧。取得元は J-Quants API V2（Light プラン）、EDINET API v2（配当方針の文章だけ）と JPX の上場銘柄一覧。Yahoo Finance は使わない。

## 取得元の API

| API | エンドポイント | 呼び方 | 保存先 |
|---|---|---|---|
| 株価四本値 | `GET /v2/equities/bars/daily?date=YYYY-MM-DD` | 日付指定で全上場銘柄。ページ分割（`pagination_key`）をたどる | 全項目: R2 `jquants/bars/<日付>.json.gz`。終値だけ: D1 `price_history`（1銘柄1行、`YYYYMMDD,終値;…`） |
| 財務情報サマリー | `GET /v2/fins/summary?date=YYYY-MM-DD` | 日付指定でその日に開示された全銘柄。同上 | 全項目: R2 `jquants/fins/<日付>.json.gz`。本決算（`CurPerType=FY`）: D1 `financials_annual`。四半期（1Q/2Q/3Q）: D1 `financials_quarterly` |
| 上場銘柄一覧 | JPX `data_j.xlsx` | 日次バッチの最初に全件 | D1 `stocks`（名称・種類・市場・33業種・規模区分） |
| 祝日 | 内閣府 `syukujitsu.csv` | バッチ起動時 | 保存しない（休場日の判定のみ） |
| 書類一覧 | EDINET `GET /api/v2/documents.json?date=YYYY-MM-DD&type=2` | 日付指定で全社分（1日1回）。有報（`docTypeCode=120`）と半期報告書（`160`）で、CSV があり取下げでないものだけ。訂正（130・170）は使わない | 保存しない（取る書類を選ぶだけ） |
| 書類 | EDINET `GET /api/v2/documents/<docID>?type=5` | 銘柄・種類ごとに最新の1通だけ、まだ持っていないものを取る | 文章: R2 `edinet/<コード>/annual.json.gz`・`semiannual.json.gz`（新しい書類で上書き）。手がかり: D1 `edinet_docs`（1銘柄・1種類1行） |

取り込みは `batch-cli sync-jq`。初回だけ過去5年分、以後は前回の続き（`sync_state.jq_synced_until`）。財務は直近3日分を毎回取り直す。

EDINET は `batch-cli sync-edinet`。初回は過去400日分の書類一覧（有報は年1回なので、提出の遅い会社も含めて1年強）、以後は前回の続き（`sync_state.edinet_listed_until`）。API キーは URL に載せずヘッダー（`Ocp-Apim-Subscription-Key`。`Subscription-Key` ヘッダーは 401 になる）で渡す。古い書類の扱い:

- 同じ銘柄・種類の新しい書類が出たら、D1 の行と R2 のファイルを上書きする（古い書類は残らない）
- 同じ事業年度かそれより後の有報が出たら、その半期報告書は消す（初回の取り込みでは最初から取らない）。EDINET の `periodEnd` は半期報告書でも事業年度の末日なので、有報の `periodEnd` ≥ 半期報告書の `periodEnd` で比べる
- 書類一覧には、その日に縦覧の状態などが変わった過去の書類も載るので、その日に提出された書類だけを使う
- 上場していない銘柄（最新の JPX 一覧にない）の書類は消す
- R2 から消すキーはバッチが `raw/edinet-delete.txt` に書き、ワークフローの最後に `wrangler r2 object delete` で消す

## 項目の対応

### 株価（`/equities/bars/daily`）

| 使う項目 | 意味 | 用途 |
|---|---|---|
| `Code` | 5桁コード（末尾0）。4桁に直す（`130A0` → `130A`）。末尾が0でない優先株は除く | 銘柄の特定 |
| `Date` | 日付 | 履歴の日付 |
| `C` | 終値（**分割調整前**） | 株価、利回り、PBR、PER。配当（`DivAnn`）も調整前なので、分割をまたいでも整合する |

`AdjC`（調整済み終値）は使わない。約定がない日（`C` が null）は履歴に入れない。

### 財務（`/fins/summary`）

| 使う項目 | 意味 | D1 の列 |
|---|---|---|
| `Code`, `DiscNo`, `DiscDate` | 銘柄、開示番号、開示日 | `code`, `doc_id`, `disclosed` |
| `CurPerType` | `1Q`/`2Q`/`3Q`/`FY`（4Q・5Q の変則決算は対象外） | `period_type`（四半期のみ） |
| `CurPerEn`, `CurFYEn` | 当期間の終了日、当事業年度の終了日 | `period_end`, `fiscal_year_end` |
| `Sales` / `NCSales` | 売上高（連結 / 非連結。連結がなければ非連結） | `net_sales` |
| `OP` / `NCOP` | 営業利益 | `operating_income` |
| `OdP` / `NCOdP` | 経常利益 | `ordinary_income` |
| `NP` / `NCNP` | 当期純利益 | `net_income` |
| `ShEq` / `Eq` / `NCShEq` / `NCEq` | 自己資本（なければ純資産） | `equity` |
| `EPS` / `NCEPS` | 1株当たり当期純利益 | `eps` |
| `BPS` / `NCBPS` | 1株当たり純資産 | `bps` |
| `DivAnn` / `DivUnit` | 1株当たり年間配当実績 / 1口当たり分配金（REIT） | `dps` |
| `CFO` | 営業活動によるキャッシュ・フロー | `operating_cf` |

四半期の行は期首からの累計値。四半期単独の値は計算時に差分で出す（Q1 = 1Q、Q2 = 2Q − 1Q、Q3 = 3Q − 2Q、Q4 = FY − 3Q）。前の四半期の行がない場合は不明（None）。

ETF は財務情報サマリーに載らないため、価格だけで利回りは出ない。

## 指標の計算式（`domain/src/indicators.rs`, `analysis.rs`）

| 指標 | 計算 | 元データ |
|---|---|---|
| 株価 | 最新の終値 | `C` |
| 配当利回り | その時点で開示済みの最新の年間配当 ÷ 終値 | `DivAnn`（開示日 `DiscDate` から有効）, `C` |
| 利回りの過去5年比（パーセンタイル） | 週1点（5営業日ごと）の過去5年の利回りの中で、今の利回りが下から何%か。同じ値は半分だけ数える。52点未満なら出さない | 上と同じ |
| PBR | 終値 ÷ BPS（最新の本決算） | `C`, `BPS` |
| PER | 終値 ÷ EPS（最新の本決算） | `C`, `EPS` |
| 配当性向 | 1株配当 ÷ EPS | `DivAnn`, `EPS` |
| DOE（簡易） | 1株配当 ÷ BPS | `DivAnn`, `BPS` |
| 営業利益率 | 営業利益 ÷ 売上高（最新の本決算） | `OP`, `Sales` |
| 連続増配年数 | 1株配当が前年を上回った年の連続数（最大5年分） | `DivAnn` × 5期 |
| 5年連続で配当あり | 直近5期すべて 1株配当 > 0。5期なければ不明 | `DivAnn` × 5期 |
| 通期で赤字なし | 直近5期の純利益と営業利益がすべてプラス | `NP`, `OP` × 5期 |
| 営業利益の増加傾向 | 直近5期の営業利益の最小二乗法の傾きがプラス（3期以上） | `OP` × 5期 |
| 四半期で赤字なし | 直近20四半期の四半期単独の純利益・営業利益がすべてプラス。判定できる四半期が4未満なら不明 | 四半期の累計値の差分 |
| バリュー株 | PBR ≤ 1.0 かつ 0 < PER ≤ 15（基準は `analysis.rs` の定数） | PBR, PER |

| 売上の伸び・営業利益の伸び | 直近の期（1Q〜FY の期首からの累計）と、1年前の同じ期の累計の比 − 1（前年同期比）。前年の値がないか 0 以下なら出さない | `Sales`, `OP`（四半期の行と本決算の行） |

比率は小数で保存し（0.035 = 3.5%）、画面で % にする。

### 配当方針の手がかり（EDINET、`domain/src/disclosure.rs`）

有報・半期報告書の CSV（`XBRL_TO_CSV/jpcrp*.csv`、UTF-16LE の TSV）から、要素名が次で始まる `…TextBlock` の文章だけを取り出し、タグを除いて全角英数字を半角にする。

| 項目 | 要素名（`jpcrp_cor:`） |
|---|---|
| 配当政策 | `DividendPolicyTextBlock` |
| 経営方針、経営環境及び対処すべき課題等 | `BusinessPolicyBusinessEnvironmentIssuesToAddressEtcTextBlock` |
| 経営者による財政状態、経営成績及びキャッシュ・フローの状況の分析 | `ManagementAnalysisOfFinancialPosition…TextBlock` |

文（「。」区切り）ごとに次の言葉を探す。判定は言葉の有無だけなので、当てはまった文（最大5つ、各160文字まで）を `edinet_docs.phrases` に残し、画面で読めるようにする。

| D1 の列 | 言葉 |
|---|---|
| `progressive_dividend` | 累進配当、累進的な配当、累進的に |
| `no_dividend_cut` | 減配しない、減配を行わない、減配は行わない、減配を行わず、減配せず、非減配、維持または増配 など |
| `dividend_raise` | 増配 |
| `doe_target` | DOE、株主資本配当率、純資産配当率、自己資本配当率 |
| `payout_target` | 「配当性向」の後ろ25文字以内の「30%」など（範囲なら下限） |

一覧の「累進配当・非減配の方針」の絞り込みとバッジは、有報か半期報告書のどちらかで `progressive_dividend` か `no_dividend_cut` が 1 の銘柄。

## 一覧と詳細の分担

| | 一覧（スクリーニング） | 詳細（銘柄ページ） |
|---|---|---|
| 目的 | 条件で候補を絞る | 1銘柄の5年の推移を見る |
| 出すもの | 株価、利回り、過去5年比、PBR、PER、配当性向、DOE、連続増配、営業利益率、売上・営業利益の伸び（前年同期比）と、バリュー・5年配当・通期黒字・四半期黒字・累進配当/非減配のバッジ（押すと有報などの該当文） | 年次の売上・営業利益・純利益・EPS・BPS・配当・配当性向・営業CF の5年表、四半期単独の損益の表、利回りの5年の推移グラフ |
| 絞り込み | 種類（株式/REIT/ETF）、コード・名称の検索、33業種、市場、規模、各指標の範囲、バッジの有無 | — |

## AIレコメンド（F7）の候補と使うモデル

日次バッチが成功した日の 21:00 JST（Cron `0 12 * * 1-5`）に、API Worker が `recommendations` に1日10行を書く。同じ日（`price_date`）の結果があれば作り直さない。AI は数字の計算には使わず、説明と優先順位付けだけに使う。

| 段階 | 内容 | 場所 |
|---|---|---|
| 候補の絞り込み（ルール） | 種類が株式で、配当利回りが分かっていて、バリュー株（PBR ≤ 1.0 かつ 0 < PER ≤ 15）または「5年連続で配当あり かつ 通期で赤字なし」。満たす条件（バリュー・5年配当・通期黒字・四半期黒字・増益傾向）の多い順、同点なら利回りの高い順に上位50銘柄 | `domain/src/recommend.rs`（`select_candidates`）、SQL は `adapter-cf/src/query.rs` |
| AI への依頼 | 候補50銘柄の指標（コード・名称・業種・株価・利回り・過去5年比・PBR・PER・配当性向・DOE・連続増配・営業利益率・各バッジ）を JSON で渡し、上位10銘柄と各1〜2文の理由を JSON Schema（`response_format`）で返させる | `adapter-cf/src/recommend.rs` |
| モデル | Workers AI `@cf/meta/llama-3.1-8b-instruct-fp8`（無料枠で動く軽いテキスト生成モデル。`@cf/meta/llama-3.1-8b-instruct` は廃止済み） | 同上（`MODEL`） |
| 答えの検証 | ちょうど10件、すべて候補にある銘柄、重複なし、理由が空でなく200文字以内。1つでも外れたら保存せずエラーをログに出す | `domain/src/recommend.rs`（`validate_picks`） |

画面（AIレコメンドのタブ）には「AIによる参考情報であり投資助言ではありません」を明記する。
