import { type FC } from "react";
import { FileIcon } from "lucide-react";
import { type ContentBlock } from "@/stores/agent-chat-store";
import { StatusIcon } from "./shared";

export const GlobWidget: FC<{
  input: any;
  result?: ContentBlock;
  isStreaming?: boolean;
}> = ({ input, result, isStreaming }) => {
  const pattern = input?.pattern || input?.path || ".";
  return (
    <div className="my-1.5 flex items-center gap-2 rounded-lg border border-border bg-muted/50 px-3 py-2 text-sm">
      <StatusIcon result={result} isStreaming={isStreaming} />
      <FileIcon className="size-3.5 shrink-0 text-muted-foreground" />
      <span className="min-w-0 truncate text-muted-foreground">
        {result ? "Searched" : "Searching"}{" "}
        <code className="rounded bg-muted px-1 text-xs">{pattern}</code>
      </span>
    </div>
  );
};

export const GrepWidget: FC<{
  toolName: string;
  input: any;
  result?: ContentBlock;
  isStreaming?: boolean;
}> = ({ toolName, input, result, isStreaming }) => {
  const pattern = input?.pattern || input?.query || "(missing query)";
  const verb =
    toolName === "get_document_evidence"
      ? result
        ? "Gathered evidence for"
        : "Gathering evidence for"
      : toolName === "search_document_text"
        ? result
          ? "Searched document for"
          : "Searching document for"
        : result
          ? "Grepped"
          : "Grepping";
  return (
    <div className="my-1.5 flex items-center gap-2 rounded-lg border border-border bg-muted/50 px-3 py-2 text-sm">
        <StatusIcon result={result} isStreaming={isStreaming} />
        <FileIcon className="size-3.5 shrink-0 text-muted-foreground" />
        <span className="min-w-0 truncate text-muted-foreground">
          {verb}{" "}
          <code className="rounded bg-muted px-1 text-xs">{pattern}</code>
        </span>
    </div>
  );
};
