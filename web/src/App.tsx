import { LogOut } from "lucide-react";
import { useEffect, useState } from "react";
import { Button } from "@/components/ui/button";
import { Tabs, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { ApiError, apiGet, type Me, type Status } from "@/lib/api";
import { useAuthState } from "@/lib/auth";
import { jstDateTime, ymd } from "@/lib/format";
import { PortfolioProvider, usePortfolio } from "@/lib/portfolio";
import { stockCodeFromPath, usePath } from "@/lib/router";
import { cn } from "@/lib/utils";
import { PortfolioPage } from "@/pages/portfolio-page";
import { RecommendPage } from "@/pages/recommend-page";
import { ScreeningPage } from "@/pages/screening-page";
import { StockDetailPage } from "@/pages/stock-detail-page";
import { ThemesPage } from "@/pages/themes-page";

type View = "screening" | "portfolio" | "recommend" | "themes";

// 画面の切り替えは URL の # に残す（? はスクリーニングの条件が使う）
const viewFromHash = (): View => {
  switch (window.location.hash) {
    case "#portfolio":
      return "portfolio";
    case "#recommend":
      return "recommend";
    case "#themes":
      return "themes";
    default:
      return "screening";
  }
};

type Session =
  | { state: "checking" }
  | { state: "ok"; me: Me }
  | { state: "forbidden"; message: string }
  | { state: "error" };

export function App() {
  const auth = useAuthState();
  if (auth.status === "loading") return <Centered>読み込み中…</Centered>;
  if (auth.status === "signed-out") return <LoginPage />;
  return <SignedIn />;
}

function SignedIn() {
  const { getToken, signOut } = useAuthState();
  const [session, setSession] = useState<Session>({ state: "checking" });

  useEffect(() => {
    apiGet<Me>("/api/me", getToken)
      .then((me) => setSession({ state: "ok", me }))
      .catch((e: unknown) => {
        if (e instanceof ApiError && e.status === 403) {
          // 拒否の理由（Google 以外・許可リスト外）はサーバーが一時的にしか持たないため、まとめて伝える
          setSession({
            state: "forbidden",
            message:
              "このアカウントは利用できません。Google アカウントでログインしているか確認し、管理者に利用の許可を依頼してください。",
          });
        } else {
          setSession({ state: "error" });
        }
      });
  }, [getToken]);

  if (session.state === "checking") return <Centered>確認中…</Centered>;
  if (session.state !== "ok") {
    return (
      <Centered>
        <p className="mb-4">
          {session.state === "forbidden"
            ? session.message
            : "サーバーに接続できませんでした。時間をおいて再度お試しください。"}
        </p>
        <Button variant="outline" onClick={() => void signOut()}>
          ログアウト
        </Button>
      </Centered>
    );
  }

  return (
    <PortfolioProvider>
      <Shell me={session.me} onSignOut={() => void signOut()} />
    </PortfolioProvider>
  );
}

function Shell({ me, onSignOut }: { me: Me; onSignOut: () => void }) {
  const [view, setView] = useState<View>(viewFromHash);
  const { codes } = usePortfolio();
  // `/stocks/<コード>` は銘柄の詳細。それ以外はタブ（スクリーニング / ポートフォリオ）
  const detailCode = stockCodeFromPath(usePath());

  useEffect(() => {
    const onHash = () => setView(viewFromHash());
    window.addEventListener("hashchange", onHash);
    return () => window.removeEventListener("hashchange", onHash);
  }, []);

  const go = (v: View) => {
    setView(v);
    const url = `${window.location.pathname}${window.location.search}${v === "screening" ? "" : `#${v}`}`;
    window.history.replaceState(null, "", url);
  };

  return (
    <div className="min-h-svh">
      {/* スマホでは画面が狭いので、ヘッダーは固定しない */}
      <header className="z-10 border-b bg-background/90 backdrop-blur md:sticky md:top-0">
        <div className="mx-auto flex max-w-7xl flex-wrap items-center gap-x-6 gap-y-1 px-4 py-2 md:py-3">
          <div className="flex items-baseline gap-3">
            <h1 className="text-base font-semibold tracking-wide">Maa'un 配当分析</h1>
            <span className="hidden text-xs text-muted-foreground sm:inline">
              日本株の配当と企業価値を見極める
            </span>
          </div>
          <div className="order-last basis-full md:order-none md:basis-auto">
            <Freshness />
          </div>
          <div className="ml-auto flex items-center gap-2 text-sm">
            <span className="hidden text-muted-foreground sm:inline">{me.display_name}</span>
            <span className="text-xs text-muted-foreground">
              {me.role === "superuser" ? "スーパーユーザー" : "一般"}
            </span>
            <Button variant="ghost" size="sm" onClick={onSignOut} aria-label="ログアウト">
              <LogOut aria-hidden />
              <span className="hidden sm:inline">ログアウト</span>
            </Button>
          </div>
        </div>
      </header>
      <main className="mx-auto flex max-w-7xl flex-col gap-4 px-4 py-4 md:gap-6 md:py-6">
        {detailCode ? (
          <StockDetailPage key={detailCode} code={detailCode} />
        ) : (
          <>
            <Tabs value={view} onValueChange={(v) => go(v as View)}>
              <TabsList
                aria-label="画面"
                className="max-w-full justify-start overflow-x-auto [scrollbar-width:none]"
              >
                <TabsTrigger value="screening">スクリーニング</TabsTrigger>
                <TabsTrigger value="portfolio">
                  ポートフォリオ
                  {codes.size > 0 && <span className="ml-1 tabular-nums">({codes.size})</span>}
                </TabsTrigger>
                <TabsTrigger value="recommend">AIレコメンド</TabsTrigger>
                {me.permissions.manage_themes && (
                  <TabsTrigger value="themes">注目テーマ</TabsTrigger>
                )}
              </TabsList>
            </Tabs>
            {view === "screening" && <ScreeningPage />}
            {view === "portfolio" && <PortfolioPage />}
            {view === "recommend" && (
              <RecommendPage canRegenerate={me.permissions.regenerate_recommendations} />
            )}
            {view === "themes" && me.permissions.manage_themes && (
              <ThemesPage canManage={me.permissions.manage_themes} />
            )}
          </>
        )}
      </main>
      <Footer />
    </div>
  );
}

// F8: 取得日時を常に見せる。直近のバッチが失敗、または2日以上更新がなければ警告色
function Freshness() {
  const { getToken } = useAuthState();
  const [status, setStatus] = useState<Status["latest_run"] | undefined>(undefined);
  useEffect(() => {
    apiGet<Status>("/api/status", getToken)
      .then((s) => setStatus(s.latest_run))
      .catch(() => setStatus(null));
  }, [getToken]);

  if (status === undefined) return null;
  const stale =
    !status ||
    status.status === "failed" ||
    (status.finished_at != null && Date.now() / 1000 - status.finished_at > 2 * 86400);
  return (
    <p
      className={cn(
        "text-xs tabular-nums",
        stale ? "rounded bg-warning/15 px-2 py-0.5 text-foreground" : "text-muted-foreground",
      )}
    >
      {status
        ? `株価: ${ymd(status.latest_price_date)}終値・財務: ${ymd(status.latest_fin_date)}開示分（${jstDateTime(status.finished_at ?? status.started_at)}取得）`
        : "データがまだありません"}
      {stale && status ? "・更新が止まっています" : ""}
    </p>
  );
}

function LoginPage() {
  const { signInWithGoogle } = useAuthState();
  const [busy, setBusy] = useState(false);
  return (
    <Centered>
      <h1 className="mb-2 text-2xl font-semibold tracking-wide">Maa'un 配当分析</h1>
      <p className="mb-6 text-sm text-muted-foreground">配当と財務から、割安な日本株を見つける。</p>
      <Button
        size="lg"
        disabled={busy}
        onClick={() => {
          setBusy(true);
          signInWithGoogle().catch(() => setBusy(false));
        }}
      >
        Google でログイン
      </Button>
      <Footer />
    </Centered>
  );
}

function Centered({ children }: { children: React.ReactNode }) {
  return (
    <main className="flex min-h-svh flex-col items-center justify-center px-4 text-center">
      {children}
    </main>
  );
}

function Footer() {
  return (
    <footer className="mx-auto max-w-7xl px-4 py-8 text-xs text-muted-foreground">
      <p className="mb-1 tabular-nums">v{__APP_VERSION__}</p>
      情報提供のみを目的としており、投資助言ではありません。投資判断はご自身の責任で行ってください。
      株価・財務は J-Quants API（日本取引所グループ）、配当方針の記述は EDINET（金融庁）、銘柄一覧は
      JPX から取得しています。
    </footer>
  );
}
