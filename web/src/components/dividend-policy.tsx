import { useEffect, useState } from "react";
import { doeNow } from "@/components/results-table";
import { apiGet, type EdinetDoc, type EdinetDocs, type PolicyGap, type StockRow } from "@/lib/api";
import { useAuthState } from "@/lib/auth";
import { growth, pct, points, yen, ymd } from "@/lib/format";
import { cn } from "@/lib/utils";

const KIND_LABEL: Record<EdinetDoc["doc_kind"], string> = {
  annual: "有価証券報告書",
  semiannual: "半期報告書",
};

type Load = { state: "loading" } | { state: "ok"; docs: EdinetDoc[] } | { state: "error" };

/**
 * 銘柄詳細の「配当方針」。有報・半期報告書（EDINET）から拾った方針のバッジと、
 * 今の DOE（実績・会社予想）を DOE の下限・目標と並べ、下限・目標を 1株配当（円）に直して出す。
 * BPS は 株価 ÷ PBR（配当・株価と同じ分割の基準）で出す
 */
export function DividendPolicy({ r }: { r: StockRow }) {
  const { getToken } = useAuthState();
  const [load, setLoad] = useState<Load>({ state: "loading" });

  useEffect(() => {
    const ctrl = new AbortController();
    apiGet<EdinetDocs>(`/api/stocks/${encodeURIComponent(r.code)}/edinet`, getToken, ctrl.signal)
      .then((d) => setLoad({ state: "ok", docs: d.docs }))
      .catch((e: unknown) => {
        if (!(e instanceof DOMException && e.name === "AbortError")) setLoad({ state: "error" });
      });
    return () => ctrl.abort();
  }, [r.code, getToken]);

  if (load.state === "loading") {
    return <p className="text-sm text-muted-foreground">配当方針を読み込み中…</p>;
  }
  if (load.state === "error") {
    return <p className="text-sm text-muted-foreground">配当方針を読み込めませんでした。</p>;
  }
  if (load.docs.length === 0) {
    return (
      <p className="text-sm text-muted-foreground">
        有価証券報告書・半期報告書（EDINET）をまだ取り込んでいません。
      </p>
    );
  }

  // 新しい書類を先に。DOE の下限・目標は一覧と同じく、書かれている新しい書類の値
  const docs = [...load.docs].sort((a, b) => b.submitted.localeCompare(a.submitted));
  const floor = r.doe_floor;
  const goal = r.doe_goal;
  const payoutFloor = r.payout_floor;
  const payoutGoal = r.payout_goal;
  // 下限・目標に分ける前に取り込んだ書類の値（取り直せば消える）
  const payoutOld =
    payoutFloor == null && payoutGoal == null
      ? (docs.find((d) => d.payout_target != null)?.payout_target ?? null)
      : null;
  const badges = [
    docs.some((d) => d.progressive_dividend) && { label: "累進配当", tone: "good" },
    docs.some((d) => d.no_dividend_cut) && { label: "非減配", tone: "good" },
    floor != null && { label: `DOE 下限 ${pct(floor, 1)}`, tone: "doe" },
    goal != null && { label: `DOE 目標 ${pct(goal, 1)}`, tone: "doe" },
    floor == null &&
      goal == null &&
      docs.some((d) => d.doe_target) && {
        label: "DOE（数字なし）",
        tone: "doe",
      },
    payoutFloor != null && { label: `配当性向 下限 ${pct(payoutFloor, 0)}`, tone: "doe" },
    payoutGoal != null && { label: `配当性向 目標 ${pct(payoutGoal, 0)}`, tone: "doe" },
    payoutOld != null && { label: `配当性向 ${pct(payoutOld, 0)}以上`, tone: "doe" },
  ].filter((b): b is { label: string; tone: string } => Boolean(b));
  // docs は空でない（上で返している）
  const source =
    docs.find((d) => d.doe_phrase) ?? docs.find((d) => d.payout_phrase) ?? (docs[0] as EdinetDoc);
  const quotes = [
    source.doe_phrase,
    source.payout_phrase !== source.doe_phrase ? source.payout_phrase : null,
  ].filter((q): q is string => Boolean(q));
  if (quotes.length === 0 && source.phrases[0]) quotes.push(source.phrases[0]);

  return (
    <div className="rounded-xl border p-4">
      <div className="mb-2 flex flex-wrap items-baseline justify-between gap-2">
        <h3 className="text-sm">配当方針</h3>
        <span className="text-xs text-muted-foreground">
          {KIND_LABEL[source.doc_kind]}・{ymd(source.submitted)} 提出
        </span>
      </div>
      {badges.length > 0 ? (
        <ul className="mb-3 flex flex-wrap gap-1.5">
          {badges.map((b) => (
            <li
              key={b.label}
              className={cn(
                "rounded-md px-2 py-0.5 text-xs",
                b.tone === "good" ? "bg-cheap/10 text-cheap" : "bg-primary/10 text-primary",
              )}
            >
              {b.label}
            </li>
          ))}
        </ul>
      ) : (
        <p className="mb-3 text-sm text-muted-foreground">
          累進配当・非減配・DOE・配当性向の下限や目標の記述は見つかりませんでした。
        </p>
      )}
      {r.policy_gap && <GapNote gap={r.policy_gap} />}
      {(floor != null || goal != null) && <DoeCompare r={r} floor={floor} goal={goal} />}
      {(payoutFloor != null || payoutGoal != null) && (
        <PayoutCompare r={r} floor={payoutFloor} goal={payoutGoal} />
      )}
      {quotes.length > 0 && (
        <blockquote className="mt-3 border-t pt-3 text-sm leading-relaxed text-muted-foreground">
          {quotes.map((q) => (
            <p key={q}>「{q}」</p>
          ))}
          <span className="text-xs">（書類管理番号 {source.doc_id}）</span>
        </blockquote>
      )}
      <p className="mt-2 text-xs text-muted-foreground">
        言葉と数字を機械的に拾っています。判断は原文で確かめてください。DOE は 1株配当 ÷ BPS
        の簡易な値で、会社の計算（株主資本の平均など）とは少しずれます。
      </p>
    </div>
  );
}

