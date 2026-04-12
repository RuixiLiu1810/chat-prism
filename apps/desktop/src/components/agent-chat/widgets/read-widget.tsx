import { type FC, useState } from "react";
import {
  ChevronDownIcon,
  ChevronRightIcon,
  FileIcon,
} from "lucide-react";
import { type ContentBlock } from "@/stores/agent-chat-store";
import { StatusIcon, getToolDisplay, truncate } from "./shared";

export const ReadWidget: FC<{
  toolName: string;
  input: any;
  result?: ContentBlock;
  isStreaming?: boolean;
}> = ({ toolName, input, result, isStreaming }) => {
  const targetPath = input?.file_path || input?.path || "(missing path)";
  const preview = getToolDisplay(result)?.textPreview ?? "";
  const [expanded, setExpanded] = useState(false);
  const verb =
    toolName === "read_document"
      ? result
        ? "Read document"
        : "Reading document"
      : toolName === "inspect_resource"
      ? result
        ? "Inspected"
        : "Inspecting"
      : toolName === "read_document_excerpt"
        ? result
          ? "Read excerpt from"
          : "Reading excerpt from"
        : result
          ? "Read"
          : "Reading";
  return (
    <div className="my-1.5 rounded-lg border border-border bg-muted/50 text-sm">
      <button
        type="button"
        className="flex w-full items-center gap-2 px-3 py-2"
        onClick={() => setExpanded(!expanded)}
      >
        <StatusIcon result={result} isStreaming={isStreaming} />
        <FileIcon className="size-3.5 shrink-0 text-muted-foreground" />
        <span className="min-w-0 truncate text-muted-foreground">
          {verb}{" "}
          <code className="rounded bg-muted px-1 text-xs">{targetPath}</code>
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
          {!!preview && (
            <pre
              className={`overflow-auto whitespace-pre-wrap rounded-md px-2 py-1.5 font-mono text-xs ${
                result?.is_error
                  ? "bg-destructive/10 text-destructive"
                  : "bg-background/60 text-foreground"
              }`}
              style={{ maxHeight: 320 }}
            >
              {truncate(preview, 2000)}
            </pre>
          )}
        </div>
      )}
    </div>
  );
};
