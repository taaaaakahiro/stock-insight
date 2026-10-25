import { useEffect, useState } from "react";
import { Button } from "@/components/ui/button";
import {
  apiGet,
  type RecommendationHistory,
  type RecommendationHistoryDetail,
  type RecommendationOutcome,
} from "@/lib/api";
import { useAuthState } from "@/lib/auth";
import { growth, jstDateTime, pct, points, yen, ymd } from "@/lib/format";
import { RecommendComparison } from "./recommend-comparison";

const PERIODS = [
  [0, "最新・最大1年"],
  [21, "約1か月"],
  [63, "約3か月"],
  [126, "約6か月"],
  [252, "約1年"],
] as const;
const STATES = {
  pending: "期間未到達",
  tracking: "追跡中",
  complete: "評価済み",
  data_missing: "データ不足",
  entry_unavailable: "開始価格なし",
};

function OutcomeCell({ outcome }: { outcome: RecommendationOutcome | undefined }) {
  if (!outcome) return <span className="text-muted-foreground">集計待ち</span>;
  const r = outcome.result;
  return (
    <span className="flex flex-col gap-1">
      <span className="tabular-nums">{growth(r.price_return)}</span>
      <span className="text-xs text-muted-foreground">{STATES[r.status]}</span>
    </span>
  );
}

