import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { App } from "./App";
import { ClerkAuth } from "./lib/clerk-auth";
import "./styles.css";

// OS の設定に合わせてダークモードを切り替える（配色は .dark クラスで変わる）
const dark = window.matchMedia("(prefers-color-scheme: dark)");
const applyTheme = () => document.documentElement.classList.toggle("dark", dark.matches);
applyTheme();
dark.addEventListener("change", applyTheme);

const root = createRoot(document.getElementById("root") as HTMLElement);
const mockClerkUrl = import.meta.env.VITE_MOCK_CLERK_URL;

if (import.meta.env.DEV && mockClerkUrl) {
  // ローカル検証だけ偽の Clerk を使う。DEV の分岐ごと本番のビルドから消える
  void import("./lib/mock-auth").then(({ MockAuth }) =>
    root.render(
      <StrictMode>
        <MockAuth baseUrl={mockClerkUrl}>
          <App />
        </MockAuth>
      </StrictMode>,
    ),
  );
} else {
  const key = import.meta.env.VITE_CLERK_PUBLISHABLE_KEY;
  root.render(
    <StrictMode>
      {key ? (
        <ClerkAuth publishableKey={key}>
          <App />
        </ClerkAuth>
      ) : (
        <p style={{ padding: 16 }}>VITE_CLERK_PUBLISHABLE_KEY が設定されていません。</p>
      )}
    </StrictMode>,
  );
}
