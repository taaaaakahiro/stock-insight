import { createContext, type ReactNode, useContext } from "react";

// 画面が使うログインの形。本番は Clerk、ローカル検証だけ偽の Clerk（mock-auth.tsx）が中身を入れる。
export type AuthState = {
  status: "loading" | "signed-in" | "signed-out";
  /** API に渡すセッショントークン。未ログインなら null */
  getToken: () => Promise<string | null>;
  signInWithGoogle: () => Promise<void>;
  signOut: () => Promise<void>;
};

const AuthContext = createContext<AuthState | null>(null);

export function AuthProvider({ value, children }: { value: AuthState; children: ReactNode }) {
  return <AuthContext.Provider value={value}>{children}</AuthContext.Provider>;
}

export function useAuthState(): AuthState {
  const v = useContext(AuthContext);
  if (!v) throw new Error("AuthProvider がありません");
  return v;
}
