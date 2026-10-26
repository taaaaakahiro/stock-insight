import { ArrowLeft } from "lucide-react";
import { useEffect, useState } from "react";
import { DividendPolicy } from "@/components/dividend-policy";
import { DocPreview } from "@/components/doc-preview";
import { PortfolioToggle } from "@/components/portfolio-toggle";
import { ReitDebt } from "@/components/reit-debt";
import { ReitAssessment } from "@/components/reit-rag";
import { DividendAmount, Flags } from "@/components/results-table";
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
import { count, growth, jstDateTime, oku, pct, times, yen, yen1, ymd } from "@/lib/format";
import { groupByQuarter } from "@/lib/quarters";
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
  const metrics: { label: string; value: React.ReactNode }[] = [
    { label: metricLabel("price", kind), value: yen(s.price) },
    { label: metricLabel("dividend_ttm", kind), value: <DividendAmount r={s} /> },
    { label: metricLabel("dividend_yield", kind), value: pct(s.dividend_yield) },
    { label: metricLabel("yield_percentile", kind), value: pct(s.yield_percentile, 0) },
    ...(stock
      ? [
          { label: "PBR", value: times(s.pbr) },
          { label: "PER", value: times(s.per) },
          { label: "配当性向", value: pct(s.payout_ratio, 1) },
          { label: "DOE（実績）", value: pct(s.doe) },
          { label: "DOE（予想）", value: pct(s.doe_forecast) },
          { label: "連続増配", value: count(s.consecutive_increases) },
          { label: "営業利益率", value: pct(s.operating_margin, 1) },
          { label: "売上の伸び", value: growth(s.sales_growth) },
          { label: "営業利益の伸び", value: growth(s.op_growth) },
          { label: "直近四半期の営業利益の伸び", value: growth(s.op_growth_q) },
          { label: "直近4四半期の営業利益の伸び", value: growth(s.op_growth_ttm) },
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
          {stock && <DocPreview r={s} />}
        </div>
        {stock && <Flags r={s} />}
        <p className="text-xs text-muted-foreground">
          {metricLabel("price", kind)}は {ymd(s.price_date)} の終値、財務は決算短信（J-Quants）の
          {latestDisclosed ? ` ${ymd(latestDisclosed)} 開示分` : "最新値"}。指標の計算は{" "}
          {jstDateTime(s.fetched_at)}。
        </p>
      </header>

      {kind === "reit" && (
        <>
          <ReitAssessment code={s.code} />
          <ReitDebt code={s.code} />
        </>
      )}

      {stock && (
        <section aria-label="配当方針">
          <DividendPolicy r={s} />
        </section>
      )}

      {kind !== "etf" && (
        <section aria-labelledby="dps-chart">
          <h3 id="dps-chart" className="mb-2 text-sm">
            1株{divWord}は増えているか・増配の余地はあるか（本決算、最大5期）
          </h3>
          {d.annual.some((r) => r.dps != null) ? (
            <>
              <DpsChart rows={d.annual} divWord={divWord} />
              <PayoutChart rows={d.annual} divWord={divWord} />
            </>
          ) : (
            <p className="text-sm text-muted-foreground">
              1株{divWord}の開示がまだ取り込まれていません。
            </p>
          )}
        </section>
      )}

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
            四半期ごとの損益（単独、同じ四半期ごと・最大20四半期）
          </h3>
          <p className="mb-2 text-xs text-muted-foreground">
            決算短信の累計値の差分。前の四半期の開示がない期間は「—」。下の段は前年度の同じ四半期との比較で、前年が赤字・ゼロのときは「—」。
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

const QUARTER_METRICS: {
  label: string;
  value: (q: QuarterRow) => number | null;
  growth: (q: QuarterRow) => number | null;
}[] = [
  { label: "売上高", value: (q) => q.net_sales, growth: (q) => q.sales_growth },
  { label: "営業利益", value: (q) => q.operating_income, growth: (q) => q.operating_growth },
  { label: "経常利益", value: (q) => q.ordinary_income, growth: (q) => q.ordinary_growth },
  { label: "純利益", value: (q) => q.net_income, growth: (q) => q.net_growth },
];

function Growth({ ratio }: { ratio: number | null }) {
  return (
    <span
      className={cn(
        "block text-xs",
        ratio == null && "text-muted-foreground",
        ratio != null && ratio > 0 && "text-good",
        ratio != null && ratio < 0 && "text-bad",
      )}
    >
      {growth(ratio)}
    </span>
  );
}

/** 同じ四半期（1Q〜4Q）ごとに年度を並べ、各値の下に前年同期比を出す */
function QuarterTable({ rows }: { rows: QuarterRow[] }) {
  return (
    <div className="flex flex-col gap-4">
      {groupByQuarter(rows).map((g) => (
        <section key={g.quarter} aria-label={`Q${g.quarter}`}>
          <h4 className="mb-1 text-sm font-medium">Q{g.quarter}</h4>
          {/* Q1〜Q4 の表で列の位置をそろえる */}
          <Table className="min-w-[32rem] table-fixed">
            <TableHeader>
              <TableRow>
                <TableHead>年度</TableHead>
                {QUARTER_METRICS.map((m) => (
                  <TableHead key={m.label} className="text-right">
                    {m.label}
                  </TableHead>
                ))}
              </TableRow>
            </TableHeader>
            <TableBody>
              {g.rows.map((q) => (
                <TableRow key={q.fiscal_year_end}>
                  <TableCell>
                    {q.fiscal_year_end.slice(0, 4)}年度
                    <span className="block text-xs text-muted-foreground">
                      〜{ymd(q.period_end)}
                    </span>
                  </TableCell>
                  {QUARTER_METRICS.map((m) => (
                    <TableCell key={m.label} className="text-right tabular-nums">
                      <Money v={m.value(q)} />
                      <Growth ratio={m.growth(q)} />
                    </TableCell>
                  ))}
                </TableRow>
              ))}
            </TableBody>
          </Table>
        </section>
      ))}
    </div>
  );
}

/** "2026-03-31" → "2026/03"（決算期の表記） */
const fiscalMonth = (p: string | null | undefined) => (p ? p.slice(0, 7).replace("-", "/") : "—");

const BAR_H = 240;
const BAR_PAD = { top: 22, bottom: 40 };

/**
 * 1株配当の棒グラフ。後ろの枠は1株利益（EPS）で、枠と棒のすき間が増配の余地。
 * 前の期より減った期は赤で強調する（色だけに頼らず増減率も添える）
 */
function DpsChart({ rows, divWord }: { rows: AnnualRow[]; divWord: string }) {
  // API は新しい期が先頭。グラフは左から古い順
  const list = [...rows].reverse();
  const max = Math.max(...list.flatMap((r) => [r.dps ?? 0, r.eps ?? 0])) || 1;
  const slot = (W - PAD.left - PAD.right) / list.length;
  const barW = Math.min(56, slot * 0.5);
  const plotH = BAR_H - BAR_PAD.top - BAR_PAD.bottom;
  const base = BAR_H - BAR_PAD.bottom;
  const height = (v: number | null) => (v != null && v > 0 ? (plotH * v) / max : 0);
  const change = (i: number): number | null => {
    const cur = list[i]?.dps;
    const prev = list[i - 1]?.dps;
    return cur != null && prev != null && prev > 0 ? cur / prev - 1 : null;
  };
  const first = list.find((r) => r.dps != null);
  const latest = list.at(-1);
  const total =
    first?.dps && latest?.dps != null && first !== latest ? latest.dps / first.dps - 1 : null;
  return (
    <figure>
      <svg
        viewBox={`0 0 ${W} ${BAR_H}`}
        role="img"
        aria-label={`1株${divWord}と1株利益の推移。${list
          .map(
            (r) =>
              `${fiscalMonth(r.period_end)}期 ${divWord}${yen1(r.dps)}・利益${yen1(r.eps)}・性向${pct(r.payout_ratio, 0)}`,
          )
          .join("、")}`}
        className="h-auto w-full text-xs"
      >
        <line x1={PAD.left} x2={W - PAD.right} y1={base} y2={base} className="stroke-border" />
        <text x={PAD.left - 8} y={base + 15} textAnchor="end" className="fill-muted-foreground">
          期
        </text>
        <text x={PAD.left - 8} y={base + 31} textAnchor="end" className="fill-muted-foreground">
          前期比
        </text>
        {list.map((r, i) => {
          const cx = PAD.left + slot * (i + 0.5);
          const h = height(r.dps);
          const eh = height(r.eps);
          const g = change(i);
          const cut = g != null && g < 0;
          return (
            <g key={r.period_end ?? i}>
              {eh > 0 && (
                <rect
                  x={cx - barW / 2 - 4}
                  y={base - eh}
                  width={barW + 8}
                  height={eh}
                  rx={2}
                  className="fill-cheap/10 stroke-cheap"
                  strokeDasharray="3 2"
                />
              )}
              {r.dps != null && (
                <rect
                  x={cx - barW / 2}
                  y={base - h}
                  width={barW}
                  height={h}
                  rx={2}
                  className={cut ? "fill-expensive" : "fill-foreground/70"}
                />
              )}
              {eh > 0 && (
                // 利益と配当が近い期は、配当のラベルと重ならないよう上へずらす
                <text
                  x={cx}
                  y={Math.min(base - eh - 6, base - h - 22)}
                  textAnchor="middle"
                  className="fill-cheap"
                >
                  利益 {yen1(r.eps)}
                </text>
              )}
              <text x={cx} y={base - h - 6} textAnchor="middle" className="fill-foreground">
                {yen1(r.dps)}
              </text>
              <text x={cx} y={base + 15} textAnchor="middle" className="fill-muted-foreground">
                {fiscalMonth(r.period_end)}
              </text>
              <text
                x={cx}
                y={base + 31}
                textAnchor="middle"
                className={cut ? "fill-expensive" : "fill-muted-foreground"}
              >
                {growth(g)}
              </text>
            </g>
          );
        })}
      </svg>
      <figcaption className="mt-1 text-xs text-muted-foreground">
        棒は決算短信の年間{divWord}（実績）、緑の点線の枠は1株利益（EPS）。
        {`枠と棒のすき間が大きい（${divWord}性向が低い）ほど、利益から見て増配の余地がある。`}
        前の期より減った期は赤。
        {total != null && ` ${fiscalMonth(first?.period_end)}期から直近まで ${growth(total)}。`}
      </figcaption>
    </figure>
  );
}

/** 配当性向の目安の線。50% を超えると注意、60% を超えると配当が重い */
const PAYOUT_LINES = [
  { v: 0.5, cls: "warning" },
  { v: 0.6, cls: "expensive" },
] as const;
const payoutClass = (v: number | null, deficit: boolean) =>
  deficit || (v != null && v > 0.6)
    ? "fill-expensive"
    : v != null && v > 0.5
      ? "fill-warning"
      : "fill-foreground";
const PAYOUT_H = 170;
const PAYOUT_PAD = { top: 22, bottom: 26 };

/**
 * 配当性向（1株配当 ÷ 1株利益）の推移。DpsChart と同じ横位置に期を並べる。
 * 50%・60% に線を引き、60% 超は赤・50% 超は注意の色。利益がマイナスで配当を出した期は「赤字」と出す
 */
function PayoutChart({ rows, divWord }: { rows: AnnualRow[]; divWord: string }) {
  const list = [...rows].reverse();
  const top = Math.max(1, ...list.map((r) => r.payout_ratio ?? 0));
  const slot = (W - PAD.left - PAD.right) / list.length;
  const plotH = PAYOUT_H - PAYOUT_PAD.top - PAYOUT_PAD.bottom;
  const base = PAYOUT_H - PAYOUT_PAD.bottom;
  const y = (v: number) => base - (plotH * Math.min(v, top)) / top;
  const pts = list.map((r, i) => ({
    r,
    cx: PAD.left + slot * (i + 0.5),
    v: r.payout_ratio,
    deficit: r.payout_ratio == null && r.eps != null && r.eps <= 0 && (r.dps ?? 0) > 0,
  }));
  const path = pts
    .filter((p) => p.v != null)
    .map((p, i) => `${i ? "L" : "M"}${p.cx.toFixed(1)},${y(p.v ?? 0).toFixed(1)}`)
    .join(" ");
  const latest = list.at(-1);
  return (
    <figure className="mt-4">
      <figcaption className="mb-1 text-sm">{divWord}性向の推移</figcaption>
      <svg
        viewBox={`0 0 ${W} ${PAYOUT_H}`}
        role="img"
        aria-label={`${divWord}性向の推移。${pts
          .map((p) => `${fiscalMonth(p.r.period_end)}期 ${p.deficit ? "赤字" : pct(p.v, 0)}`)
          .join("、")}`}
        className="h-auto w-full text-xs"
      >
        {PAYOUT_LINES.map((l) => (
          <g key={l.v}>
            <line
              x1={PAD.left}
              x2={W - PAD.right}
              y1={y(l.v)}
              y2={y(l.v)}
              className={l.cls === "warning" ? "stroke-warning" : "stroke-expensive"}
              strokeDasharray="4 3"
            />
            <text
              x={PAD.left - 8}
              y={y(l.v) + 4}
              textAnchor="end"
              className={l.cls === "warning" ? "fill-warning" : "fill-expensive"}
            >
              {pct(l.v, 0)}
            </text>
          </g>
        ))}
        <text x={PAD.left - 8} y={base + 4} textAnchor="end" className="fill-muted-foreground">
          0%
        </text>
        <line x1={PAD.left} x2={W - PAD.right} y1={base} y2={base} className="stroke-border" />
        <path d={path} fill="none" className="stroke-foreground" strokeWidth={1.5} />
        {pts.map((p) => {
          const cls = payoutClass(p.v, p.deficit);
          return (
            <g key={p.r.period_end ?? p.cx}>
              {p.v != null && <circle cx={p.cx} cy={y(p.v)} r={4} className={cls} />}
              <text
                x={p.cx}
                y={p.v != null ? y(p.v) - 9 : base - 9}
                textAnchor="middle"
                className={cls}
              >
                {p.deficit ? "赤字" : pct(p.v, 0)}
              </text>
              <text x={p.cx} y={base + 16} textAnchor="middle" className="fill-muted-foreground">
                {fiscalMonth(p.r.period_end)}
              </text>
            </g>
          );
        })}
      </svg>
      <p className="mt-1 text-xs text-muted-foreground">
        {`利益のうち${divWord}に回した割合。低いほど増配の余地がある。50% を超えると注意（黄）、60% を超えると利益に対して${divWord}が重い（赤）。`}
        直近は {latest?.payout_ratio != null ? pct(latest.payout_ratio, 1) : "—"}。
      </p>
    </figure>
  );
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
