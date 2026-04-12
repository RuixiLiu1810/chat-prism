import { type FC, useState } from "react";
import {
  CheckCircle2Icon,
  ClockIcon,
  FileEditIcon,
  FileOutputIcon,
  TerminalIcon,
  XIcon,
} from "lucide-react";
import { toast } from "sonner";
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from "@/components/ui/tooltip";
import {
  useAgentChatStore,
  type PendingApprovalState,
} from "@/stores/agent-chat-store";

type ApprovalDecision = "allow_once" | "allow_session" | "deny_session";

interface ApprovalCardProps {
  approval: PendingApprovalState;
}

function isWholeFileWriteApproval(toolName?: string | null): boolean {
  return (toolName || "").toLowerCase() === "write_file";
}

function isPatchApproval(toolName?: string | null): boolean {
  return [
    "patch_file",
    "replace_selected_text",
    "apply_text_patch",
  ].includes((toolName || "").toLowerCase());
}

function truncate(str: string, max: number): string {
  if (!str) return "";
  return str.length > max ? `${str.slice(0, max)}...` : str;
}

function getApprovalCopy(
  toolName: string,
  targetLabel?: string | null,
  reviewReady = false,
  canResume = true,
) {
  if (isWholeFileWriteApproval(toolName)) {
    const target = targetLabel ?? "this file";
    return {
      contextLabel: "Edit request",
      question: reviewReady
        ? `Do you want to allow writing ${target} after reviewing the diff?`
        : `Do you want to allow editing ${target}?`,
      nextStep: reviewReady
        ? "Review is already ready in the diff panel. Approve only if you want this file write to be applied."
        : canResume
          ? "Allow this edit once or for the current chat session to let the agent continue."
          : "Allow this edit once or for the current chat session.",
      allowOnceLabel: "Allow This Edit",
      allowSessionLabel: "Allow Edits In Chat",
      denySessionLabel: "Deny Edits In Chat",
      allowedOnceContinue: "Allowed once. Continuing the interrupted edit...",
      allowedSessionContinue:
        "Allowed for this chat session. Continuing the interrupted edit...",
      allowedOnceReviewReady:
        "Allowed once. Review remains ready in the diff panel.",
      allowedSessionReviewReady:
        "Allowed for this chat session. Review remains ready in the diff panel.",
      deniedSession: "Denied for this chat session.",
    };
  }

  if (isPatchApproval(toolName)) {
    const target = targetLabel ?? "this file";
    return {
      contextLabel: "Patch request",
      question: reviewReady
        ? `Do you want to allow applying this precise patch to ${target} after reviewing the diff?`
        : `Do you want to allow this precise patch to ${target}?`,
      nextStep: reviewReady
        ? "Review is already ready in the diff panel. Approve only if you want this patch applied."
        : canResume
          ? "Allow this patch once or for the current chat session to let the agent continue."
          : "Allow this patch once or for the current chat session.",
      allowOnceLabel: "Allow This Patch",
      allowSessionLabel: "Allow Patches In Chat",
      denySessionLabel: "Deny Patches In Chat",
      allowedOnceContinue: "Allowed once. Continuing the interrupted patch...",
      allowedSessionContinue:
        "Allowed for this chat session. Continuing the interrupted patch...",
      allowedOnceReviewReady:
        "Allowed once. Review remains ready in the diff panel.",
      allowedSessionReviewReady:
        "Allowed for this chat session. Review remains ready in the diff panel.",
      deniedSession: "Denied for this chat session.",
    };
  }

  if (toolName === "run_shell_command") {
    return {
      contextLabel: "Command request",
      question: "Do you want to allow this command to run?",
      nextStep:
        "Review the command below, then allow it once or for the current chat session.",
      allowOnceLabel: "Run Once",
      allowSessionLabel: "Allow Commands In Chat",
      denySessionLabel: "Deny Commands In Chat",
      allowedOnceContinue: "Allowed once. Continuing the interrupted command...",
      allowedSessionContinue:
        "Allowed for this chat session. Continuing the interrupted command...",
      allowedOnceReviewReady: "Allowed once.",
      allowedSessionReviewReady: "Allowed for this chat session.",
      deniedSession: "Denied for this chat session.",
    };
  }

  return {
    contextLabel: "Tool request",
    question: "Do you want to allow this tool action?",
    nextStep: "Allow this tool action once or for the current chat session.",
    allowOnceLabel: "Allow Once",
    allowSessionLabel: "Allow Session",
    denySessionLabel: "Deny Session",
    allowedOnceContinue: "Allowed once. Continuing the interrupted tool call...",
    allowedSessionContinue:
      "Allowed for this chat session. Continuing the interrupted tool call...",
    allowedOnceReviewReady: "Allowed once.",
    allowedSessionReviewReady: "Allowed for this chat session.",
    deniedSession: "Denied for this chat session.",
  };
}

