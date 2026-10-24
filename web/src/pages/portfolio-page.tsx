import { useEffect, useState } from "react";
import { DocPreview } from "@/components/doc-preview";
import { PortfolioToggle } from "@/components/portfolio-toggle";
import { DividendAmount, DoeMargin, doeNow, NameCell } from "@/components/results-table";
import { Badge } from "@/components/ui/badge";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";
import { apiGet, type Holding, type Portfolio, type SellSignal } from "@/lib/api";
import { useAuthState } from "@/lib/auth";
import { pct, times, yen } from "@/lib/format";
import { usePortfolio } from "@/lib/portfolio";
import { cn } from "@/lib/utils";

type Load = { state: "loading" } | { state: "ok"; data: Portfolio } | { state: "error" };

/** 売り時のサイン。判定の中身は domain::portfolio と合わせる（主は配当性向・PBR・赤字配当、ほかは参考） */
const SIGNALS: Record<SellSignal, { label: string; hint: string }> = {
  high_payout: {
    label: "配当性向50%超",
    hint: "利益の半分以上を配当に回していて、増配の余地が小さい",
  },
  high_pbr: { label: "PBR1.5倍超", hint: "株価が1株あたり純資産の1.5倍を超えている" },
  loss_dividend: {
    label: "赤字で配当",
    hint: "最新の本決算が赤字（EPS が0以下）なのに配当を出している",
  },
  expensive: { label: "割高", hint: "配当利回りが過去5年の下位20%（株価が上がっている）" },
  dividend_cut: {
    label: "減配",
    hint: "直近の配当が前期より減った、または会社予想が直近の配当を下回る（据え置きは含めない）",
  },
  profit_downtrend: { label: "減益傾向", hint: "直近の営業利益の流れが下向き" },
  loss: { label: "赤字あり", hint: "直近5期または20四半期に赤字がある" },
};

function Signals({ h }: { h: Holding }) {
  return (
    <span className="flex flex-col gap-1">
      <span className="flex flex-wrap gap-1">
        {h.signals.length === 0 ? (
          <Badge variant="outline" className="border-cheap/40 bg-cheap/10 text-cheap">
            売りサインなし
          </Badge>
        ) : (
          h.signals.map((s) => (
            <Badge
              key={s}
              variant="outline"
              title={SIGNALS[s].hint}
              className="border-expensive/40 bg-expensive/10 text-expensive"
            >
              {SIGNALS[s].label}
            </Badge>
          ))
        )}
      </span>
      {h.reference_signals.length > 0 && (
        <span className="flex flex-wrap items-center gap-1 text-xs text-muted-foreground">
          参考:
          {h.reference_signals.map((s) => (
            <Badge key={s} variant="outline" title={SIGNALS[s].hint} className="font-normal">
              {SIGNALS[s].label}
            </Badge>
          ))}
        </span>
      )}
    </span>
  );
}

/** 赤字で配当を出しているときは、計算できない「—」の代わりに「赤字」と出す */
function Payout({ h }: { h: Holding }) {
  if (h.signals.includes("loss_dividend")) {
    return <span className="font-medium text-expensive">赤字</span>;
  }
  const warn = h.signals.includes("high_payout");
  return <span className={cn(warn && "font-medium text-expensive")}>{pct(h.payout_ratio, 1)}</span>;
}

/** 今の DOE（予想があれば予想）と、有報の DOE の下限・余裕 */
function Doe({ h }: { h: Holding }) {
  if (h.kind !== "stock") return pct(null);
  return (
    <span>
      {pct(doeNow(h))}
      {h.doe_floor != null && (
        <span className="block text-xs text-muted-foreground">
          下限 {pct(h.doe_floor, 1)}・<DoeMargin r={h} />
        </span>
      )}
    </span>
  );
}

function Pbr({ h }: { h: Holding }) {
  const warn = h.signals.includes("high_pbr");
  return <span className={cn(warn && "font-medium text-expensive")}>{times(h.pbr)}</span>;
}

