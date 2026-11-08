import { useEffect, useState } from "react";
import { ReitComparison, ScoreQuality, ScoreRun } from "@/components/reit-score";
import { Button } from "@/components/ui/button";
import { apiGet } from "@/lib/api";
import { useAuthState } from "@/lib/auth";
import {
  metricNumber,
  type Profile,
  profiles,
  type RankingsResponse,
  type ScoredReit,
  type ScoreResponse,
  scoreNumber,
  sectorLabel,
  sectors,
} from "@/lib/reit-scores";

const inputClass = "w-full rounded border bg-background px-2 py-1.5 text-sm";
const filters = [
  ["min_yield", "確定利回り下限 (%)", undefined, 100],
  ["max_floating_ratio", "実質変動比率上限 (%)", 100, 100],
  ["max_ltv", "LTV上限 (%)", 100, 100],
  ["min_coverage", "充足率下限 (%)", 100, 1],
] as const;

export function ReitRankingPage() {
  const { getToken } = useAuthState();
  const [profile, setProfile] = useState<Profile>("total");
  const [query, setQuery] = useState("");
  const [data, setData] = useState<RankingsResponse | null>(null);
  const [rankError, setRankError] = useState(false);
  const [selected, setSelected] = useState<string[]>([]);
  const [comparison, setComparison] = useState<ScoreResponse<ScoredReit> | null>(null);
  const [comparisonError, setComparisonError] = useState(false);
  const [comparing, setComparing] = useState(false);

  useEffect(() => {
    const ac = new AbortController();
    const params = new URLSearchParams(query);
    params.set("profile", profile);
    apiGet<RankingsResponse>(`/api/reits/rankings?${params}`, getToken, ac.signal)
      .then((v) => {
        if (!ac.signal.aborted) {
          setData(v);
          setRankError(false);
        }
      })
      .catch(() => {
        if (!ac.signal.aborted) {
          setRankError(true);
          setData(null);
        }
      });
    return () => ac.abort();
  }, [profile, query, getToken]);

  useEffect(() => {
    if (!comparing || !data?.run || selected.length === 0) return;
    const ac = new AbortController();
    const params = new URLSearchParams({ tickers: selected.join(","), run_id: data.run.run_id });
    apiGet<ScoreResponse<ScoredReit>>(`/api/reits/compare?${params}`, getToken, ac.signal)
      .then((v) => {
        if (!ac.signal.aborted) setComparison(v);
      })
      .catch(() => {
        if (!ac.signal.aborted) setComparisonError(true);
      });
    return () => ac.abort();
  }, [comparing, data?.run, getToken, selected]);

  function clearComparison() {
    setComparing(false);
    setComparison(null);
    setComparisonError(false);
  }

  return (
    <section className="space-y-4">
      <h2 className="text-xl font-semibold">J-REITランキング</h2>
      <p className="text-sm text-muted-foreground">
        上場J-REIT全体で採点し、選んだ評価軸の得点率で並べます。絞り込み後も全体順位・同用途内順位は変わりません。
      </p>
      <fieldset className="flex flex-wrap gap-2">
        <legend className="sr-only">評価軸</legend>
        {profiles.map(([key, label]) => (
          <Button
            key={key}
            size="sm"
            variant={profile === key ? "default" : "outline"}
            aria-pressed={profile === key}
            onClick={() => {
              if (profile !== key) {
                setData(null);
                setRankError(false);
                setProfile(key);
                clearComparison();
              }
            }}
          >
            {label}
          </Button>
        ))}
      </fieldset>
      <form
        className="grid items-end gap-2 rounded border p-3 sm:grid-cols-3 lg:grid-cols-7"
        onSubmit={(e) => {
          e.preventDefault();
          const form = new FormData(e.currentTarget);
          const params = new URLSearchParams();
          for (const key of ["sector", "as_of"]) {
            const v = form.get(key);
            if (typeof v === "string" && v) params.set(key, v);
          }
          for (const [key, , , scale] of filters) {
            const v = form.get(key);
            if (typeof v === "string" && v) params.set(key, String(Number(v) / scale));
          }
          if (params.toString() !== query) {
            if (params.get("as_of") !== new URLSearchParams(query).get("as_of")) setSelected([]);
            setData(null);
            setRankError(false);
            setQuery(params.toString());
            clearComparison();
          }
        }}
      >
        <label className="space-y-1 text-xs">
          <span>用途</span>
          <select className={inputClass} name="sector">
            <option value="">全用途</option>
            {sectors.map(([key, label]) => (
              <option key={key} value={key}>
                {label}
              </option>
            ))}
          </select>
        </label>
        {filters.map(([key, label, max]) => (
          <label key={key} className="space-y-1 text-xs">
            <span>{label}</span>
            <input className={inputClass} name={key} type="number" min="0" max={max} step="any" />
          </label>
        ))}
        <label className="space-y-1 text-xs">
          <span>評価基準日（任意）</span>
          <input className={inputClass} name="as_of" type="date" />
        </label>
        <Button type="submit" variant="outline" size="sm">
          絞り込む
        </Button>
      </form>
      {data?.run && <ScoreRun run={data.run} />}
      {!data && (
        <p role="status" className="text-sm">
          {rankError ? "ランキングを取得できませんでした。" : "読み込み中…"}
        </p>
      )}
      {data?.status === "not_generated" && (
        <p className="text-sm">指定日までに公開された定量評価はありません。</p>
      )}
      {data?.status === "ready" && (
        <>
          <p className="text-sm text-muted-foreground">
            重視する評価：{profiles.find(([key]) => key === data.profile)?.[1]}。
            同点は丸め前の得点、充足率、証券コード順で決めます。暫定評価は順位の対象外です。
          </p>
          {data.items.length > 0 && data.items.every((r) => r.rank == null) && (
            <p className="rounded border bg-muted/30 px-3 py-2 text-sm">
              この条件で順位が付く銘柄はありません。暫定評価の数値・根拠は銘柄詳細で確認できます。
            </p>
          )}
          <div className="flex flex-wrap items-center gap-3">
            <Button
              size="sm"
              type="button"
              disabled={!selected.length || comparing}
              onClick={() => {
                setComparisonError(false);
                setComparing(true);
              }}
            >
              比較する ({selected.length}/5)
            </Button>
            {!!selected.length && (
              <Button
                size="sm"
                type="button"
                variant="ghost"
                onClick={() => {
                  setSelected([]);
                  clearComparison();
                }}
              >
                選択を解除
              </Button>
            )}
            {selected.length > 0 && (
              <span className="text-xs text-muted-foreground">選択中：{selected.join("、")}</span>
            )}
          </div>
          {comparisonError && (
            <p role="alert" className="text-sm">
              比較を取得できませんでした。{" "}
              <button type="button" className="underline" onClick={clearComparison}>
                閉じる
              </button>
            </p>
          )}
          {comparing && !comparisonError && !comparison && (
            <p role="status" className="text-sm">
              比較を読み込み中…
            </p>
          )}
          {comparison?.items.length ? <ReitComparison rows={comparison.items} /> : null}
          {data.items.length === 0 ? (
            <p className="text-sm">条件に合う銘柄はありません。</p>
          ) : (
            <div className="overflow-x-auto rounded border">
              <table className="w-full min-w-max text-left text-sm">
                <thead className="bg-muted/50">
                  <tr>
                    {[
                      "比較",
                      "全体 / 用途順位",
                      "銘柄 / 用途",
                      "価格時点",
                      "確定12か月利回り",
                      "予想年利回り",
                      ...(profile === "total" ? [] : ["評価軸の得点率"]),
                      "総合点",
                      "データ充足率",
                      "財務警告・主な理由",
                    ].map((label) => (
                      <th key={label} scope="col" className="px-2 py-2">
                        {label}
                      </th>
                    ))}
                  </tr>
                </thead>
                <tbody>
                  {data.items.map((r) => (
                    <tr key={r.ticker} className="border-t align-top">
                      <td className="px-2 py-2">
                        <input
                          type="checkbox"
                          aria-label={`${r.ticker}を比較に追加`}
                          checked={selected.includes(r.ticker)}
                          disabled={selected.length >= 5 && !selected.includes(r.ticker)}
                          onChange={(e) => {
                            const checked = e.target.checked;
                            clearComparison();
                            setSelected((old) =>
                              checked ? [...old, r.ticker] : old.filter((c) => c !== r.ticker),
                            );
                          }}
                        />
                      </td>
                      <td className="px-2 py-2 tabular-nums">
                        {r.rank ?? "—"} / {r.sector_rank ?? "—"}
                      </td>
                      <th scope="row" className="px-2 py-2 font-medium">
                        <a
                          href={`/stocks/${r.ticker}?${new URLSearchParams({ reit_run: data.run?.run_id ?? "" })}#reit-score`}
                          className="underline"
                        >
                          {r.ticker} {r.name}
                        </a>
                        <div className="text-xs text-muted-foreground">{sectorLabel(r.sector)}</div>
                      </th>
                      <td className="px-2 py-2 tabular-nums">{r.price_date ?? "—"}</td>
                      <td className="px-2 py-2 tabular-nums">
                        {metricNumber(r.actual_yield, "ratio")}
                      </td>
                      <td className="px-2 py-2 tabular-nums">
                        {r.forecast.status === "partial" ? (
                          <span title="12か月分の会社予想がそろっていません">部分予想</span>
                        ) : (
                          metricNumber(r.forecast_yield, "ratio")
                        )}
                      </td>
                      {profile !== "total" && (
                        <td className="px-2 py-2 tabular-nums">
                          {r.profile_score == null ? "—" : `${scoreNumber(r.profile_score)}%`}
                        </td>
                      )}
                      <td className="px-2 py-2 tabular-nums">{scoreNumber(r.total_score)}</td>
                      <td className="px-2 py-2">
                        <ScoreQuality summary={r} />
                      </td>
                      <td className="max-w-56 px-2 py-2 text-xs">
                        {r.risks.length > 0 && (
                          <p className="font-medium text-destructive">
                            {r.risks.map((v) => v.message).join("、")}
                          </p>
                        )}
                        {r.positives.map((v) => v.label).join("、") || "—"}
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          )}
        </>
      )}
    </section>
  );
}
