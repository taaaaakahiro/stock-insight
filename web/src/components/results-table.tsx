import { ArrowDown, ArrowUp, ArrowUpDown } from "lucide-react";
import { DocPreview } from "@/components/doc-preview";
import { EdinetPolicy } from "@/components/edinet-policy";
import { InfoTip } from "@/components/info-tip";
import { PortfolioToggle } from "@/components/portfolio-toggle";
import { Badge } from "@/components/ui/badge";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";
import type { StockRow } from "@/lib/api";
import {
  type Filters,
  type Kind,
  type MetricKey,
  metricLabel,
  metricsFor,
  SIZES,
  yieldPercentileTip,
} from "@/lib/filters";
import { count, growth, pct, times, yen } from "@/lib/format";
import { navigate } from "@/lib/router";
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
  sales_growth: (r) => growth(r.sales_growth),
  op_growth: (r) => growth(r.op_growth),
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
  "sales_growth",
  "op_growth",
];

function columnsFor(kind: Kind): Column[] {
  const keys = new Set(metricsFor(kind).map((m) => m.key));
  return ORDER.filter((k) => keys.has(k)).map((key) => ({ key, render: RENDER[key] }));
}

const SIZE_LABEL: Record<string, string> = { large: "大型", mid: "中型", small: "小型" };

function SizeBadge({ size }: { size: StockRow["size_class"] }) {
  if (!size || !SIZE_LABEL[size]) return null;
  return (
    <Badge variant="secondary" title={`JPX の規模区分: ${SIZES.find((s) => s.key === size)?.hint}`}>
      {SIZE_LABEL[size]}
    </Badge>
  );
}

/** 名称（なければコードだけ）と、コード・業種・規模 */
export function NameCell({ r, link = true }: { r: StockRow; link?: boolean }) {
  const sub = [r.code, r.kind === "stock" ? r.sector33 : null].filter(Boolean).join("・");
  return (
    <div className="flex min-w-0 flex-col">
      {link ? (
        <a
          href={`/stocks/${r.code}`}
          onClick={(e) => {
            // 普通のクリックだけ画面内で切り替える（新しいタブなどはブラウザに任せる）
            if (e.metaKey || e.ctrlKey || e.shiftKey || e.button !== 0) return;
            e.preventDefault();
            navigate(`/stocks/${r.code}`);
          }}
          className="truncate font-medium underline-offset-4 hover:underline"
          title={r.name ?? r.code}
        >
          {r.name ?? r.code}
        </a>
      ) : (
        <span className="truncate font-medium" title={r.name ?? r.code}>
          {r.name ?? r.code}
        </span>
      )}
      <span className="flex items-center gap-1.5 text-xs text-muted-foreground">
        {r.name ? sub : r.kind === "stock" ? r.sector33 : null}
        <SizeBadge size={r.size_class} />
      </span>
    </div>
  );
}

/** 一覧のバッジ。当てはまるものだけ出す（判定できない＝null は出さない）。色だけに意味を持たせない */
const BADGES: { key: keyof StockRow; label: string; hint: string; className: string }[] = [
  {
    key: "is_value",
    label: "バリュー",
    hint: "PBR 1倍以下 かつ PER 15倍以下",
    className: "border-cheap/40 bg-cheap/10 text-cheap",
  },
  {
    key: "dividend_every_year",
    label: "5年配当",
    hint: "直近5期すべてで配当がある",
    className: "",
  },
  {
    key: "no_loss_years",
    label: "通期黒字",
    hint: "直近5期の純利益と営業利益がすべてプラス",
    className: "",
  },
  {
    key: "no_quarterly_loss",
    label: "四半期黒字",
    hint: "直近20四半期の単独の損益がすべてプラス",
    className: "",
  },
];

export function Flags({ r }: { r: StockRow }) {
  const on = BADGES.filter((b) => r[b.key] === true);
  const policy = r.progressive_dividend === true || r.no_dividend_cut === true;
  if (on.length === 0 && !policy) {
    return (
      <span className="text-xs text-muted-foreground" title="判定できる財務データがまだ足りません">
        —
      </span>
    );
  }
  return (
    <span className="flex flex-wrap gap-1">
      {on.map((b) => (
        <Badge key={b.key} variant="outline" title={b.hint} className={b.className}>
          {b.label}
        </Badge>
      ))}
      {policy && <EdinetPolicy r={r} />}
    </span>
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
                  <div className="inline-flex items-center gap-1">
                    {c.key === "yield_percentile" && (
                      <InfoTip label={metricLabel(c.key, kind)} text={yieldPercentileTip(kind)} />
                    )}
                    <SortButton
                      label={metricLabel(c.key, kind)}
                      active={sort === c.key}
                      order={order}
                      numeric
                      onClick={() => onSort(c.key)}
                    />
                  </div>
                </TableHead>
              ))}
              {kind === "stock" && <TableHead>判定</TableHead>}
            </TableRow>
          </TableHeader>
          <TableBody>
            {rows.map((r) => (
              <TableRow key={r.code}>
                <TableCell className="max-w-72">
                  <div className="flex items-center gap-1">
                    <PortfolioToggle code={r.code} name={r.name ?? r.code} />
                    <NameCell r={r} />
                    {kind === "stock" && <DocPreview r={r} />}
                  </div>
                </TableCell>
                {columns.map((c) => (
                  <TableCell key={c.key} className="text-right tabular-nums">
                    {c.render(r)}
                  </TableCell>
                ))}
                {kind === "stock" && (
                  <TableCell>
                    <Flags r={r} />
                  </TableCell>
                )}
              </TableRow>
            ))}
          </TableBody>
        </Table>
      </div>

      <ul className="flex flex-col gap-3 md:hidden">
        {rows.map((r) => (
          <li key={r.code} className="rounded-xl border p-4">
            <div className="mb-3 flex items-start justify-between gap-2">
              <div className="flex min-w-0 items-center gap-1">
                <PortfolioToggle code={r.code} name={r.name ?? r.code} />
                <NameCell r={r} />
                {kind === "stock" && <DocPreview r={r} />}
              </div>
              {kind === "stock" && <Flags r={r} />}
            </div>
            <dl className="grid grid-cols-2 gap-x-4 gap-y-1 text-sm">
              {columns.map((c) => (
                <div key={c.key} className="flex justify-between gap-2">
                  <dt className="text-muted-foreground">
                    {metricLabel(c.key, kind)}
                    {c.key === "yield_percentile" && (
                      <span className="ml-0.5 inline-block align-[-2px]">
                        <InfoTip label={metricLabel(c.key, kind)} text={yieldPercentileTip(kind)} />
                      </span>
                    )}
                  </dt>
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
