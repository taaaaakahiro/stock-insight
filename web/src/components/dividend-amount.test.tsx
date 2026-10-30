import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import type { StockRow } from "@/lib/api";
import { metricLabel } from "@/lib/filters";
import { DividendAmount } from "./results-table";

const row = (fields: Partial<StockRow>) => ({ kind: "stock", ...fields }) as StockRow;
describe("ordinary dividend display", () => {
  it("keeps unverified actual dividends visible with a label", () => {
    const html = renderToStaticMarkup(
      <DividendAmount r={row({ dividend: 475, dividend_total: 475, dividend_basis: "actual" })} />,
    );
    expect(html).toContain("内訳未確認");
    expect(html).toContain("475");
    expect(html).toContain("実績");
  });
  it("shows the company forecast total without claiming extraordinary dividends are excluded", () => {
    const html = renderToStaticMarkup(
      <DividendAmount
        r={row({
          dividend: 475,
          dividend_total: 475,
          dividend_special: 415,
          dividend_commemorative: 0,
          dividend_basis: "forecast",
        })}
      />,
    );
    expect(html).toContain("475");
    expect(html).not.toContain("臨時配当除外");
    expect(html).toContain("予想");
  });
  it("keeps known zero distinct from unknown and labels fund distributions separately", () => {
    const html = renderToStaticMarkup(
      <DividendAmount
        r={row({ dividend: 0, dividend_total: 0, dividend_special: 0, dividend_commemorative: 0 })}
      />,
    );
    expect(html).not.toContain("内訳未確認");
    expect(metricLabel("dividend_yield", "stock")).toBe("配当利回り");
    expect(metricLabel("dividend_yield", "reit")).toBe("分配金利回り");
  });
});
