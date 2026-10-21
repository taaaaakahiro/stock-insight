import { describe, expect, it } from "vitest";
import { DEFAULT_FILTERS, fromQuery, isInvalidInput, toApiNumber, toQuery } from "./filters";
import { jstDateTime, pct, times, yen, ymd } from "./format";

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
      "dividend_yield_min=0.035&pbr_max=1.2&valuation=cheap&no_loss_years=1&sort=pbr&order=asc&page=2",
    );
    expect(fromQuery(q)).toEqual(f);
  });

  it("omits defaults and ignores bad values", () => {
    expect(toQuery(DEFAULT_FILTERS).toString()).toBe("");
    const f = fromQuery(new URLSearchParams("sort=drop&order=up&page=-1&valuation=x&pbr_min=abc"));
    expect(f).toEqual({ ...DEFAULT_FILTERS, ranges: {} });
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
  });

  it("shows times in JST", () => {
    // 2026-10-21 09:30 UTC = 18:30 JST
    expect(jstDateTime(Date.UTC(2026, 9, 21, 9, 30) / 1000)).toBe("10/21 18:30");
  });
});
