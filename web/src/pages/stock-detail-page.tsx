import { ArrowLeft } from "lucide-react";
import { useEffect, useState } from "react";
import { PortfolioToggle } from "@/components/portfolio-toggle";
import { Flags } from "@/components/results-table";
import { Button } from "@/components/ui/button";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";
import { type AnnualRow, ApiError, apiGet, type QuarterRow, type StockDetail } from "@/lib/api";
import { useAuthState } from "@/lib/auth";
import { type Kind, metricLabel } from "@/lib/filters";
import { count, jstDateTime, oku, pct, times, yen, yen1, ymd } from "@/lib/format";
import { backToList } from "@/lib/router";
import { cn } from "@/lib/utils";

type Load =
  | { state: "loading" }
  | { state: "ok"; data: StockDetail }
  | { state: "missing" }
  | { state: "error" };

export function StockDetailPage({ code }: { code: string }) {
  const { getToken } = useAuthState();
  const [load, setLoad] = useState<Load>({ state: "loading" });

  useEffect(() => {
    const ctrl = new AbortController();
    apiGet<StockDetail>(`/api/stocks/${code}`, getToken, ctrl.signal)
      .then((data) => setLoad({ state: "ok", data }))
      .catch((e: unknown) => {
        if (ctrl.signal.aborted) return;
        setLoad(
          e instanceof ApiError && e.status === 404 ? { state: "missing" } : { state: "error" },
        );
      });
    return () => ctrl.abort();
  }, [code, getToken]);

  return (
    <div className="flex flex-col gap-6">
      <div>
        <Button variant="ghost" size="sm" onClick={backToList}>
          <ArrowLeft aria-hidden />
          一覧へ戻る
        </Button>
      </div>
      {load.state === "loading" && <p className="text-sm text-muted-foreground">読み込み中…</p>}
      {load.state === "missing" && (
        <p role="alert" className="rounded-lg border p-6 text-sm">
          銘柄コード {code} のデータはありません。
        </p>
      )}
      {load.state === "error" && (
        <p role="alert" className="rounded-lg border border-warning/50 bg-warning/10 p-3 text-sm">
          詳細を読み込めませんでした。時間をおいて再度お試しください。
        </p>
      )}
      {load.state === "ok" && <Detail d={load.data} />}
    </div>
  );
}

const KIND_LABEL: Record<Kind, string> = { stock: "株式", reit: "REIT", etf: "ETF" };

