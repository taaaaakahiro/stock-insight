import type { RecommendationQuality } from "@/lib/api";
import { yen, ymd } from "@/lib/format";

/** All values here are the saved recommendation-time assessment. */
export function RecommendationAssessment({
  quality: q,
}: {
  quality?: RecommendationQuality | null;
}) {
  if (!q)
    return (
      <p className="text-xs text-muted-foreground">評価点・財務情報・懸念：推薦時の記録なし</p>
    );
  const excerpts = (q.evidence?.excerpts ?? []).filter(
    (excerpt, index, all) =>
      all.findIndex((x) => x.text === excerpt.text && x.section === excerpt.section) === index,
  );
  const a = q.assessment;
  const status = !q.financial_applicable
    ? "対象外"
    : !a
      ? "確認状況の記録なし"
      : {
          complete: "必要項目を取得済み",
          partial: "一部未確認",
          unknown: "未確認",
          not_applicable: "対象外",
        }[a.financial_status];
  return (
    <div className="rounded-lg border bg-muted/30 p-3 text-xs">
      <div className="flex flex-wrap gap-x-4 gap-y-1">
        <p className="font-medium">
          評価点 {q.total}/10{" "}
          <span className="font-normal text-muted-foreground">（推薦時の条件適合度）</span>
        </p>
        <p>
          財務情報：{status}
          {a && q.financial_applicable ? `（${a.checked}/${a.total}項目）` : ""}
        </p>
      </div>
      <div className="mt-2">
        <p className="font-medium">主な懸念</p>
        {!a ? (
          <p>推薦時の懸念の記録なし</p>
        ) : a.concerns.length > 0 ? (
          <ul className="mt-1 list-disc pl-4">
            {a.concerns.map((text) => (
              <li key={text}>{text}</li>
            ))}
          </ul>
        ) : (
          <p>取得済みの数値では該当なし。資料上の懸念は別途確認が必要です。</p>
        )}
      </div>
      <details className="mt-2">
        <summary className="cursor-pointer">内訳・未確認項目・資料を見る</summary>
        <p className="mt-2">
          割安 {q.value}/2・財務 {q.financial}/3・稼ぐ力 {q.earnings}/3・配当余力 {q.dividend}/2
        </p>
        <p className="mt-1 text-muted-foreground">
          点数は勝率や将来の利益を表しません。未確認は加点せず、0点は不健全を意味しません。
        </p>
        {!q.financial_applicable && (
          <p className="mt-1">
            一般企業向けの財務・CF加点は対象外のため、対象企業との合計点の単純比較はできません。
          </p>
        )}
        {!!a?.missing.length && <p className="mt-1">未確認：{a.missing.join("、")}</p>}
        {q.evidence && (
          <div className="mt-2">
            <p>
              {ymd(q.evidence.period_end)}・{q.evidence.scope}決算
            </p>
            <p>
              現金 {yen(q.evidence.cash)}／有利子負債 {yen(q.evidence.debt)}／負債合計{" "}
              {yen(q.evidence.liabilities)}
            </p>
            <a
              className="underline"
              href={`https://disclosure2dl.edinet-fsa.go.jp/searchdocument/pdf/${encodeURIComponent(q.evidence.doc_id)}.pdf`}
              target="_blank"
              rel="noreferrer"
            >
              根拠の有価証券報告書
            </a>
            {!!excerpts.length && (
              <div className="mt-2">
                <p>資金計画・配当方針などの抜粋（懸念の有無は原文を確認）</p>
                {excerpts.map((excerpt) => (
                  <blockquote
                    className="mt-1 border-l-2 pl-2 text-muted-foreground"
                    key={`${excerpt.section}-${excerpt.text}`}
                  >
                    {excerpt.text}
                  </blockquote>
                ))}
              </div>
            )}
          </div>
        )}
      </details>
    </div>
  );
}
