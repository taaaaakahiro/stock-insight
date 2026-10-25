import { useEffect, useState } from "react";
import { Button } from "@/components/ui/button";
import { apiGet, type RecommendationComparison } from "@/lib/api";
import { useAuthState } from "@/lib/auth";
import { growth, pct, points, ymd } from "@/lib/format";

export function RecommendComparison() {
  const { getToken } = useAuthState();
  const [data, setData] = useState<RecommendationComparison | null>(null);
  const [error, setError] = useState(false);
  const [horizon, setHorizon] = useState(126);
  useEffect(() => {
    const ctrl = new AbortController();
    apiGet<RecommendationComparison>("/api/recommendations/comparison", getToken, ctrl.signal)
      .then((value) => {
        if (!ctrl.signal.aborted) setData(value);
      })
      .catch(() => {
        if (!ctrl.signal.aborted) setError(true);
      });
    return () => ctrl.abort();
  }, [getToken]);
  const groups = data?.report?.groups.filter((g) => g.horizon === horizon) ?? [];
  return (
    <section
      aria-labelledby="comparison-heading"
      className="flex flex-col gap-3 rounded-xl border p-4"
    >
      <div className="flex flex-wrap items-baseline justify-between gap-2">
        <h3 id="comparison-heading" className="font-medium">
          公開した推薦の成績を比較
        </h3>
        {data?.as_of && (
          <p className="text-xs text-muted-foreground">{ymd(data.as_of)}までのデータ</p>
        )}
      </div>
      <p className="text-sm text-muted-foreground">
        各日の初回公開版と、同時点のルール上位10銘柄を比較します。現在の順位はルールで決まり、AIは理由を書きます。全件が公開された日は両者が一致するため、この差はAIの選別能力を示しません。
        期間を満了し、両方のデータが揃った推薦日だけで成績差を計算します。
      </p>
      <fieldset className="flex flex-wrap gap-2" aria-label="比較期間">
        {(
          [
            [21, "約1か月"],
            [63, "約3か月"],
            [126, "約6か月・主評価"],
            [252, "約1年"],
          ] as const
        ).map(([days, label]) => (
          <Button
            key={days}
            size="sm"
            variant={horizon === days ? "default" : "outline"}
            aria-pressed={horizon === days}
            onClick={() => setHorizon(days)}
          >
            {label}
          </Button>
        ))}
      </fieldset>
      {error && (
        <p role="alert" className="text-sm">
          比較レポートを読み込めませんでした。
        </p>
      )}
      {!data && !error && <p className="text-sm text-muted-foreground">比較を読み込み中…</p>}
      {data && groups.length === 0 && (
        <p className="text-sm text-muted-foreground">
          まだ比較データがありません。推薦公開後の日次追跡から集計が始まります。
        </p>
      )}
      {groups.map((g) => (
        <div
          key={`${g.model}/${g.prompt_version}/${g.rule_version}`}
          className="flex flex-col gap-3 border-t pt-3"
        >
          <p className="text-xs text-muted-foreground">
            {g.model} · {g.prompt_version} · {g.rule_version}
          </p>
          <div className="grid grid-cols-2 gap-3 md:grid-cols-4">
            {[
              ["公開推薦の平均成績", growth(g.mean_ai)],
              ["ルールの平均成績", growth(g.mean_rule)],
              ["平均の成績差", points(g.mean_excess)],
              ["ルールを上回った割合", pct(g.beat_rate, 1)],
            ].map(([label, value]) => (
              <dl key={label}>
                <dt className="text-xs text-muted-foreground">{label}</dt>
                <dd className="mt-1 text-lg tabular-nums">{value}</dd>
              </dl>
            ))}
          </div>
          <p className="text-xs text-muted-foreground">
            対象 {g.total}日 · 比較済み {g.paired}日 · 期間未到達 {g.pending}日 · データ不足{" "}
            {g.missing}日 · 比較済みの銘柄数 {g.unique_stocks} · 差の中央値{" "}
            {points(g.median_excess)}
          </p>
          {g.paired === 0 && <p className="text-sm">この期間の結果はまだ揃っていません。</p>}
          {g.months.length > 0 && (
            <details className="text-sm">
              <summary className="cursor-pointer">推薦月別の差を見る</summary>
              <ul className="mt-2 flex flex-wrap gap-x-6 gap-y-2 text-xs">
                {g.months.map((m) => (
                  <li key={m.month}>
                    {m.month}：{points(m.mean_excess)}（{m.pairs}日）
                  </li>
                ))}
              </ul>
            </details>
          )}
        </div>
      ))}
      <p className="text-xs text-muted-foreground">
        現在は配当・税・費用を含まない株価成績です。同じ銘柄や保有期間が重複するため、比較日数は独立した検証件数ではありません。
        版ごとに対象期間が異なるので、平均値だけで版の優劣を決めません。
      </p>
      <details className="text-sm">
        <summary className="cursor-pointer">この結果をどう改善に使うか</summary>
        <ol className="mt-3 list-decimal space-y-2 pl-5 text-sm text-muted-foreground">
          <li>業種・利回り・配当余力などから、結果が悪かった条件を調べます。</li>
          <li>
            改善仮説を1つ決め、選定ルールを1か所ずつ変えます。AIの理由文は、根拠の正しさや欠落率で別に評価します。
          </li>
          <li>改善版を非公開で並走させ、同じ候補・同じ期間で現行版と比べます。</li>
          <li>半年の成績差、下落、減配、偏りを確認して採用を判断します。</li>
        </ol>
        <p className="mt-3 text-xs text-muted-foreground">
          改善版の並走実験と自動採用は未実装です。現時点ではデータ蓄積と比較を行い、AIの設定を自動変更しません。
        </p>
      </details>
    </section>
  );
}
