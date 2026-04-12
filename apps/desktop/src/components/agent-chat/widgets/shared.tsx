import { type FC } from "react";
import {
  CheckIcon,
  CircleIcon,
  ClockIcon,
  LoaderIcon,
} from "lucide-react";
import {
  adaptToolResultDisplayContent,
  getToolResultDisplayApproval,
  getToolResultDisplayPreview,
  type AgentToolResultDisplayContent,
} from "@/lib/agent-message-adapter";
import { type ContentBlock } from "@/stores/agent-chat-store";

export type { ContentBlock };

export const StatusIcon: FC<{ result?: ContentBlock; isStreaming?: boolean }> = ({
  result,
  isStreaming = false,
}) => {
  const display = getToolDisplay(result);
  if (!result) {
    if (!isStreaming) {
      return <CircleIcon className="size-3.5 text-muted-foreground" />;
    }
    return (
      <LoaderIcon className="size-3.5 animate-spin text-muted-foreground" />
    );
  }
  if (display?.status === "review_ready" || display?.status === "awaiting_approval") {
    return <ClockIcon className="size-3.5 text-amber-500" />;
  }
  if (result.is_error) {
    return <span className="text-destructive text-sm">!</span>;
  }
  return <CheckIcon className="size-3.5 text-green-600" />;
};

export function getToolDisplay(
  result?: ContentBlock,
): AgentToolResultDisplayContent | null {
  if (!result) return null;
  return adaptToolResultDisplayContent(result.content, {
    preview: typeof result.content === "string" ? result.content : undefined,
    isError: result.is_error === true,
  });
}

export function getApprovalPayload(result?: ContentBlock): {
  reason?: string;
  reviewReady: boolean;
  approvalToolName?: string;
} | null {
  if (!result?.is_error) {
    return null;
  }
  return getToolResultDisplayApproval(result.content);
}

export function extractResultTextPreview(result?: ContentBlock): string {
  return getToolDisplay(result)?.textPreview ?? getToolResultDisplayPreview(result?.content);
}

export function truncate(str: string, max: number): string {
  if (!str) return "";
  return str.length > max ? `${str.slice(0, max)}...` : str;
}
