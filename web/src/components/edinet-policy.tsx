import { useEffect, useId, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { apiGet, type EdinetDoc, type EdinetDocs, type StockRow } from "@/lib/api";
import { useAuthState } from "@/lib/auth";
import { pct, ymd } from "@/lib/format";
import { cn } from "@/lib/utils";

const KIND_LABEL: Record<EdinetDoc["doc_kind"], string> = {
  annual: "有価証券報告書",
  semiannual: "半期報告書",
};

function tags(d: EdinetDoc): string[] {
  return [
    d.progressive_dividend && "累進配当",
    d.no_dividend_cut && "非減配",
    d.dividend_raise && "増配の記述",
    d.doe_target && "DOE目標",
    d.payout_target != null && `配当性向${pct(d.payout_target, 0)}以上`,
  ].filter((t): t is string => Boolean(t));
}

/**
 * 一覧のバッジ「累進配当」「非減配」。押すと、有報・半期報告書（EDINET）から拾った文を出す。
 * 文は開いたときに初めて読む（一覧の応答を軽くするため）。
 * 表は横にスクロールする枠の中にあるので、枠で切れないよう body の直下に固定位置で出す
 */
export function EdinetPolicy({ r }: { r: StockRow }) {
  const { getToken } = useAuthState();
  const [open, setOpen] = useState(false);
  /** 開いたときのボタンの位置から決めた、画面に対する位置 */
  const [pos, setPos] = useState<{ top: number; right: number } | null>(null);
  const [docs, setDocs] = useState<EdinetDoc[] | null>(null);
  const [error, setError] = useState(false);
  const panelId = useId();
  const buttonRef = useRef<HTMLButtonElement>(null);
  const panelRef = useRef<HTMLElement>(null);

  useEffect(() => {
    if (!open || docs || error) return;
    const ctrl = new AbortController();
    apiGet<EdinetDocs>(`/api/stocks/${encodeURIComponent(r.code)}/edinet`, getToken, ctrl.signal)
      .then((d) => setDocs(d.docs))
      .catch((e: unknown) => {
        if (!(e instanceof DOMException && e.name === "AbortError")) setError(true);
      });
    return () => ctrl.abort();
  }, [open, docs, error, r.code, getToken]);

  // 外側を押す・Esc・スクロール・画面の大きさの変更で閉じる（位置がずれるため）
  useEffect(() => {
    if (!open) return;
    const close = () => setOpen(false);
    const onDown = (e: MouseEvent) => {
      const t = e.target as Node;
      if (!buttonRef.current?.contains(t) && !panelRef.current?.contains(t)) close();
    };
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== "Escape") return;
      close();
      buttonRef.current?.focus();
    };
    const onScroll = (e: Event) => {
      if (!panelRef.current?.contains(e.target as Node)) close();
    };
    document.addEventListener("mousedown", onDown);
    document.addEventListener("keydown", onKey);
    window.addEventListener("scroll", onScroll, true);
    window.addEventListener("resize", close);
    return () => {
      document.removeEventListener("mousedown", onDown);
      document.removeEventListener("keydown", onKey);
      window.removeEventListener("scroll", onScroll, true);
      window.removeEventListener("resize", close);
    };
  }, [open]);

  const toggle = () => {
    const rect = buttonRef.current?.getBoundingClientRect();
    if (rect) setPos({ top: rect.bottom + 4, right: Math.max(16, window.innerWidth - rect.right) });
    setOpen((o) => !o);
  };

  const label = r.progressive_dividend ? "累進配当" : "非減配";
  return (
    <>
      <button
        ref={buttonRef}
        type="button"
        aria-expanded={open}
        aria-controls={panelId}
        title="有価証券報告書・半期報告書（EDINET）の記述を見る"
        onClick={toggle}
        className={cn(
          "inline-flex h-5 items-center rounded-md border border-primary/40 bg-primary/10 px-2 text-xs font-medium text-primary",
          "hover:bg-primary/20 focus-visible:outline-2 focus-visible:outline-ring",
        )}
      >
        {label}
      </button>
      {open &&
        pos &&
        createPortal(
          <section
            ref={panelRef}
            id={panelId}
            aria-label={`${r.name ?? r.code}の配当方針の記述`}
            style={{ top: pos.top, right: pos.right, maxHeight: `calc(100vh - ${pos.top + 16}px)` }}
            className="fixed z-50 w-[min(26rem,calc(100vw-2rem))] overflow-y-auto rounded-lg border bg-popover p-3 text-left text-sm text-popover-foreground shadow-lg"
          >
            {error && <p className="text-destructive">読み込めませんでした。</p>}
            {!error && !docs && <p className="text-muted-foreground">読み込み中…</p>}
            {docs?.length === 0 && <p className="text-muted-foreground">書類がありません。</p>}
            {docs?.map((d) => (
              <div key={d.doc_kind} className="not-first:mt-3 not-first:border-t not-first:pt-3">
                <h3 className="text-xs text-muted-foreground">
                  {KIND_LABEL[d.doc_kind]}（{ymd(d.period_end)} 期・{ymd(d.submitted)} 提出）
                </h3>
                {tags(d).length > 0 && (
                  <p className="mt-1 text-xs font-medium">{tags(d).join("・")}</p>
                )}
                <ul className="mt-1 flex list-disc flex-col gap-1 pl-4">
                  {d.phrases.map((p) => (
                    <li key={p} className="leading-relaxed">
                      {p}
                    </li>
                  ))}
                </ul>
              </div>
            ))}
            <p className="mt-3 text-xs text-muted-foreground">
              言葉の有無で拾っています。判断は原文で確かめてください（書類管理番号{" "}
              {docs?.map((d) => d.doc_id).join("・")}）。
            </p>
          </section>,
          document.body,
        )}
    </>
  );
}
