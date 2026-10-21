import { RotateCcw } from "lucide-react";
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
  isInvalidInput,
  METRICS,
  UNIT_SUFFIX,
  type Valuation,
} from "@/lib/filters";

type Props = {
  value: Filters;
  onChange: (next: Filters) => void;
};

const ALL = "all";

export function FilterPanel({ value, onChange }: Props) {
  // 条件を変えたら1ページ目に戻す
  const set = (patch: Partial<Filters>) => onChange({ ...value, ...patch, page: 1 });
  const setRange = (key: keyof Filters["ranges"], v: string) =>
    set({ ranges: { ...value.ranges, [key]: v } });

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
          onClick={() => onChange({ ...DEFAULT_FILTERS, sort: value.sort, order: value.order })}
        >
          <RotateCcw aria-hidden />
          リセット
        </Button>
      </div>

      <div className="flex flex-col gap-2">
        <Label htmlFor="valuation">割安・割高</Label>
        <Select
          value={value.valuation || ALL}
          onValueChange={(v) => set({ valuation: (v === ALL ? "" : v) as Valuation })}
        >
          <SelectTrigger id="valuation" className="w-full">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            <SelectItem value={ALL}>すべて</SelectItem>
            <SelectItem value="cheap">割安のみ</SelectItem>
            <SelectItem value="neutral">中立のみ</SelectItem>
            <SelectItem value="expensive">割高のみ</SelectItem>
          </SelectContent>
        </Select>
      </div>

      <fieldset className="flex flex-col gap-3">
        <legend className="mb-2 text-sm font-medium">範囲</legend>
        {METRICS.map((m) => {
          const minKey = `${m.key}_min` as const;
          const maxKey = `${m.key}_max` as const;
          return (
            <div key={m.key} className="grid grid-cols-[1fr_auto_1fr] items-center gap-x-2 gap-y-1">
              <span className="col-span-3 text-xs text-muted-foreground">
                {m.label}（{UNIT_SUFFIX[m.unit]}）
              </span>
              <Input
                inputMode="decimal"
                placeholder="下限"
                aria-label={`${m.label}の下限`}
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
                aria-label={`${m.label}の上限`}
                aria-invalid={isInvalidInput(value.ranges[maxKey]) || undefined}
                value={value.ranges[maxKey] ?? ""}
                onChange={(e) => setRange(maxKey, e.target.value)}
                className="text-right tabular-nums"
              />
            </div>
          );
        })}
      </fieldset>

      <fieldset className="flex flex-col gap-3">
        <legend className="mb-2 text-sm font-medium">業績</legend>
        <Label className="font-normal">
          <Checkbox
            checked={value.noLossYears}
            onCheckedChange={(c) => set({ noLossYears: c === true })}
          />
          直近5期に赤字なし
        </Label>
        <Label className="font-normal">
          <Checkbox
            checked={value.profitUptrend}
            onCheckedChange={(c) => set({ profitUptrend: c === true })}
          />
          営業利益が増加傾向
        </Label>
      </fieldset>
    </form>
  );
}
