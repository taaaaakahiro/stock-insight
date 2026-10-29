import {
  ArrowDownWideNarrow,
  ArrowUpNarrowWide,
  ChevronLeft,
  ChevronRight,
  Search,
  SlidersHorizontal,
  X,
} from "lucide-react";
import { Dialog } from "radix-ui";
import { useEffect, useMemo, useRef, useState } from "react";
import { FilterPanel } from "@/components/filter-panel";
import { ReitSectors } from "@/components/reit-rag";
import { ResultsTable } from "@/components/results-table";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Tabs, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { ApiError, apiGet, type StockPage } from "@/lib/api";
import { useAuthState } from "@/lib/auth";
import {
  activeConditions,
  type Filters,
  fromQuery,
  KINDS,
  type Kind,
  metricLabel,
  metricsFor,
  switchKind,
  toQuery,
} from "@/lib/filters";

type Load =
  | { state: "loading" }
  | { state: "ok"; data: StockPage }
  | { state: "error"; message: string };

// 小さいほうが割安・好ましい列。初めてクリックしたときは小さい順に並べる
const ASCENDING_FIRST = new Set<Filters["sort"]>(["code", "price", "pbr", "per", "payout_ratio"]);
const firstOrder = (key: Filters["sort"]): Filters["order"] =>
  ASCENDING_FIRST.has(key) ? "asc" : "desc";

// 入力のたびに API を呼ばないよう、少し待ってから取りに行く
const DEBOUNCE_MS = 300;

