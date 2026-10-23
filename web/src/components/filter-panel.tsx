import { Check, ChevronDown, RotateCcw } from "lucide-react";
import { useId, useState } from "react";
import { InfoTip } from "@/components/info-tip";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import {
  DEFAULT_FILTERS,
  type Filters,
  FLAGS,
  isInvalidInput,
  MARKETS,
  type MetricKey,
  metricGroupsFor,
  metricLabel,
  metricsFor,
  metricTip,
  REIT_SECTORS,
  SECTORS33,
  SIZE_TIP,
  SIZES,
  UNIT_SUFFIX,
  type Valuation,
  yieldPercentileTip,
} from "@/lib/filters";
import { cn } from "@/lib/utils";

type Props = {
  value: Filters;
  onChange: (next: Filters) => void;
};

const ALL = "all";

function toggle(list: string[], item: string, on: boolean): string[] {
  return on ? [...new Set([...list, item])] : list.filter((x) => x !== item);
}

/** 押して入れ・切りする条件（チェックボックスの縦並びより場所を取らず、スマホでも押しやすい） */
function Chip({
  on,
  onToggle,
  title,
  children,
}: {
  on: boolean;
  onToggle: (on: boolean) => void;
  title?: string;
  children: React.ReactNode;
}) {
  return (
    <button
      type="button"
      aria-pressed={on}
      title={title}
      onClick={() => onToggle(!on)}
      className={cn(
        "inline-flex min-h-8 items-center gap-1 rounded-full border px-3 py-1 text-xs transition-colors outline-none focus-visible:ring-2 focus-visible:ring-ring/50",
        on
          ? "border-primary bg-primary/15 text-foreground"
          : "text-muted-foreground hover:text-foreground",
      )}
    >
      {on && <Check aria-hidden className="size-3" />}
      {children}
    </button>
  );
}

function ChipGroup({ legend, children }: { legend: React.ReactNode; children: React.ReactNode }) {
  return (
    <fieldset className="flex flex-col gap-2">
      <legend className="mb-2 flex items-center gap-1 text-sm font-medium">{legend}</legend>
      <div className="flex flex-wrap gap-1.5">{children}</div>
    </fieldset>
  );
}

/**
 * 畳めるまとまり。最初に開くかは表示したときだけ決め、あとは利用者に任せる
 * （open を条件から毎回決めると、入力を消した瞬間に閉じてしまう）
 */
function Section({
  title,
  count,
  defaultOpen,
  children,
}: {
  title: string;
  count: number;
  defaultOpen: boolean;
  children: React.ReactNode;
}) {
  const [open, setOpen] = useState(defaultOpen);
  return (
    <details
      className="group border-t pt-3"
      open={open}
      onToggle={(e) => setOpen(e.currentTarget.open)}
    >
      <summary className="flex cursor-pointer list-none items-center justify-between text-sm font-medium">
        <span>
          {title}
          {count > 0 && (
            <span className="ml-1.5 rounded-full bg-primary/15 px-1.5 text-xs tabular-nums">
              {count}
            </span>
          )}
        </span>
        <ChevronDown aria-hidden className="size-4 transition-transform group-open:rotate-180" />
      </summary>
      <div className="mt-3 flex flex-col gap-3">{children}</div>
    </details>
  );
}

