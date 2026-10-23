// API Worker との通信。トークンは Authorization ヘッダーで渡す（Cookie は使わない）

export type Me = { id: string; email: string; display_name: string; is_admin: boolean };

export type StockRow = {
  code: string;
  price: number;
  price_date: string;
  /** 利回りに使った1株（REIT は1口）配当。会社予想、予想がなければ実績。配当の開示がなければ null */
  dividend: number | null;
  dividend_basis: "forecast" | "actual" | null;
  dividend_yield: number | null;
  yield_percentile: number | null;
  pbr: number | null;
  per: number | null;
  payout_ratio: number | null;
  doe: number | null;
  consecutive_increases: number | null;
  operating_margin: number | null;
  no_loss_years: boolean | null;
  profit_uptrend: boolean | null;
  valuation: "cheap" | "neutral" | "expensive" | "unknown";
  fetched_at: number;
  /** 銘柄マスタ（JPX）から。まだ取り込んでいなければ null */
  name: string | null;
  kind: "stock" | "reit" | "etf";
  market: string | null;
  sector33: string | null;
  size_class: "large" | "mid" | "small" | "none" | null;
  /** 5年分の判定。期数が足りなければ null */
  dividend_every_year: boolean | null;
  no_quarterly_loss: boolean | null;
  is_value: boolean | null;
  data_years: number | null;
  /** 売上高・営業利益の前年同期比（直近の決算の期首からの累計どうし） */
  sales_growth: number | null;
  op_growth: number | null;
  /** 減配した、または会社予想が減配（比べる配当がなければ null） */
  dividend_cut: boolean | null;
  /** 有報・半期報告書（EDINET）の記述。書類をまだ取っていなければ null */
  progressive_dividend: boolean | null;
  no_dividend_cut: boolean | null;
  /** 有報・半期報告書に書かれた DOE の下限・目標（新しい書類の値。書かれていなければ null） */
  doe_floor: number | null;
  doe_goal: number | null;
  /** 会社予想の配当で見た DOE（予想がなければ null。実績は doe） */
  doe_forecast: number | null;
  /** 有報・半期報告書に書かれた配当性向の下限・目標（新しい書類の値。書かれていなければ null） */
  payout_floor: number | null;
  payout_goal: number | null;
  /** 会社予想の配当で見た配当性向（予想がなければ null。実績は payout_ratio） */
  payout_forecast: number | null;
  /** 会社の配当方針（DOE・配当性向の下限・目標）と今の配当の乖離。方針がないか満たしていれば null */
  policy_gap: PolicyGap | null;
  /** REIT の金利・稼働率（有報から。REIT 以外や未取り込みなら null）。比率は小数 */
  reit?: ReitListFields | null;
};

/** 方針どおりに払った場合、配当が何 % 増えるか（upside: 0.2 = 20% 増）。下限割れを目標未達より重く見る */
export type PolicyGap = {
  basis: "doe_floor" | "doe_goal" | "payout_floor" | "payout_goal";
  policy: number;
  now: number;
  upside: number;
};

export type ReitListFields = {
  sector: string | null;
  period_end: string | null;
  floating_ratio: number | null;
  unknown_ratio: number | null;
  reprice_ratio: number | null;
  reprice_max_ratio: number | null;
  avg_rate: number | null;
  occupancy: number | null;
};

export type ReitRateType = "fixed" | "swap_fixed" | "floating" | null;

/** /api/stocks/:code/reit。金額は円、面積は㎡、avg_rate・occupancy は % */
export type ReitDetail = {
  code: string;
  doc_id: string;
  submitted: string;
  period_end: string;
  sector: string | null;
  balance: { short_loans: number; long_loans: number; bonds: number };
  rate_mix: {
    fixed: number;
    swap_fixed: number;
    swap_amount_only: number;
    floating: number;
    unknown: number;
    bonds: number;
  };
  swap_notional: number | null;
  avg_rate: number | null;
  avg_years: number | null;
  reprice_1y: number;
  reprice_1y_max: number;
  property_count: number;
  leasable_m2: number | null;
  leased_m2: number | null;
  occupancy: number | null;
  checks: { loans: string; properties: string } | null;
  detail: {
    loans: {
      kind: "short" | "long" | null;
      lender: string;
      balance: number;
      rate: number | null;
      due: string | null;
      rate_type: ReitRateType;
    }[];
    ladder: { item: string; amounts: number[] }[];
    rate_notes: string[];
    properties: {
      id: string;
      name: string;
      leasable: number;
      leased: number | null;
      occupancy: number | null;
      rent: number | null;
      rent_note: string | null;
    }[];
  } | null;
  history: {
    period_end: string;
    loans_total: number;
    bonds: number;
    rate_floating: number;
    rate_unknown: number;
    swap_notional: number | null;
    avg_rate: number | null;
    property_count: number;
    leasable_m2: number | null;
    occupancy: number | null;
  }[];
};

