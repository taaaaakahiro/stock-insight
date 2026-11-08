export const profiles = [
  ["total", "総合"],
  ["distribution", "増配・分配金持続性"],
  ["profitability", "収益成長"],
  ["finance", "財務・金利安全性"],
  ["nav", "含み益・NAV"],
  ["valuation", "割安度"],
] as const;

export const sectors = [
  ["office", "オフィス"],
  ["logistics", "物流"],
  ["residential", "住宅"],
  ["hotel", "ホテル"],
  ["retail", "商業"],
  ["healthcare", "ヘルスケア"],
  ["diversified", "総合・複合"],
  ["unknown", "用途未分類"],
] as const;

export function sectorLabel(key: string) {
  return sectors.find(([sector]) => sector === key)?.[1] ?? key;
}

export const reasons: Record<string, string> = {
  low_coverage: "データ充足率が80%未満",
  missing_or_stale_price: "価格が未取得または古い",
  incomplete_or_stale_distribution: "確定分配の期数が不足、または決算期が古い",
  missing_or_stale_disclosure: "確認できる財務開示が未取得または古い",
};

export type Profile = (typeof profiles)[number][0];

export type Run = {
  run_id: string;
  as_of_date: string;
  score_version: string;
  universe_count: number;
  universe_definition: string;
  completed_at: number;
  source_watermark: string;
  tie_break: string;
};

export type Evidence = {
  metric_key: string;
  metric_value: number | null;
  unit: string;
  period_start: string | null;
  period_end: string;
  as_of_date: string;
  source_url: string;
  source_document_id: string | null;
  source_page: number | null;
  disclosure_published_at: string;
  data_kind: string;
  quality_flags: string[];
  normalization_flags?: string[];
};

export type MetricValue = { value: number; valid: boolean; sources: Evidence[] };
type Driver = { metric_key: string; label: string; percentile: number };
type Forecast = {
  status: "annual" | "partial" | "missing";
  dpu: number | null;
  period_count: number;
};
export type ReitSummary = {
  ticker: string;
  name: string;
  sector: string;
  price_date: string | null;
  actual_yield: number | null;
  forecast_yield: number | null;
  forecast: Forecast;
  total_score: number | null;
  coverage_pct: number;
  rank_eligible: boolean;
  provisional_reasons: string[];
  risks: { metric_key: string; message: string }[];
  positives: Driver[];
  weaknesses: Driver[];
};
export type RankedReit = ReitSummary & {
  rank: number | null;
  sector_rank: number | null;
  profile_score: number | null;
};

export type Component = {
  metric_key: string;
  label: string;
  formula: string;
  unit: string;
  metric_value: number | null;
  percentile: number | null;
  weighted_points: number | null;
  weight: number;
  status: string;
  benchmark_size: number;
  source_ref: Evidence[] | null;
};

export type HistoryPoint = {
  period_end: string;
  actual_dpu: number | null;
  normalized_dpu: number | null;
  ffo_per_unit: number | null;
  nav_per_unit?: number | null;
};

export type ScoredReit = {
  summary: ReitSummary;
  detail: {
    categories: {
      key: string;
      label: string;
      points: number;
      available: number;
      maximum: number;
    }[];
    components: Component[];
    values: Record<string, MetricValue | undefined>;
    history: HistoryPoint[];
  };
};
export type ScoreResponse<T> = { status: "ready" | "not_generated"; run: Run | null; items: T[] };
export type RankingsResponse = ScoreResponse<RankedReit> & {
  profile_label: string;
  profile: Profile;
};

export const comparisonMetrics = [
  ["actual_dpu", "確定年間DPU", "JPY/unit"],
  ["dpu_cagr", "確定年間DPU 5年CAGR", "ratio"],
  ["normalized_dpu", "正常化年間DPU", "JPY/unit"],
  ["normalized_dpu_cagr", "正常化年間DPU 5年CAGR", "ratio"],
  ["same_noi", "同一物件の年間NOI", "JPY"],
  ["same_noi_cagr", "同一物件NOI 3年CAGR", "ratio"],
  ["ffo_per_unit", "年間1口FFO", "JPY/unit"],
  ["unrealized_equity", "含み益 / 純資産", "ratio"],
  ["p_nav", "NAV倍率", "times"],
  ["ltv", "比較可能LTV", "ratio"],
  ["floating_ratio", "実質変動金利比率", "ratio"],
  ["interest_noi", "支払利息 / NOI", "ratio"],
  ["stress_100", "金利+1.0%ptの負担率", "ratio"],
] as const;

export function scoreNumber(value: number | null | undefined) {
  return value == null ? "—" : value.toFixed(1);
}

export function metricNumber(value: number | null | undefined, unit: string): string {
  if (value == null) return "—";
  if (unit === "ratio") return `${(value * 100).toFixed(2)}%`;
  if (unit === "times") return `${value.toFixed(2)}倍`;
  if (unit === "years") return `${value.toFixed(1)}年`;
  if (unit === "JPY" || unit === "JPY/unit") {
    return `${value.toLocaleString("ja-JP", { maximumFractionDigits: 1 })}円${unit === "JPY/unit" ? "/口" : ""}`;
  }
  return value.toLocaleString("ja-JP", { maximumFractionDigits: 2 });
}