function Detail({ d }: { d: StockDetail }) {
  const s = d.stock;
  const kind = s.kind;
  const stock = kind === "stock";
  const divWord = stock ? "配当" : "分配金";
  const latestDisclosed = d.latest_disclosed;
  const metrics: { label: string; value: string }[] = [
    { label: metricLabel("price", kind), value: yen(s.price) },
    { label: metricLabel("dividend_yield", kind), value: pct(s.dividend_yield) },
    { label: metricLabel("yield_percentile", kind), value: pct(s.yield_percentile, 0) },
    ...(stock
      ? [
          { label: "PBR", value: times(s.pbr) },
          { label: "PER", value: times(s.per) },
          { label: "配当性向", value: pct(s.payout_ratio, 1) },
          { label: "DOE", value: pct(s.doe) },
          { label: "連続増配", value: count(s.consecutive_increases) },
          { label: "営業利益率", value: pct(s.operating_margin, 1) },
        ]
      : []),
  ];

  return (
    <>
      <header className="flex flex-col gap-3">
        <div className="flex items-start gap-2">
          <PortfolioToggle code={s.code} name={s.name ?? s.code} />
          <div className="min-w-0 flex-1">
            <h2 className="text-xl">{s.name ?? s.code}</h2>
            <p className="text-sm text-muted-foreground">
              {[s.code, KIND_LABEL[kind], s.market, stock ? s.sector33 : null]
                .filter(Boolean)
                .join("・")}
            </p>
          </div>
        </div>
        {stock && <Flags r={s} />}
        <p className="text-xs text-muted-foreground">
          {metricLabel("price", kind)}は {ymd(s.price_date)} の終値、財務は決算短信（J-Quants）の
          {latestDisclosed ? ` ${ymd(latestDisclosed)} 開示分` : "最新値"}。指標の計算は{" "}
          {jstDateTime(s.fetched_at)}。
        </p>
      </header>

      <section aria-label="主要な指標">
        <dl className="grid grid-cols-2 gap-3 sm:grid-cols-3 lg:grid-cols-5">
          {metrics.map((m) => (
            <div key={m.label} className="rounded-xl border p-3">
              <dt className="text-xs text-muted-foreground">{m.label}</dt>
              <dd className="text-lg tabular-nums">{m.value}</dd>
            </div>
          ))}
        </dl>
      </section>

      <section aria-labelledby="yield-chart">
        <h3 id="yield-chart" className="mb-2 text-sm">
          {divWord}利回りの推移（週1点、最長5年）
        </h3>
        {d.yield_series.length >= 2 ? (
          <YieldChart series={d.yield_series} />
        ) : (
          <p className="text-sm text-muted-foreground">
            {stock ? "配当" : "分配金"}の開示が取り込まれていないため、推移はまだ出せません。
          </p>
        )}
      </section>

      <section aria-labelledby="price-chart">
        <h3 id="price-chart" className="mb-2 text-sm">
          {metricLabel("price", kind)}の推移（週1点、最長5年）
        </h3>
        {d.price_series.length >= 2 ? (
          <LineChart series={d.price_series} format={yen} />
        ) : (
          <p className="text-sm text-muted-foreground">株価の履歴がまだありません。</p>
        )}
      </section>

      {kind !== "etf" && (
        <section aria-labelledby="annual">
          <h3 id="annual" className="mb-2 text-sm">
            年次の財務（最大5期）
          </h3>
          {d.annual.length > 0 ? (
            <AnnualTable rows={d.annual} divWord={divWord} />
          ) : (
            <p className="text-sm text-muted-foreground">本決算のデータがまだありません。</p>
          )}
        </section>
      )}

      {stock && (
        <section aria-labelledby="quarters">
          <h3 id="quarters" className="mb-2 text-sm">
            四半期ごとの損益（単独、最大20四半期）
          </h3>
          <p className="mb-2 text-xs text-muted-foreground">
            決算短信の累計値の差分。前の四半期の開示がない期間は「—」。
          </p>
          {d.quarters.length > 0 ? (
            <QuarterTable rows={d.quarters} />
          ) : (
            <p className="text-sm text-muted-foreground">四半期のデータがまだありません。</p>
          )}
        </section>
      )}
    </>
  );
}

/** 負の値は赤字として強調（色だけに頼らず「▲」も付ける） */
function Money({ v }: { v: number | null }) {
  if (v == null) return <>—</>;
  return <span className={cn(v < 0 && "text-expensive")}>{v < 0 ? `▲${oku(-v)}` : oku(v)}</span>;
}

function AnnualTable({ rows, divWord }: { rows: AnnualRow[]; divWord: string }) {
  const cols: { label: string; render: (r: AnnualRow) => React.ReactNode }[] = [
    { label: "期末", render: (r) => ymd(r.period_end) },
    { label: "売上高", render: (r) => <Money v={r.net_sales} /> },
    { label: "営業利益", render: (r) => <Money v={r.operating_income} /> },
    { label: "営業利益率", render: (r) => pct(r.operating_margin, 1) },
    { label: "純利益", render: (r) => <Money v={r.net_income} /> },
    { label: "EPS", render: (r) => yen1(r.eps) },
    { label: "BPS", render: (r) => yen1(r.bps) },
    { label: `1株${divWord}`, render: (r) => yen1(r.dps) },
    { label: `${divWord}性向`, render: (r) => pct(r.payout_ratio, 1) },
    { label: "営業CF", render: (r) => <Money v={r.operating_cf} /> },
  ];
  return (
    <Table>
      <TableHeader>
        <TableRow>
          {cols.map((c) => (
            <TableHead key={c.label} className="text-right first:text-left">
              {c.label}
            </TableHead>
          ))}
        </TableRow>
      </TableHeader>
      <TableBody>
        {rows.map((r) => (
          <TableRow key={r.period_end ?? "?"}>
            {cols.map((c) => (
              <TableCell key={c.label} className="text-right tabular-nums first:text-left">
                {c.render(r)}
              </TableCell>
            ))}
          </TableRow>
        ))}
      </TableBody>
    </Table>
  );
}