const GAP_LABEL: Record<PolicyGap["basis"], string> = {
  doe_floor: "DOE の下限",
  doe_goal: "DOE の目標",
  payout_floor: "配当性向の下限",
  payout_goal: "配当性向の目標",
};

/** 方針と今の配当の乖離（方針どおりなら配当が何 % 増えるか）。下限割れは赤で強く出す */
function GapNote({ gap }: { gap: PolicyGap }) {
  const breach = gap.basis.endsWith("_floor");
  const digits = gap.basis.startsWith("doe") ? 1 : 0;
  return (
    <p
      className={cn(
        "mb-3 rounded-lg px-3 py-2 text-sm",
        breach ? "bg-expensive/10 text-expensive" : "bg-primary/10 text-primary",
      )}
    >
      {breach ? "方針の下限を下回っています。" : "方針の目標に届いていません。"}
      {GAP_LABEL[gap.basis]} {pct(gap.policy, digits)} に対して今は {pct(gap.now, digits)}。
      方針どおりなら配当は <span className="font-medium">{growth(gap.upside)}</span>
      （増配の余地）
    </p>
  );
}

/** 今の配当性向（実績・予想）を下限・目標と比べ、下限・目標を 1株配当（円）に直す。EPS は 株価 ÷ PER */
function PayoutCompare({
  r,
  floor,
  goal,
}: {
  r: StockRow;
  floor: number | null;
  goal: number | null;
}) {
  const now = r.dividend_basis === "forecast" ? r.payout_forecast : r.payout_ratio;
  const eps = r.per != null && r.per > 0 ? r.price / r.per : null;
  const dpsAt = (payout: number | null) => (eps != null && payout != null ? eps * payout : null);
  const epsNote = (payout: number) =>
    eps != null ? `EPS ${yen(eps)} × ${pct(payout, 0)}` : "EPS が出ていません（赤字など）";
  const diff = now != null && floor != null ? now - floor : null;
  return (
    <div className="mt-3 border-t pt-3">
      <p className="mb-1 text-xs text-muted-foreground">配当性向（1株配当 ÷ EPS）</p>
      <p className="mb-2 text-sm">
        今 <span className="tabular-nums">{pct(now, 1)}</span>
        <span className="ml-1 text-xs text-muted-foreground">
          （{r.dividend_basis === "forecast" ? "会社予想の配当 ÷ 直近の本決算の EPS" : "実績"}）
        </span>
        {floor != null && (
          <span className="ml-3">
            下限 <span className="tabular-nums">{pct(floor, 0)}</span>
          </span>
        )}
        {goal != null && (
          <span className="ml-3">
            目標 <span className="tabular-nums">{pct(goal, 0)}</span>
          </span>
        )}
      </p>
      {diff != null && (
        <p className="mb-3 text-sm">
          下限まで{" "}
          <span className={cn("font-medium", diff < 0 ? "text-expensive" : "text-cheap")}>
            {points(diff)}
          </span>
          {diff < 0 ? "（下限の方針より配当が少ない）" : " の余裕"}
        </p>
      )}
      <dl className="grid grid-cols-2 gap-2 sm:grid-cols-3">
        {floor != null && (
          <Amount
            label="下限の配当性向で守られる配当"
            dps={dpsAt(floor)}
            price={r.price}
            note={epsNote(floor)}
          />
        )}
        {goal != null && (
          <Amount
            label="目標の配当性向での配当"
            dps={dpsAt(goal)}
            price={r.price}
            note={epsNote(goal)}
          />
        )}
      </dl>
    </div>
  );
}

