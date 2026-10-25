# 13 バッチ運用

設定元: [daily-batch.yml](../.github/workflows/daily-batch.yml)、実装: [batch-cli](../crates/batch-cli/src/main.rs)。

## 起動とジョブ

通常はWorker Cronの平日18:30 JST、予備はGitHub scheduleの18:45 JST。同一ワークフローはconcurrencyで直列化し、実行中の処理を取り消さない。

`should-run` はJSTの日付、土日・祝日・休場日、当日の `daily` 成功記録を確認する。祝日データを取得・解釈できなければ失敗する。`force=true` は休場日・実行済み判定を省略する。

| ジョブ | 内容 |
|---|---|
| build | CLIをビルドし、後続へ渡す |
| probe（指定時） | J-Quantsの直近日付とEDINET少数書類を取得してSQL成果物へ出力。D1へ保存しない |
| prepare | 実行判定、マイグレーション、JPX銘柄マスタ同期 |
| jquants | 株価・分割・財務・予想の同期、`compute --label daily`、`evaluate-recommendations`、原本をR2へ保存 |
| edinet | 配当方針・事業内容を同期、原本をR2へ保存し不要キーを削除 |
| reit | 有報から借入・物件を同期。D1へ保存 |

prepare後の3取得ジョブは並列。jquants内のcomputeが書く `batch_runs` の成功は、EDINET・REIT・R2アップロードまでの成功を示すものではない。

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

コマンド: `migrate`, `sync-master`, `sync-jq`, `sync-edinet`, `sync-reit`, `compute`, `should-run`, `reit-probe`, `reit-dump`。

オプション: `--dry-run <file>`, `--from-file <file>`, `--label <英数字>`, `--force`, `--from <YYYY-MM-DD>`, `--until <YYYY-MM-DD>`, `--rate <回/分>`, `--max-minutes`, `--persist-every`, `--raw-dir`, `--limit`, `--out`, `--refill-prices`, `--refill-forecasts`, `--refetch-doe`, `--refetch-business`。各コマンドで使用するオプションはmain.rsの該当処理を参照する。すべてのオプションがすべてのコマンドに作用するわけではない。

停止する場合はWorker CronとGitHub側の予備scheduleの両方を対象にする。アプリの管理者権限だけではバッチ・基盤を操作できない。

推薦追跡はcompute成功後に実行する。株価・財務の変更を追記し、既存の公開推薦の成績と比較レポートを更新する。価格基準日・欠損・再実行の扱いは[成績仕様](16_recommendation_evaluation.md)を参照。
