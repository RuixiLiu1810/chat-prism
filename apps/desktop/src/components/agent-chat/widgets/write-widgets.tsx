import { type FC } from "react";
import { FileEditIcon, FileOutputIcon } from "lucide-react";
import { type ContentBlock } from "@/stores/agent-chat-store";
import { StatusIcon, getApprovalPayload } from "./shared";

export const WriteWidget: FC<{
  input: any;
  result?: ContentBlock;
  isStreaming?: boolean;
}> = ({ input, result, isStreaming }) => {
  const targetPath = input?.file_path || input?.path || "(missing path)";
  const approval = getApprovalPayload(result);
  const statusLabel = approval
    ? approval.reviewReady
      ? "Review ready for"
      : "Approval required for"
    : result
      ? "Wrote"
      : "Writing";
  return (
    <div className="my-1.5 flex items-center gap-2 rounded-lg border border-border bg-muted/50 px-3 py-2 text-sm">
      <StatusIcon result={result} isStreaming={isStreaming} />
      <FileOutputIcon className="size-3.5 shrink-0 text-muted-foreground" />
      <div className="min-w-0 flex-1">
        <div className="truncate text-muted-foreground">
          {statusLabel}{" "}
          <code className="rounded bg-muted px-1 text-xs">{targetPath}</code>
        </div>
      </div>
    </div>
  );
};

export const PreciseEditWidget: FC<{
  toolName: string;
  input: any;
  result?: ContentBlock;
  isStreaming?: boolean;
}> = ({ toolName, input, result, isStreaming }) => {
  const targetPath = input?.file_path || input?.path || "(missing path)";
  const approval = getApprovalPayload(result);
  const statusLabel = approval
    ? approval.reviewReady
      ? "Review ready for"
      : "Approval required for"
    : result
      ? "Edited"
      : "Editing";

  return (
    <div className="my-1.5 flex items-center gap-2 rounded-lg border border-border bg-muted/50 px-3 py-2 text-sm">
      <StatusIcon result={result} isStreaming={isStreaming} />
      <FileEditIcon className="size-3.5 shrink-0 text-muted-foreground" />
      <div className="min-w-0 flex-1">
        <div className="truncate text-muted-foreground">
          {statusLabel}{" "}
          <code className="rounded bg-muted px-1 text-xs">{targetPath}</code>
        </div>
      </div>
    </div>
  );
};