function ApprovalButton({
  label,
  onClick,
  disabled = false,
}: {
  label: string;
  onClick: () => void;
  disabled?: boolean;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      disabled={disabled}
      className="rounded-md border border-border bg-background px-2.5 py-1.5 text-xs text-foreground transition-colors hover:bg-muted disabled:cursor-not-allowed disabled:opacity-60"
    >
      {label}
    </button>
  );
}

export const ApprovalCard: FC<ApprovalCardProps> = ({ approval }) => {
  const setToolApproval = useAgentChatStore((s) => s.setToolApproval);
  const continueAfterApproval = useAgentChatStore(
    (s) => s.continueAfterApproval,
  );
  const [pending, setPending] = useState<ApprovalDecision | null>(null);
  const [doneLabel, setDoneLabel] = useState<string | null>(null);
  const copy = getApprovalCopy(
    approval.toolName,
    approval.targetPath,
    approval.reviewReady,
    approval.canResume,
  );

  const Icon =
    approval.toolName === "run_shell_command"
      ? TerminalIcon
      : isWholeFileWriteApproval(approval.toolName)
        ? FileOutputIcon
        : FileEditIcon;

  const handleDecision = async (decision: ApprovalDecision) => {
    try {
      setPending(decision);
      await setToolApproval(approval.toolName, decision);
      const shouldContinue =
        decision !== "deny_session" && approval.canResume;
      setDoneLabel(
        decision === "allow_once"
          ? shouldContinue
            ? approval.reviewReady ? copy.allowedOnceReviewReady : copy.allowedOnceContinue
            : approval.reviewReady ? copy.allowedOnceReviewReady : copy.allowedOnceContinue
          : decision === "allow_session"
            ? shouldContinue
              ? approval.reviewReady ? copy.allowedSessionReviewReady : copy.allowedSessionContinue
              : approval.reviewReady ? copy.allowedSessionReviewReady : copy.allowedSessionContinue
            : copy.deniedSession,
      );
      if (shouldContinue) {
        await continueAfterApproval(approval.toolName, approval.targetPath ?? undefined);
      }
    } catch (err) {
      toast.error("Approval action failed");
      setPending(null);
    }
  };

  return (
    <div className="mx-3 mb-2 rounded-xl border border-amber-500/35 bg-amber-500/8 px-3 py-2">
      {/* Row 1: Icon + summary (with tooltip for details) + status badge */}
      <Tooltip>
        <TooltipTrigger asChild>
          <div className="flex items-center gap-2">
            <div className="flex size-6 shrink-0 items-center justify-center rounded-full bg-amber-500/15 text-amber-700 dark:text-amber-200">
              {approval.phase === "review_ready" ? (
                <CheckCircle2Icon className="size-3.5" />
              ) : (
                <ClockIcon className="size-3.5" />
              )}
            </div>
            <Icon className="size-3.5 shrink-0 text-muted-foreground" />
            <span className="min-w-0 flex-1 truncate font-medium text-foreground text-sm">
              {copy.contextLabel}
              {approval.targetPath ? (
                <>
                  {" "}
                  <code className="rounded bg-background/80 px-1 py-0.5 text-[11px] text-muted-foreground">
                    {truncate(approval.targetPath, 60)}
                  </code>
                </>
              ) : null}
            </span>
          </div>
        </TooltipTrigger>
        <TooltipContent side="top" className="max-w-sm">
          <p className="font-medium">{copy.question}</p>
          {approval.message ? <p className="mt-1 opacity-80">{approval.message}</p> : null}
          <p className="mt-1 opacity-70">{copy.nextStep}</p>
        </TooltipContent>
      </Tooltip>

      {/* Row 2: Action buttons or result */}
      {doneLabel ? (
        <div className="mt-1.5 flex items-center gap-1.5 pl-8 text-muted-foreground text-xs">
          <CheckCircle2Icon className="size-3 text-green-500" />
          <span>{doneLabel}</span>
        </div>
      ) : (
        <div className="mt-1.5 flex flex-wrap gap-1.5 pl-8">
          <ApprovalButton
            label={pending === "allow_once" ? "Allowing..." : copy.allowOnceLabel}
            onClick={() => void handleDecision("allow_once")}
            disabled={pending !== null}
          />
          <ApprovalButton
            label={
              pending === "allow_session"
                ? "Allowing..."
                : copy.allowSessionLabel
            }
            onClick={() => void handleDecision("allow_session")}
            disabled={pending !== null}
          />
          <ApprovalButton
            label={pending === "deny_session" ? "Saving..." : copy.denySessionLabel}
            onClick={() => void handleDecision("deny_session")}
            disabled={pending !== null}
          />
        </div>
      )}
    </div>
  );
};
