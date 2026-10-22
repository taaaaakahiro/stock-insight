import { useEffect, useState } from "react";
import { PortfolioToggle } from "@/components/portfolio-toggle";
import { Badge } from "@/components/ui/badge";
import { apiGet, type Recommendation, type Recommendations } from "@/lib/api";
import { useAuthState } from "@/lib/auth";
import { count, jstDateTime, pct, times, yen, ymd } from "@/lib/format";
import { navigate } from "@/lib/router";

type Load = { state: "loading" } | { state: "ok"; data: Recommendations } | { state: "error" };

const DISCLAIMER =
  "AIによる参考情報であり投資助言ではありません。理由の文章は AI が指標から書いたもので、誤りを含むことがあります。投資判断はご自身の責任で行ってください。";

/** 銘柄名。クリックで詳細（/stocks/<コード>）へ */
function NameLink({ r }: { r: Recommendation }) {
  const path = `/stocks/${r.code}`;
  return (
    <a
      href={path}
      onClick={(e) => {
        // 普通のクリックだけ画面内で切り替える（新しいタブなどはブラウザに任せる）
        if (e.metaKey || e.ctrlKey || e.shiftKey || e.button !== 0) return;
        e.preventDefault();
        navigate(path);
      }}
      className="font-medium underline-offset-4 hover:underline"
      title={r.name ?? r.code}
    >
      {r.name ?? r.code}
    </a>
  );
}

/** 一覧と同じ5年分のバッジ（当てはまるものだけ） */
function Flags({ r }: { r: Recommendation }) {
  const flags = [
    r.is_value && "バリュー",
    r.dividend_every_year && "5年配当",
    r.no_loss_years && "通期黒字",
    r.no_quarterly_loss && "四半期黒字",
    r.profit_uptrend && "増益傾向",
  ].filter((f): f is string => typeof f === "string");
  if (flags.length === 0) return null;
  return (
    <span className="flex flex-wrap gap-1">
      {flags.map((f) => (
        <Badge key={f} variant="outline" className="border-cheap/40 bg-cheap/10 text-cheap">
          {f}
        </Badge>
      ))}
    </span>
  );
}

const METRICS: { label: string; render: (r: Recommendation) => string }[] = [
  { label: "株価", render: (r) => yen(r.price) },
  { label: "配当利回り", render: (r) => pct(r.dividend_yield) },
  { label: "過去5年比", render: (r) => pct(r.yield_percentile, 0) },
  { label: "PBR", render: (r) => times(r.pbr) },
  { label: "PER", render: (r) => times(r.per) },
  { label: "配当性向", render: (r) => pct(r.payout_ratio, 1) },
  { label: "連続増配", render: (r) => count(r.consecutive_increases) },
  { label: "営業利益率", render: (r) => pct(r.operating_margin, 1) },
];

function Item({ r }: { r: Recommendation }) {
  const sub = [r.code, r.sector33].filter(Boolean).join("・");
  return (
    <li className="rounded-xl border p-4">
      <div className="flex items-start gap-3">
        <span
          className="flex size-8 shrink-0 items-center justify-center rounded-full bg-muted text-sm font-medium tabular-nums"
          title={`${r.rank}位`}
        >
          {r.rank}
        </span>
        <div className="flex min-w-0 flex-1 flex-col gap-2">
          <div className="flex min-w-0 flex-wrap items-center gap-x-2 gap-y-1">
            <PortfolioToggle code={r.code} name={r.name ?? r.code} />
            <NameLink r={r} />
            <span className="text-xs text-muted-foreground">{sub}</span>
            <Flags r={r} />
          </div>
          <p className="text-sm">{r.reason}</p>
          <dl className="grid grid-cols-2 gap-x-4 gap-y-1 text-sm sm:grid-cols-4 lg:grid-cols-8">
            {METRICS.map((m) => (
              <div key={m.label}>
                <dt className="text-xs text-muted-foreground">{m.label}</dt>
                <dd className="tabular-nums">{m.render(r)}</dd>
              </div>
            ))}
          </dl>
        </div>
      </div>
    </li>
  );
}

export function RecommendPage() {
  const { getToken } = useAuthState();
  const [load, setLoad] = useState<Load>({ state: "loading" });

  useEffect(() => {
    const ctrl = new AbortController();
    apiGet<Recommendations>("/api/recommendations", getToken, ctrl.signal)
      .then((data) => setLoad({ state: "ok", data }))
      .catch(() => {
        if (!ctrl.signal.aborted) setLoad({ state: "error" });
      });
    return () => ctrl.abort();
  }, [getToken]);

  if (load.state === "loading") {
    return <p className="text-sm text-muted-foreground">読み込み中…</p>;
  }
  if (load.state === "error") {
    return (
      <p role="alert" className="rounded-lg border border-warning/50 bg-warning/10 p-3 text-sm">
        AIレコメンドを読み込めませんでした。時間をおいて再度お試しください。
      </p>
    );
  }

  const day = load.data.day;
  return (
    <section aria-labelledby="recommend-heading" className="flex flex-col gap-4">
      <div className="flex flex-wrap items-baseline justify-between gap-2">
        <h2 id="recommend-heading" className="text-sm">
          AIレコメンド
        </h2>
        {day && (
          <p className="text-xs tabular-nums text-muted-foreground">
            {ymd(day.date)}終値の指標から、{jstDateTime(day.created_at)}に作成（{day.model}）
          </p>
        )}
      </div>

      <p role="note" className="rounded-lg border border-warning/50 bg-warning/10 p-3 text-sm">
        {DISCLAIMER}
      </p>

      {day ? (
        <>
          <p className="text-sm text-muted-foreground">
            ルールで絞った候補（株式で、バリュー株か「5年連続で配当あり かつ
            通期で赤字なし」）の指標を AI
            に渡し、増配が期待できて割安な順に10銘柄を選ばせています。数字の計算は AI
            にさせていません。
          </p>
          <ol className="flex flex-col gap-3">
            {day.rows.map((r) => (
              <Item key={r.code} r={r} />
            ))}
          </ol>
        </>
      ) : (
        <p className="rounded-lg border p-6 text-center text-sm text-muted-foreground">
          まだレコメンドがありません。平日の日次バッチが成功した日の 21:00 ごろに作られます。
        </p>
      )}
    </section>
  );
}
