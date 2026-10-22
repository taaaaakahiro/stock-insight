import { useEffect, useMemo, useState } from "react";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";
import { ApiError, apiGet, type ReitDetail, type ReitRateType } from "@/lib/api";
import { useAuthState } from "@/lib/auth";
import { reitSectorLabel } from "@/lib/filters";
import { oku, pct, ymd } from "@/lib/format";
import { cn } from "@/lib/utils";

type Load =
  | { state: "loading" }
  | { state: "ok"; data: ReitDetail }
  | { state: "missing" }
  | { state: "error" };

/** 金利の内訳の並び（色の順も兼ねる。styles.css の --reit-*） */
const MIX: {
  key: keyof ReitDetail["rate_mix"];
  label: string;
  color: string;
  hatch?: boolean;
  hint: string;
}[] = [
  {
    key: "fixed",
    label: "固定（明記）",
    color: "var(--reit-fixed)",
    hint: "有報に固定金利と書かれている借入",
  },
  {
    key: "bonds",
    label: "投資法人債",
    color: "var(--reit-bonds)",
    hint: "ほとんどが固定金利（明記はされていない）",
  },
  {
    key: "swap_fixed",
    label: "スワップで実質固定",
    color: "var(--reit-swap)",
    hint: "変動金利だが金利スワップで固定している借入（借入ごとに書かれている分）",
  },
  {
    key: "swap_amount_only",
    label: "スワップで実質固定（額のみ）",
    color: "var(--reit-swap)",
    hatch: true,
    hint: "スワップの契約額のうち、どの借入か書かれていない分",
  },
  {
    key: "floating",
    label: "変動（明記）",
    color: "var(--reit-floating)",
    hint: "有報に変動金利と書かれている借入。金利が上がると利息がすぐ増える",
  },
  {
    key: "unknown",
    label: "記載なし",
    color: "var(--bar-muted)",
    hatch: true,
    hint: "固定か変動かが有報に書かれていない借入",
  },
];

const HATCH =
  "repeating-linear-gradient(45deg, transparent 0 3px, color-mix(in oklch, var(--background) 70%, transparent) 3px 6px)";

const RATE_LABEL: Record<NonNullable<ReitRateType>, string> = {
  fixed: "固定",
  swap_fixed: "スワップで固定",
  floating: "変動",
};

const LADDER_LABELS = ["1年以内", "1〜2年", "2〜3年", "3〜4年", "4〜5年", "5年超"];

/** REIT の金利の影響（有利子負債の内訳・返済の予定）と物件の稼働率。有報（EDINET）の表から読んだ値 */
export function ReitDebt({ code }: { code: string }) {
  const { getToken } = useAuthState();
  const [load, setLoad] = useState<Load>({ state: "loading" });

  useEffect(() => {
    const ctrl = new AbortController();
    apiGet<ReitDetail>(`/api/stocks/${encodeURIComponent(code)}/reit`, getToken, ctrl.signal)
      .then((data) => setLoad({ state: "ok", data }))
      .catch((e: unknown) => {
        if (ctrl.signal.aborted) return;
        setLoad(
          e instanceof ApiError && e.status === 404 ? { state: "missing" } : { state: "error" },
        );
      });
    return () => ctrl.abort();
  }, [code, getToken]);

  return (
    <section aria-labelledby="reit-debt" className="flex flex-col gap-4">
      <h3 id="reit-debt" className="text-sm">
        金利の影響と物件の稼働率（有価証券報告書から）
      </h3>
      {load.state === "loading" && <p className="text-sm text-muted-foreground">読み込み中…</p>}
      {load.state === "missing" && (
        <p className="text-sm text-muted-foreground">
          この銘柄の有価証券報告書はまだ取り込まれていません。
        </p>
      )}
      {load.state === "error" && (
        <p role="alert" className="rounded-lg border border-warning/50 bg-warning/10 p-3 text-sm">
          読み込めませんでした。時間をおいて再度お試しください。
        </p>
      )}
      {load.state === "ok" && <Body d={load.data} />}
    </section>
  );
}

