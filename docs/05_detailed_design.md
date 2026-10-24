# 05 認証・キャッシュ・ポートフォリオ

## 認証

1. `/api/*` への要求からBearerトークンを取得する。
2. ClerkのJWKSでRS256署名を検証し、issuer、authorized party、時刻のクレームを確認する。
3. 保存済みのユーザー情報を参照する。通常は1時間ごとにClerkへ再確認する。
4. 主メールアドレスと同じGoogleアカウントが確認済みであることを要求する。
5. 管理者アドレスとの一致、または `public_metadata.stock_insight=true` で利用を許可する。
6. ルートごとの権限・本人データの範囲を確認する。

管理者アドレスは前後の空白と大文字小文字を無視して比較する。管理者でもGoogle確認は必要。未設定なら管理者としての一致は発生しない。

JWKSのキャッシュは1時間。許可判定はD1 `users` とWorker内のメモリキャッシュ（最大256件）を使う。Clerkへの問い合わせに失敗した場合、保存済みの判定を期限の上限なしで再利用する。ユーザーの許可変更は即時反映を保証しない。

ログイン記録はClerkへの再確認時に残し、API要求や画面ログインのたびに必ず追加するものではない。記録保存の失敗だけでは認証を止めない。

実装: [auth.rs](../crates/usecase/src/auth.rs)、[Clerk adapter](../crates/adapter-cf/src)、[権限仕様](user-permissions.md)。

## キャッシュ

| 対象 | ブラウザ | Worker共有キャッシュ |
|---|---|---|
| 銘柄一覧、EDINET、REIT詳細、status | `private, max-age=300` | 有効なデータ版があるとき最大86400秒、ETag対応 |
| 銘柄詳細 | `private, max-age=300` | なし、ETagなし |
| me、ポートフォリオ、レコメンド、テーマ、変更API | `no-store` | なし |
| 静的 `/assets/*` | `public, max-age=31536000, immutable` | 静的配信基盤 |
| その他の静的配信 | `no-cache` | 静的配信基盤 |

認証は共有キャッシュの参照より先に行う。共有キーはURLとアプリ版・データ版を含み、データ版にはバッチとEDINET・REITの同期情報を使う。データ版の確認自体を60秒間メモリに保持する。有効な版を得られない場合は共有キャッシュを使わず取得する。

ETagはアプリ版・データ版から作る弱いETag。一致する `If-None-Match` には304を返す。ブラウザの300秒キャッシュがあるため、更新が60秒以内に必ず画面へ反映されるとは限らない。

実装: [API Worker](../crates/api-worker/src/lib.rs)、[D1 query](../crates/adapter-cf/src/query.rs)。

## ポートフォリオ

登録対象は銘柄コードのみ。ユーザーIDは認証情報から決定し、入力では受け取らない。最大100件。登録済みの再登録、未登録の解除は重複を作らない。指標がない登録コードも `codes` に残るが、指標のある行だけを `rows` に返す。

| 区分 | 条件 | 対象 |
|---|---|---|
| 主 `high_payout` | 実績配当性向 > 0.5 | 株式 |
| 主 `high_pbr` | PBR > 1.5 | 株式 |
| 主 `loss_dividend` | EPS ≤ 0 かつ採用配当 > 0 | 株式 |
| 参考 `expensive` | 利回り評価が割高 | 全種類 |
| 参考 `dividend_cut` | 減配判定が真 | 全種類 |
| 参考 `profit_downtrend` | 営業利益増加傾向が偽 | 全種類 |
| 参考 `loss` | 通期赤字なし、または四半期赤字なしが偽 | 全種類 |

不明な判定は警告に置き換えない。主サイン数の降順、参考サイン数の降順、コード昇順で表示する。閾値の個別設定・売買・通知は行わない。

実装: [portfolio domain](../crates/domain/src/portfolio.rs)、[portfolio usecase](../crates/usecase/src/portfolio.rs)。
