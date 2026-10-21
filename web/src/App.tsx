import { LogOut } from "lucide-react";
import { useEffect, useState } from "react";
import { Button } from "@/components/ui/button";
import { ApiError, apiGet, type Me, type Status } from "@/lib/api";
import { useAuthState } from "@/lib/auth";
import { jstDateTime, ymd } from "@/lib/format";
import { cn } from "@/lib/utils";
import { ScreeningPage } from "@/pages/screening-page";

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
    <div className="min-h-svh">
      <header className="sticky top-0 z-10 border-b bg-background/90 backdrop-blur">
        <div className="mx-auto flex max-w-7xl flex-wrap items-center gap-x-6 gap-y-1 px-4 py-3">
          <h1 className="text-base">増配期待株ダッシュボード</h1>
          <Freshness />
          <div className="ml-auto flex items-center gap-2 text-sm">
            <span className="hidden text-muted-foreground sm:inline">
              {session.me.display_name}
            </span>
            <Button variant="ghost" size="sm" onClick={() => void signOut()}>
              <LogOut aria-hidden />
              ログアウト
            </Button>
          </div>
        </div>
      </header>
      <main className="mx-auto max-w-7xl px-4 py-6">
        <ScreeningPage />
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
        ? `株価: ${ymd(status.latest_price_date)}終値（${jstDateTime(status.finished_at ?? status.started_at)}取得）`
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
      <h1 className="mb-2 text-xl">増配期待株ダッシュボード</h1>
      <p className="mb-6 text-sm text-muted-foreground">割安で増配が期待できる日本株を探す</p>
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
      情報提供のみを目的としており、投資助言ではありません。投資判断はご自身の責任で行ってください。
      株価・財務は J-Quants API（日本取引所グループ）、銘柄一覧は JPX から取得しています。
    </footer>
  );
}
