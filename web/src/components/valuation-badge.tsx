import { Badge } from "@/components/ui/badge";
import type { StockRow } from "@/lib/api";
import { cn } from "@/lib/utils";

const LABEL: Record<StockRow["valuation"], string> = {
  cheap: "割安",
  neutral: "中立",
  expensive: "割高",
  unknown: "判定なし",
};

// 色だけに意味を持たせず、必ず文字を出す。判定の根拠はマウスを乗せると出る
export function ValuationBadge({
  valuation,
  percentile,
}: {
  valuation: StockRow["valuation"];
  percentile: number | null;
}) {
  const hint =
    percentile == null
      ? "配当の履歴が1年未満などで、過去10年の位置を計算できません"
      : `配当利回りが過去10年の下から${Math.round(percentile * 100)}%の位置（80%以上で割安、20%以下で割高）`;
  return (
    <Badge
      variant="outline"
      title={hint}
      className={cn(
        valuation === "cheap" && "border-cheap/40 bg-cheap/10 text-cheap",
        valuation === "expensive" && "border-expensive/40 bg-expensive/10 text-expensive",
        valuation === "unknown" && "text-muted-foreground",
      )}
    >
      {LABEL[valuation]}
    </Badge>
  );
}
