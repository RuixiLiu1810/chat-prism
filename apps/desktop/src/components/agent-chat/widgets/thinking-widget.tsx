import { type FC, useState } from "react";
import {
  BotIcon,
  ChevronRightIcon,
  SparklesIcon,
} from "lucide-react";

export const ThinkingWidget: FC<{ thinking: string; signature?: string }> = ({
  thinking,
}) => {
  const [expanded, setExpanded] = useState(false);
  const trimmed = thinking.trim();

  return (
    <div className="my-1.5 overflow-hidden rounded-lg border border-muted-foreground/20 bg-muted-foreground/5">
      <button
        type="button"
        onClick={() => setExpanded(!expanded)}
        className="flex w-full items-center justify-between px-3 py-2 transition-colors hover:bg-muted-foreground/10"
      >
        <div className="flex items-center gap-2">
          <div className="relative">
            <BotIcon className="size-4 text-muted-foreground" />
            <SparklesIcon className="absolute -top-1 -right-1 size-2.5 animate-pulse text-muted-foreground/70" />
          </div>
          <span className="font-medium text-muted-foreground text-sm italic">
            Thinking...
          </span>
        </div>
        <ChevronRightIcon
          className={`size-4 text-muted-foreground transition-transform ${expanded ? "rotate-90" : ""}`}
        />
      </button>
      {expanded && (
        <div className="border-muted-foreground/20 border-t px-3 pt-2 pb-3">
          <pre className="whitespace-pre-wrap rounded-lg bg-muted-foreground/5 p-3 font-mono text-muted-foreground text-xs italic">
            {trimmed}
          </pre>
        </div>
      )}
    </div>
  );
};
