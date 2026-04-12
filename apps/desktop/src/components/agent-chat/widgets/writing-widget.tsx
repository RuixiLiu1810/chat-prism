import { type FC } from "react";
import { SparklesIcon } from "lucide-react";
import { type ContentBlock } from "@/stores/agent-chat-store";
import { StatusIcon, getToolDisplay, truncate } from "./shared";

export const WritingWidget: FC<{
  toolName: string;
  input: any;
  result?: ContentBlock;
  isStreaming?: boolean;
}> = ({ toolName, input, result, isStreaming }) => {
  const actionLabel =
    toolName === "draft_section"
      ? result
        ? "Drafted section"
        : "Drafting section"
      : toolName === "restructure_outline"
        ? result
          ? "Restructured outline"
          : "Restructuring outline"
        : toolName === "check_consistency"
          ? result
            ? "Checked consistency"
            : "Checking consistency"
          : toolName === "generate_abstract"
            ? result
              ? "Generated abstract"
              : "Generating abstract"
            : result
              ? "Inserted citation"
              : "Inserting citation";
  const context =
    input?.section_type ||
    input?.manuscript_type ||
    input?.path ||
    input?.citation_key ||
    "";
  const preview = getToolDisplay(result)?.textPreview ?? "";

  return (
    <div className="my-1.5 rounded-lg border border-border bg-muted/50 text-sm">
      <div className="flex items-center gap-2 px-3 py-2">
        <StatusIcon result={result} isStreaming={isStreaming} />
        <SparklesIcon className="size-3.5 shrink-0 text-muted-foreground" />
        <span className="min-w-0 truncate text-muted-foreground">
          {actionLabel}
          {context ? (
            <>
              {" "}
              <code className="rounded bg-muted px-1 text-xs">{context}</code>
            </>
          ) : null}
        </span>
      </div>
      {!!preview && (
        <div className="border-border border-t px-3 py-2">
          <pre
            className={`overflow-auto whitespace-pre-wrap rounded-md px-2 py-1.5 font-mono text-xs ${
              result?.is_error
                ? "bg-destructive/10 text-destructive"
                : "bg-background/60 text-foreground"
            }`}
            style={{ maxHeight: 320 }}
          >
            {truncate(preview, 1200)}
          </pre>
        </div>
      )}
    </div>
  );
};
