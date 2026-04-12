import { type FC, useState } from "react";
import {
  ChevronDownIcon,
  ChevronRightIcon,
  WrenchIcon,
} from "lucide-react";
import { type ContentBlock } from "@/stores/agent-chat-store";
import { StatusIcon, getToolDisplay, extractResultTextPreview, truncate } from "./shared";

export const GenericWidget: FC<{
  name: string;
  input: any;
  result?: ContentBlock;
  isStreaming?: boolean;
}> = ({ name, input, result, isStreaming }) => {
  const [expanded, setExpanded] = useState(false);
  const resultPreview = extractResultTextPreview(result);
  const display = getToolDisplay(result);

  return (
    <div className="my-1.5 rounded-lg border border-border bg-muted/50 text-sm">
      <button
        type="button"
        className="flex w-full items-center gap-2 px-3 py-2"
        onClick={() => setExpanded(!expanded)}
      >
        <StatusIcon result={result} isStreaming={isStreaming} />
        <WrenchIcon className="size-3.5 text-muted-foreground" />
        <span className="text-muted-foreground">
          {result ? "Ran" : "Running"} <code className="text-xs">{name}</code>
        </span>
        {expanded ? (
          <ChevronDownIcon className="ml-auto size-3.5 text-muted-foreground" />
        ) : (
          <ChevronRightIcon className="ml-auto size-3.5 text-muted-foreground" />
        )}
      </button>
      {expanded && (
        <div className="space-y-2 border-border border-t px-3 py-2">
          <pre className="whitespace-pre-wrap font-mono text-muted-foreground text-xs">
            {JSON.stringify(input ?? {}, null, 2)}
          </pre>
          {!!resultPreview && (
            <pre
              className={`overflow-auto whitespace-pre-wrap rounded-md px-2 py-1.5 font-mono text-xs ${
                display?.isError
                  ? "bg-destructive/10 text-destructive"
                  : "bg-background/60 text-foreground"
              }`}
              style={{ maxHeight: 320 }}
            >
              {truncate(resultPreview, 2000)}
            </pre>
          )}
        </div>
      )}
    </div>
  );
};