function Body({ d }: { d: ReitDetail }) {
  const debt = Object.values(d.rate_mix).reduce((a, b) => a + b, 0);
  const share = (v: number) => (debt > 0 ? v / debt : null);
  const sector = reitSectorLabel(d.sector);
  const unreliable = d.checks?.loans === "ずれ" || d.checks?.loans === "明細なし";
  const tiles = [
    { label: "有利子負債", value: oku(debt) },
    {
      label: "1年内に金利が見直される額",
      value: range(oku(d.reprice_1y), oku(d.reprice_1y_max)),
      sub: range(pct(share(d.reprice_1y), 0), pct(share(d.reprice_1y_max), 0)),
    },
    {
      label: "金利+1%で増える年間の利息",
      value: range(oku(d.reprice_1y * 0.01), oku(d.reprice_1y_max * 0.01)),
      sub: "1年内に見直される額 × 1%",
    },
    { label: "平均利率", value: d.avg_rate == null ? "—" : `${d.avg_rate.toFixed(2)}%` },
    { label: "平均残存年数", value: d.avg_years == null ? "—" : `${d.avg_years.toFixed(1)}年` },
    {
      label: "稼働率",
      value: d.occupancy == null ? "—" : `${d.occupancy.toFixed(1)}%`,
      sub: d.property_count > 0 ? `${d.property_count}物件` : undefined,
    },
  ];

  return (
    <>
      <p className="text-xs text-muted-foreground">
        {ymd(d.period_end)} 期末の有価証券報告書（{ymd(d.submitted)}{" "}
        提出）の借入金明細表・物件の表から読んだ値
        {sector ? `。用途: ${sector}` : ""}
        。金額は四捨五入、幅のある値は「記載なし」の借入をどう見るかの差です。
      </p>
      {unreliable && (
        <p role="note" className="rounded-lg border border-warning/50 bg-warning/10 p-3 text-xs">
          借入金明細の合計が貸借対照表と合わなかったため、内訳は参考値です。
        </p>
      )}

      <dl className="grid grid-cols-2 gap-3 sm:grid-cols-3">
        {tiles.map((t) => (
          <div key={t.label} className="rounded-xl border p-3">
            <dt className="text-xs text-muted-foreground">{t.label}</dt>
            <dd className="text-lg tabular-nums">{t.value}</dd>
            {t.sub && <dd className="text-xs text-muted-foreground tabular-nums">{t.sub}</dd>}
          </div>
        ))}
      </dl>

      <MixBar d={d} debt={debt} />
      <Ladder d={d} />
      {d.history.length > 1 && <History d={d} />}
      {d.detail && d.detail.properties.length > 0 && <Properties d={d} />}
      {d.detail && d.detail.loans.length > 0 && <Loans d={d} />}
    </>
  );
}

function range(lo: string, hi: string) {
  return lo === hi ? lo : `${lo}〜${hi}`;
}

/** 有利子負債の金利の内訳。帯の下の表が凡例と値を兼ねる（色だけに頼らない） */
function MixBar({ d, debt }: { d: ReitDetail; debt: number }) {
  const parts = MIX.map((m) => ({ ...m, value: d.rate_mix[m.key] })).filter((m) => m.value > 0);
  if (debt <= 0) return null;
  return (
    <figure className="flex flex-col gap-2">
      <figcaption className="text-xs text-muted-foreground">有利子負債の金利の内訳</figcaption>
      <div
        className="flex h-4 w-full gap-0.5 overflow-hidden rounded"
        role="img"
        aria-label={parts.map((p) => `${p.label} ${pct(p.value / debt, 0)}`).join("、")}
      >
        {parts.map((p) => (
          <div
            key={p.key}
            title={`${p.label}: ${oku(p.value)}（${pct(p.value / debt, 1)}）`}
            style={{
              width: `${(p.value / debt) * 100}%`,
              backgroundColor: p.color,
              backgroundImage: p.hatch ? HATCH : undefined,
            }}
            className="h-full min-w-0.5 first:rounded-l last:rounded-r"
          />
        ))}
      </div>
      <ul className="grid grid-cols-1 gap-x-4 gap-y-1 text-xs sm:grid-cols-2">
        {parts.map((p) => (
          <li key={p.key} className="flex items-center gap-2" title={p.hint}>
            <span
              aria-hidden
              className="inline-block size-3 shrink-0 rounded-sm"
              style={{ backgroundColor: p.color, backgroundImage: p.hatch ? HATCH : undefined }}
            />
            <span className="flex-1">{p.label}</span>
            <span className="tabular-nums">{oku(p.value)}</span>
            <span className="w-12 text-right tabular-nums text-muted-foreground">
              {pct(p.value / debt, 1)}
            </span>
          </li>
        ))}
      </ul>
      {d.detail && d.detail.rate_notes.length > 0 && (
        <p className="text-xs text-muted-foreground">
          判定に使った注記: {d.detail.rate_notes.join("、")}
        </p>
      )}
    </figure>
  );
}

