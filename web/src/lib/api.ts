// API Worker との通信。トークンは Authorization ヘッダーで渡す（Cookie は使わない）

export type Me = { id: string; email: string; display_name: string; is_admin: boolean };

export type StockRow = {
  code: string;
  price: number;
  price_date: string;
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
};

export type StockPage = { rows: StockRow[]; total: number; page: number; page_size: number };

/** 売り時のサイン（domain::portfolio::SellSignal） */
export type SellSignal = "high_payout" | "expensive" | "no_increase" | "profit_downtrend" | "loss";

export type Holding = StockRow & { signals: SellSignal[] };

/** codes は登録した全銘柄。rows は指標がある銘柄だけ（サインの多い順） */
export type Portfolio = { codes: string[]; rows: Holding[] };

export type Status = {
  latest_run: {
    run_id: string;
    started_at: number;
    finished_at: number | null;
    status: string;
    latest_price_date: string | null;
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
