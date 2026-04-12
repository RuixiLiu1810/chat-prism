import { type FC, useState } from "react";
import {
  ChevronDownIcon,
  ChevronRightIcon,
  TerminalIcon,
} from "lucide-react";
import { type ContentBlock } from "@/stores/agent-chat-store";
import { StatusIcon, getToolDisplay, getApprovalPayload, truncate } from "./shared";

export const BashWidget: FC<{
  input: any;
  result?: ContentBlock;
  isStreaming?: boolean;
}> = ({ input, result, isStreaming }) => {
  const [expanded, setExpanded] = useState(false);
  const command = input?.command || input?.description || "";
  const resultContent = getToolDisplay(result)?.textPreview ?? "";
  const approval = getApprovalPayload(result);

  return (
    <div className="my-1.5 rounded-lg border border-border bg-muted text-sm">
      <button
        type="button"
        className="flex w-full items-center gap-2 px-3 py-2"
        onClick={() => setExpanded(!expanded)}
      >
        <StatusIcon result={result} isStreaming={isStreaming} />
        <TerminalIcon className="size-3.5 shrink-0 text-muted-foreground" />
        <code className="min-w-0 truncate font-mono text-foreground text-xs">
          $ {truncate(command, 80)}
        </code>
        {result &&
          (expanded ? (
            <ChevronDownIcon className="ml-auto size-3.5 text-muted-foreground" />
          ) : (
            <ChevronRightIcon className="ml-auto size-3.5 text-muted-foreground" />
          ))}
      </button>
      {expanded && !!resultContent && (
        <div className="overflow-auto border-border/50 border-t px-3 py-2" style={{ maxHeight: expanded ? 320 : undefined }}>
          <pre className="whitespace-pre-wrap font-mono text-foreground/80 text-xs">
            {truncate(resultContent, 2000)}
          </pre>
        </div>
      )}
    </div>
  );
};