function Detail({ detail }: { detail: RecommendationHistoryDetail }) {
  const [allCandidates, setAllCandidates] = useState(false);
  const [horizon, setHorizon] = useState(0);
  const outcomes = new Map(
    detail.outcomes.filter((o) => o.horizon === horizon).map((o) => [o.code, o]),
  );
  const ai = outcomes.get("__ai10")?.result.price_return;
  const rule = outcomes.get("__rule10")?.result.price_return;
  const evaluatedAt = Math.max(0, ...detail.outcomes.map((o) => o.evaluated_at));
  const candidates = detail.candidates.filter((c) => allCandidates || c.rank !== null);
  return (
    <div className="flex flex-col gap-4">
      <p className="text-xs text-muted-foreground">
        {detail.run.model} · {detail.run.prompt_version ?? "版の記録なし"} ·
        {detail.run.status === "legacy_incomplete"
          ? "導入前の記録"
          : detail.run.is_primary === 1
            ? "その日の初回公開版"
            : "再生成版"}
        {evaluatedAt > 0 && ` · 最終評価 ${jstDateTime(evaluatedAt)}`}
      </p>
      {detail.run.status === "legacy_incomplete" ? (
        <p className="rounded-lg border p-3 text-sm">
          追跡機能の導入前の推薦です。当時の指標と公開時刻の記録がないため、成績比較の対象外です。
        </p>
      ) : (
        <>
          <fieldset className="flex flex-wrap gap-2" aria-label="評価期間">
            {PERIODS.map(([days, label]) => (
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
          <p className="text-xs text-muted-foreground">
            公開後の翌営業日終値から等金額で保有した場合の株価成績です。株式分割を調整し、配当・税・売買費用は含みません。
            配当込み成績は未算出です。1・3・6・12か月は21・63・126・252営業日で評価します。
          </p>
          <div className="grid grid-cols-2 gap-3 lg:grid-cols-4">
            {(
              [
                ["__ai10", "公開した推薦銘柄"],
                ["__rule10", "ルール上位10銘柄"],
                ["__candidates", "候補全体"],
                ["1306", "TOPIX連動ETF（1306）"],
              ] as const
            ).map(([code, label]) => (
              <div key={code} className="rounded-lg border p-3 text-sm">
                <p className="mb-2 text-xs text-muted-foreground">{label}</p>
                <OutcomeCell outcome={outcomes.get(code)} />
              </div>
            ))}
          </div>
          <p className="text-sm">
            公開推薦とルールの差：
            <strong>{points(ai != null && rule != null ? ai - rule : null)}</strong>
            <span className="ml-2 text-xs text-muted-foreground">
              構成銘柄のデータがすべて揃った場合に比較します。
            </span>
          </p>
        </>
      )}
      <div className="flex items-center justify-between gap-3">
        <p className="text-sm">{allCandidates ? "当時の全候補" : "推薦した銘柄"}</p>
        <Button size="sm" variant="outline" onClick={() => setAllCandidates((v) => !v)}>
          {allCandidates ? "推薦銘柄だけ表示" : "選ばなかった候補も表示"}
        </Button>
      </div>
      <div className="overflow-x-auto rounded-lg border">
        <table className="w-full min-w-[700px] text-left text-sm">
          <thead className="bg-muted text-xs">
            <tr>
              {[
                "公開順位",
                "銘柄・当時の理由",
                "当時の株価 / 利回り",
                "株価成績",
                "最大下落",
                "開始日 → 評価日",
              ].map((label) => (
                <th key={label} scope="col" className="p-3 font-medium">
                  {label}
                </th>
              ))}
            </tr>
          </thead>
          <tbody>
            {candidates.map((c) => {
              const outcome = outcomes.get(c.code);
              return (
                <tr key={c.code} className="border-t align-top">
                  <td className="p-3">{c.rank ?? "候補"}</td>
                  <td className="max-w-sm p-3">
                    <a className="font-medium hover:underline" href={`/stocks/${c.code}`}>
                      {c.stock?.name ?? c.code} <span className="text-xs">{c.code}</span>
                    </a>
                    <p className="mt-1 text-xs text-muted-foreground">
                      {c.reason ?? "公開推薦の対象外"}
                    </p>
                    {c.rule_rank !== null && (
                      <p className="mt-1 text-xs">ルール順位 {c.rule_rank}</p>
                    )}
                  </td>
                  <td className="p-3 tabular-nums">
                    <p>{yen(c.stock?.price)}</p>
                    <p>{pct(c.stock?.dividend_yield)}</p>
                  </td>
                  <td className="p-3">
                    {detail.run.status === "legacy_incomplete" ? (
                      <span className="text-muted-foreground">対象外</span>
                    ) : (
                      <OutcomeCell outcome={outcome} />
                    )}
                  </td>
                  <td className="p-3 tabular-nums">{pct(outcome?.result.max_drawdown)}</td>
                  <td className="p-3 text-xs tabular-nums">
                    <p>{ymd(outcome?.result.entry_date)}</p>
                    <p>→ {ymd(outcome?.result.end_date)}</p>
                  </td>
                </tr>
              );
            })}
          </tbody>
        </table>
      </div>
    </div>
  );
}

export function RecommendHistory({ refreshKey }: { refreshKey: number }) {
  const { getToken } = useAuthState();
  const [history, setHistory] = useState<RecommendationHistory | null>(null);
  const [selected, setSelected] = useState<number | null>(null);
  const [detail, setDetail] = useState<RecommendationHistoryDetail | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [loadingMore, setLoadingMore] = useState(false);
  useEffect(() => {
    const ctrl = new AbortController();
    setHistory(null);
    setError(null);
    // 再生成後も新しい履歴一覧を取得する。
    void refreshKey;
    apiGet<RecommendationHistory>("/api/recommendations/history", getToken, ctrl.signal)
      .then((data) => {
        if (ctrl.signal.aborted) return;
        setHistory(data);
        setSelected(data.runs[0]?.id ?? null);
      })
      .catch(() => {
        if (!ctrl.signal.aborted) setError("推薦履歴を読み込めませんでした。");
      });
    return () => ctrl.abort();
  }, [getToken, refreshKey]);
  useEffect(() => {
    setDetail(null);
    if (selected === null) return;
    const ctrl = new AbortController();
    setError(null);
    apiGet<RecommendationHistoryDetail>(
      `/api/recommendations/history/${selected}`,
      getToken,
      ctrl.signal,
    )
      .then((data) => {
        if (!ctrl.signal.aborted) setDetail(data);
      })
      .catch(() => {
        if (!ctrl.signal.aborted) setError("この推薦の成績を読み込めませんでした。");
      });
    return () => ctrl.abort();
  }, [getToken, selected]);
  async function loadMore() {
    if (history?.next_before == null || loadingMore) return;
    setLoadingMore(true);
    try {
      const data = await apiGet<RecommendationHistory>(
        `/api/recommendations/history?before=${history.next_before}`,
        getToken,
      );
      setHistory(
        (previous) =>
          previous && { runs: [...previous.runs, ...data.runs], next_before: data.next_before },
      );
    } catch {
      setError("以前の履歴を読み込めませんでした。");
    } finally {
      setLoadingMore(false);
    }
  }
  return (
    <section aria-labelledby="history-heading" className="flex flex-col gap-4 border-t pt-6">
      <RecommendComparison />
      <h3 id="history-heading" className="font-medium">
        過去の推薦とその後の成績
      </h3>
      {error && (
        <p role="alert" className="text-sm">
          {error}
        </p>
      )}
      {!history && !error && <p className="text-sm text-muted-foreground">履歴を読み込み中…</p>}
      {history?.runs.length === 0 && (
        <p className="text-sm text-muted-foreground">推薦が公開されると、ここに履歴が残ります。</p>
      )}
      {history && history.runs.length > 0 && (
        <div className="flex flex-wrap items-center gap-3">
          <label className="text-sm" htmlFor="recommendation-run">
            公開履歴
          </label>
          <select
            id="recommendation-run"
            className="max-w-full rounded-md border bg-background p-2 text-sm"
            value={selected ?? ""}
            onChange={(e) => setSelected(Number(e.target.value))}
          >
            {history.runs.map((r) => (
              <option key={r.id} value={r.id}>
                {ymd(r.price_date)}基準 · {jstDateTime(r.published_at ?? r.created_at)} · #{r.id}
                {r.status === "legacy_incomplete"
                  ? "（旧データ）"
                  : r.is_primary
                    ? "（初回）"
                    : "（再生成）"}
              </option>
            ))}
          </select>
          {history.next_before !== null && (
            <Button
              variant="outline"
              size="sm"
              disabled={loadingMore}
              onClick={() => void loadMore()}
            >
              {loadingMore ? "読み込み中…" : "以前の履歴を追加"}
            </Button>
          )}
        </div>
      )}
      {selected !== null && !detail && !error && (
        <p className="text-sm text-muted-foreground">成績を読み込み中…</p>
      )}
      {detail && <Detail key={detail.run.id} detail={detail} />}
    </section>
  );
}
