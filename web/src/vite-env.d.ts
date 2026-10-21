/// <reference types="vite/client" />

// ビルド時に Cargo.toml の version から入る（vite.config.ts）
declare const __APP_VERSION__: string;

interface ImportMetaEnv {
  // Clerk の公開キー（pk_...）。ブラウザに渡してよい値だが、リポジトリには書かない
  readonly VITE_CLERK_PUBLISHABLE_KEY?: string;
  // ローカル検証用の偽の Clerk（scripts/mock-clerk.mjs）の URL。開発サーバーでだけ使う
  readonly VITE_MOCK_CLERK_URL?: string;
}

interface ImportMeta {
  readonly env: ImportMetaEnv;
}
