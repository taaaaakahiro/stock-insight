# 13 バッチ運用

設定元: [daily-batch.yml](../.github/workflows/daily-batch.yml)・[reit-batch.yml](../.github/workflows/reit-batch.yml)、実装: [batch-cli](../crates/batch-cli/src/main.rs)。

## 起動とジョブ

日次workflowはWorker Cronの平日18:30 JST、予備はGitHub scheduleの18:45 JST。同一ワークフローはconcurrencyで直列化し、実行中の処理を取り消さない。

`should-run` はJSTの日付、土日・祝日・休場日、当日の `daily` 成功記録を確認する。祝日データを取得・解釈できなければ失敗する。`force=true` は休場日・実行済み判定を省略する。

| ジョブ | 内容 |
|---|---|
| build | CLIをビルドし、後続へ渡す |
| probe（指定時） | J-Quantsの直近日付とEDINET少数書類を取得してSQL成果物へ出力。D1へ保存しない |
| prepare | 実行判定、マイグレーション、JPX銘柄マスタ同期 |
| jquants | 株価・分割・財務・予想の同期、`compute --label daily`、`evaluate-recommendations`、原本をR2へ保存 |
| edinet | 配当方針・事業内容を同期、原本をR2へ保存し不要キーを削除 |

prepare後のJ-Quants・EDINET取得は並列。computeが書く `batch_runs` の成功は、EDINET取得・R2アップロードや別workflowのREIT処理の成功を示すものではない。

## 手動実行

```bash
gh workflow run daily-batch --repo taaaaakahiro/stock-insight -f probe=true
```

保存を伴う通常実行:

```bash
gh workflow run daily-batch --repo taaaaakahiro/stock-insight -f force=true
```

| workflow入力 | 既定値・意味 |
|---|---|
| `force` | false。休場日・実行済みでも実行 |
| `probe` | false。取得確認とSQL成果物のみ |
| `max_minutes` | 300。取得時間の制限 |
| `persist_every` | 1500。J-Quantsの途中保存間隔 |
| `refill_prices` | false。過去5年の株価と分割を再取得 |
| `refill_forecasts` | false。配当予想の履歴を再取得（CLI既定は直近366日） |
| `refetch_doe` | false。方針数値の追加取得が必要なEDINET書類を再取得 |
| `refetch_business` | false。事業内容のない有報を再取得 |

銘柄コードを限定するworkflow入力はない。再取得は通常のジョブも動かす。`persist_every` を小さくすると履歴の書き直しが増える。

## 続きの位置と障害対応

| 系統 | 続きの位置・初回 |
|---|---|
| J-Quants | `sync_state.jq_synced_until`。初回は5×365日前から。財務は直近3日を再取得 |
| EDINET | `sync_state.edinet_listed_until`。初回400日。日付カーソルは前日までとし、当日を再確認 |
| REIT | `sync_state.reit_listed_until`。初回400日。古い書類から処理し、最新期を古い期で上書きしない |

時間制限時は保存済みの進捗から次回継続する。ただし株価の強制再取得は通常カーソルとは別で、再実行すると再取得範囲の先頭からとなる。途中までの履歴でもcomputeが成功する場合があり、成功記録だけで初回バックフィル完了と判断しない。

失敗時はActionsの対象ジョブ・カーソル・最新データ日付を確認する。D1接続は一時的な通信障害・429・5xxを再試行し、通常の4xxは再試行しない。R2への保存失敗はD1の保存を自動的に巻き戻さない。

EDINETの原本は種類ごとに上書きし、削除対象キーをmanifestへ出す。再取得後にテーマ対応まで更新する場合は[テーマ仕様](12_recommendations_and_themes.md)に従って再照合する。

## CLI

コマンド: `migrate`, `sync-master`, `sync-jq`, `sync-edinet`, `sync-reit`, `compute`, `should-run`, `reit-probe`, `reit-dump`, `evaluate-recommendations`, `reit-rag`。

オプション: `--dry-run <file>`, `--from-file <file>`, `--label <英数字>`, `--force`, `--from <YYYY-MM-DD>`, `--until <YYYY-MM-DD>`, `--rate <回/分>`, `--max-minutes`, `--persist-every`, `--raw-dir`, `--limit`, `--out`, `--refill-prices`, `--refill-forecasts`, `--refetch-doe`, `--refetch-business`。各コマンドで使用するオプションはmain.rsの該当処理を参照する。すべてのオプションがすべてのコマンドに作用するわけではない。

停止する場合はWorker CronとGitHub側の予備scheduleの両方を対象にする。アプリの管理者権限だけではバッチ・基盤を操作できない。

推薦追跡はcompute成功後に実行する。株価・財務の変更を追記し、既存の公開推薦の成績と比較レポートを更新する。価格基準日・欠損・再実行の扱いは[成績仕様](16_recommendation_evaluation.md)を参照。

