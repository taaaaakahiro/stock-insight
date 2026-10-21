import { ChevronLeft, ChevronRight, SlidersHorizontal } from "lucide-react";
import { useEffect, useMemo, useState } from "react";
import { FilterPanel } from "@/components/filter-panel";
import { ResultsTable } from "@/components/results-table";
import { Button } from "@/components/ui/button";
import { Tabs, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { ApiError, apiGet, type StockPage } from "@/lib/api";
import { useAuthState } from "@/lib/auth";
import { type Filters, fromQuery, KINDS, type Kind, switchKind, toQuery } from "@/lib/filters";

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
  const [filtersOpen, setFiltersOpen] = useState(false);

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

  const page = load.state === "ok" ? load.data : null;
  const lastPage = page ? Math.max(1, Math.ceil(page.total / page.page_size)) : 1;

  return (
    <div className="flex flex-col gap-6">
      <Tabs value={filters.kind} onValueChange={(v) => setFilters((f) => switchKind(f, v as Kind))}>
        <TabsList aria-label="種類">
          {KINDS.map((k) => (
            <TabsTrigger key={k.key} value={k.key}>
              {k.label}
            </TabsTrigger>
          ))}
        </TabsList>
      </Tabs>
      <div className="grid gap-6 md:grid-cols-[18rem_1fr]">
        <aside className="md:sticky md:top-20 md:self-start">
          {/* スマホでは絞り込みを畳み、一覧をすぐ見せる */}
          <Button
            variant="outline"
            className="w-full md:hidden"
            aria-expanded={filtersOpen}
            aria-controls="filter-panel"
            onClick={() => setFiltersOpen((o) => !o)}
          >
            <SlidersHorizontal aria-hidden />
            {filtersOpen ? "絞り込みを閉じる" : "絞り込み"}
          </Button>
          <div id="filter-panel" className={filtersOpen ? "mt-4 md:mt-0" : "hidden md:block"}>
            <FilterPanel value={filters} onChange={setFilters} />
          </div>
        </aside>

        <section aria-labelledby="results-heading" className="min-w-0">
          <div className="mb-3 flex items-baseline justify-between gap-2">
            <h2 id="results-heading" className="text-sm">
              一覧
            </h2>
            <p className="text-sm text-muted-foreground tabular-nums" aria-live="polite">
              {page ? `${page.total.toLocaleString("ja-JP")}件` : "読み込み中…"}
            </p>
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
                onClick={() => setFilters((f) => ({ ...f, page: f.page - 1 }))}
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
                onClick={() => setFilters((f) => ({ ...f, page: f.page + 1 }))}
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
