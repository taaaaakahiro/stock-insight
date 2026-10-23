import { useEffect, useState } from "react";
import { DocPreview } from "@/components/doc-preview";
import { PortfolioToggle } from "@/components/portfolio-toggle";
import { Badge } from "@/components/ui/badge";
import { apiGet, type Grade, type Recommendation, type Recommendations } from "@/lib/api";
import { useAuthState } from "@/lib/auth";
import { count, growth, jstDateTime, pct, times, yen, ymd } from "@/lib/format";
import { navigate } from "@/lib/router";
import { cn } from "@/lib/utils";

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

/** 一覧と同じ5年分のバッジ（当てはまるものだけ）。一過性の疑いは赤で */
function Flags({ r }: { r: Recommendation }) {
  const flags = [
    r.is_value && "バリュー",
    r.dividend_every_year && "5年配当",
    r.no_loss_years && "通期黒字",
    r.no_quarterly_loss && "四半期黒字",
    r.profit_uptrend && "増益傾向",
  ].filter((f): f is string => typeof f === "string");
  if (flags.length === 0 && !r.profit_one_off) return null;
  return (
    <span className="flex flex-wrap gap-1">
      {flags.map((f) => (
        <Badge key={f} variant="outline" className="border-cheap/40 bg-cheap/10 text-cheap">
          {f}
        </Badge>
      ))}
      {r.profit_one_off && (
        <Badge
          variant="outline"
          className="border-expensive/40 bg-expensive/10 text-expensive"
          title="直近の四半期の利益が一過性の疑い（純利益の伸びが営業利益の伸びを大きく上回る、または直近の四半期だけ増益で4四半期の合計は減益）"
        >
          一過性?
        </Badge>
      )}
    </span>
  );
}

const GRADES: Record<Grade, { stars: string; label: string; className: string; hint: string }> = {
  strong: {
    stars: "★★★",
    label: "強く推奨",
    className: "bg-cheap/15 text-cheap",
    hint: "会社の配当方針（DOE・配当性向の下限・目標）と今の配当の差が大きく（下限割れ、または方針どおりなら配当 +20% 以上）、業績の質（通期黒字・増益・一過性でない）を満たし、割安",
  },
  recommend: {
    stars: "★★",
    label: "推奨",
    className: "bg-primary/10 text-primary",
    hint: "業績の質を満たし、増配の根拠（方針との差・2年以上の連続増配・累進配当の方針）がある",
  },
  watch: {
    stars: "★",
    label: "注目",
    className: "bg-muted text-muted-foreground",
    hint: "割安で増配も期待できるが、一過性の疑いがあるか、業績の判定材料が足りない",
  },
};

/** 3段階の評価（★の数）。評価を付ける前の日の結果にはない */
function GradeBadge({ grade }: { grade: Grade | null }) {
  if (!grade) return null;
  const g = GRADES[grade];
  return (
    <span
      className={cn("rounded-md px-2 py-0.5 text-xs font-medium whitespace-nowrap", g.className)}
      title={g.hint}
    >
      <span aria-hidden>{g.stars}</span> {g.label}
    </span>
  );
}

const GAP_LABEL: Record<NonNullable<Recommendation["policy_gap"]>["basis"], string> = {
  doe_floor: "DOE の下限",
  doe_goal: "DOE の目標",
  payout_floor: "配当性向の下限",
  payout_goal: "配当性向の目標",
};

/** 評価の根拠（方針との差と直近四半期の伸び）。理由の文章とは別に、数字で確かめられるようにする */
function Basis({ r }: { r: Recommendation }) {
  const gap = r.policy_gap;
  const items = [
    gap && {
      key: "gap",
      text: `${GAP_LABEL[gap.basis]} ${pct(gap.policy, gap.basis.startsWith("doe") ? 1 : 0)} に対して今は ${pct(gap.now, gap.basis.startsWith("doe") ? 1 : 0)}。方針どおりなら配当 ${growth(gap.upside)}`,
      className: gap.basis.endsWith("_floor") ? "text-expensive" : "text-primary",
    },
    r.op_growth_q != null && {
      key: "q",
      text: `直近四半期の営業利益 ${growth(r.op_growth_q)}`,
      className: r.op_growth_q >= 0 ? "text-cheap" : "text-expensive",
    },
    r.op_growth_ttm != null && {
      key: "ttm",
      text: `直近4四半期の合計 ${growth(r.op_growth_ttm)}`,
      className: r.op_growth_ttm >= 0 ? "text-cheap" : "text-expensive",
    },
  ].filter((i): i is { key: string; text: string; className: string } => Boolean(i));
  if (items.length === 0) return null;
  return (
    <ul className="flex flex-wrap gap-x-3 gap-y-0.5 text-xs">
      {items.map((i) => (
        <li key={i.key} className={cn("tabular-nums", i.className)}>
          {i.text}
        </li>
      ))}
    </ul>
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
            <DocPreview r={r} />
            <span className="text-xs text-muted-foreground">{sub}</span>
            <GradeBadge grade={r.grade} />
            <Flags r={r} />
            {r.themes.map((t) => (
              <Badge
                key={t}
                variant="secondary"
                title="管理者が登録した注目テーマ。有価証券報告書の「事業の内容」にテーマの言葉がある"
              >
                #{t}
              </Badge>
            ))}
          </div>
          <p className="text-sm">{r.reason}</p>
          <Basis r={r} />
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
            ルールで絞った候補（株式で、配当利回りが8%未満、配当性向50%以下・PBR1.5倍以下、減配なし、バリュー株か「5年連続で配当あり
            かつ 通期で赤字なし」）を、評価（★★★ 強く推奨 → ★★ 推奨 → ★
            注目）と、注目テーマ（同じ評価の中で先）・会社の配当方針との差・業績の質の順に並べて上位10銘柄を選んでいます。評価と順位はルールで決め、AI
            には有価証券報告書の配当方針の文と直近の四半期の数字を渡して、理由の文章だけを書かせています。理由を書けなかった銘柄は除くので、10銘柄より少ない日もあります。
          </p>
          <ul className="flex flex-wrap gap-x-4 gap-y-1 text-xs text-muted-foreground">
            {(Object.keys(GRADES) as Grade[]).map((g) => (
              <li key={g}>
                <span aria-hidden>{GRADES[g].stars}</span> {GRADES[g].label}: {GRADES[g].hint}
              </li>
            ))}
          </ul>
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