## REIT専用workflow

REIT取得・分析・診断は `reit-batch` で実行する。`daily-batch` にREITジョブは含めない。専用の実行履歴・同時実行制御を持ち、REIT側の失敗で日次workflowの結果は変わらない。D1・R2の保存先は共通。

通常はmainの日次workflow成功後に別実行として起動する。その実行の `jquants` が成功していることを確認し、休場日・当日処理済み・probeによるスキップ時は起動しない。REIT専用workflow内で `sync-reit` を実行してから分析へ進む。AIが無効でもREIT取得は行う。

手動ではActionsの `reit-batch` → Run workflow で `mode` を選ぶ。

| mode | 対象 |
|---|---|
| `full` | REIT取得と分析（既定） |
| `analyze` | 保存済みの株価・REIT情報から分析のみ |
| `resume` | `rag_run_id` に指定した固定入力からAI評価・公開を再開 |
| `diagnose` | トークン状態とD1への読み取り診断。DB更新・AI実行なし |
| `ai-diagnose` | D1へのSELECTと共通モデルへの最小推論でAI認証を確認 |

どのmodeもJ-Quants、compute、銘柄マスタ更新、マイグレーションは実行しない。分析には `REIT_RAG_ENABLED=true` が必要。`resume` は保存済み入力のハッシュ・モデル・バージョンを検証して再利用する。

```bash
# REIT分析だけを実行
gh workflow run reit-batch.yml --ref main -f mode=analyze
# AI評価から再開
gh workflow run reit-batch.yml --ref main -f mode=resume -f rag_run_id="入力確定済みのID"
```

AIの401・403は、対象アカウントのWorkers AI Read/Edit権限を確認する。日次用トークンを変更せず対応する場合は、AI専用の `CLOUDFLARE_AI_API_TOKEN` をActions Secretに設定する。

AI分析は最初にD1とWorkers AIの接続を検証し、失敗した場合は資料の保存・検索へ進まない。接続診断のログはHTTP状態、エラーコード、リクエスト識別子、REIT保存件数のみ。古い実行の再試行は元のコード・入力を使うため、コード変更後は新しく実行する。

## REIT分析の実行

REIT専用workflowは次の順で分析する。各処理の上限は100分、ジョブ全体は105分。資料準備とAI評価は4分割し、同時実行は各2ジョブまで。

| 段階 | コマンドの指定 | 内容 |
|---|---|---|
| 訂正監査 | `--stage audit` | 訂正・取下げを監査。完了日までのカーソルから再開 |
| 資料準備 | `--stage archive --shard 0 --shards 4` | 銘柄順に4分割して資料を保存。shardは0〜3 |
| 入力確定 | `--stage prepare` | 訂正監査・資料検索・価格計算を行い、入力をR2に固定。`run_id`を出力 |
| AI評価 | `--stage assess --run-id <ID> --shard 0 --shards 4` | 固定した入力から担当銘柄を評価。shardは0〜3 |
| 公開 | `--stage publish --run-id <ID>` | 全銘柄の完了を検証し、セクター解説を生成。全件を再検証して公開 |

各コマンドは `batch-cli reit-rag` に上記指定と `--max-minutes 100` を付ける。段階を省略すると全工程を単一プロセスで実行する。CLIはdry-run、from-file、from/untilの日付指定を受け付けない。

[設定一覧](06_secrets_and_repository.md)のD1・Cloudflare・EDINET接続情報と保存先バケットが必要。意味検索は任意で、Vectorize indexと埋め込みモデルを両方設定する。埋め込みモデルを変更する場合は別indexを作る。生成モデルはレコメンドと共通で追加設定不要。

失敗時は公開済みの結果を維持する。資料準備は保存済み文書を再利用する。AI評価・公開は同じ `run_id` を指定すると完了済みの銘柄・セクターを再利用する。入力確定からやり直す場合は新しいrunとなる（公開中の入力と同一なら既存runを再利用）。Vectorizeの索引が検索可能になる前は公開を止めるため、反映後に再実行する。失敗した段階・分割だけを再実行できる。入力確定の途中からの再開と費用上限の自動制御はない。

`REIT_RAG_ENABLED` の変更は次のworkflowから適用される。停止しても既存の公開結果は残る。稼働状態はActionsの `reit-rag-audit`・`reit-rag-archive`・`reit-rag-prepare`・`reit-rag-assess`・`reit-rag` ジョブと `sync_state.reit_rag_attempt_status`、公開結果は `reit_rag_publication` で確認する。初回バッチの完了とデプロイ完了は別。

進捗ログには段階、銘柄コード、分割番号、経過秒を記録する。各ジョブは110分未満で停止する設定だが、全工程の所要時間や時間内の完走は保証しない。
