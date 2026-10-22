import { describe, expect, it } from "vitest";
import type { QuarterRow } from "./api";
import { groupByQuarter } from "./quarters";

function q(fy: string, quarter: QuarterRow["quarter"]): QuarterRow {
  return {
    fiscal_year_end: fy,
    quarter,
    period_end: fy,
    net_sales: 1,
    operating_income: 1,
    ordinary_income: 1,
    net_income: 1,
    sales_growth: null,
    operating_growth: null,
    ordinary_growth: null,
    net_growth: null,
  };
}

describe("groupByQuarter", () => {
  it("groups the same quarter across years, newest first", () => {
    const groups = groupByQuarter([
      q("2025-03-31", 1),
      q("2025-03-31", 2),
      q("2026-03-31", 1),
      q("2024-03-31", 1),
    ]);
    expect(groups.map((g) => g.quarter)).toEqual([1, 2]);
    expect(groups[0]?.rows.map((r) => r.fiscal_year_end)).toEqual([
      "2026-03-31",
      "2025-03-31",
      "2024-03-31",
    ]);
  });
});