export function ScreeningPage() {
  const { getToken } = useAuthState();
  const [filters, setFilters] = useState<Filters>(() =>
    fromQuery(new URLSearchParams(window.location.search)),
  );
  const query = useMemo(() => toQuery(filters).toString(), [filters]);
  const [load, setLoad] = useState<Load>({ state: "loading" });
  const [sheetOpen, setSheetOpen] = useState(false);
  const resultsRef = useRef<HTMLElement>(null);

  // 条件を URL に残す（共有・再読み込みで同じ一覧に戻れる）
  useEffect(() => {
    const url = query ? `?${query}` : window.location.pathname;
    window.history.replaceState(null, "", url);
  }, [query]);

  useEffect(() => {
    const ctrl = new AbortController();
    const timer = window.setTimeout(() => {
      setLoad((prev) => (prev.state === "ok" ? prev : { state: "loading" }));
      apiGet<StockPage>(`/api/stocks${query ? `?${query}` : ""}`, getToken, ctrl.signal)
        .then((data) => setLoad({ state: "ok", data }))
        .catch((e: unknown) => {
          if (ctrl.signal.aborted) return;
          setLoad({
            state: "error",
            message:
              e instanceof ApiError && e.status === 400
                ? `条件を読み取れませんでした（${e.message}）`
                : "一覧を読み込めませんでした。時間をおいて再度お試しください。",
          });
        });
    }, DEBOUNCE_MS);
    return () => {
      ctrl.abort();
      window.clearTimeout(timer);
    };
  }, [query, getToken]);

  const onSort = (key: Filters["sort"]) =>
    setFilters((f) => ({
      ...f,
      sort: key,
      // 同じ列ならもう一方の向き、別の列なら「割安・好ましい」側から
      order: f.sort === key ? (f.order === "asc" ? "desc" : "asc") : firstOrder(key),
      page: 1,
    }));

  // ページを移ったら一覧の頭に戻す（スマホでは「次へ」が一番下にある）
  const goPage = (page: number) => {
    setFilters((f) => ({ ...f, page }));
    resultsRef.current?.scrollIntoView({ block: "start" });
  };

  const page = load.state === "ok" ? load.data : null;
  const lastPage = page ? Math.max(1, Math.ceil(page.total / page.page_size)) : 1;
  const conditions = activeConditions(filters);
  const totalText = page ? `${page.total.toLocaleString("ja-JP")}件` : "読み込み中…";

  return (
    <div className="flex flex-col gap-4 md:gap-6">
      <Tabs value={filters.kind} onValueChange={(v) => setFilters((f) => switchKind(f, v as Kind))}>
        <TabsList aria-label="種類">
          {KINDS.map((k) => (
            <TabsTrigger key={k.key} value={k.key}>
              {k.label}
            </TabsTrigger>
          ))}
        </TabsList>
      </Tabs>
      {filters.kind === "reit" && (
        <ReitSectors
          onSelect={(sector) =>
            setFilters((f) => ({
              ...f,
              reitSectors: sector === "unknown" ? [] : [sector],
              page: 1,
            }))
          }
        />
      )}

      <div className="grid gap-6 md:grid-cols-[18rem_1fr]">
        {/* PC は横に常に出す。欄の中のスクロールが端でページに抜けないようにする */}
        <aside className="hidden md:sticky md:top-20 md:block md:max-h-[calc(100svh-6rem)] md:self-start md:overflow-y-auto md:overscroll-contain md:pr-2 md:[scrollbar-gutter:stable]">
          <FilterPanel value={filters} onChange={setFilters} />
        </aside>

        <section ref={resultsRef} aria-labelledby="results-heading" className="min-w-0 scroll-mt-4">
          <div className="mb-3 flex gap-2">
            <div className="relative flex-1">
              <Search
                aria-hidden
                className="pointer-events-none absolute top-1/2 left-2.5 size-4 -translate-y-1/2 text-muted-foreground"
              />
              <Input
                type="search"
                placeholder="コードまたは銘柄名"
                aria-label="コードまたは銘柄名で検索"
                value={filters.query}
                onChange={(e) => setFilters((f) => ({ ...f, query: e.target.value, page: 1 }))}
                className="h-9 pl-8"
              />
            </div>
            {/* スマホでは絞り込みを下から出すシートにし、一覧を押し下げない */}
            <Dialog.Root open={sheetOpen} onOpenChange={setSheetOpen}>
              <Dialog.Trigger asChild>
                <Button variant="outline" className="h-9 md:hidden">
                  <SlidersHorizontal aria-hidden />
                  絞り込み
                  {conditions.length > 0 && (
                    <span className="rounded-full bg-primary px-1.5 text-xs text-primary-foreground tabular-nums">
                      {conditions.length}
                    </span>
                  )}
                </Button>
              </Dialog.Trigger>
              <Dialog.Portal>
                <Dialog.Overlay className="fixed inset-0 z-50 bg-black/50 md:hidden" />
                <Dialog.Content
                  aria-describedby={undefined}
                  className="fixed inset-x-0 bottom-0 z-50 flex max-h-[88svh] flex-col rounded-t-2xl border-t bg-background shadow-lg md:hidden"
                >
                  <Dialog.Title className="sr-only">絞り込み</Dialog.Title>
                  <div className="min-h-0 flex-1 overflow-y-auto overscroll-contain px-4 pt-4 pb-6">
                    <FilterPanel value={filters} onChange={setFilters} />
                  </div>
                  <div className="flex items-center gap-2 border-t px-4 pt-3 pb-[max(0.75rem,env(safe-area-inset-bottom))]">
                    <Dialog.Close asChild>
                      <Button className="h-10 flex-1">{totalText}を表示</Button>
                    </Dialog.Close>
                  </div>
                </Dialog.Content>
              </Dialog.Portal>
            </Dialog.Root>
          </div>

          {filters.kind === "stock" && (
            <p className="mb-3 text-xs text-muted-foreground">
              利回りは確認できた特別・記念配当を除いて計算します。予想の内訳が不明なら実績を優先し、実績も内訳不明の場合は合計額を使って「内訳未確認」と表示します。
            </p>
          )}
          {conditions.length > 0 && (
            <ul aria-label="効いている条件" className="mb-3 flex flex-wrap gap-1.5">
              {conditions.map((c) => (
                <li key={c.id}>
                  <button
                    type="button"
                    onClick={() => setFilters(c.clear)}
                    aria-label={`${c.label} を外す`}
                    className="inline-flex items-center gap-1 rounded-full border border-primary/40 bg-primary/10 py-0.5 pr-1.5 pl-2.5 text-xs hover:bg-primary/20"
                  >
                    {c.label}
                    <X aria-hidden className="size-3 opacity-70" />
                  </button>
                </li>
              ))}
            </ul>
          )}

          <div className="mb-3 flex items-center justify-between gap-2">
            <h2 id="results-heading" className="text-sm">
              一覧
              <span className="ml-2 text-muted-foreground tabular-nums" aria-live="polite">
                {totalText}
              </span>
            </h2>
            {/* PC は表の見出しで並べ替える。スマホのカードには見出しがないのでここで選ぶ */}
            <SortControl
              kind={filters.kind}
              sort={filters.sort}
              order={filters.order}
              onChange={(sort, order) => setFilters((f) => ({ ...f, sort, order, page: 1 }))}
            />
          </div>

          {load.state === "error" && (
            <p
              role="alert"
              className="rounded-lg border border-warning/50 bg-warning/10 p-3 text-sm"
            >
              {load.message}
            </p>
          )}
          {page && page.rows.length === 0 && (
            <p className="rounded-lg border p-6 text-center text-sm text-muted-foreground">
              {filters.kind === "stock"
                ? "条件に合う銘柄はありません。条件をゆるめてみてください。"
                : "条件に合う銘柄はありません。REIT・ETF はまだ価格・分配金を取り込んでいない場合があります。"}
            </p>
          )}
          {page && page.rows.length > 0 && (
            <ResultsTable
              rows={page.rows}
              kind={filters.kind}
              sort={filters.sort}
              order={filters.order}
              onSort={onSort}
            />
          )}

          {page && lastPage > 1 && (
            <nav aria-label="ページ" className="mt-4 flex items-center justify-center gap-3">
              <Button
                variant="outline"
                size="sm"
                disabled={filters.page <= 1}
                onClick={() => goPage(filters.page - 1)}
              >
                <ChevronLeft aria-hidden />
                前へ
              </Button>
              <span className="text-sm tabular-nums">
                {filters.page} / {lastPage}
              </span>
              <Button
                variant="outline"
                size="sm"
                disabled={filters.page >= lastPage}
                onClick={() => goPage(filters.page + 1)}
              >
                次へ
                <ChevronRight aria-hidden />
              </Button>
            </nav>
          )}
        </section>
      </div>
    </div>
  );
}

