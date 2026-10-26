import { useEffect, useState } from "react";
import { apiGet } from "@/lib/api";
import { useAuthState } from "@/lib/auth";
import { reitSectorLabel } from "@/lib/filters";

type Publication<T> = {
  status: "ready" | "not_generated";
  generated_at?: number;
  update_status?: string;
  price_date?: string;
  items: T[];
};
type Axis = {
  status: "positive" | "neutral" | "caution" | "unavailable";
  reason: string;
  evidence_ids: string[];
};
type Evidence = {
  id: string;
  location: string;
  source_url: string;
  period_end: string;
  submitted: string;
  body: string;
};
type Assessment = {
  code: string;
  model: string;
  version: string;
  assessment: {
    axes: Record<"income" | "debt" | "value", Axis>;
    facts: { topic: string; chunk_id: string; quote: string }[];
    missing_information: string[];
  };
  evidence: Evidence[];
  document_issue?: string | null;
  verified_metrics?: {
    ltv?: number | null;
    maturing_ratio?: number | null;
    reprice_ratio?: number | null;
    reprice_max_ratio?: number | null;
    quality: string;
    reason: string;
    period_end?: string;
    source_url?: string;
  };
};
type Period = {
  sessions: number;
  return: number | null;
  direction: string;
  available: number;
  count: number;
  up_share: number | null;
  series: { date: string; value: number }[];
};
type Sector = { ai_summary?: string; sector: string; periods: Period[]; members: string[] };
const directions: Record<string, string> = {
  up: "上昇傾向",
  down: "下落傾向",
  flat: "横ばい",
  unavailable: "判定保留",
};
const percent = (v: number | null) =>
  v == null ? "—" : `${v > 0 ? "+" : ""}${(v * 100).toFixed(1)}%`;

function usePublication<T>(path: string) {
  const { getToken } = useAuthState();
  const [data, setData] = useState<Publication<T> | null>(null);
  const [error, setError] = useState(false);
  useEffect(() => {
    const controller = new AbortController();
    setData(null);
    setError(false);
    apiGet<Publication<T>>(path, getToken, controller.signal)
      .then((v) => {
        if (!controller.signal.aborted) setData(v);
      })
      .catch(() => {
        if (!controller.signal.aborted) setError(true);
      });
    return () => controller.abort();
  }, [path, getToken]);
  return { data, error };
}
function State({ error, data }: { error: boolean; data: Publication<unknown> | null }) {
  if (error)
    return (
      <p role="alert" className="text-sm text-muted-foreground">
        バッチ結果を読み込めませんでした。
      </p>
    );
  if (!data) return <p className="text-sm text-muted-foreground">読み込み中…</p>;
  if (data.status === "not_generated")
    return (
      <p className="text-sm text-muted-foreground">
        {data.update_status === "failed"
          ? "バッチ分析が完了していません。前回の取得・検証エラーを確認してください。"
          : "初回のバッチ分析を待っています。"}
      </p>
    );
  return (
    <p className="text-xs text-muted-foreground">
      {data.update_status === "failed"
        ? "更新未完了・前回の結果を表示しています。 "
        : data.update_status === "running"
          ? "更新中・前回の結果を表示しています。 "
          : ""}
      価格基準日: {data.price_date} ・ 分析日時:{" "}
      {data.generated_at
        ? new Date(data.generated_at * 1000).toLocaleString("ja-JP", { timeZone: "Asia/Tokyo" })
        : "—"}
      （日本時間）。保存された分析時点の評価です。
    </p>
  );
}

export function ReitAssessment({ code }: { code: string }) {
  const { data, error } = usePublication<Assessment>(
    `/api/stocks/${encodeURIComponent(code)}/reit-assessment`,
  );
  return <ReitAssessmentContent data={data} error={error} />;
}

