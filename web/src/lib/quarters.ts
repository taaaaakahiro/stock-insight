import type { QuarterRow } from "./api";

export type QuarterGroup = { quarter: QuarterRow["quarter"]; rows: QuarterRow[] };

/** 同じ四半期（1Q〜4Q）ごとにまとめる。各組の中は新しい年度が先頭。値のない四半期の組は出さない */
export function groupByQuarter(quarters: QuarterRow[]): QuarterGroup[] {
  return ([1, 2, 3, 4] as const)
    .map((quarter) => ({
      quarter,
      rows: quarters
        .filter((q) => q.quarter === quarter)
        .sort((a, b) => b.fiscal_year_end.localeCompare(a.fiscal_year_end)),
    }))
    .filter((g) => g.rows.length > 0);
}
