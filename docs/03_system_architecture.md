# 03 システム構成図

> 出典: Claude Docs「増配期待株ダッシュボード 要件定義」（rev 56, 2026-10-21）

## 方針

バッチをCloudflareだけで作ることもできるが、無料プランでは1回の実行がCPU 10msまでで、有報の解析が収まらない恐れがある。そこで「取得と整形はGitHub Actions上のRust CLI、配信・判定・AI・通知はCloudflare」に分ける構成を推奨する。

[embed: 全体構成 · 日次バッチ → D1/R2 → 判定Workflow → メール]

日次バッチが取得と指標計算を終えてD1とR2に書き込み、完了後に判定Workflowが警告・売り時・AIレコメンドを作ってメールを送る。画面側のAPI WorkerはD1を読むだけ。

## 全体構成

取得はGitHub Actions、配信と判定はCloudflareで行う。

```mermaid
flowchart LR
  subgraph SRC[データ取得元]
    JPX[JPX 上場銘柄一覧<br/>規模区分]
    YF[Yahoo Finance<br/>株価・配当]
    ED[EDINET API<br/>財務・配当方針]
  end

  subgraph GH[GitHub Actions]
    BATCH[日次バッチ（平日18:30 JST）<br/>Rust CLI / 4並列で取得・指標計算]
  end

  subgraph CF[Cloudflare（無料枠）]
    CRON[Cron Trigger<br/>平日18:30 JST]
    API[API Worker（Rust）<br/>認証・一覧・詳細・保有株・条件<br/>画面も同じWorkerから配信]
    D1[(D1（SQLite）<br/>最新の指標・ユーザーごとのデータ)]
    R2[(R2<br/>株価履歴・有報の生データ)]
    WF[判定Workflow（バッチ後）<br/>警告・売り時の判定<br/>AIレコメンド・メール作成]
    AI[Workers AI（Llama 3.1 8B）<br/>推薦理由・配当方針の抽出]
    MAIL[Email Service]
  end

  CLERK[Clerk<br/>Google SSO]
  BR[ブラウザ<br/>React + shadcn/ui<br/>一覧・詳細・設定]
  ADMIN[管理者のメール<br/>警告・売り時・バッチ結果]

  CRON -- workflow_dispatch --> BATCH
  SRC -- 取得 --> BATCH
  BATCH -- 書込（D1 HTTP API） --> D1
  BATCH -- 履歴（S3互換 API） --> R2
  BATCH -- 完了通知（HMAC署名）で起動 --> WF
  API <--> D1
  WF <--> D1
  WF <--> AI
  WF --> MAIL --> ADMIN
  BR <--> API
  BR <-- セッション --> CLERK
  CLERK -- Webhook（session.created） --> API
```

日次バッチが取得と指標計算を終えてD1とR2に書き込み、完了後に判定Workflowが警告・売り時・AIレコメンドを作ってメールを送る。画面側のAPI WorkerはD1を読むだけ。

## コンポーネントの役割

| コンポーネント | 実行基盤 | 役割 | 対応する機能 |
|---|---|---|---|
| ブラウザ（SPA） | API Workerから配信 | 一覧・詳細・保有株・売り時条件・AIのおすすめ・通知履歴・ログイン記録の表示 | F1〜F8 |
| API Worker | Cloudflare Workers（Rust / workers-rs） | セッショントークンの検証、許可リストの確認、D1の読み書き、Clerk Webhookの受信、内部エンドポイント | F1〜F6, F8 |
| Cron Trigger | Cloudflare Workers | 平日18:30 JSTにGitHubの`workflow_dispatch`を呼ぶ | 日次バッチの起動 |
| 日次バッチ（batch-cli） | GitHub Actions（Rust / tokio） | JPX・Yahoo・EDINETから取得、XBRL解析、指標計算、D1・R2への書き込み | データ要件 |
| 判定Workflow | Cloudflare Workflows | 警告・売り時の判定、AIレコメンド、通知の作成、管理者へのメール | F4〜F7 |
| D1 | Cloudflare D1 | 最新の指標、年次財務、ユーザーごとのデータ | 全機能 |
| R2 | Cloudflare R2 | 株価・配当の履歴ファイル、EDINETの生データ | F3、パーセンタイル計算 |
| Workers AI | Cloudflare Workers AI | おすすめ銘柄の理由づけ、配当方針の抽出 | F3, F4, F7 |
| Clerk | 外部SaaS（PropFolioと共用） | Google SSO、セッション、ログインのWebhook | F1 |
| Email Service | Cloudflare Email Service | 検証済みの管理者アドレスへのメール送信 | F6 |

