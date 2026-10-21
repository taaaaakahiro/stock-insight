import { describe, expect, it } from "vitest";
import {
  DEFAULT_FILTERS,
  fromQuery,
  isInvalidInput,
  metricLabel,
  switchKind,
  toApiNumber,
  toQuery,
} from "./filters";
import { jstDateTime, oku, pct, times, yen, yen1, ymd } from "./format";

describe("filters", () => {
  it("converts percent inputs to ratios and back", () => {
    expect(toApiNumber("3.5", "pct")).toBe("0.035");
    expect(toApiNumber("1,000", "yen")).toBe("1000");
    expect(toApiNumber(" ", "pct")).toBeUndefined();
    expect(toApiNumber("abc", "times")).toBeUndefined();
  });

  it("round-trips through the query string", () => {
    const f = {
      ...DEFAULT_FILTERS,
      ranges: { dividend_yield_min: "3.5", pbr_max: "1.2" },
      valuation: "cheap" as const,
      noLossYears: true,
      sort: "pbr" as const,
      order: "asc" as const,
      page: 2,
    };
    const q = toQuery(f);
    expect(q.toString()).toBe(
      "kind=stock&dividend_yield_min=0.035&pbr_max=1.2&valuation=cheap&no_loss_years=1&sort=pbr&order=asc&page=2",
    );
    expect(fromQuery(q)).toEqual(f);
  });

  it("omits defaults and ignores bad values", () => {
    expect(toQuery(DEFAULT_FILTERS).toString()).toBe("kind=stock");
    const f = fromQuery(new URLSearchParams("sort=drop&order=up&page=-1&valuation=x&pbr_min=abc"));
    expect(f).toEqual({ ...DEFAULT_FILTERS, ranges: {} });
  });

  it("round-trips search and master filters", () => {
    const f = {
      ...DEFAULT_FILTERS,
      query: "トヨタ",
      sectors: ["銀行業", "保険業"],
      markets: ["プライム"],
      sizes: ["large"],
      isValue: true,
      noQuarterlyLoss: true,
    };
    const q = toQuery(f);
    expect(q.toString()).toBe(
      "kind=stock&q=%E3%83%88%E3%83%A8%E3%82%BF&sector=%E9%8A%80%E8%A1%8C%E6%A5%AD%2C%E4%BF%9D%E9%99%BA%E6%A5%AD&market=%E3%83%97%E3%83%A9%E3%82%A4%E3%83%A0&size=large&is_value=1&no_quarterly_loss=1",
    );
    expect(fromQuery(q)).toEqual(f);
    // 知らない業種・規模は捨てる。REIT では業種などを持たない
    expect(fromQuery(new URLSearchParams("sector=銀行業,宇宙業&size=huge,mid")).sectors).toEqual([
      "銀行業",
    ]);
    expect(fromQuery(new URLSearchParams("kind=reit&sector=銀行業&is_value=1")).sectors).toEqual(
      [],
    );
    expect(switchKind(f, "etf").isValue).toBe(false);
  });

  it("drops stock-only conditions when switching to REIT", () => {
    const f = {
      ...DEFAULT_FILTERS,
      ranges: { dividend_yield_min: "4", pbr_max: "1" },
      noLossYears: true,
      sort: "pbr" as const,
      order: "asc" as const,
      page: 3,
    };
    const r = switchKind(f, "reit");
    expect(r).toEqual({
      ...DEFAULT_FILTERS,
      kind: "reit",
      ranges: { dividend_yield_min: "4" },
    });
    expect(toQuery(r).toString()).toBe("kind=reit&dividend_yield_min=0.04");
    // REIT の URL に株式だけの条件が混ざっていても無視する
    expect(fromQuery(new URLSearchParams("kind=reit&pbr_max=1&sort=pbr")).ranges).toEqual({});
    expect(metricLabel("dividend_yield", "reit")).toBe("分配金利回り");
    expect(metricLabel("yield_percentile", "etf")).toBe("利回りの過去5年比");
    expect(metricLabel("price", "etf")).toBe("価格");
  });

  it("flags inputs that are not numbers", () => {
    expect(isInvalidInput("1.5")).toBe(false);
    expect(isInvalidInput("")).toBe(false);
    expect(isInvalidInput("1.5x")).toBe(true);
  });
});

describe("format", () => {
  it("formats numbers for display", () => {
    expect(pct(0.0402)).toBe("4.02%");
    expect(pct(null)).toBe("—");
    expect(times(0.5666)).toBe("0.57倍");
    expect(yen(176.1999969482422)).toBe("176.2円");
    expect(yen(3431)).toBe("3,431円");
    expect(ymd("2026-10-21")).toBe("2026/10/21");
    expect(oku(1234567890)).toBe("12.3億円");
    expect(oku(-250000000)).toBe("-2.5億円");
    expect(oku(45000000)).toBe("4,500万円");
    expect(oku(49000000000000)).toBe("490,000億円");
    expect(yen1(62)).toBe("62.0円");
  });

  it("shows times in JST", () => {
    // 2026-10-21 09:30 UTC = 18:30 JST
    expect(jstDateTime(Date.UTC(2026, 9, 21, 9, 30) / 1000)).toBe("10/21 18:30");
  });
});
