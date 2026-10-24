import { Pencil, Trash2 } from "lucide-react";
import { useCallback, useEffect, useState } from "react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { ApiError, apiFetch, apiGet, type Theme, type ThemeDetail, type Themes } from "@/lib/api";
import { useAuthState } from "@/lib/auth";
import { jstDateTime } from "@/lib/format";
import { navigate } from "@/lib/router";

type Load = { state: "loading" } | { state: "ok"; themes: Theme[] } | { state: "error" };

/** 入力欄は読点・カンマ・改行で区切る */
const splitWords = (s: string) =>
  s
    .split(/[、,\n]/)
    .map((w) => w.trim())
    .filter(Boolean);

/**
 * 管理者の「注目テーマ」。テーマ名と言葉を登録すると、有価証券報告書の「事業の内容」の文と突き合わせて
 * テーマ株を決める。AIレコメンドでは同じ評価の中でテーマ株を先に出し、テーマ名をタグで示す
 */
export function ThemesPage({ canManage }: { canManage: boolean }) {
  const { getToken } = useAuthState();
  const [load, setLoad] = useState<Load>({ state: "loading" });
  const [editing, setEditing] = useState<Theme | null>(null);
  const [opened, setOpened] = useState<number | null>(null);

  const reload = useCallback(() => {
    apiGet<Themes>("/api/themes", getToken)
      .then((d) => setLoad({ state: "ok", themes: d.themes }))
      .catch(() => setLoad({ state: "error" }));
  }, [getToken]);

  useEffect(reload, [reload]);

  if (load.state === "loading") {
    return <p className="text-sm text-muted-foreground">読み込み中…</p>;
  }
  if (load.state === "error") {
    return (
      <p role="alert" className="rounded-lg border border-warning/50 bg-warning/10 p-3 text-sm">
        注目テーマを読み込めませんでした。時間をおいて再度お試しください。
      </p>
    );
  }

  return (
    <section aria-labelledby="themes-heading" className="flex flex-col gap-4">
      <div>
        <h2 id="themes-heading" className="text-sm">
          注目テーマ
        </h2>
        <p className="mt-1 text-sm text-muted-foreground">
          テーマ名と、そのテーマを表す言葉を登録すると、有価証券報告書の「事業の内容」にその言葉がある銘柄をテーマ株とみなします。AIレコメンドでは同じ評価の中でテーマ株を先に出し、テーマ名をタグで示します（テーマだけで評価は上げません）。流行りは人が決め、対応づけは機械が行います。
        </p>
      </div>

      {canManage && (
        <ThemeForm
          key={editing?.id ?? "new"}
          theme={editing}
          onDone={() => {
            setEditing(null);
            reload();
          }}
          onCancel={() => setEditing(null)}
        />
      )}

      {load.themes.length === 0 ? (
        <p className="rounded-lg border p-6 text-center text-sm text-muted-foreground">
          まだテーマがありません。{canManage ? "上の欄から登録してください。" : ""}
        </p>
      ) : (
        <ul className="flex flex-col gap-2">
          {load.themes.map((t) => (
            <li key={t.id} className="rounded-xl border p-3">
              <div className="flex flex-wrap items-center gap-x-3 gap-y-1">
                <button
                  type="button"
                  className="font-medium underline-offset-4 hover:underline"
                  onClick={() => setOpened(opened === t.id ? null : t.id)}
                  aria-expanded={opened === t.id}
                >
                  {t.name}
                </button>
                <span className="text-xs tabular-nums text-muted-foreground">
                  {t.count}銘柄・{jstDateTime(t.updated_at)}更新
                </span>
                <span className="flex flex-wrap gap-1">
                  {t.words.map((w) => (
                    <Badge key={w} variant="secondary">
                      {w}
                    </Badge>
                  ))}
                </span>
                {canManage && (
                  <span className="ml-auto flex gap-1">
                    <Button
                      variant="ghost"
                      size="icon-sm"
                      aria-label={`${t.name}を編集`}
                      onClick={() => setEditing(t)}
                    >
                      <Pencil aria-hidden />
                    </Button>
                    <DeleteButton theme={t} onDone={reload} />
                  </span>
                )}
              </div>
              {opened === t.id && <ThemeStocks id={t.id} />}
            </li>
          ))}
        </ul>
      )}
    </section>
  );
}

