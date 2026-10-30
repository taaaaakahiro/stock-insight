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
  metricTip,
  reitSectorLabel,
  SIZES,
} from "@/lib/filters";
import { count, growth, pct, points, times, yen } from "@/lib/format";
import { navigate } from "@/lib/router";
import { cn } from "@/lib/utils";

type Column = {
  key: MetricKey;
  render: (r: StockRow) => React.ReactNode;
};

/** 1株配当と、それが会社予想か実績か（利回りはこの配当で出している） */
export function DividendAmount({ r }: { r: StockRow }) {
  if (r.dividend == null) return yen(null);
  return (
    <>
      {yen(r.dividend)}
      {r.kind === "stock" && (r.dividend_special == null || r.dividend_commemorative == null) && (
        <span
          className="ml-1 text-[10px] text-muted-foreground"
          title="特別・記念配当の内訳は未確認です。表示額をそのまま利回り計算に使用しています。"
        >
          内訳未確認
        </span>
      )}
      {r.kind === "stock" &&
        r.dividend_basis !== "forecast" &&
        ((r.dividend_special ?? 0) > 0 || (r.dividend_commemorative ?? 0) > 0) && (
          <span
            className="ml-1 text-[10px] text-muted-foreground"
            title={`特別配当 ${yen(r.dividend_special ?? null)}・記念配当 ${yen(r.dividend_commemorative ?? null)}を除外。年間合計 ${yen(r.dividend_total ?? null)}`}
          >
            臨時配当除外
          </span>
        )}
      {r.dividend_basis && (
        <span className="ml-1 text-[10px] text-muted-foreground">
          {r.dividend_basis === "forecast" ? "予想" : "実績"}
        </span>
      )}
    </>
  );
}

// 数値の列。どのタブで出すかは filters.ts の METRICS（kinds）に合わせる
const RENDER: Record<MetricKey, (r: StockRow) => React.ReactNode> = {
  price: (r) => yen(r.price),
  dividend_ttm: (r) => <DividendAmount r={r} />,
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
  op_growth_q: (r) => growth(r.op_growth_q),
  op_growth_ttm: (r) => growth(r.op_growth_ttm),
  floating_ratio: (r) => pct(r.reit?.floating_ratio, 1),
  unknown_ratio: (r) => pct(r.reit?.unknown_ratio, 1),
  reprice_ratio: (r) => pct(r.reit?.reprice_ratio, 1),
  reprice_max_ratio: (r) => pct(r.reit?.reprice_max_ratio, 1),
  avg_rate: (r) => pct(r.reit?.avg_rate, 2),
  occupancy: (r) => pct(r.reit?.occupancy, 1),
  doe_floor: (r) => pct(r.doe_floor, 1),
  doe_margin: (r) => <DoeMargin r={r} />,
};

/** 比べる DOE: 利回りが会社予想の配当なら予想の DOE、実績なら実績の DOE（API の doe_margin と同じ） */
export const doeNow = (r: StockRow) => (r.dividend_basis === "forecast" ? r.doe_forecast : r.doe);

/** DOE の余裕（今の DOE − 下限）。下限を割っていれば赤 */
export function DoeMargin({ r }: { r: StockRow }) {
  const now = doeNow(r);
  if (now == null || r.doe_floor == null) return points(null);
  const diff = now - r.doe_floor;
  return (
    <span className={cn(diff < 0 ? "text-expensive" : "text-cheap")}>
      {points(diff)}
      <span className="ml-1 text-[10px] text-muted-foreground">
        {r.doe_forecast != null ? "予想" : "実績"}
      </span>
    </span>
  );
}

/** 利回りの下に、その元になった1株配当を小さく添える（配当の列は別に持たない） */
function YieldCell({ r }: { r: StockRow }) {
  return (
    <>
      {pct(r.dividend_yield)}
      {r.dividend != null && (
        <span className="block text-xs text-muted-foreground">
          <DividendAmount r={r} />
        </span>
      )}
    </>
  );
}

/** 1年内の金利見直しを「少なく見た値〜多く見た値」で1つの列に */
function RepriceCell({ r }: { r: StockRow }) {
  const min = r.reit?.reprice_ratio;
  const max = r.reit?.reprice_max_ratio;
  return (
    <>
      {pct(min, 1)}
      {max != null && max !== min && (
        <span className="block text-xs text-muted-foreground">最大 {pct(max, 1)}</span>
      )}
    </>
  );
}

// 一覧の列の並び（株価を先頭に）。買うかを比べるのに要る値だけにし、残りは銘柄のページで見せる
// （DOE・DOE下限・DOEの余裕は配当方針、営業利益率・伸びは主要な指標、金利の内訳・平均利率は REIT の借入）。
// 列にない指標も条件では絞り込める
const ORDER: MetricKey[] = [
  "price",
  "dividend_yield",
  "yield_percentile",
  "pbr",
  "per",
  "payout_ratio",
  "consecutive_increases",
  "floating_ratio",
  "reprice_ratio",
  "occupancy",
];

// 一覧では、隣の値をまとめて1つのセルに出す列
const LIST_RENDER: Partial<Record<MetricKey, (r: StockRow) => React.ReactNode>> = {
  dividend_yield: (r) => <YieldCell r={r} />,
  reprice_ratio: (r) => <RepriceCell r={r} />,
};