const LADDER_H = 170;
const LADDER_W = 640;
const LADDER_PAD = { left: 8, right: 8, top: 22, bottom: 24 };

/** 返済・償還の予定（長期借入金と投資法人債。短期借入金は1年以内に入れる） */
function Ladder({ d }: { d: ReitDetail }) {
  const buckets = useMemo(() => {
    return LADDER_LABELS.map(
      (_, i) =>
        (i === 0 ? d.balance.short_loans : 0) +
        (d.detail?.ladder ?? []).reduce((sum, row) => sum + (row.amounts[i] ?? 0), 0),
    );
  }, [d]);
  const max = Math.max(...buckets);
  if (max <= 0) return null;
  const slot = (LADDER_W - LADDER_PAD.left - LADDER_PAD.right) / buckets.length;
  const barW = Math.min(56, slot * 0.55);
  const plotH = LADDER_H - LADDER_PAD.top - LADDER_PAD.bottom;
  const base = LADDER_H - LADDER_PAD.bottom;
  return (
    <figure>
      <figcaption className="mb-1 text-xs text-muted-foreground">
        返済・償還の予定（借り換えのときに、その時点の金利になる）
      </figcaption>
      <svg
        viewBox={`0 0 ${LADDER_W} ${LADDER_H}`}
        role="img"
        aria-label={`返済・償還の予定。${buckets.map((v, i) => `${LADDER_LABELS[i]} ${oku(v)}`).join("、")}`}
        className="h-auto w-full max-w-2xl text-xs"
      >
        <line
          x1={LADDER_PAD.left}
          x2={LADDER_W - LADDER_PAD.right}
          y1={base}
          y2={base}
          className="stroke-border"
        />
        {buckets.map((v, i) => {
          const cx = LADDER_PAD.left + slot * (i + 0.5);
          const h = (plotH * v) / max;
          return (
            <g key={LADDER_LABELS[i]}>
              <title>{`${LADDER_LABELS[i]}: ${oku(v)}`}</title>
              {/* 当たり判定は棒より広く */}
              <rect
                x={cx - slot / 2}
                y={LADDER_PAD.top}
                width={slot}
                height={plotH}
                fill="transparent"
              />
              {v > 0 && (
                <rect
                  x={cx - barW / 2}
                  y={base - h}
                  width={barW}
                  height={h}
                  rx={2}
                  className={i === 0 ? "fill-terracotta" : "fill-foreground/70"}
                />
              )}
              <text x={cx} y={base - h - 6} textAnchor="middle" className="fill-foreground">
                {oku(v)}
              </text>
              <text x={cx} y={base + 15} textAnchor="middle" className="fill-muted-foreground">
                {LADDER_LABELS[i]}
              </text>
            </g>
          );
        })}
      </svg>
    </figure>
  );
}

function History({ d }: { d: ReitDetail }) {
  return (
    <div>
      <h4 className="mb-1 text-xs text-muted-foreground">決算ごとの推移</h4>
      <Table>
        <TableHeader>
          <TableRow>
            <TableHead>期末</TableHead>
            <TableHead className="text-right">有利子負債</TableHead>
            <TableHead className="text-right">変動（明記）</TableHead>
            <TableHead className="text-right">記載なし</TableHead>
            <TableHead className="text-right">平均利率</TableHead>
            <TableHead className="text-right">稼働率</TableHead>
          </TableRow>
        </TableHeader>
        <TableBody>
          {[...d.history].reverse().map((h) => {
            const total = h.loans_total + h.bonds;
            return (
              <TableRow key={h.period_end}>
                <TableCell>{ymd(h.period_end)}</TableCell>
                <TableCell className="text-right tabular-nums">{oku(total)}</TableCell>
                <TableCell className="text-right tabular-nums">
                  {pct(total > 0 ? h.rate_floating / total : null, 1)}
                </TableCell>
                <TableCell className="text-right tabular-nums">
                  {pct(total > 0 ? h.rate_unknown / total : null, 1)}
                </TableCell>
                <TableCell className="text-right tabular-nums">
                  {h.avg_rate == null ? "—" : `${h.avg_rate.toFixed(2)}%`}
                </TableCell>
                <TableCell className="text-right tabular-nums">
                  {h.occupancy == null ? "—" : `${h.occupancy.toFixed(1)}%`}
                </TableCell>
              </TableRow>
            );
          })}
        </TableBody>
      </Table>
    </div>
  );
}

type PropSort = "occupancy" | "leasable" | "id";