/** EDINET の書類（有報・半期報告書の最新1通ずつ）から拾った配当方針の手がかり */
export type EdinetDoc = {
  doc_kind: "annual" | "semiannual";
  doc_id: string;
  submitted: string;
  period_end: string | null;
  progressive_dividend: boolean;
  no_dividend_cut: boolean;
  dividend_raise: boolean;
  doe_target: boolean;
  /** 配当性向の下限（なければ目標）（0.3 = 30%）。古い列で、中身は payout_floor ?? payout_goal */
  payout_target: number | null;
  /** DOE の下限・目標（0.035 = 3.5%）と、その数字を拾った文 */
  doe_floor: number | null;
  doe_goal: number | null;
  doe_phrase: string | null;
  /** 配当性向の下限・目標と、その数字を拾った文 */
  payout_floor: number | null;
  payout_goal: number | null;
  payout_phrase: string | null;
  /** 手がかりの言葉を含む文 */
  phrases: string[];
};

export type EdinetDocs = { code: string; docs: EdinetDoc[] };

export type StockPage = { rows: StockRow[]; total: number; page: number; page_size: number };

/** 詳細ページ（GET /api/stocks/:code） */
export type AnnualRow = {
  period_end: string | null;
  net_sales: number | null;
  operating_income: number | null;
  ordinary_income: number | null;
  net_income: number | null;
  equity: number | null;
  eps: number | null;
  bps: number | null;
  dps: number | null;
  operating_cf: number | null;
  payout_ratio: number | null;
  operating_margin: number | null;
};
export type QuarterRow = {
  fiscal_year_end: string;
  quarter: 1 | 2 | 3 | 4;
  period_end: string;
  net_sales: number | null;
  operating_income: number | null;
  ordinary_income: number | null;
  net_income: number | null;
  /** 前年度の同じ四半期からの伸び率（前年がない・赤字・ゼロなら null） */
  sales_growth: number | null;
  operating_growth: number | null;
  ordinary_growth: number | null;
  net_growth: number | null;
};
export type StockDetail = {
  stock: StockRow;
  /** 新しい期が先頭 */
  annual: AnnualRow[];
  /** 古い順 */
  quarters: QuarterRow[];
  /** [日付, 利回り]（古い順、週1点） */
  yield_series: [string, number][];
  /** [日付, 終値]（古い順、週1点） */
  price_series: [string, number][];
  latest_disclosed: string | null;
};

/** 売り時のサイン（domain::portfolio::SellSignal） */
export type SellSignal =
  | "high_payout"
  | "high_pbr"
  | "loss_dividend"
  | "expensive"
  | "dividend_cut"
  | "profit_downtrend"
  | "loss";

/** signals は売る判断に使う主のサイン（配当性向・PBR・赤字配当）、reference_signals は参考 */
export type Holding = StockRow & { signals: SellSignal[]; reference_signals: SellSignal[] };

/** codes は登録した全銘柄。rows は指標がある銘柄だけ（主のサインの多い順） */
export type Portfolio = { codes: string[]; rows: Holding[] };

export type Status = {
  latest_run: {
    run_id: string;
    started_at: number;
    finished_at: number | null;
    status: string;
    latest_price_date: string | null;
    latest_fin_date: string | null;
  } | null;
};

export class ApiError extends Error {
  constructor(
    readonly status: number,
    readonly code: string,
    message: string,
  ) {
    super(message);
  }
}

export function apiGet<T>(
  path: string,
  getToken: () => Promise<string | null>,
  signal?: AbortSignal,
): Promise<T> {
  return apiFetch<T>("GET", path, getToken, signal);
}

export async function apiFetch<T>(
  method: "GET" | "PUT" | "DELETE",
  path: string,
  getToken: () => Promise<string | null>,
  signal?: AbortSignal,
): Promise<T> {
  const token = await getToken();
  const res = await fetch(path, {
    method,
    headers: token ? { Authorization: `Bearer ${token}` } : {},
    signal,
  });
  if (!res.ok) {
    const body = (await res.json().catch(() => null)) as {
      error?: { code?: string; message?: string };
    } | null;
    throw new ApiError(
      res.status,
      body?.error?.code ?? "unknown",
      body?.error?.message ?? res.statusText,
    );
  }
  return (await res.json()) as T;
}

/** AIレコメンド（F7）の1銘柄。一覧と同じ指標に、順位と理由がつく */
export type Recommendation = StockRow & { rank: number; reason: string };

/** day は最新日の結果。まだ1日も作っていなければ null */
export type Recommendations = {
  day: {
    date: string;
    model: string;
    created_at: number;
    rows: Recommendation[];
  } | null;
};