/** 今の DOE（実績・予想）を下限・目標と同じ物差しで並べ、下限・目標を円に直す */
function DoeCompare({
  r,
  floor,
  goal,
}: {
  r: StockRow;
  floor: number | null;
  goal: number | null;
}) {
  const now = doeNow(r);
  const values = [r.doe, r.doe_forecast, floor, goal].filter((v): v is number => v != null);
  // 物差しの右端: いちばん大きい値の 1.25 倍を 1% 単位で切り上げ
  const max = Math.max(0.01, Math.ceil(Math.max(...values) * 1.25 * 100) / 100);
  const at = (v: number) => `${Math.min(100, (v / max) * 100)}%`;
  const bps = r.pbr != null && r.pbr > 0 ? r.price / r.pbr : null;
  const dpsAt = (doe: number | null) => (bps != null && doe != null ? bps * doe : null);
  const diff = now != null && floor != null ? now - floor : null;

  return (
    <div>
      <p className="mb-1 text-xs text-muted-foreground">DOE（1株配当 ÷ BPS）</p>
      <div
        className="relative mx-1 h-12"
        role="img"
        aria-label={[
          r.doe != null && `実績 ${pct(r.doe)}`,
          r.doe_forecast != null && `予想 ${pct(r.doe_forecast)}`,
          floor != null && `下限 ${pct(floor, 1)}`,
          goal != null && `目標 ${pct(goal, 1)}`,
        ]
          .filter(Boolean)
          .join("、")}
      >
        <div className="absolute inset-x-0 top-6 h-1.5 rounded-full border bg-muted" />
        {floor != null && (
          <>
            <div
              className="absolute top-6 left-0 h-1.5 rounded-l-full bg-expensive/20"
              style={{ width: at(floor) }}
            />
            <Marker
              left={at(floor)}
              label={`下限 ${pct(floor, 1)}`}
              className="text-expensive"
              line
            />
          </>
        )}
        {goal != null && (
          <Marker
            left={at(goal)}
            label={`目標 ${pct(goal, 1)}`}
            className="text-primary"
            line
            dashed
          />
        )}
        {r.doe != null && <Dot left={at(r.doe)} className="bg-muted-foreground" />}
        {r.doe_forecast != null && <Dot left={at(r.doe_forecast)} className="bg-primary" />}
      </div>
      <div className="mx-1 mb-2 flex justify-between text-[11px] text-muted-foreground">
        <span>0%</span>
        <span className="flex gap-3">
          {r.doe != null && (
            <span>
              <span className="mr-1 inline-block size-2 rounded-full bg-muted-foreground" />
              実績 {pct(r.doe)}
            </span>
          )}
          {r.doe_forecast != null && (
            <span>
              <span className="mr-1 inline-block size-2 rounded-full bg-primary" />
              予想 {pct(r.doe_forecast)}
            </span>
          )}
        </span>
        <span>{pct(max, 0)}</span>
      </div>
      {diff != null && (
        <p className="mb-3 text-sm">
          下限まで{" "}
          <span className={cn("font-medium", diff < 0 ? "text-expensive" : "text-cheap")}>
            {points(diff)}
          </span>
          {diff < 0 ? "（下限の方針より配当が少ない）" : " の余裕"}
          <span className="ml-1 text-xs text-muted-foreground">
            （{r.doe_forecast != null ? "会社予想" : "実績"}の配当で比較）
          </span>
        </p>
      )}
      <dl className="grid grid-cols-2 gap-2 sm:grid-cols-3">
        <Amount
          label="今の配当"
          dps={r.dividend}
          price={r.price}
          note={r.dividend_basis === "forecast" ? "予想" : "実績"}
        />
        {floor != null && (
          <Amount
            label="下限の DOE で守られる配当"
            dps={dpsAt(floor)}
            price={r.price}
            note={bpsNote(bps, floor)}
          />
        )}
        {goal != null && (
          <Amount
            label="目標の DOE での配当"
            dps={dpsAt(goal)}
            price={r.price}
            note={bpsNote(bps, goal)}
          />
        )}
      </dl>
    </div>
  );
}

function bpsNote(bps: number | null, doe: number) {
  return bps != null ? `BPS ${yen(bps)} × ${pct(doe, 1)}` : "BPS が出ていません";
}

function Marker({
  left,
  label,
  className,
  line,
  dashed,
}: {
  left: string;
  label: string;
  className: string;
  line?: boolean;
  dashed?: boolean;
}) {
  return (
    <>
      {line && (
        <div
          className={cn("absolute top-4 h-5 border-l-2", dashed && "border-dashed", className)}
          style={{ left, borderColor: "currentColor" }}
        />
      )}
      <div
        className={cn("absolute top-0 -translate-x-1/2 text-[11px] whitespace-nowrap", className)}
        style={{ left }}
      >
        {label}
      </div>
    </>
  );
}

function Dot({ left, className }: { left: string; className: string }) {
  return (
    <div
      className={cn("absolute top-[1.3rem] size-3.5 -translate-x-1/2 rounded-full", className)}
      style={{ left }}
    />
  );
}

function Amount({
  label,
  dps,
  price,
  note,
}: {
  label: string;
  dps: number | null;
  price: number;
  note: string;
}) {
  return (
    <div className="rounded-lg bg-muted/60 p-2.5">
      <dt className="text-xs text-muted-foreground">{label}</dt>
      <dd className="text-lg tabular-nums">{yen(dps)}</dd>
      <dd className="text-xs text-muted-foreground">
        {dps != null && price > 0 ? `利回り ${pct(dps / price)}・` : ""}
        {note}
      </dd>
    </div>
  );
}
