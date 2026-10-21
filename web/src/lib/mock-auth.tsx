import { type ReactNode, useState } from "react";
import { AuthProvider } from "./auth";

// ローカル検証専用。scripts/mock-clerk.mjs が発行するトークンを使う。
// 開発サーバー（import.meta.env.DEV）でだけ読み込まれ、本番のビルドには入らない。
const KEY = "mock-clerk-user";

export function MockAuth({ baseUrl, children }: { baseUrl: string; children: ReactNode }) {
  const [user, setUser] = useState<string | null>(() => sessionStorage.getItem(KEY));
  return (
    <AuthProvider
      value={{
        status: user ? "signed-in" : "signed-out",
        getToken: async () => {
          if (!user) return null;
          const res = await fetch(`${baseUrl}/token/${user}`);
          return res.ok ? res.text() : null;
        },
        signInWithGoogle: async () => {
          const id = new URLSearchParams(window.location.search).get("mock_user") ?? "user_member";
          sessionStorage.setItem(KEY, id);
          setUser(id);
        },
        signOut: async () => {
          sessionStorage.removeItem(KEY);
          setUser(null);
        },
      }}
    >
      {children}
    </AuthProvider>
  );
}