/** スマホの並べ替え: 何で並べるかと、向き */
function SortControl({
  kind,
  sort,
  order,
  onChange,
}: {
  kind: Kind;
  sort: Filters["sort"];
  order: Filters["order"];
  onChange: (sort: Filters["sort"], order: Filters["order"]) => void;
}) {
  const options: { key: Filters["sort"]; label: string }[] = [
    ...metricsFor(kind).map((m) => ({ key: m.key, label: metricLabel(m.key, kind) })),
    { key: "code", label: "コード" },
  ];
  return (
    <div className="flex items-center gap-1 md:hidden">
      <Select
        value={sort}
        onValueChange={(v) => onChange(v as Filters["sort"], firstOrder(v as Filters["sort"]))}
      >
        <SelectTrigger size="sm" aria-label="並べ替え" className="max-w-44">
          <SelectValue />
        </SelectTrigger>
        <SelectContent>
          {options.map((o) => (
            <SelectItem key={o.key} value={o.key}>
              {o.label}
            </SelectItem>
          ))}
        </SelectContent>
      </Select>
      <Button
        variant="outline"
        size="icon-sm"
        aria-label={order === "asc" ? "小さい順（押すと大きい順）" : "大きい順（押すと小さい順）"}
        title={order === "asc" ? "小さい順" : "大きい順"}
        onClick={() => onChange(sort, order === "asc" ? "desc" : "asc")}
      >
        {order === "asc" ? <ArrowUpNarrowWide aria-hidden /> : <ArrowDownWideNarrow aria-hidden />}
      </Button>
    </div>
  );
}
