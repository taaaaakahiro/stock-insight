# 07 デプロイ手順

対象はWorker `maaun-stock-insight` と同時配信する画面。コマンドはリポジトリルートで実行する。以下ではリポジトリの運用で使用する `npx wrangler@4.151.3` を使う。

## 公開前

1. 公開するコミットを確定し、CI成功を確認する。mainの対象コミットからクリーンな作業ツリーを用意する。
2. Worker・画面・DB・シークレット・Cronの変更範囲を確認する。
3. `npx wrangler@4.151.3 whoami` で接続先、`npx wrangler@4.151.3 deployments list` で現在のWorkerバージョンを確認し、切り戻し先を控える。
4. DB変更がある場合、適用済みの列・索引を確認し、D1を書き出すかTime Travelの復旧位置を記録する。復旧手段を確認してから適用する。

```bash
mkdir -p backups
npx wrangler@4.151.3 d1 export maaun-stock-insight --remote --output backups/before-deploy.sql
# Time Travelの復旧位置の確認
npx wrangler@4.151.3 d1 time-travel info maaun-stock-insight
```

バックアップはGitへ入れない。既存ファイルを上書きしない保存名を選ぶ。マイグレーションは[データモデル](11_data_model.md)を参照し、新コードが必要とするものをWorker公開より先に適用する。`ADD COLUMN` を含むSQLを未確認で再実行しない。

## ビルドとプレビュー

```bash
pnpm --dir web install --frozen-lockfile
VITE_CLERK_PUBLISHABLE_KEY="$(gh variable get CLERK_PUBLISHABLE_KEY --repo taaaaakahiro/stock-insight)" pnpm --dir web build
npx wrangler@4.151.3 versions upload
```

アップロードしたバージョンIDとプレビューURLを控える。トップ・深いURLの静的表示、アセット、ヘッダー、認証なしAPIの401を確認する。プレビューは本番と独立したDBではなく、設定されたbindingを使う。本番Clerkのドメイン制約があるため、プレビューだけでログイン完了を検証済みとしない。

## 本番反映と確認

必要な公開承認が得られていることを確認して、検証したバージョンを100%へ切り替える。既に依頼で公開が承認されている場合は、その範囲で実行する。

```bash
npx wrangler@4.151.3 versions deploy <VERSION_ID>@100%
```

routes・Cronなどの設定も変更する場合は、その設定の反映が必要か確認する。通常の設定込みデプロイはビルド済みの同じ作業ツリーから `npx wrangler@4.151.3 deploy` で行う。

- 公開URLの画面、深いURL、アセットを確認する。
- 未認証APIが401、許可済み一般ユーザーの閲覧が成功、管理APIが403になることを確認する。
- 管理者のテーマ操作・AI再生成は必要な範囲で確認する。共通データを書き換える操作であることを考慮する。
- `/api/status` とフッターの版、Workerログを確認する。
- 公開コミット、バージョンID、検証結果を報告する。

## ログインの監視

[production-login-monitor](https://github.com/taaaaakahiro/stock-insight/actions/workflows/login-monitor.yml) は、main に反映されると15分おきに本番を確認する。Actions から手動実行もできる。新しいブラウザで「Google でログイン」を押して Google のアカウント入力画面が出ること、`/sso-callback` が画面を配信すること、未認証の `/api/me` が想定どおり401を返すこと、Clerk の署名検証用公開鍵が取得できることを確認する。失敗した項目は1回再試行し、再度失敗した場合は項目名を Actions に記録する。失敗通知は [GitHub Actions の通知設定](https://docs.github.com/en/actions/concepts/workflows-and-actions/notifications-for-workflow-runs)で有効にする。

Google アカウントは入力しない。Google から戻った後のセッション確立とログイン済みの `/api/me` は、公開時に許可済みの一般ユーザーで確認する。利用者から報告があった場合は、Cloudflare の Workers Logs で `/api/me` の応答と `auth unavailable` または `unauthenticated` のログを時刻で照合する。

同じワークフローの `d1-capacity` は、既存の Cloudflare 用シークレットで本番 D1 の容量を読み取り、450 MB 以上で失敗する。これは [Workers Free の1データベース上限 500 MB](https://developers.cloudflare.com/d1/platform/limits/) に達する前に気付くためのしきい値。プランが変わったらリポジトリ変数 `D1_STORAGE_ALERT_BYTES` で見直す。超過中は D1 への保存が失敗し、ポートフォリオ更新やバッチにも影響する。Workers Logs に `auth_cache_write_failed reason=d1_capacity` が出ている場合も、D1 の使用量と契約を確認する。

## 切り戻し

Workerの不具合は、記録した既知の正常なバージョンへ戻す。DB・シークレット・外部設定はWorkerの切り戻しだけでは復元されない。追加列を旧コードが許容するかを確認し、DB復元が必要な場合は復元範囲と新規データへの影響を確認してからバックアップまたはTime Travelを使う。

公開停止やDB復元は通常のWorker切り戻しとは別操作として扱う。

## バージョンとリリース

`Cargo.toml` の `workspace.package.version` がAPIと画面の版の元。版を変更する場合は `web/package.json` も揃える。`v<MAJOR.MINOR.PATCH>` タグのpushで [release.yml](../.github/workflows/release.yml) が版の一致を検証しGitHub Releaseを作る。本番デプロイは自動実行されない。

日次バッチの手動実行・再取得は[バッチ運用](13_batch_operations.md)を参照。
