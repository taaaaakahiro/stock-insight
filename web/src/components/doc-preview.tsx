import { ExternalLink, FileText, X } from "lucide-react";
import { Dialog } from "radix-ui";
import { useEffect, useState } from "react";
import { Button, buttonVariants } from "@/components/ui/button";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { apiGet, type EdinetDoc, type EdinetDocs, type StockRow } from "@/lib/api";
import { useAuthState } from "@/lib/auth";
import { ymd } from "@/lib/format";

const KIND_LABEL: Record<EdinetDoc["doc_kind"], string> = {
  annual: "有価証券報告書",
  semiannual: "半期報告書",
};

/** EDINET 閲覧サイトの PDF（API キーなしで取れ、埋め込みも拒まない。CSP の frame-src で許可している） */
export function edinetPdfUrl(docId: string): string {
  return `https://disclosure2dl.edinet-fsa.go.jp/searchdocument/pdf/${encodeURIComponent(docId)}.pdf`;
}

/** 四季報オンラインの銘柄ページ。有料・会員の内容なので埋め込まず、別のタブで開くだけ */
export function shikihoUrl(code: string): string {
  return `https://shikiho.toyokeizai.net/stocks/${encodeURIComponent(code)}`;
}

function OutLink({ href, children }: { href: string; children: React.ReactNode }) {
  return (
    <a
      href={href}
      target="_blank"
      rel="noopener noreferrer"
      className={buttonVariants({ variant: "outline", size: "sm" })}
    >
      {children}
      <ExternalLink aria-hidden />
    </a>
  );
}

/**
 * 銘柄の横の書類ボタン。押すと、持っている最新の有報・半期報告書（EDINET）の PDF を画面の中で見せる。
 * 書類の一覧は開いたときに初めて読む。四季報は別のタブで開く
 */
export function DocPreview({ r }: { r: StockRow }) {
  const { getToken } = useAuthState();
  const [open, setOpen] = useState(false);
  const [docs, setDocs] = useState<EdinetDoc[] | null>(null);
  const [error, setError] = useState(false);
  const name = r.name ?? r.code;

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

  // 有報を先に
  const sorted = docs
    ? [...docs].sort((a, b) => Number(a.doc_kind !== "annual") - Number(b.doc_kind !== "annual"))
    : null;

  return (
    <Dialog.Root open={open} onOpenChange={setOpen}>
      <Dialog.Trigger asChild>
        <Button
          variant="ghost"
          size="icon-sm"
          aria-label={`${name}の有報・四季報を見る`}
          title="有報・半期報告書・四季報を見る"
        >
          <FileText aria-hidden className="opacity-50" />
        </Button>
      </Dialog.Trigger>
      <Dialog.Portal>
        <Dialog.Overlay className="fixed inset-0 z-50 bg-black/50" />
        <Dialog.Content
          aria-describedby={undefined}
          className="fixed inset-2 z-50 flex flex-col gap-3 rounded-lg border bg-background p-3 shadow-lg md:inset-6"
        >
          <div className="flex items-center justify-between gap-2">
            <Dialog.Title className="truncate font-medium">
              {name}（{r.code}）
            </Dialog.Title>
            <div className="flex shrink-0 items-center gap-2">
              <OutLink href={shikihoUrl(r.code)}>四季報オンライン</OutLink>
              <Dialog.Close asChild>
                <Button variant="ghost" size="icon-sm" aria-label="閉じる">
                  <X aria-hidden />
                </Button>
              </Dialog.Close>
            </div>
          </div>
          {error && <p className="text-sm text-destructive">読み込めませんでした。</p>}
          {!error && !sorted && <p className="text-sm text-muted-foreground">読み込み中…</p>}
          {sorted?.length === 0 && (
            <p className="text-sm text-muted-foreground">
              有報・半期報告書をまだ取り込んでいません。
            </p>
          )}
          {sorted && sorted.length > 0 && (
            <Tabs defaultValue={sorted[0]?.doc_kind} className="min-h-0 flex-1">
              <TabsList>
                {sorted.map((d) => (
                  <TabsTrigger key={d.doc_kind} value={d.doc_kind}>
                    {KIND_LABEL[d.doc_kind]}
                  </TabsTrigger>
                ))}
              </TabsList>
              {sorted.map((d) => (
                <TabsContent
                  key={d.doc_kind}
                  value={d.doc_kind}
                  className="flex min-h-0 flex-1 flex-col gap-2"
                >
                  <div className="flex flex-wrap items-center justify-between gap-2 text-xs text-muted-foreground">
                    <span>
                      {ymd(d.period_end)} 期・{ymd(d.submitted)} 提出（書類管理番号 {d.doc_id}）
                    </span>
                    <OutLink href={edinetPdfUrl(d.doc_id)}>別のタブで開く</OutLink>
                  </div>
                  <iframe
                    title={`${name}の${KIND_LABEL[d.doc_kind]}`}
                    src={edinetPdfUrl(d.doc_id)}
                    className="min-h-0 w-full flex-1 rounded-md border bg-muted"
                  />
                </TabsContent>
              ))}
            </Tabs>
          )}
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
  );
}