function QuarterTable({ rows }: { rows: QuarterRow[] }) {
  // 新しい四半期を上に
  const list = [...rows].reverse();
  return (
    <Table>
      <TableHeader>
        <TableRow>
          <TableHead>期末</TableHead>
          <TableHead>四半期</TableHead>
          <TableHead className="text-right">売上高</TableHead>
          <TableHead className="text-right">営業利益</TableHead>
          <TableHead className="text-right">経常利益</TableHead>
          <TableHead className="text-right">純利益</TableHead>
        </TableRow>
      </TableHeader>
      <TableBody>
        {list.map((q) => (
          <TableRow key={`${q.fiscal_year_end}-${q.quarter}`}>
            <TableCell>{ymd(q.period_end)}</TableCell>
            <TableCell className="text-muted-foreground">
              {q.fiscal_year_end.slice(0, 4)}年度 Q{q.quarter}
            </TableCell>
            <TableCell className="text-right tabular-nums">
              <Money v={q.net_sales} />
            </TableCell>
            <TableCell className="text-right tabular-nums">
              <Money v={q.operating_income} />
            </TableCell>
            <TableCell className="text-right tabular-nums">
              <Money v={q.ordinary_income} />
            </TableCell>
            <TableCell className="text-right tabular-nums">
              <Money v={q.net_income} />
            </TableCell>
          </TableRow>
        ))}
      </TableBody>
    </Table>
  );
}

/** 下から何%の位置にある値か（同じ値は半分だけ数える。indicators.rs と同じ） */
function percentile(sorted: number[], p: number): number {
  if (sorted.length === 0) return 0;
  const i = (sorted.length - 1) * p;
  const lo = sorted[Math.floor(i)] ?? 0;
  const hi = sorted[Math.ceil(i)] ?? lo;
  return lo + (hi - lo) * (i - Math.floor(i));
}

const W = 640;
const H = 220;
const PAD = { top: 10, right: 12, bottom: 24, left: 72 };

function scales(values: number[]) {
  const min = Math.min(...values);
  const max = Math.max(...values);
  const span = max - min || max || 1;
  const y = (v: number) => PAD.top + (H - PAD.top - PAD.bottom) * (1 - (v - min) / span);
  const x = (i: number, n: number) =>
    PAD.left + ((W - PAD.left - PAD.right) * i) / Math.max(1, n - 1);
  return { min, max, x, y };
}

