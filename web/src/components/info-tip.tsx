import { Info } from "lucide-react";
import { Tooltip } from "radix-ui";
import { useState } from "react";

/** 用語の説明。PC はホバー・フォーカス、スマホはタップで開く。text の改行はそのまま出す */
export function InfoTip({ label, text }: { label: string; text: string }) {
  const [open, setOpen] = useState(false);
  return (
    <Tooltip.Provider delayDuration={150}>
      <Tooltip.Root open={open} onOpenChange={setOpen}>
        <Tooltip.Trigger asChild>
          <button
            type="button"
            aria-label={`${label}とは`}
            // Radix の Tooltip は押すと閉じる。スマホでも開けるよう、押下は自前で開け閉めする
            onPointerDown={(e) => e.preventDefault()}
            onClick={(e) => {
              e.preventDefault();
              setOpen((o) => !o);
            }}
            className="inline-flex size-4 shrink-0 items-center justify-center rounded-full text-muted-foreground outline-none hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring/50"
          >
            <Info aria-hidden className="size-3.5" />
          </button>
        </Tooltip.Trigger>
        <Tooltip.Portal>
          <Tooltip.Content
            side="top"
            sideOffset={4}
            collisionPadding={8}
            className="z-50 max-w-72 rounded-md bg-foreground px-3 py-2 text-xs leading-relaxed font-normal whitespace-pre-line text-background shadow-md"
          >
            {text}
            <Tooltip.Arrow className="fill-foreground" />
          </Tooltip.Content>
        </Tooltip.Portal>
      </Tooltip.Root>
    </Tooltip.Provider>
  );
}
