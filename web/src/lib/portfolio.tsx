import { createContext, type ReactNode, useCallback, useContext, useEffect, useState } from "react";
import { apiFetch, apiGet, type Portfolio } from "@/lib/api";
import { useAuthState } from "@/lib/auth";

// 登録した銘柄コードを画面全体で共有する（一覧の☆とポートフォリオのタブで同じものを見る）
type PortfolioState = {
  codes: ReadonlySet<string>;
  toggle: (code: string) => Promise<void>;
  /** 登録・解除のたびに増える。ポートフォリオの一覧を取り直す合図 */
  version: number;
  error: string | null;
};

const PortfolioContext = createContext<PortfolioState | null>(null);

export function PortfolioProvider({ children }: { children: ReactNode }) {
  const { getToken } = useAuthState();
  const [codes, setCodes] = useState<ReadonlySet<string>>(new Set());
  const [version, setVersion] = useState(0);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    apiGet<Portfolio>("/api/portfolio", getToken)
      .then((p) => setCodes(new Set(p.codes)))
      .catch(() => setError("ポートフォリオを読み込めませんでした。"));
  }, [getToken]);

  const toggle = useCallback(
    async (code: string) => {
      const has = codes.has(code);
      // 先に見た目を変え、失敗したら戻す
      const next = new Set(codes);
      if (has) next.delete(code);
      else next.add(code);
      setCodes(next);
      setError(null);
      try {
        await apiFetch(
          has ? "DELETE" : "PUT",
          `/api/portfolio/${encodeURIComponent(code)}`,
          getToken,
        );
        setVersion((v) => v + 1);
      } catch {
        setCodes(codes);
        setError(has ? "登録を外せませんでした。" : "登録できませんでした（最大100銘柄まで）。");
      }
    },
    [codes, getToken],
  );

  return (
    <PortfolioContext.Provider value={{ codes, toggle, version, error }}>
      {children}
    </PortfolioContext.Provider>
  );
}

export function usePortfolio(): PortfolioState {
  const v = useContext(PortfolioContext);
  if (!v) throw new Error("PortfolioProvider がありません");
  return v;
}