/** 利回りの折れ線。上位20%（株価が過去と比べて安い）と下位20%（高い）の帯を重ねる */
function YieldChart({ series }: { series: [string, number][] }) {
  const values = series.map(([, v]) => v);
  const sorted = [...values].sort((a, b) => a - b);
  const p20 = percentile(sorted, 0.2);
  const p80 = percentile(sorted, 0.8);
  const { min, max, x, y } = scales(values);
  const path = values
    .map((v, i) => `${i ? "L" : "M"}${x(i, values.length).toFixed(1)},${y(v).toFixed(1)}`)
    .join(" ");
  const last = values.at(-1) ?? 0;
  const first = series[0]?.[0] ?? "";
  const lastDate = series.at(-1)?.[0] ?? "";
  return (
    <figure>
      <svg
        viewBox={`0 0 ${W} ${H}`}
        role="img"
        aria-label={`利回りの推移。${ymd(first)}から${ymd(lastDate)}。今は${pct(last)}`}
        className="h-auto w-full text-xs"
      >
        <rect
          x={PAD.left}
          y={PAD.top}
          width={W - PAD.left - PAD.right}
          height={y(p80) - PAD.top}
          className="fill-cheap/10"
        />
        <rect
          x={PAD.left}
          y={y(p20)}
          width={W - PAD.left - PAD.right}
          height={H - PAD.bottom - y(p20)}
          className="fill-expensive/10"
        />
        <line
          x1={PAD.left}
          x2={W - PAD.right}
          y1={y(p80)}
          y2={y(p80)}
          className="stroke-cheap"
          strokeDasharray="4 3"
        />
        <line
          x1={PAD.left}
          x2={W - PAD.right}
          y1={y(p20)}
          y2={y(p20)}
          className="stroke-expensive"
          strokeDasharray="4 3"
        />
        <path d={path} fill="none" className="stroke-foreground" strokeWidth={1.5} />
        <circle
          cx={x(values.length - 1, values.length)}
          cy={y(last)}
          r={3.5}
          className="fill-foreground"
        />
        <text x={PAD.left - 6} y={y(max) + 4} textAnchor="end" className="fill-muted-foreground">
          {pct(max, 1)}
        </text>
        <text x={PAD.left - 6} y={y(min) + 4} textAnchor="end" className="fill-muted-foreground">
          {pct(min, 1)}
        </text>
        <text x={W - PAD.right} y={y(p80) - 3} textAnchor="end" className="fill-cheap">
          上位20% {pct(p80, 1)}
        </text>
        <text x={W - PAD.right} y={y(p20) + 11} textAnchor="end" className="fill-expensive">
          下位20% {pct(p20, 1)}
        </text>
        <text x={PAD.left} y={H - 6} className="fill-muted-foreground">
          {ymd(first)}
        </text>
        <text x={W - PAD.right} y={H - 6} textAnchor="end" className="fill-muted-foreground">
          {ymd(lastDate)}
        </text>
      </svg>
      <figcaption className="mt-1 text-xs text-muted-foreground">
        緑の帯は利回りが過去の上位20%（株価が過去と比べて安い）、赤の帯は下位20%（高い）。今は{" "}
        {pct(last)}。
      </figcaption>
    </figure>
  );
}

function LineChart({
  series,
  format,
}: {
  series: [string, number][];
  format: (v: number) => string;
}) {
  const values = series.map(([, v]) => v);
  const { min, max, x, y } = scales(values);
  const path = values
    .map((v, i) => `${i ? "L" : "M"}${x(i, values.length).toFixed(1)},${y(v).toFixed(1)}`)
    .join(" ");
  return (
    <svg
      viewBox={`0 0 ${W} ${H}`}
      role="img"
      aria-label={`推移。${ymd(series[0]?.[0])}から${ymd(series.at(-1)?.[0])}`}
      className="h-auto w-full text-xs"
    >
      <path d={path} fill="none" className="stroke-foreground" strokeWidth={1.5} />
      <text x={PAD.left - 6} y={y(max) + 4} textAnchor="end" className="fill-muted-foreground">
        {format(max)}
      </text>
      <text x={PAD.left - 6} y={y(min) + 4} textAnchor="end" className="fill-muted-foreground">
        {format(min)}
      </text>
      <text x={PAD.left} y={H - 6} className="fill-muted-foreground">
        {ymd(series[0]?.[0])}
      </text>
      <text x={W - PAD.right} y={H - 6} textAnchor="end" className="fill-muted-foreground">
        {ymd(series.at(-1)?.[0])}
      </text>
    </svg>
  );
}