function ThemeForm({
  theme,
  onDone,
  onCancel,
}: {
  theme: Theme | null;
  onDone: () => void;
  onCancel: () => void;
}) {
  const { getToken } = useAuthState();
  const [name, setName] = useState(theme?.name ?? "");
  const [words, setWords] = useState(theme?.words.join("、") ?? "");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const submit = async (e: React.FormEvent) => {
    e.preventDefault();
    setBusy(true);
    setError(null);
    try {
      const body = { name: name.trim(), words: splitWords(words) };
      if (theme) {
        await apiFetch("PUT", `/api/themes/${theme.id}`, getToken, undefined, body);
      } else {
        await apiFetch("POST", "/api/themes", getToken, undefined, body);
      }
      setName("");
      setWords("");
      onDone();
    } catch (err) {
      setError(
        err instanceof ApiError && err.status === 409
          ? "同じ名前のテーマがあります。"
          : err instanceof ApiError && err.status === 400
            ? "テーマ名（30文字まで）と言葉（1〜20個、各30文字まで）を確かめてください。"
            : "保存できませんでした。時間をおいて再度お試しください。",
      );
    } finally {
      setBusy(false);
    }
  };

  return (
    <form onSubmit={(e) => void submit(e)} className="flex flex-col gap-3 rounded-xl border p-4">
      <p className="text-sm">{theme ? `「${theme.name}」を編集` : "テーマを登録"}</p>
      <div className="grid gap-3 sm:grid-cols-[1fr_2fr]">
        <div className="flex flex-col gap-1.5">
          <Label htmlFor="theme-name">テーマ名</Label>
          <Input
            id="theme-name"
            value={name}
            onChange={(e) => setName(e.target.value)}
            placeholder="例: 半導体"
            maxLength={30}
            required
          />
        </div>
        <div className="flex flex-col gap-1.5">
          <Label htmlFor="theme-words">言葉（読点・カンマ区切り）</Label>
          <Input
            id="theme-words"
            value={words}
            onChange={(e) => setWords(e.target.value)}
            placeholder="例: 半導体、パワー半導体、SiC"
            required
          />
          <p className="text-xs text-muted-foreground">
            「事業の内容」の文にそのまま含まれる言葉にしてください（部分一致。「電力」は「電力会社」にも当たります）。
          </p>
        </div>
      </div>
      {error && (
        <p role="alert" className="text-sm text-expensive">
          {error}
        </p>
      )}
      <div className="flex gap-2">
        <Button type="submit" size="sm" disabled={busy}>
          {theme ? "保存して照合しなおす" : "登録して照合する"}
        </Button>
        {theme && (
          <Button type="button" variant="ghost" size="sm" onClick={onCancel} disabled={busy}>
            やめる
          </Button>
        )}
      </div>
    </form>
  );
}

function DeleteButton({ theme, onDone }: { theme: Theme; onDone: () => void }) {
  const { getToken } = useAuthState();
  const [busy, setBusy] = useState(false);
  return (
    <Button
      variant="ghost"
      size="icon-sm"
      aria-label={`${theme.name}を消す`}
      disabled={busy}
      onClick={() => {
        if (!window.confirm(`テーマ「${theme.name}」を消します。よろしいですか？`)) return;
        setBusy(true);
        apiFetch("DELETE", `/api/themes/${theme.id}`, getToken)
          .then(onDone)
          .catch(() => window.alert("消せませんでした。時間をおいて再度お試しください。"))
          .finally(() => setBusy(false));
      }}
    >
      <Trash2 aria-hidden />
    </Button>
  );
}

/** テーマに当たった銘柄（開いたときに読む） */
function ThemeStocks({ id }: { id: number }) {
  const { getToken } = useAuthState();
  const [detail, setDetail] = useState<ThemeDetail | null | "error">(null);
  useEffect(() => {
    const ctrl = new AbortController();
    apiGet<ThemeDetail>(`/api/themes/${id}`, getToken, ctrl.signal)
      .then(setDetail)
      .catch((e: unknown) => {
        if (!(e instanceof DOMException && e.name === "AbortError")) setDetail("error");
      });
    return () => ctrl.abort();
  }, [id, getToken]);

  if (detail === null) return <p className="mt-2 text-xs text-muted-foreground">読み込み中…</p>;
  if (detail === "error") {
    return <p className="mt-2 text-xs text-muted-foreground">銘柄を読み込めませんでした。</p>;
  }
  if (detail.stocks.length === 0) {
    return (
      <p className="mt-2 text-xs text-muted-foreground">
        当たった銘柄はありません（「事業の内容」をまだ取り込んでいない銘柄は照合できません）。
      </p>
    );
  }
  return (
    <ul className="mt-2 flex flex-wrap gap-x-4 gap-y-1 text-sm">
      {detail.stocks.map((s) => (
        <li key={s.code}>
          <a
            href={`/stocks/${s.code}`}
            onClick={(e) => {
              if (e.metaKey || e.ctrlKey || e.shiftKey || e.button !== 0) return;
              e.preventDefault();
              navigate(`/stocks/${s.code}`);
            }}
            className="underline-offset-4 hover:underline"
          >
            {s.name ?? s.code}
          </a>
          <span className="ml-1 text-xs text-muted-foreground">
            {s.code}・「{s.matched_word}」
          </span>
        </li>
      ))}
    </ul>
  );
}
