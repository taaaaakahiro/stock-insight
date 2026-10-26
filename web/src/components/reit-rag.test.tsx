import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import { ReitAssessmentContent, ReitSectorsContent } from "./reit-rag";

describe("saved REIT analysis", () => {
  it("shows a failed refresh without representing the old result as today's evaluation", () => {
    const html = renderToStaticMarkup(
      <ReitAssessmentContent
        error={false}
        data={{ status: "ready", update_status: "failed", price_date: "2026-10-23", items: [] }}
      />,
    );
    expect(html).toContain("更新未完了・前回の結果");
    expect(html).toContain("2026-10-23");
  });
  it("shows unavailable returns as missing instead of zero or an upward trend", () => {
    const html = renderToStaticMarkup(
      <ReitSectorsContent
        onSelect={() => {}}
        error={false}
        data={{
          status: "ready",
          items: [
            {
              sector: "hotel",
              members: ["a", "b"],
              periods: [
                {
                  sessions: 21,
                  return: null,
                  direction: "unavailable",
                  available: 1,
                  count: 2,
                  up_share: null,
                  series: [],
                },
              ],
            },
          ],
        }}
      />,
    );
    expect(html).toContain("判定保留");
    expect(html).not.toContain("0.0%");
    expect(html).toContain("分配金を含みません");
  });
  it("renders model text as text and includes the retrieved source", () => {
    const unavailable = { status: "unavailable" as const, reason: "資料不足", evidence_ids: [] };
    const html = renderToStaticMarkup(
      <ReitAssessmentContent
        error={false}
        data={{
          status: "ready",
          items: [
            {
              code: "8951",
              model: "test",
              version: "v2",
              assessment: {
                axes: { income: unavailable, debt: unavailable, value: unavailable },
                facts: [{ topic: "debt", chunk_id: "one", quote: "<script>bad()</script>" }],
                missing_information: ["NAV"],
              },
              evidence: [
                {
                  id: "one",
                  body: "<script>bad()</script>",
                  location: "block:1",
                  source_url:
                    "https://disclosure2dl.edinet-fsa.go.jp/searchdocument/pdf/example.pdf",
                  period_end: "2026-06-30",
                  submitted: "2026-08-01",
                },
              ],
            },
          ],
        }}
      />,
    );
    expect(html).not.toContain("<script>");
    expect(html).toContain("&lt;script&gt;");
    expect(html).toContain("EDINET原資料");
  });
});
