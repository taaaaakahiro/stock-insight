import { Star } from "lucide-react";
import { Button } from "@/components/ui/button";
import { usePortfolio } from "@/lib/portfolio";
import { cn } from "@/lib/utils";

/** ☆ を押すとポートフォリオに入れる・外す */
export function PortfolioToggle({ code, name }: { code: string; name: string }) {
  const { codes, toggle } = usePortfolio();
  const on = codes.has(code);
  const label = on ? `${name}をポートフォリオから外す` : `${name}をポートフォリオに入れる`;
  return (
    <Button
      variant="ghost"
      size="icon-sm"
      aria-pressed={on}
      aria-label={label}
      title={label}
      onClick={() => void toggle(code)}
    >
      <Star aria-hidden className={cn(on ? "fill-warning text-warning" : "opacity-50")} />
    </Button>
  );
}
