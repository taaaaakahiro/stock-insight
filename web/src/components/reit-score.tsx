import { useEffect, useState } from "react";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { apiGet } from "@/lib/api";
import { useAuthState } from "@/lib/auth";
import { jstDateTime } from "@/lib/format";
import {
  comparisonMetrics,
  type Evidence,
  type HistoryPoint,
  metricNumber,
  type ReitSummary,
  type Run,
  reasons,
  type ScoredReit,
  type ScoreResponse,
  scoreNumber,
} from "@/lib/reit-scores";

export function ScoreRun({ run }: { run: Run }) {
  return (
    <p className="text-sm text-muted-foreground">
      評価基準日 {run.as_of_date}・評価版 {run.score_version}・上場J-REIT {run.universe_count}銘柄
      ・計算日時 {jstDateTime(run.completed_at)}
    </p>
  );
}

export function ScoreQuality({ summary }: { summary: ReitSummary }) {
  const explanation = summary.provisional_reasons.map((r) => reasons[r] ?? r).join("、");
  return (
    <span title={explanation || undefined}>
      {summary.coverage_pct}%
      {!summary.rank_eligible ? (
        <span className="ml-1 rounded bg-muted px-1.5 py-0.5 text-xs">暫定評価</span>
      ) : (
        summary.coverage_pct < 90 && (
          <span className="ml-1 rounded bg-muted px-1.5 py-0.5 text-xs">欠損あり</span>
        )
      )}
    </span>
  );
}

function Sources({ rows }: { rows: Evidence[] | null | undefined }) {
  if (!rows?.length) return null;
  return (
    <details className="text-xs">
      <summary className="cursor-pointer underline">元データ・出典 ({rows.length})</summary>
      <ul className="space-y-1 py-1">
        {rows.map((s) => (
          <li key={`${s.metric_key}-${s.period_end}-${s.source_document_id ?? s.source_url}`}>
            {s.metric_key}: {metricNumber(s.metric_value, s.unit)}・期末 {s.period_end}・開示{" "}
            {s.disclosure_published_at}・{" "}
            <a href={s.source_url} target="_blank" rel="noreferrer" className="underline">
              {s.source_document_id ?? new URL(s.source_url).hostname}
              {s.source_page ? ` p.${s.source_page}` : ""}
            </a>
            {s.quality_flags.length > 0 && `（${s.quality_flags.join("、")}）`}
            {s.normalization_flags?.map((flag) => (
              <div key={flag} className="break-all text-muted-foreground">
                {flag}
              </div>
            ))}
          </li>
        ))}
      </ul>
    </details>
  );
}

const chartSeries = [
  ["actual_dpu", "確定年間DPU", "#0e7490"],
  ["normalized_dpu", "正常化年間DPU", "#9333ea"],
  ["ffo_per_unit", "年間1口FFO", "#15803d"],
] as const;