export function PortfolioPage() {
  const { getToken } = useAuthState();
  const { version, error } = usePortfolio();
  const [load, setLoad] = useState<Load>({ state: "loading" });

  // 登録・解除のたびに取り直す（version が合図）
  // biome-ignore lint/correctness/useExhaustiveDependencies: version は中で使わないが、変わったら取り直す
  useEffect(() => {
    const ctrl = new AbortController();
    apiGet<Portfolio>("/api/portfolio", getToken, ctrl.signal)
      .then((data) => setLoad({ state: "ok", data }))
      .catch(() => {
        if (!ctrl.signal.aborted) setLoad({ state: "error" });
      });
    return () => ctrl.abort();
  }, [getToken, version]);

  if (load.state === "loading") {
    return <p className="text-sm text-muted-foreground">読み込み中…</p>;
  }
  if (load.state === "error") {
    return (
      <p role="alert" className="rounded-lg border border-warning/50 bg-warning/10 p-3 text-sm">
        ポートフォリオを読み込めませんでした。時間をおいて再度お試しください。
      </p>
    );
  }

  const { rows, codes } = load.data;
  const flagged = rows.filter((h) => h.signals.length > 0).length;
  const shown = new Set(rows.map((h) => h.code));
  const pending = codes.filter((c) => !shown.has(c));

  if (codes.length === 0) {
    return (
      <p className="rounded-lg border p-6 text-center text-sm text-muted-foreground">
        まだ銘柄がありません。スクリーニングの一覧で ☆ を押すと、ここに入ります。
      </p>
    );
  }

  return (
    <section aria-labelledby="portfolio-heading" className="flex flex-col gap-4">
      <div className="flex flex-wrap items-baseline justify-between gap-2">
        <h2 id="portfolio-heading" className="text-sm">
          ポートフォリオ
        </h2>
        <p className="text-sm tabular-nums" aria-live="polite">
          {codes.length}銘柄のうち
          <span className={cn("mx-1 font-medium", flagged > 0 && "text-expensive")}>
            {flagged}銘柄
          </span>
          に売りサイン
        </p>
      </div>

      {error && (
        <p role="alert" className="rounded-lg border border-warning/50 bg-warning/10 p-3 text-sm">
          {error}
        </p>
      )}

      <div className="hidden md:block">
        <Table>
          <TableHeader>
            <TableRow>
              <TableHead>銘柄</TableHead>
              <TableHead className="text-right">株価</TableHead>
              <TableHead className="text-right">1株配当</TableHead>
              <TableHead className="text-right">配当利回り</TableHead>
              <TableHead className="text-right">配当性向</TableHead>
              <TableHead className="text-right">PBR</TableHead>
              <TableHead className="text-right">DOE</TableHead>
              <TableHead>売りサイン</TableHead>
            </TableRow>
          </TableHeader>
          <TableBody>
            {rows.map((h) => (
              <TableRow key={h.code} className={cn(h.signals.length > 0 && "bg-expensive/5")}>
                <TableCell className="max-w-72">
                  <div className="flex items-center gap-1">
                    <PortfolioToggle code={h.code} name={h.name ?? h.code} />
                    <NameCell r={h} />
                    {h.kind === "stock" && <DocPreview r={h} />}
                  </div>
                </TableCell>
                <TableCell className="text-right tabular-nums">{yen(h.price)}</TableCell>
                <TableCell className="text-right tabular-nums">
                  <DividendAmount r={h} />
                </TableCell>
                <TableCell className="text-right tabular-nums">{pct(h.dividend_yield)}</TableCell>
                <TableCell className="text-right tabular-nums">
                  <Payout h={h} />
                </TableCell>
                <TableCell className="text-right tabular-nums">
                  <Pbr h={h} />
                </TableCell>
                <TableCell className="text-right tabular-nums">
                  <Doe h={h} />
                </TableCell>
                <TableCell>
                  <Signals h={h} />
                </TableCell>
              </TableRow>
            ))}
          </TableBody>
        </Table>
      </div>

      <ul className="flex flex-col gap-3 md:hidden">
        {rows.map((h) => (
          <li
            key={h.code}
            className={cn(
              "rounded-xl border p-4",
              h.signals.length > 0 && "border-expensive/40 bg-expensive/5",
            )}
          >
            <div className="mb-2 flex min-w-0 items-start gap-1">
              <PortfolioToggle code={h.code} name={h.name ?? h.code} />
              <NameCell r={h} wrap />
              {h.kind === "stock" && <DocPreview r={h} />}
            </div>
            <div className="mb-3">
              <Signals h={h} />
            </div>
            <dl className="grid grid-cols-3 gap-x-3 gap-y-2 text-sm">
              <div>
                <dt className="text-xs text-muted-foreground">株価</dt>
                <dd className="tabular-nums">{yen(h.price)}</dd>
              </div>
              <div>
                <dt className="text-xs text-muted-foreground">利回り</dt>
                <dd className="tabular-nums">
                  {pct(h.dividend_yield)}
                  <div className="text-xs text-muted-foreground">
                    <DividendAmount r={h} />
                  </div>
                </dd>
              </div>
              <div>
                <dt className="text-xs text-muted-foreground">配当性向</dt>
                <dd className="tabular-nums">
                  <Payout h={h} />
                </dd>
              </div>
              <div>
                <dt className="text-xs text-muted-foreground">PBR</dt>
                <dd className="tabular-nums">
                  <Pbr h={h} />
                </dd>
              </div>
              <div className="col-span-2">
                <dt className="text-xs text-muted-foreground">DOE</dt>
                <dd className="tabular-nums">
                  <Doe h={h} />
                </dd>
              </div>
            </dl>
          </li>
        ))}
      </ul>

      {pending.length > 0 && (
        <div className="text-sm text-muted-foreground">
          <p className="mb-1">指標がまだない銘柄（次の日次バッチのあとに出ます）</p>
          <ul className="flex flex-wrap gap-2">
            {pending.map((c) => (
              <li key={c} className="flex items-center gap-1 rounded-lg border px-2">
                {c}
                <PortfolioToggle code={c} name={c} />
              </li>
            ))}
          </ul>
        </div>
      )}

      <p className="text-xs text-muted-foreground">
        売りサインは配当性向50%超・PBR1.5倍超・赤字で配当の3つで、株式だけに出します。参考のサインは判断の材料です。どれも最新の指標だけで出す目安です（取得価格は見ません）。売買の判断はご自身で行ってください。
      </p>
    </section>
  );
}
