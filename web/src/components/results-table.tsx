import { ArrowDown, ArrowUp, ArrowUpDown } from "lucide-react";
import { Badge } from "@/components/ui/badge";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";
import { ValuationBadge } from "@/components/valuation-badge";
import type { StockRow } from "@/lib/api";
import { type Filters, type Kind, type MetricKey, metricLabel, metricsFor } from "@/lib/filters";
import { count, pct, times, yen } from "@/lib/format";
import { cn } from "@/lib/utils";

type Column = {
  key: MetricKey;
  render: (r: StockRow) => string;
};

// 数値の列。どのタブで出すかは filters.ts の METRICS（kinds）に合わせる
const RENDER: Record<MetricKey, (r: StockRow) => string> = {
  price: (r) => yen(r.price),
  dividend_yield: (r) => pct(r.dividend_yield),
  yield_percentile: (r) => pct(r.yield_percentile, 0),
  pbr: (r) => times(r.pbr),
  per: (r) => times(r.per),
  payout_ratio: (r) => pct(r.payout_ratio, 1),
  doe: (r) => pct(r.doe),
  consecutive_increases: (r) => count(r.consecutive_increases),
  operating_margin: (r) => pct(r.operating_margin, 1),
};

// 表の列の並び（株価を先頭に）
const ORDER: MetricKey[] = [
  "price",
  "dividend_yield",
  "yield_percentile",
  "pbr",
  "per",
  "payout_ratio",
  "doe",
  "consecutive_increases",
  "operating_margin",
];

function columnsFor(kind: Kind): Column[] {
  const keys = new Set(metricsFor(kind).map((m) => m.key));
  return ORDER.filter((k) => keys.has(k)).map((key) => ({ key, render: RENDER[key] }));
}

const SIZE_LABEL: Record<string, string> = { large: "大型", mid: "中型", small: "小型" };
const SIZE_HINT: Record<string, string> = {
  large: "TOPIX Core30・Large70（時価総額と流動性が特に高い上位100銘柄）",
  mid: "TOPIX Mid400（大型に次ぐ400銘柄）",
  small: "TOPIX Small（上記以外の TOPIX 構成銘柄）",
};

function SizeBadge({ size }: { size: StockRow["size_class"] }) {
  if (!size || !SIZE_LABEL[size]) return null;
  return (
    <Badge variant="secondary" title={`JPX の規模区分: ${SIZE_HINT[size]}`}>
      {SIZE_LABEL[size]}
    </Badge>
  );
}

/** 名称（なければコードだけ）と、コード・業種・規模 */
function NameCell({ r }: { r: StockRow }) {
  const sub = [r.code, r.kind === "stock" ? r.sector33 : null].filter(Boolean).join("・");
  return (
    <div className="flex min-w-0 flex-col">
      <span className="truncate font-medium" title={r.name ?? r.code}>
        {r.name ?? r.code}
      </span>
      <span className="flex items-center gap-1.5 text-xs text-muted-foreground">
        {r.name ? sub : r.kind === "stock" ? r.sector33 : null}
        <SizeBadge size={r.size_class} />
      </span>
    </div>
  );
}

type Props = {
  rows: StockRow[];
  kind: Kind;
  sort: Filters["sort"];
  order: Filters["order"];
  onSort: (key: Filters["sort"]) => void;
};

function SortIcon({ active, order }: { active: boolean; order: Filters["order"] }) {
  if (!active) return <ArrowUpDown aria-hidden className="size-3 opacity-40" />;
  return order === "asc" ? (
    <ArrowUp aria-hidden className="size-3" />
  ) : (
    <ArrowDown aria-hidden className="size-3" />
  );
}

function SortButton({
  label,
  active,
  order,
  numeric,
  onClick,
}: {
  label: string;
  active: boolean;
  order: Filters["order"];
  numeric: boolean;
  onClick: () => void;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      className={cn(
        "inline-flex items-center gap-1 hover:text-foreground",
        numeric && "flex-row-reverse",
      )}
    >
      {label}
      <SortIcon active={active} order={order} />
    </button>
  );
}

// PC は表、スマホ（md 未満）はカードで見せる
export function ResultsTable({ rows, kind, sort, order, onSort }: Props) {
  const columns = columnsFor(kind);
  const ariaSort = (key: Filters["sort"]) =>
    sort === key ? (order === "asc" ? "ascending" : "descending") : undefined;
  return (
    <>
      <div className="hidden md:block">
        <Table>
          <TableHeader>
            <TableRow>
              <TableHead aria-sort={ariaSort("code")}>
                <SortButton
                  label="銘柄"
                  active={sort === "code"}
                  order={order}
                  numeric={false}
                  onClick={() => onSort("code")}
                />
              </TableHead>
              {columns.map((c) => (
                <TableHead key={c.key} className="text-right" aria-sort={ariaSort(c.key)}>
                  <SortButton
                    label={metricLabel(c.key, kind)}
                    active={sort === c.key}
                    order={order}
                    numeric
                    onClick={() => onSort(c.key)}
                  />
                </TableHead>
              ))}
              <TableHead>判定</TableHead>
            </TableRow>
          </TableHeader>
          <TableBody>
            {rows.map((r) => (
              <TableRow key={r.code}>
                <TableCell className="max-w-64">
                  <NameCell r={r} />
                </TableCell>
                {columns.map((c) => (
                  <TableCell key={c.key} className="text-right tabular-nums">
                    {c.render(r)}
                  </TableCell>
                ))}
                <TableCell>
                  <ValuationBadge valuation={r.valuation} percentile={r.yield_percentile} />
                </TableCell>
              </TableRow>
            ))}
          </TableBody>
        </Table>
      </div>

      <ul className="flex flex-col gap-3 md:hidden">
        {rows.map((r) => (
          <li key={r.code} className="rounded-xl border p-4">
            <div className="mb-3 flex items-start justify-between gap-2">
              <NameCell r={r} />
              <ValuationBadge valuation={r.valuation} percentile={r.yield_percentile} />
            </div>
            <dl className="grid grid-cols-2 gap-x-4 gap-y-1 text-sm">
              {columns.map((c) => (
                <div key={c.key} className="flex justify-between gap-2">
                  <dt className="text-muted-foreground">{metricLabel(c.key, kind)}</dt>
                  <dd className="tabular-nums">{c.render(r)}</dd>
                </div>
              ))}
            </dl>
          </li>
        ))}
      </ul>
    </>
  );
}