function History({ points }: { points: HistoryPoint[] }) {
  const values = points.flatMap((p) =>
    chartSeries.flatMap(([key]) => (p[key] == null ? [] : [p[key]])),
  );
  if (values.length === 0)
    return <p className="text-sm text-muted-foreground">履歴は未取得です。</p>;
  const max = Math.max(1, ...values);
  const min = Math.min(0, ...values);
  const x = (i: number) => 68 + (500 * i) / Math.max(1, points.length - 1);
  const y = (v: number) => 154 - (125 * (v - min)) / (max - min);
  return (
    <figure className="max-w-3xl">
      <figcaption className="text-sm font-medium">決算期末ごとの推移（円/口）</figcaption>
      <svg viewBox="0 0 610 188" role="img" aria-label="分配金と1口FFOの履歴グラフ">
        {[min, (min + max) / 2, max].map((v) => (
          <g key={v}>
            <line x1="64" y1={y(v)} x2="576" y2={y(v)} stroke="currentColor" opacity={0.18} />
            <text x="59" y={y(v) + 4} textAnchor="end" fill="currentColor" fontSize="11">
              {Math.round(v).toLocaleString("ja-JP")}
            </text>
          </g>
        ))}
        {points.map((p, i) => (
          <text
            key={p.period_end}
            x={x(i)}
            y="176"
            textAnchor="middle"
            fill="currentColor"
            fontSize="11"
          >
            {p.period_end.slice(0, 7)}
          </text>
        ))}
        {chartSeries.map(([key, label, color]) => (
          <g key={key} fill={color} stroke={color}>
            {points.map((p, i) => {
              const value = p[key];
              const prev = points[i - 1]?.[key];
              if (value == null) return null;
              return (
                <g key={p.period_end}>
                  {prev != null &&
                    Number(p.period_end.slice(0, 4)) -
                      Number(points[i - 1]?.period_end.slice(0, 4)) ===
                      1 && (
                      <line x1={x(i - 1)} x2={x(i)} y1={y(prev)} y2={y(value)} strokeWidth={2} />
                    )}
                  <circle cx={x(i)} cy={y(value)} r="3">
                    <title>{`${p.period_end} ${label} ${metricNumber(value, "JPY/unit")}`}</title>
                  </circle>
                </g>
              );
            })}
          </g>
        ))}
      </svg>
      <ul className="flex flex-wrap gap-x-4 gap-y-1 text-xs">
        {chartSeries.map(([key, label, color]) => (
          <li key={key}>
            <span style={{ color }} aria-hidden="true">
              ●{" "}
            </span>
            {label}
          </li>
        ))}
      </ul>
      <details className="mt-2 text-xs">
        <summary className="cursor-pointer underline">履歴の数値</summary>
        <div className="overflow-x-auto">
          <table className="w-full text-left">
            <thead>
              <tr>
                <th>決算期末</th>
                {chartSeries.map(([key, label]) => (
                  <th key={key}>{label}</th>
                ))}
              </tr>
            </thead>
            <tbody>
              {points.map((p) => (
                <tr key={p.period_end}>
                  <th>{p.period_end}</th>
                  {chartSeries.map(([key]) => (
                    <td key={key}>{metricNumber(p[key], "JPY/unit")}</td>
                  ))}
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      </details>
    </figure>
  );
}

const statusLabel: Record<string, string> = {
  scored: "採点済み",
  insufficient_peers: "有効な比較対象が8銘柄未満",
  missing: "有効な入力なし",
  not_rated: "評価方法の整備待ち",
};

const financeMetrics = [
  ["swap_notional", "金利スワップ契約残高", "JPY"],
  ["remaining_years", "借換えまでの加重平均残存年数", "years"],
  ["issuer_ltv", "発行体公表LTV（発行体定義）", "ratio"],
  ["stress_50", "金利+0.5%pt", "ratio"],
  ["stress_100", "金利+1.0%pt", "ratio"],
  ["stress_200", "金利+2.0%pt", "ratio"],
] as const;

function ReitScoreBody({ row }: { row: ScoredReit }) {
  const { summary, detail } = row;
  return (
    <>
      <p className="text-lg">
        <strong>{scoreNumber(summary.total_score)} / 100</strong>・データ充足率{" "}
        <ScoreQuality summary={summary} />
      </p>
      {!summary.rank_eligible && (
        <p className="text-sm text-muted-foreground">
          順位対象外：{summary.provisional_reasons.map((r) => reasons[r] ?? r).join("、")}
        </p>
      )}
      <ul className="grid gap-2 text-sm sm:grid-cols-2 lg:grid-cols-3">
        {detail.categories.map((c) => (
          <li key={c.key} className="rounded border p-2">
            {c.label}{" "}
            <strong>
              {c.available ? scoreNumber(c.points) : "—"} / {c.maximum}点
            </strong>
            <div className="text-xs text-muted-foreground">
              採点可能 {c.available}点分・未採点 {c.maximum - c.available}点分
            </div>
          </li>
        ))}
      </ul>
      <div className="grid gap-2 text-sm sm:grid-cols-2">
        <div>
          <strong>主な評価理由</strong>
          <p>{summary.positives.map((p) => p.label).join("、") || "比較できる強みは未確認"}</p>
        </div>
        <div>
          <strong>注意点</strong>
          <p>
            {[
              ...summary.risks.map((r) => r.message),
              ...summary.weaknesses.map((p) => p.label),
            ].join("、") || "該当する警告は確認されていません"}
          </p>
        </div>
      </div>
      <History points={detail.history} />
      <section className="space-y-2 text-sm">
        <h3 className="font-medium">分配金と価格</h3>
        <p>
          価格 {metricNumber(detail.values.price?.value, "JPY/unit")}（
          {summary.price_date ?? "未取得"}）
        </p>
        <p>
          確定年間DPU {metricNumber(detail.values.actual_dpu?.value, "JPY/unit")}・利回り{" "}
          {metricNumber(summary.actual_yield, "ratio")}
        </p>
        <Sources rows={detail.values.actual_dpu?.sources} />
        <p>
          {summary.forecast.status === "partial" ? "部分予想" : "予想年間DPU"}{" "}
          {metricNumber(summary.forecast.dpu, "JPY/unit")}・予想年利回り{" "}
          {metricNumber(summary.forecast_yield, "ratio")}
        </p>
        <Sources rows={detail.values.forecast_dpu?.sources} />
      </section>
      <section className="space-y-2 text-sm">
        <h3 className="font-medium">借入と金利ストレス（正常化年間分配額に対する負担）</h3>
        <dl className="grid gap-2 sm:grid-cols-2 lg:grid-cols-3">
          {financeMetrics.map(([key, label, unit]) => (
            <div key={key} className="rounded border p-2">
              <dt>{label}</dt>
              <dd className="font-medium">
                {metricNumber(detail.values[key]?.valid ? detail.values[key]?.value : null, unit)}
              </dd>
              {key.startsWith("stress_") && (
                <dd className="text-xs text-muted-foreground">
                  借換え込み 1年目{" "}
                  {metricNumber(
                    detail.values[`${key}_y1`]?.valid ? detail.values[`${key}_y1`]?.value : null,
                    "ratio",
                  )}
                  ・2年目{" "}
                  {metricNumber(
                    detail.values[`${key}_y2`]?.valid ? detail.values[`${key}_y2`]?.value : null,
                    "ratio",
                  )}
                </dd>
              )}
              <dd>
                <Sources rows={detail.values[key]?.sources} />
              </dd>
            </div>
          ))}
        </dl>
        <p className="text-xs text-muted-foreground">
          期末の残高で試算。借換え込みは対象額が確認できた場合のみ表示し、即時に変動する負債は二重計上しません。税務、固定化契約の変更、物件の入替え、賃料の変動は含めません。
        </p>
      </section>
      <div className="overflow-x-auto">
        <table className="w-full text-left text-sm">
          <caption className="py-2 text-left font-medium">採点内訳・計算式・根拠</caption>
          <thead>
            <tr className="border-b">
              <th>指標 / 計算式</th>
              <th>観測値</th>
              <th>得点</th>
              <th>根拠</th>
            </tr>
          </thead>
          <tbody>
            {detail.components.map((c) => (
              <tr key={c.metric_key} className="border-b align-top">
                <th scope="row" className="py-2 pr-3 font-medium">
                  {c.label}
                  <div className="text-xs font-normal text-muted-foreground">{c.formula}</div>
                </th>
                <td className="py-2 pr-3">{metricNumber(c.metric_value, c.unit)}</td>
                <td className="py-2 pr-3">
                  {scoreNumber(c.weighted_points)} / {c.weight}
                  <div className="text-xs text-muted-foreground">
                    {statusLabel[c.status]}・{c.benchmark_size}銘柄
                  </div>
                </td>
                <td className="py-2">
                  <Sources rows={c.source_ref} />
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
      <p className="text-xs text-muted-foreground">
        総合点は取得点の合計を採点可能な配点で割った値です。鑑定評価額は即時の売却価格を示しません。
      </p>
    </>
  );
}

export function ReitScoreCard({ code, runId }: { code: string; runId?: string }) {
  const { getToken } = useAuthState();
  const [data, setData] = useState<ScoreResponse<ScoredReit> | null>(null);
  const [error, setError] = useState(false);
  useEffect(() => {
    const ac = new AbortController();
    const query = runId ? `?${new URLSearchParams({ run_id: runId })}` : "";
    apiGet<ScoreResponse<ScoredReit>>(`/api/reits/${code}/score${query}`, getToken, ac.signal)
      .then((result) => {
        if (!ac.signal.aborted) setData(result);
      })
      .catch(() => {
        if (!ac.signal.aborted) setError(true);
      });
    return () => ac.abort();
  }, [code, getToken, runId]);

  return (
    <Card id="reit-score" className="scroll-mt-24">
      <CardHeader>
        <CardTitle>J-REIT定量評価</CardTitle>
      </CardHeader>
      <CardContent className="space-y-4">
        {data?.run && <ScoreRun run={data.run} />}
        {data?.items[0] ? (
          <ReitScoreBody row={data.items[0]} />
        ) : (
          <p className="text-sm text-muted-foreground">
            {error
              ? "定量評価を取得できませんでした。"
              : data
                ? "定量評価はまだ公開されていません。"
                : "読み込み中…"}
          </p>
        )}
      </CardContent>
    </Card>
  );
}

export function ReitComparison({ rows }: { rows: ScoredReit[] }) {
  return (
    <div className="overflow-x-auto rounded border">
      <table className="w-full min-w-max text-left text-sm">
        <caption className="px-3 py-2 text-left font-medium">選択したJ-REITの比較</caption>
        <thead>
          <tr>
            <th className="px-3 py-2">指標</th>
            {rows.map(({ summary: s }) => (
              <th key={s.ticker} className="px-3 py-2">
                {s.ticker} {s.name}
              </th>
            ))}
          </tr>
        </thead>
        <tbody>
          <tr className="border-t">
            <th className="px-3 py-2">総合点 / 充足率</th>
            {rows.map(({ summary: s }) => (
              <td key={s.ticker} className="px-3 py-2">
                {scoreNumber(s.total_score)} / <ScoreQuality summary={s} />
              </td>
            ))}
          </tr>
          {comparisonMetrics.map(([key, label, unit]) => (
            <tr key={key} className="border-t align-top">
              <th scope="row" className="px-3 py-2 font-medium">
                {label}
              </th>
              {rows.map(({ summary, detail }) => {
                const v = detail.values[key];
                return (
                  <td key={summary.ticker} className="px-3 py-2 tabular-nums">
                    {metricNumber(v?.valid ? v.value : null, unit)}
                    <Sources rows={v?.sources} />
                  </td>
                );
              })}
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}
