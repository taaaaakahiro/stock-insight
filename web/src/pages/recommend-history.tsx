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
  const [page, setPage] = useState(0);
  const outcomes = new Map(
    detail.outcomes.filter((o) => o.horizon === horizon).map((o) => [o.code, o]),
  );
  const ai = outcomes.get("__ai10")?.result.price_return;
  const rule = outcomes.get("__rule10")?.result.price_return;
  const evaluatedAt = Math.max(0, ...detail.outcomes.map((o) => o.evaluated_at));
  const candidates = detail.candidates.filter((c) => allCandidates || c.rank !== null);
  const pageCount = Math.max(1, Math.ceil(candidates.length / 10));
  const visible = candidates.slice(page * 10, page * 10 + 10);
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
          追跡機能の導入前のレコメンドです。当時の指標と公開時刻の記録がないため、成績比較の対象外です。銘柄名は現在の名称を表示しています。
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
                ["__ai10", "公開したレコメンド銘柄"],
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
            公開レコメンドとルールの差：
            <strong>{points(ai != null && rule != null ? ai - rule : null)}</strong>
            <span className="ml-2 text-xs text-muted-foreground">
              構成銘柄のデータがすべて揃った場合に比較します。
            </span>
          </p>
        </>
      )}
      <div className="flex items-center justify-between gap-3">
        <p className="text-sm">{allCandidates ? "当時の全候補" : "レコメンドした銘柄"}</p>
        <Button
          size="sm"
          variant="outline"
          onClick={() => {
            setAllCandidates((v) => !v);
            setPage(0);
          }}
        >
          {allCandidates ? "レコメンド銘柄だけ表示" : "選ばなかった候補も表示"}
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
            {visible.map((c) => {
              const outcome = outcomes.get(c.code);
              return (
                <tr key={c.code} className="border-t align-top">
                  <td className="p-3">{c.rank ?? "候補"}</td>
                  <td className="max-w-sm p-3">
                    <a className="font-medium hover:underline" href={`/stocks/${c.code}`}>
                      {c.stock?.name?.trim() || c.current_name?.trim() || "銘柄名未取得"}
                      <span className="ml-2 text-xs text-muted-foreground">{c.code}</span>
                    </a>
                    {c.reason ? (
                      <details className="mt-2 text-xs text-muted-foreground">
                        <summary className="cursor-pointer">レコメンド理由を見る</summary>
                        <p className="mt-2 leading-relaxed">{c.reason}</p>
                      </details>
                    ) : (
                      <p className="mt-1 text-xs text-muted-foreground">公開レコメンドの対象外</p>
                    )}
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
      <nav
        aria-label="銘柄のページ切り替え"
        className="flex flex-wrap items-center justify-between gap-3"
      >
        <p className="text-xs text-muted-foreground" aria-live="polite">
          全{candidates.length}銘柄 · {candidates.length ? page * 10 + 1 : 0}〜
          {Math.min((page + 1) * 10, candidates.length)}件を表示
        </p>
        <div className="flex items-center gap-2">
          <Button
            size="sm"
            variant="outline"
            disabled={page === 0}
            onClick={() => setPage((n) => n - 1)}
          >
            前の10件
          </Button>
          <span className="text-xs tabular-nums">
            {page + 1} / {pageCount}
          </span>
          <Button
            size="sm"
            variant="outline"
            disabled={page + 1 >= pageCount}
            onClick={() => setPage((n) => n + 1)}
          >
            次の10件
          </Button>
        </div>
      </nav>
    </div>
  );
}

export function RecommendHistory({ refreshKey }: { refreshKey: number }) {
  return <History key={refreshKey} />;
}

function History() {
  const { getToken } = useAuthState();
  const [history, setHistory] = useState<RecommendationHistory | null>(null);
  const [cursors, setCursors] = useState<(number | null)[]>([null]);
  const [page, setPage] = useState(0);
  const [retry, setRetry] = useState(0);
  const [selected, setSelected] = useState<number | null>(null);
  const [detail, setDetail] = useState<RecommendationHistoryDetail | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [detailError, setDetailError] = useState(false);
  const [detailRetry, setDetailRetry] = useState(0);
  const cursor = cursors[page];
  useEffect(() => {
    const ctrl = new AbortController();
    setHistory(null);
    setSelected(null);
    setDetail(null);
    setError(null);
    void retry;
    const query = cursor == null ? "" : `?before=${cursor}`;
    apiGet<RecommendationHistory>(`/api/recommendations/history${query}`, getToken, ctrl.signal)
      .then((data) => {
        if (ctrl.signal.aborted) return;
        setHistory(data);
        setSelected(data.runs[0]?.id ?? null);
      })
      .catch(() => {
        if (!ctrl.signal.aborted) setError("レコメンド履歴を読み込めませんでした。");
      });
    return () => ctrl.abort();
  }, [getToken, cursor, retry]);
  useEffect(() => {
    setDetail(null);
    setDetailError(false);
    void detailRetry;
    if (selected === null) return;
    const ctrl = new AbortController();
    apiGet<RecommendationHistoryDetail>(
      `/api/recommendations/history/${selected}`,
      getToken,
      ctrl.signal,
    )
      .then((data) => {
        if (!ctrl.signal.aborted) setDetail(data);
      })
      .catch(() => {
        if (!ctrl.signal.aborted) setDetailError(true);
      });
    return () => ctrl.abort();
  }, [getToken, selected, detailRetry]);
  function changePage(next: number) {
    setHistory(null);
    setSelected(null);
    setDetail(null);
    setPage(next);
  }
  return (
    <section aria-labelledby="history-heading" className="flex flex-col gap-4 border-t pt-6">
      <div>
        <h3 id="history-heading" className="font-medium">
          過去のレコメンドとその後の成績
        </h3>
        <p className="mt-1 text-xs text-muted-foreground">
          新しい順に20回ずつ表示します。日付を選ぶと、その回の銘柄と成績を確認できます。
        </p>
      </div>
      <nav
        aria-label="履歴のページ切り替え"
        className="flex flex-wrap items-center justify-between gap-3"
      >
        <span className="text-sm tabular-nums" aria-live="polite">
          {page + 1}ページ目{history && ` · ${history.runs.length}回のレコメンド`}
        </span>
        <div className="flex flex-wrap gap-2">
          {page > 1 && (
            <Button size="sm" variant="ghost" onClick={() => changePage(0)}>
              最新へ
            </Button>
          )}
          <Button
            size="sm"
            variant="outline"
            disabled={page === 0}
            onClick={() => changePage(page - 1)}
          >
            新しい20回
          </Button>
          <Button
            size="sm"
            variant="outline"
            disabled={!history?.next_before}
            onClick={() => {
              if (history?.next_before == null) return;
              setCursors((values) => [...values.slice(0, page + 1), history.next_before]);
              changePage(page + 1);
            }}
          >
            以前の20回
          </Button>
        </div>
      </nav>
      {error && (
        <div role="alert" className="flex items-center gap-3 text-sm">
          <p>{error}</p>
          <Button size="sm" variant="outline" onClick={() => setRetry((n) => n + 1)}>
            再読み込み
          </Button>
        </div>
      )}
      {!history && !error && (
        <p role="status" className="text-sm text-muted-foreground">
          履歴を読み込み中…
        </p>
      )}
      {history?.runs.length === 0 && (
        <p className="text-sm text-muted-foreground">このページにはレコメンドがありません。</p>
      )}
      {history && history.runs.length > 0 && (
        <div className="flex flex-col gap-2 rounded-lg border bg-muted/30 p-3 sm:flex-row sm:items-center sm:gap-3">
          <label className="shrink-0 text-sm" htmlFor="recommendation-run">
            表示するレコメンド
          </label>
          <select
            id="recommendation-run"
            className="w-full min-w-0 rounded-md border bg-background p-2 text-sm"
            value={selected ?? ""}
            onChange={(e) => {
              setDetail(null);
              setSelected(Number(e.target.value));
            }}
          >
            {history.runs.map((r) => (
              <option key={r.id} value={r.id}>
                {ymd(r.price_date)}基準 · {jstDateTime(r.published_at ?? r.created_at)}
                {r.status === "legacy_incomplete"
                  ? "（旧データ）"
                  : r.is_primary
                    ? "（初回）"
                    : "（再生成）"}
              </option>
            ))}
          </select>
        </div>
      )}
      {detailError && (
        <div role="alert" className="flex items-center gap-3 text-sm">
          <p>このレコメンドの成績を読み込めませんでした。</p>
          <Button size="sm" variant="outline" onClick={() => setDetailRetry((n) => n + 1)}>
            再読み込み
          </Button>
        </div>
      )}
      {selected !== null && !detail && !detailError && (
        <p role="status" className="text-sm text-muted-foreground">
          成績を読み込み中…
        </p>
      )}
      {detail && <Detail key={detail.run.id} detail={detail} />}
      <details className="rounded-lg border p-4">
        <summary className="cursor-pointer text-sm font-medium">
          レコメンド全体の成績を比較する
        </summary>
        <div className="mt-4">
          <RecommendComparison />
        </div>
      </details>
    </section>
  );
}
