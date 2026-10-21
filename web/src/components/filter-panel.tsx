import { ChevronDown, RotateCcw, Search } from "lucide-react";
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
  metricLabel,
  metricsFor,
  SECTORS33,
  SIZES,
  UNIT_SUFFIX,
  type Valuation,
} from "@/lib/filters";

type Props = {
  value: Filters;
  onChange: (next: Filters) => void;
};

const ALL = "all";

function toggle(list: string[], item: string, on: boolean): string[] {
  return on ? [...new Set([...list, item])] : list.filter((x) => x !== item);
}

export function FilterPanel({ value, onChange }: Props) {
  // 条件を変えたら1ページ目に戻す
  const set = (patch: Partial<Filters>) => onChange({ ...value, ...patch, page: 1 });
  const setRange = (key: keyof Filters["ranges"], v: string) =>
    set({ ranges: { ...value.ranges, [key]: v } });
  const stock = value.kind === "stock";

  return (
    <form
      className="flex flex-col gap-5"
      onSubmit={(e) => e.preventDefault()}
      aria-label="絞り込み"
    >
      <div className="flex items-center justify-between">
        <h2 className="text-sm">絞り込み</h2>
        <Button
          type="button"
          variant="ghost"
          size="sm"
          onClick={() =>
            onChange({ ...DEFAULT_FILTERS, kind: value.kind, sort: value.sort, order: value.order })
          }
        >
          <RotateCcw aria-hidden />
          リセット
        </Button>
      </div>

      <div className="relative">
        <Search
          aria-hidden
          className="pointer-events-none absolute top-1/2 left-2.5 size-4 -translate-y-1/2 text-muted-foreground"
        />
        <Input
          type="search"
          placeholder="コードまたは銘柄名"
          aria-label="コードまたは銘柄名で検索"
          value={value.query}
          onChange={(e) => set({ query: e.target.value })}
          className="pl-8"
        />
      </div>

      {stock && (
        <fieldset className="flex flex-col gap-2">
          <legend className="mb-2 text-sm font-medium">判定</legend>
          {FLAGS.map((f) => (
            <Label key={f.key} className="font-normal" title={f.hint}>
              <Checkbox
                checked={value[f.key]}
                onCheckedChange={(c) => set({ [f.key]: c === true })}
              />
              {f.label}
            </Label>
          ))}
        </fieldset>
      )}

      {stock && (
        <>
          <fieldset className="flex flex-col gap-2">
            <legend className="mb-2 text-sm font-medium">市場</legend>
            {MARKETS.map((m) => (
              <Label key={m} className="font-normal">
                <Checkbox
                  checked={value.markets.includes(m)}
                  onCheckedChange={(c) => set({ markets: toggle(value.markets, m, c === true) })}
                />
                {m}
              </Label>
            ))}
          </fieldset>
          <fieldset className="flex flex-col gap-2">
            <legend className="mb-2 text-sm font-medium">
              規模
              <span className="ml-1 font-normal text-muted-foreground">（TOPIX の区分）</span>
            </legend>
            {SIZES.map((s) => (
              <Label key={s.key} className="font-normal">
                <Checkbox
                  checked={value.sizes.includes(s.key)}
                  onCheckedChange={(c) => set({ sizes: toggle(value.sizes, s.key, c === true) })}
                />
                {s.label}
              </Label>
            ))}
          </fieldset>
          <details className="group" open={value.sectors.length > 0}>
            <summary className="flex cursor-pointer list-none items-center justify-between text-sm font-medium">
              <span>
                業種
                {value.sectors.length > 0 && (
                  <span className="ml-1 font-normal text-muted-foreground">
                    {value.sectors.length}件選択
                  </span>
                )}
              </span>
              <ChevronDown
                aria-hidden
                className="size-4 transition-transform group-open:rotate-180"
              />
            </summary>
            <div className="mt-2 grid grid-cols-2 gap-x-2 gap-y-1.5">
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
          </details>
        </>
      )}

      <div className="flex flex-col gap-2">
        <Label htmlFor="valuation">{stock ? "配当" : "分配金"}利回りの過去5年比</Label>
        <Select
          value={value.valuation || ALL}
          onValueChange={(v) => set({ valuation: (v === ALL ? "" : v) as Valuation })}
        >
          <SelectTrigger id="valuation" className="w-full">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            <SelectItem value={ALL}>すべて</SelectItem>
            <SelectItem value="cheap">高い（上位20%。株価は過去と比べて安い）</SelectItem>
            <SelectItem value="neutral">ふつう</SelectItem>
            <SelectItem value="expensive">低い（下位20%。株価は過去と比べて高い）</SelectItem>
          </SelectContent>
        </Select>
      </div>

      <fieldset className="flex flex-col gap-3">
        <legend className="mb-2 text-sm font-medium">範囲</legend>
        {metricsFor(value.kind).map((m) => {
          const minKey = `${m.key}_min` as const;
          const maxKey = `${m.key}_max` as const;
          const label = metricLabel(m.key, value.kind);
          return (
            <div key={m.key} className="grid grid-cols-[1fr_auto_1fr] items-center gap-x-2 gap-y-1">
              <span className="col-span-3 text-xs text-muted-foreground">
                {label}（{UNIT_SUFFIX[m.unit]}）
              </span>
              <Input
                inputMode="decimal"
                placeholder="下限"
                aria-label={`${label}の下限`}
                aria-invalid={isInvalidInput(value.ranges[minKey]) || undefined}
                value={value.ranges[minKey] ?? ""}
                onChange={(e) => setRange(minKey, e.target.value)}
                className="text-right tabular-nums"
              />
              <span className="text-muted-foreground" aria-hidden>
                〜
              </span>
              <Input
                inputMode="decimal"
                placeholder="上限"
                aria-label={`${label}の上限`}
                aria-invalid={isInvalidInput(value.ranges[maxKey]) || undefined}
                value={value.ranges[maxKey] ?? ""}
                onChange={(e) => setRange(maxKey, e.target.value)}
                className="text-right tabular-nums"
              />
            </div>
          );
        })}
      </fieldset>
    </form>
  );
}