function Properties({ d }: { d: ReitDetail }) {
  const [sort, setSort] = useState<PropSort>("occupancy");
  const props = d.detail?.properties ?? [];
  const rows = useMemo(() => {
    const list = [...props];
    if (sort === "occupancy") list.sort((a, b) => (a.occupancy ?? 101) - (b.occupancy ?? 101));
    if (sort === "leasable") list.sort((a, b) => b.leasable - a.leasable);
    return list;
  }, [props, sort]);
  const m2 = new Intl.NumberFormat("ja-JP", { maximumFractionDigits: 0 });
  return (
    <details className="group rounded-xl border p-3">
      <summary className="cursor-pointer text-sm">
        物件ごとの稼働率・賃料（{props.length}物件）
      </summary>
      <div className="mt-2 flex flex-wrap items-center gap-2 text-xs">
        <span className="text-muted-foreground">並べ替え:</span>
        {(
          [
            ["occupancy", "稼働率の低い順"],
            ["leasable", "面積の大きい順"],
            ["id", "物件番号順"],
          ] as const
        ).map(([k, label]) => (
          <button
            key={k}
            type="button"
            onClick={() => setSort(k)}
            aria-pressed={sort === k}
            className={cn(
              "rounded-full border px-2 py-0.5",
              sort === k ? "bg-secondary font-medium" : "text-muted-foreground",
            )}
          >
            {label}
          </button>
        ))}
      </div>
      <Table className="mt-2">
        <TableHeader>
          <TableRow>
            <TableHead>物件</TableHead>
            <TableHead className="text-right">賃貸可能面積</TableHead>
            <TableHead className="text-right">稼働率</TableHead>
            <TableHead className="text-right">年間賃料</TableHead>
          </TableRow>
        </TableHeader>
        <TableBody>
          {rows.map((p) => (
            <TableRow key={`${p.id}-${p.name}`}>
              <TableCell className="max-w-64">
                <span className="mr-1 text-xs text-muted-foreground">{p.id}</span>
                {p.name}
              </TableCell>
              <TableCell className="text-right tabular-nums">{m2.format(p.leasable)}㎡</TableCell>
              <TableCell
                className={cn(
                  "text-right tabular-nums",
                  p.occupancy != null && p.occupancy < 90 && "text-expensive",
                )}
              >
                {p.occupancy == null ? "—" : `${p.occupancy.toFixed(1)}%`}
              </TableCell>
              <TableCell className="text-right tabular-nums">
                {p.rent != null ? oku(p.rent) : p.rent_note?.includes("非開示") ? "非開示" : "—"}
              </TableCell>
            </TableRow>
          ))}
        </TableBody>
      </Table>
      <p className="mt-2 text-xs text-muted-foreground">
        稼働率 90% 未満は赤。賃料はテナントの同意がない物件は「非開示」。
      </p>
    </details>
  );
}

function Loans({ d }: { d: ReitDetail }) {
  const loans = d.detail?.loans ?? [];
  return (
    <details className="group rounded-xl border p-3">
      <summary className="cursor-pointer text-sm">借入の明細（{loans.length}件）</summary>
      <Table className="mt-2">
        <TableHeader>
          <TableRow>
            <TableHead>借入先</TableHead>
            <TableHead className="text-right">残高</TableHead>
            <TableHead className="text-right">利率</TableHead>
            <TableHead>返済期限</TableHead>
            <TableHead>金利</TableHead>
          </TableRow>
        </TableHeader>
        <TableBody>
          {loans.map((l, i) => (
            // 同じ借入先・同じ額の借入が並ぶことがあるので位置もキーに入れる
            // biome-ignore lint/suspicious/noArrayIndexKey: 明細の行に一意の ID がない
            <TableRow key={`${l.lender}-${i}`}>
              <TableCell className="max-w-56 truncate" title={l.lender}>
                {l.kind === "short" && (
                  <span className="mr-1 text-xs text-muted-foreground">短期</span>
                )}
                {l.lender}
              </TableCell>
              <TableCell className="text-right tabular-nums">{oku(l.balance)}</TableCell>
              <TableCell className="text-right tabular-nums">
                {l.rate == null ? "—" : `${l.rate.toFixed(3)}%`}
              </TableCell>
              <TableCell className="tabular-nums">{ymd(l.due)}</TableCell>
              <TableCell>{l.rate_type ? RATE_LABEL[l.rate_type] : "記載なし"}</TableCell>
            </TableRow>
          ))}
        </TableBody>
      </Table>
    </details>
  );
}