## バッチをどこで動かすか

| 案 | 構成 | 費用 | 判断 |
|---|---|---|---|
| A ハイブリッド（推奨） | GitHub ActionsのCron（平日18:30 JST）でRust CLIを実行。D1はHTTP API、R2はS3互換 APIで書き込み、最後にWorkerの内部エンドポイントを叩いて判定Workflowを起動 | 0円。公開リポジトリはGitHub標準のランナーが無料（要確認） | CPU制限がなく、バッチも同じRustのドメインを使える |
| B Cloudflareだけ | Cron Trigger → Workflows。1銘柄1ステップ、約4インスタンスに分割 | 0円 | 株価取得は可能。有報の解析が1ステップ10msに収まるかは技術検証が必要 |
| C Workers有料プラン | Bと同じ。CPU上限が大幅に増え、メールも任意の宛先に送れる | 月$5 | 無料でなくてよいなら一番すっきりする |

## 日次の処理タイムライン

| 時刻（JST, 平日） | 処理 | 実行基盤 |
|---|---|---|
| 18:30 | Cron Triggerが`workflow_dispatch`を呼び、日次バッチを起動 | Cloudflare → GitHub Actions |
| 18:30〜19:30 | 4ジョブで取得・指標計算、D1・R2へ書き込み、集約ジョブでパーセンタイル確定 | GitHub Actions |
| 19:30〜20:30 | 判定Workflow: 警告・売り時の判定、AIレコメンド、画面通知、管理者メール | Cloudflare Workflows |

休業日（土日祝・年末年始）はバッチを実行しない。

## インフラ管理とデプロイ

Cloudflareのダッシュボードは使わず、すべてコードとコマンドで管理する。インフラは既存の[maaun-terraform](https://github.com/maa-un/maaun-terraform)（Cloudflareの`envs/global/cloudflare/account`レイヤ。Zone・DNS・D1・R2・Workersのドメイン・Cronをすでに管理している）に追加し、アプリのデプロイとシークレットの登録はコマンド（wrangler・gh）で行う。

| 対象 | 管理方法 | 置き場所 |
|---|---|---|
| D1データベース、R2バケット（ライフサイクルルールを含む） | Terraform（`cloudflare_d1_database`、`cloudflare_r2_bucket`） | maaun-terraform |
| カスタムドメイン（stock.insight.maa-un.com）、DNS | Terraform（`cloudflare_workers_custom_domain`、`cloudflare_dns_record`） | maaun-terraform |
| Cron Trigger（平日18:30 JST） | Terraform（`cloudflare_workers_cron_trigger`） | maaun-terraform |
| Cloudflare APIトークン（CI用・バッチ用） | Terraform（`cloudflare_api_token`）。権限は用途ごとに最小にする | maaun-terraform |
| GitHubリポジトリの設定（ブランチ保護、Secret scanning、Push protection） | Terraform（GitHubプロバイダ） | maaun-terraform |
| Workerのコードと紐付け（D1、R2、Workers AI、Workflows） | `wrangler.jsonc`に定義し、`wrangler deploy`で反映 | stock-insight |
| デプロイ | mainへのマージでGitHub Actionsが`wrangler deploy`を実行。プルリクエストでは`wrangler versions upload`でプレビューURLを発行 | stock-insight |
| D1のテーブル定義の変更 | `migrations/`にSQLを置き、GitHub Actionsから`batch-cli migrate`（D1 HTTP API）で適用（技術検証で確認済み） | stock-insight |
| Worker Secret | `wrangler secret put`（値は標準入力から渡す） | コマンド |
| GitHub Actions Secrets | `gh secret set`（値は標準入力から渡す） | コマンド |

- リソースの作成・変更はmaaun-terraformのプルリクエストで`plan`を確認してから`apply`する（本番への変更のため、適用前に人が承認する）
- WorkerのスクリプトはTerraformでは管理しない。Rustのビルド成果物（Wasm）を扱えるwranglerでデプロイする。Terraformが参照するのはスクリプト名だけ
- シークレットの値はTerraformのstateに残さないため、Terraformでは扱わない（[06](06_secrets_and_repository.md)）
- 切り戻しは`wrangler rollback`で直前のバージョンに戻す

## ネットワークとドメイン

| 項目 | 内容 |
|---|---|
| ドメイン | stock.insight.maa-un.com（Workerのカスタムドメイン） |
| 画面とAPI | 同じオリジンから配信（Cookieの扱いを単純にする） |
| 認証 | Clerk（PropFolioと同じルートドメインのサブドメイン間でセッションを共有） |
| 通信 | HTTPSのみ、HSTS有効 |
