# 14 開発・検証

## 実行環境

Rustは [rust-toolchain.toml](../rust-toolchain.toml)、Nodeは [.nvmrc](../.nvmrc)、pnpmは [web/package.json](../web/package.json) の指定に合わせる。Workerのビルドには `worker-build`、実行にはWranglerを使う。

```bash
pnpm --dir web install --frozen-lockfile
```

## ローカル起動

初回の空のローカルD1には `migrations/*.sql` を番号順に適用する。

```bash
for file in migrations/*.sql; do
  npx wrangler@4.151.3 d1 execute maaun-stock-insight --local --file "$file" || break
done
```

`--local` を外さない。これは初期化用であり、既存DBへ繰り返すと追加済み列で止まる。既存DBはスキーマを確認して不足分だけ適用する。`batch-cli migrate` は接続環境変数で指定したD1を変更するため、ローカル初期化と混同しない。

未追跡の `.dev.vars` に開発用設定を用意する。

```dotenv
CLERK_ISSUER=http://localhost:9871
CLERK_API_URL=http://localhost:9871/v1
AUTHORIZED_PARTIES=http://localhost:5173
CLERK_SECRET_KEY=mock
ADMIN_NOTIFY_EMAIL=admin@example.test
```

次を別々のターミナルで起動する。

```bash
node scripts/mock-clerk.mjs
```

```bash
npx wrangler@4.151.3 dev --local
```

```bash
pnpm --dir web dev
```

画面は `http://localhost:5173`、APIプロキシ先は8787。開発モードで公開Clerkキーがない場合、モックClerk（既定9871）へ自動接続し、一般ユーザーとして入る。公開キーがある場合は実Clerkを使うが、`VITE_MOCK_CLERK_URL` を明示するとモックを選べる。本番ビルドでモック認証は使わない。

| URLの `mock_user` | 用途 |
|---|---|
| `user_member` | 許可済み一般ユーザー（既定） |
| `user_admin` | 管理者 |
| `user_outsider` | 利用未許可 |
| `user_password` | Google連携なし |

例: `http://localhost:5173/?mock_user=user_admin`。選択はsessionStorageに保持され、URLの指定が優先される。モックからのログアウト後も、再読み込みすると既定の一般ユーザーへ自動ログインする。モックは署名付きJWTを発行し、APIの認証・認可処理を通す。

空DBでは銘柄データは表示されない。ローカルD1を使っても外部AIまでオフラインになるわけではない。

## 検証

[ci.yml](../.github/workflows/ci.yml)と同じ確認:

```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
python3 scripts/test_recommendation_tracking.py
python3 scripts/test_reit_rag.py
cargo clippy -p api-worker -p adapter-cf --target wasm32-unknown-unknown -- -D warnings
pnpm --dir web lint
pnpm --dir web typecheck
pnpm --dir web test
```

本番ビルドは `VITE_CLERK_PUBLISHABLE_KEY` が必要。CIではActions Variableから渡す。CIはPRとmainへのpushで動き、gitleaksで秘密情報も検査する。本番へ自動デプロイしない。

権限変更時は一般・管理者・未許可・未認証を確認し、一般からの直接API呼び出しで管理操作が拒否されることも検証する。スキーマ変更時は全マイグレーションを空DBへ適用できること、既存データとの互換性、CLIのマイグレーション一覧を確認する。

REITの結合テストは合成iXBRLと固定応答で数値・引用の検証を確認する。実モデルの判断品質・取得率・費用や本番バッチの成功を示すものではない。