// 一覧の見出しだけ短くする（説明の InfoTip には元の名前を出す）
const LIST_LABEL: Partial<Record<MetricKey, string>> = { yield_percentile: "過去5年比" };

function columnsFor(kind: Kind): Column[] {
  const keys = new Set(metricsFor(kind).map((m) => m.key));
  return ORDER.filter((k) => keys.has(k)).map((key) => ({
    key,
    render: LIST_RENDER[key] ?? RENDER[key],
  }));
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

/** 名称（なければコードだけ）と、コード・業種・規模。wrap は折り返して全部出す（スマホのカード。表は1行で省略） */
export function NameCell({
  r,
  link = true,
  wrap = false,
}: {
  r: StockRow;
  link?: boolean;
  wrap?: boolean;
}) {
  const sector =
    r.kind === "stock" ? r.sector33 : r.kind === "reit" ? reitSectorLabel(r.reit?.sector) : null;
  const sub = [r.code, sector].filter(Boolean).join("・");
  const nameClass = wrap ? "break-words" : "truncate";
  return (
    <div className="flex min-w-0 flex-1 flex-col">
      {link ? (
        <a
          href={`/stocks/${r.code}`}
          onClick={(e) => {
            // 普通のクリックだけ画面内で切り替える（新しいタブなどはブラウザに任せる）
            if (e.metaKey || e.ctrlKey || e.shiftKey || e.button !== 0) return;
            e.preventDefault();
            navigate(`/stocks/${r.code}`);
          }}
          className={cn(nameClass, "font-medium underline-offset-4 hover:underline")}
          title={r.name ?? r.code}
        >
          {r.name ?? r.code}
        </a>
      ) : (
        <span className={cn(nameClass, "font-medium")} title={r.name ?? r.code}>
          {r.name ?? r.code}
        </span>
      )}
      <span className="flex flex-wrap items-center gap-x-1.5 gap-y-0.5 text-xs text-muted-foreground">
        {r.name ? sub : sector}
        <SizeBadge size={r.size_class} />
      </span>
    </div>
  );
}

/** 一覧のバッジ。当てはまるものだけ出す（判定できない＝null は出さない）。色だけに意味を持たせない */
const BADGES: { key: keyof StockRow; label: string; hint: string; className: string }[] = [
  {
    key: "dividend_cut",
    label: "減配",
    hint: "直近の配当が前期より減った、または会社予想が直近の配当を下回る（据え置きは含めない）",
    className: "border-expensive/40 bg-expensive/10 text-expensive",
  },
  {
    key: "profit_one_off",
    label: "一過性?",
    hint: "直近の四半期の利益が一過性の疑い: 純利益の伸びが営業利益の伸びを30pt以上上回る（特別利益）、または直近の四半期だけ増益で直近4四半期の合計は減益",
    className: "border-expensive/40 bg-expensive/10 text-expensive",
  },
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

const policyOf = (r: StockRow) => r.progressive_dividend === true || r.no_dividend_cut === true;

/** 出すバッジがあるか（スマホのカードは、なければ行ごと出さない） */
export const hasFlags = (r: StockRow) => BADGES.some((b) => r[b.key] === true) || policyOf(r);

export function Flags({ r }: { r: StockRow }) {
  const on = BADGES.filter((b) => r[b.key] === true);
  const policy = policyOf(r);
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
                    {metricTip(c.key, kind) && (
                      <InfoTip
                        label={metricLabel(c.key, kind)}
                        text={metricTip(c.key, kind) ?? ""}
                      />
                    )}
                    <SortButton
                      label={LIST_LABEL[c.key] ?? metricLabel(c.key, kind)}
                      active={sort === c.key}
                      order={order}
                      numeric
                      onClick={() => onSort(c.key)}
                    />
                  </div>
                </TableHead>
              ))}
              {kind === "stock" && <TableHead className="min-w-36">判定</TableHead>}
            </TableRow>
          </TableHeader>
          <TableBody>
            {rows.map((r) => (
              <TableRow key={r.code}>
                <TableCell className="max-w-60">
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
          <li key={r.code} className="rounded-xl border p-3">
            {/* 名前に幅を全部使わせ、バッジは下の行へ（横に並べると名前が数文字で切れる） */}
            <div className="flex items-start gap-1">
              <PortfolioToggle code={r.code} name={r.name ?? r.code} />
              <NameCell r={r} wrap />
              {kind === "stock" && <DocPreview r={r} />}
            </div>
            {kind === "stock" && hasFlags(r) && (
              <div className="mt-2">
                <Flags r={r} />
              </div>
            )}
            <dl className="mt-3 grid grid-cols-2 gap-x-4 gap-y-1.5 border-t pt-3 text-sm">
              {columns.map((c) => (
                <div key={c.key} className="flex justify-between gap-2">
                  <dt className="text-muted-foreground">
                    {LIST_LABEL[c.key] ?? metricLabel(c.key, kind)}
                    {metricTip(c.key, kind) && (
                      <span className="ml-0.5 inline-block align-[-2px]">
                        <InfoTip
                          label={metricLabel(c.key, kind)}
                          text={metricTip(c.key, kind) ?? ""}
                        />
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