export function ReitAssessmentContent({
  data,
  error,
}: {
  data: Publication<Assessment> | null;
  error: boolean;
}) {
  const item = data?.items[0];
  return (
    <section className="flex flex-col gap-3" aria-label="REITのAI評価">
      <h3 className="text-sm font-medium">資料に基づくAI評価</h3>
      <State error={error} data={data} />
      {data?.status === "ready" && !item && (
        <p className="text-sm text-muted-foreground">この銘柄の分析はまだありません。</p>
      )}
      {item && (
        <>
          <div className="grid gap-3 sm:grid-cols-3">
            {(
              [
                ["income", "収益の安定性"],
                ["debt", "借入の余裕"],
                ["value", "価格の割安さ"],
              ] as const
            ).map(([key, label]) => {
              const axis = item.assessment.axes[key];
              return (
                <article key={key} className="rounded-xl border p-4">
                  <h4 className="text-sm">{label}</h4>
                  <p className="my-2 font-medium">
                    {
                      {
                        positive: "余裕の目安あり",
                        neutral: "中間",
                        caution: "確認点あり",
                        unavailable: "判定保留",
                      }[axis.status]
                    }
                  </p>
                  <p className="text-sm text-muted-foreground">{axis.reason}</p>
                </article>
              );
            })}
          </div>
          {item.document_issue && (
            <p role="note" className="rounded-lg border p-3 text-sm">
              {item.document_issue}
            </p>
          )}
          {item.verified_metrics && (
            <div className="rounded-xl border p-3">
              <h4 className="text-sm font-medium">有報の数値照合</h4>
              <p className="mt-1 text-xs text-muted-foreground">
                {item.verified_metrics.reason}{" "}
                {item.verified_metrics.period_end && `（${item.verified_metrics.period_end}期末）`}
              </p>
              <dl className="mt-3 grid grid-cols-3 gap-3 text-sm">
                <div>
                  <dt>総資産LTV</dt>
                  <dd>
                    {item.verified_metrics.ltv == null
                      ? "—"
                      : `${(item.verified_metrics.ltv * 100).toFixed(1)}%`}
                  </dd>
                </div>
                <div>
                  <dt>1年内返済比率</dt>
                  <dd>
                    {item.verified_metrics.maturing_ratio == null
                      ? "—"
                      : `${(item.verified_metrics.maturing_ratio * 100).toFixed(1)}%`}
                  </dd>
                </div>
                <div>
                  <dt>金利見直し比率</dt>
                  <dd>
                    {item.verified_metrics.reprice_ratio == null ||
                    item.verified_metrics.reprice_max_ratio == null
                      ? "—"
                      : `${(item.verified_metrics.reprice_ratio * 100).toFixed(1)}〜${(item.verified_metrics.reprice_max_ratio * 100).toFixed(1)}%`}
                  </dd>
                </div>
              </dl>
              {item.verified_metrics.source_url && (
                <a
                  className="mt-2 inline-block text-xs underline"
                  href={item.verified_metrics.source_url}
                  target="_blank"
                  rel="noreferrer"
                >
                  数値の原資料
                </a>
              )}
            </div>
          )}
          <details className="rounded-xl border p-3">
            <summary className="cursor-pointer text-sm">
              判断に使った原文と出典（{item.assessment.facts.length}件）
            </summary>
            <div className="mt-3 flex flex-col gap-4">
              {item.assessment.facts.map((fact) => {
                const source = item.evidence.find((e) => e.id === fact.chunk_id);
                return (
                  <blockquote
                    key={`${fact.chunk_id}-${fact.topic}-${fact.quote}`}
                    className="border-l-2 pl-3 text-sm"
                  >
                    <p className="whitespace-pre-wrap">{fact.quote}</p>
                    {source && (
                      <p className="mt-1 text-xs text-muted-foreground">
                        <a
                          href={source.source_url}
                          target="_blank"
                          rel="noreferrer"
                          className="underline"
                        >
                          EDINET原資料
                        </a>{" "}
                        ・ {source.period_end}期 ・ {source.submitted}提出 ・ {source.location}
                      </p>
                    )}
                  </blockquote>
                );
              })}
              <p className="text-xs text-muted-foreground">
                未確認: {item.assessment.missing_information.join("、")}
              </p>
            </div>
          </details>
        </>
      )}
    </section>
  );
}

export function ReitSectors({ onSelect }: { onSelect: (sector: string) => void }) {
  const { data, error } = usePublication<Sector>("/api/reit/sectors");
  return <ReitSectorsContent data={data} error={error} onSelect={onSelect} />;
}

export function ReitSectorsContent({
  data,
  error,
  onSelect,
}: {
  data: Publication<Sector> | null;
  error: boolean;
  onSelect: (sector: string) => void;
}) {
  return (
    <section className="flex flex-col gap-3" aria-label="REITセクター動向">
      <h2 className="text-sm font-medium">セクター別の値動き</h2>
      <State error={error} data={data} />
      {data?.status === "ready" && (
        <>
          <p className="text-xs text-muted-foreground">
            現在の構成銘柄に等金額を配分した参考比較。価格のみ・分配金を含みません。3銘柄未満や価格欠損は判定保留です。
          </p>
          <div className="grid gap-3 sm:grid-cols-2 lg:grid-cols-4">
            {data.items.map((sector) => (
              <button
                type="button"
                key={sector.sector}
                onClick={() => onSelect(sector.sector)}
                className="rounded-xl border p-3 text-left hover:bg-secondary"
              >
                <span className="text-sm font-medium">
                  {reitSectorLabel(sector.sector) ?? "用途未確認"}（{sector.members.length}銘柄）
                </span>
                {sector.periods.map((p) => (
                  <div key={p.sessions} className="mt-2 flex justify-between gap-2 text-xs">
                    <span>約{p.sessions === 21 ? 1 : p.sessions === 63 ? 3 : 6}か月</span>
                    <span>
                      {directions[p.direction] ?? "判定保留"} {percent(p.return)}
                    </span>
                  </div>
                ))}
                <p className="mt-2 text-xs text-muted-foreground">
                  短期の上昇銘柄:{" "}
                  {sector.periods[0]?.up_share == null
                    ? "—"
                    : `${Math.round(sector.periods[0].up_share * 100)}%`}
                </p>
                {sector.ai_summary && (
                  <p className="mt-3 text-xs text-muted-foreground">{sector.ai_summary}</p>
                )}
              </button>
            ))}
          </div>
        </>
      )}
    </section>
  );
}