export function FilterPanel({ value, onChange }: Props) {
  // 条件を変えたら1ページ目に戻す
  const set = (patch: Partial<Filters>) => onChange({ ...value, ...patch, page: 1 });
  const setRange = (key: keyof Filters["ranges"], v: string) =>
    set({ ranges: { ...value.ranges, [key]: v } });
  const stock = value.kind === "stock";
  // スマホのシートと PC の横の欄で同時に描くことがあるので、id は重ならないようにする
  const valuationId = useId();
  const filled = (key: MetricKey) =>
    Boolean(value.ranges[`${key}_min`]?.trim() || value.ranges[`${key}_max`]?.trim());
  const units = new Map(metricsFor(value.kind).map((m) => [m.key, m.unit]));

  return (
    <form
      className="flex flex-col gap-3"
      onSubmit={(e) => e.preventDefault()}
      aria-label="絞り込み"
    >
      <div className="flex items-center justify-between pb-1">
        <h2 className="text-sm">絞り込み</h2>
        <Button
          type="button"
          variant="ghost"
          size="sm"
          onClick={() =>
            onChange({
              ...DEFAULT_FILTERS,
              kind: value.kind,
              query: value.query,
              sort: value.sort,
              order: value.order,
            })
          }
        >
          <RotateCcw aria-hidden />
          リセット
        </Button>
      </div>

      {stock && (
        <Section title="判定" count={FLAGS.filter((f) => value[f.key]).length} defaultOpen>
          <div className="flex flex-wrap gap-1.5">
            {FLAGS.map((f) => (
              <Chip
                key={f.key}
                on={value[f.key]}
                title={f.hint}
                onToggle={(on) => set({ [f.key]: on })}
              >
                {f.label}
              </Chip>
            ))}
          </div>
        </Section>
      )}

      {stock && (
        <Section
          title="市場・規模"
          count={value.markets.length + value.sizes.length}
          defaultOpen={value.markets.length + value.sizes.length > 0}
        >
          <ChipGroup legend="市場">
            {MARKETS.map((m) => (
              <Chip
                key={m}
                on={value.markets.includes(m)}
                onToggle={(on) => set({ markets: toggle(value.markets, m, on) })}
              >
                {m}
              </Chip>
            ))}
          </ChipGroup>
          <ChipGroup
            legend={
              <>
                規模
                <span className="font-normal text-muted-foreground">（TOPIX の区分）</span>
                <InfoTip label="規模" text={SIZE_TIP} />
              </>
            }
          >
            {SIZES.map((s) => (
              <Chip
                key={s.key}
                on={value.sizes.includes(s.key)}
                title={s.hint}
                onToggle={(on) => set({ sizes: toggle(value.sizes, s.key, on) })}
              >
                {s.label}
              </Chip>
            ))}
          </ChipGroup>
        </Section>
      )}

      {stock && (
        <Section title="業種" count={value.sectors.length} defaultOpen={value.sectors.length > 0}>
          <div className="grid grid-cols-2 gap-x-2 gap-y-2">
            {SECTORS33.map((s) => (
              <Label key={s} className="font-normal text-xs">
                <Checkbox
                  checked={value.sectors.includes(s)}
                  onCheckedChange={(c) => set({ sectors: toggle(value.sectors, s, c === true) })}
                />
                {s}
              </Label>
            ))}
          </div>
        </Section>
      )}

      {value.kind === "reit" && (
        <Section
          title="用途"
          count={value.reitSectors.length}
          defaultOpen={value.reitSectors.length > 0}
        >
          <div className="flex flex-wrap gap-1.5">
            {REIT_SECTORS.map((s) => (
              <Chip
                key={s.key}
                on={value.reitSectors.includes(s.key)}
                onToggle={(on) => set({ reitSectors: toggle(value.reitSectors, s.key, on) })}
              >
                {s.label}
              </Chip>
            ))}
          </div>
        </Section>
      )}

      {metricGroupsFor(value.kind).map((g) => {
        // 利回りの過去5年比（高い・ふつう・低い）は「割安さ」に入れる
        const withValuation = g.key === "value";
        const count = g.keys.filter(filled).length + (withValuation && value.valuation ? 1 : 0);
        return (
          <Section
            key={g.key}
            title={g.label}
            count={count}
            defaultOpen={g.open === true || count > 0}
          >
            {withValuation && (
              <div className="flex flex-col gap-1.5">
                <span className="flex items-center gap-1 text-xs text-muted-foreground">
                  <Label htmlFor={valuationId} className="font-normal">
                    {stock ? "配当" : "分配金"}利回りの過去5年比
                  </Label>
                  <InfoTip label="利回りの過去5年比" text={yieldPercentileTip(value.kind)} />
                </span>
                <Select
                  value={value.valuation || ALL}
                  onValueChange={(v) => set({ valuation: (v === ALL ? "" : v) as Valuation })}
                >
                  <SelectTrigger id={valuationId} className="w-full">
                    <SelectValue />
                  </SelectTrigger>
                  <SelectContent>
                    <SelectItem value={ALL}>すべて</SelectItem>
                    <SelectItem value="cheap">高い（上位20%。株価は過去と比べて安い）</SelectItem>
                    <SelectItem value="neutral">ふつう</SelectItem>
                    <SelectItem value="expensive">
                      低い（下位20%。株価は過去と比べて高い）
                    </SelectItem>
                  </SelectContent>
                </Select>
              </div>
            )}
            {g.keys.map((key) => {
              const minKey = `${key}_min` as const;
              const maxKey = `${key}_max` as const;
              const label = metricLabel(key, value.kind);
              const unit = units.get(key);
              const tip = metricTip(key, value.kind);
              // 名前と下限・上限を1行に並べ、縦の長さを半分にする
              return (
                <div
                  key={key}
                  className="grid grid-cols-[1fr_4.25rem_auto_4.25rem] items-center gap-x-1.5"
                >
                  <span className="flex min-w-0 items-center gap-1 text-xs leading-tight text-muted-foreground">
                    <span>
                      {label}
                      {unit && <span className="whitespace-nowrap">（{UNIT_SUFFIX[unit]}）</span>}
                    </span>
                    {tip && <InfoTip label={label} text={tip} />}
                  </span>
                  <Input
                    inputMode="decimal"
                    placeholder="下限"
                    aria-label={`${label}の下限`}
                    aria-invalid={isInvalidInput(value.ranges[minKey]) || undefined}
                    value={value.ranges[minKey] ?? ""}
                    onChange={(e) => setRange(minKey, e.target.value)}
                    className="h-8 px-2 text-right tabular-nums"
                  />
                  <span className="text-xs text-muted-foreground" aria-hidden>
                    〜
                  </span>
                  <Input
                    inputMode="decimal"
                    placeholder="上限"
                    aria-label={`${label}の上限`}
                    aria-invalid={isInvalidInput(value.ranges[maxKey]) || undefined}
                    value={value.ranges[maxKey] ?? ""}
                    onChange={(e) => setRange(maxKey, e.target.value)}
                    className="h-8 px-2 text-right tabular-nums"
                  />
                </div>
              );
            })}
          </Section>
        );
      })}
    </form>
  );
}
