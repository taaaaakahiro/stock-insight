import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import type { RecommendationQuality } from "@/lib/api";
import { RecommendationAssessment } from "./recommendation-assessment";

const base: RecommendationQuality = {
  value: 2,
  financial: 0,
  earnings: 2,
  dividend: 1,
  total: 5,
  financial_applicable: true,
  net_cash: null,
  evidence: null,
};
describe("recommendation assessment", () => {
  it("does not invent missing historical assessments or imply safety", () => {
    const empty = renderToStaticMarkup(<RecommendationAssessment />);
    expect(empty).toContain("推薦時の記録なし");
    expect(empty).not.toContain("0/10");
    const old = renderToStaticMarkup(<RecommendationAssessment quality={base} />);
    expect(old).toContain("確認状況の記録なし");
    expect(old).toContain("推薦時の懸念の記録なし");
    expect(old).not.toContain("該当なし");
  });
  it("shows score, partial data and concerns together", () => {
    const html = renderToStaticMarkup(
      <RecommendationAssessment
        quality={{
          ...base,
          assessment: {
            financial_status: "partial",
            checked: 4,
            total: 9,
            missing: ["有利子負債"],
            concerns: ["営業利益が前年同期比で減少"],
          },
        }}
      />,
    );
    expect(html).toContain("5/10");
    expect(html).toContain("一部未確認");
    expect(html).toContain("4/9項目");
    expect(html).toContain("営業利益が前年同期比で減少");
    expect(html).toContain("未確認：有利子負債");
  });
  it("distinguishes out-of-scope companies and escapes source excerpts", () => {
    const html = renderToStaticMarkup(
      <RecommendationAssessment
        quality={{
          ...base,
          financial_applicable: false,
          evidence: {
            doc_id: "S100TEST",
            period_end: "2026-03-31",
            scope: "連結",
            cash: 0,
            debt: null,
            liabilities: 10,
            excerpts: [{ text: "<script>危険な命令</script>" }],
          },
        }}
      />,
    );
    expect(html).toContain("財務情報：対象外");
    expect(html).toContain("単純比較はできません");
    expect(html).not.toContain("<script>");
    expect(html).toContain("S100TEST.pdf");
  });
});
