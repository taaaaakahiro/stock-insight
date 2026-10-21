import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import tailwindcss from "@tailwindcss/vite";
import react from "@vitejs/plugin-react";
import { defineConfig, loadEnv } from "vite";

// 版はリポジトリの Cargo.toml（workspace.package.version）を唯一の元にする。
// GitHub のタグ v<version> と一致させる（release.yml が照合する）
const cargo = readFileSync(fileURLToPath(new URL("../Cargo.toml", import.meta.url)), "utf8");
const version = /\[workspace\.package\][^[]*?\nversion = "([^"]+)"/.exec(cargo)?.[1];
if (!version) throw new Error("Cargo.toml に workspace.package.version がありません");

export default defineConfig(({ mode }) => {
  // 本番ビルドで公開キーがないと、Vite が `key ? <App/> : <p/>` を静的に畳んで画面ごと消してしまう。
  // 黙って空の画面を出さないよう、ここで止める（開発サーバーと偽の Clerk は対象外）
  const env = loadEnv(mode, process.cwd(), "VITE_");
  if (mode === "production" && !env.VITE_CLERK_PUBLISHABLE_KEY) {
    throw new Error(
      "VITE_CLERK_PUBLISHABLE_KEY がありません。本番ビルドには Clerk の公開キーが必要です",
    );
  }
  return config;
});

const config = {
  define: { __APP_VERSION__: JSON.stringify(version) },
  plugins: [react(), tailwindcss()],
  resolve: {
    alias: {
      "@": fileURLToPath(new URL("./src", import.meta.url)),
    },
  },
  server: {
    port: 5173,
    // API は wrangler dev（API Worker）へ。本番は同じオリジンから配信する
    proxy: { "/api": "http://localhost:8787" },
  },
};
