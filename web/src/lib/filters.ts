// スクリーニングの絞り込み条件。画面の URL と API のクエリは同じキー・同じ単位（比率は小数）にする。
// 入力欄は人が読む単位（% や 倍）で扱い、ここで変換する。

export type Unit = "pct" | "times" | "yen" | "years";

export type MetricKey =
  | "dividend_yield"
  | "yield_percentile"
  | "pbr"
  | "per"
  | "payout_ratio"
  | "doe"
  | "consecutive_increases"
  | "operating_margin"
  | "price";

export const METRICS: { key: MetricKey; label: string; unit: Unit }[] = [
  { key: "dividend_yield", label: "配当利回り", unit: "pct" },
  { key: "yield_percentile", label: "利回りの過去10年の位置", unit: "pct" },
  { key: "pbr", label: "PBR", unit: "times" },
  { key: "per", label: "PER", unit: "times" },
  { key: "payout_ratio", label: "配当性向", unit: "pct" },
  { key: "doe", label: "DOE", unit: "pct" },
  { key: "consecutive_increases", label: "連続増配", unit: "years" },
  { key: "operating_margin", label: "営業利益率", unit: "pct" },
  { key: "price", label: "株価", unit: "yen" },
];

export const UNIT_SUFFIX: Record<Unit, string> = { pct: "%", times: "倍", yen: "円", years: "年" };

export type Valuation = "" | "cheap" | "neutral" | "expensive";

export type Filters = {
  /** 入力欄の文字列（人が読む単位）。キーは `${metric}_min` / `${metric}_max` */
  ranges: Partial<Record<`${MetricKey}_${"min" | "max"}`, string>>;
  valuation: Valuation;
  noLossYears: boolean;
  profitUptrend: boolean;
  sort: MetricKey | "code";
  order: "asc" | "desc";
  page: number;
};

export const DEFAULT_FILTERS: Filters = {
  ranges: {},
  valuation: "",
  noLossYears: false,
  profitUptrend: false,
  sort: "dividend_yield",
  order: "desc",
  page: 1,
};

const unitOf = (key: MetricKey) => METRICS.find((m) => m.key === key)?.unit ?? "times";

/** 入力欄の値 → API の値（% は 100 で割る）。数でなければ undefined */
export function toApiNumber(input: string, unit: Unit): string | undefined {
  const t = input.trim().replace(/[,，]/g, "");
  if (t === "") return undefined;
  const n = Number(t);
  if (!Number.isFinite(n)) return undefined;
  return String(unit === "pct" ? Math.round(n * 1e6) / 1e8 : n);
}

/** API の値 → 入力欄の値 */
export function fromApiNumber(value: string, unit: Unit): string {
  const n = Number(value);
  if (!Number.isFinite(n)) return "";
  return String(unit === "pct" ? Math.round(n * 1e8) / 1e6 : n);
}

/** 条件 → クエリ文字列（既定値は省く） */
export function toQuery(f: Filters): URLSearchParams {
  const q = new URLSearchParams();
  for (const m of METRICS) {
    for (const b of ["min", "max"] as const) {
      const v = f.ranges[`${m.key}_${b}`];
      const api = v === undefined ? undefined : toApiNumber(v, m.unit);
      if (api !== undefined) q.set(`${m.key}_${b}`, api);
    }
  }
  if (f.valuation) q.set("valuation", f.valuation);
  if (f.noLossYears) q.set("no_loss_years", "1");
  if (f.profitUptrend) q.set("profit_uptrend", "1");
  if (f.sort !== DEFAULT_FILTERS.sort) q.set("sort", f.sort);
  if (f.order !== DEFAULT_FILTERS.order) q.set("order", f.order);
  if (f.page > 1) q.set("page", String(f.page));
  return q;
}

/** クエリ文字列 → 条件。おかしな値は既定値に戻す */
export function fromQuery(q: URLSearchParams): Filters {
  const f: Filters = { ...DEFAULT_FILTERS, ranges: {} };
  for (const m of METRICS) {
    for (const b of ["min", "max"] as const) {
      const v = q.get(`${m.key}_${b}`);
      if (v !== null) {
        const input = fromApiNumber(v, unitOf(m.key));
        if (input !== "") f.ranges[`${m.key}_${b}`] = input;
      }
    }
  }
  const val = q.get("valuation");
  if (val === "cheap" || val === "neutral" || val === "expensive") f.valuation = val;
  f.noLossYears = q.get("no_loss_years") === "1";
  f.profitUptrend = q.get("profit_uptrend") === "1";
  const sort = q.get("sort");
  if (sort === "code" || METRICS.some((m) => m.key === sort)) f.sort = sort as Filters["sort"];
  const order = q.get("order");
  if (order === "asc" || order === "desc") f.order = order;
  const page = Number(q.get("page"));
  if (Number.isInteger(page) && page >= 1 && page <= 1000) f.page = page;
  return f;
}

/** 入力欄の値が数として読めないか（赤枠を出す） */
export function isInvalidInput(input: string | undefined): boolean {
  return input !== undefined && input.trim() !== "" && toApiNumber(input, "times") === undefined;
}
