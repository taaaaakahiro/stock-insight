import { ArrowDown, ArrowUp, ArrowUpDown } from "lucide-react";
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
import type { Filters, MetricKey } from "@/lib/filters";
import { count, pct, times, yen } from "@/lib/format";
import { cn } from "@/lib/utils";

type Column = {
  key: MetricKey | "code";
  label: string;
  render: (r: StockRow) => string;
  /** 数値は右寄せ */
  numeric: boolean;
};

const COLUMNS: Column[] = [
  { key: "code", label: "コード", render: (r) => r.code, numeric: false },
  { key: "price", label: "株価", render: (r) => yen(r.price), numeric: true },
  {
    key: "dividend_yield",
    label: "配当利回り",
    render: (r) => pct(r.dividend_yield),
    numeric: true,
  },
  {
    key: "yield_percentile",
    label: "過去10年の位置",
    render: (r) => pct(r.yield_percentile, 0),
    numeric: true,
  },
  { key: "pbr", label: "PBR", render: (r) => times(r.pbr), numeric: true },
  { key: "per", label: "PER", render: (r) => times(r.per), numeric: true },
  { key: "payout_ratio", label: "配当性向", render: (r) => pct(r.payout_ratio, 1), numeric: true },
  { key: "doe", label: "DOE", render: (r) => pct(r.doe), numeric: true },
  {
    key: "consecutive_increases",
    label: "連続増配",
    render: (r) => count(r.consecutive_increases),
    numeric: true,
  },
  {
    key: "operating_margin",
    label: "営業利益率",
    render: (r) => pct(r.operating_margin, 1),
    numeric: true,
  },
];

type Props = {
  rows: StockRow[];
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

// PC は表、スマホ（md 未満）はカードで見せる
export function ResultsTable({ rows, sort, order, onSort }: Props) {
  return (
    <>
      <div className="hidden md:block">
        <Table>
          <TableHeader>
            <TableRow>
              {COLUMNS.map((c) => (
                <TableHead
                  key={c.key}
                  className={cn(c.numeric && "text-right")}
                  aria-sort={
                    sort === c.key ? (order === "asc" ? "ascending" : "descending") : undefined
                  }
                >
                  <button
                    type="button"
                    onClick={() => onSort(c.key)}
                    className={cn(
                      "inline-flex items-center gap-1 hover:text-foreground",
                      c.numeric && "flex-row-reverse",
                    )}
                  >
                    {c.label}
                    <SortIcon active={sort === c.key} order={order} />
                  </button>
                </TableHead>
              ))}
              <TableHead>判定</TableHead>
            </TableRow>
          </TableHeader>
          <TableBody>
            {rows.map((r) => (
              <TableRow key={r.code}>
                {COLUMNS.map((c) => (
                  <TableCell
                    key={c.key}
                    className={cn(c.numeric ? "text-right tabular-nums" : "font-medium")}
                  >
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
            <div className="mb-3 flex items-center justify-between">
              <span className="font-semibold">{r.code}</span>
              <ValuationBadge valuation={r.valuation} percentile={r.yield_percentile} />
            </div>
            <dl className="grid grid-cols-2 gap-x-4 gap-y-1 text-sm">
              {COLUMNS.slice(1).map((c) => (
                <div key={c.key} className="flex justify-between gap-2">
                  <dt className="text-muted-foreground">{c.label}</dt>
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
