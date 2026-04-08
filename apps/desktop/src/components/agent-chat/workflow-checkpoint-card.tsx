import { type FC, useState } from "react";
import { CheckCircle2Icon, RefreshCwIcon, WorkflowIcon } from "lucide-react";
import {
  type PendingWorkflowCheckpointState,
  useAgentChatStore,
} from "@/stores/agent-chat-store";

interface WorkflowCheckpointCardProps {
  checkpoint: PendingWorkflowCheckpointState;
}

export const WorkflowCheckpointCard: FC<WorkflowCheckpointCardProps> = ({
  checkpoint,
}) => {
  const checkpointWorkflow = useAgentChatStore((s) => s.checkpointWorkflow);
  const [pending, setPending] = useState<"approve" | "request_changes" | null>(
    null,
  );
  const [resultLabel, setResultLabel] = useState<string | null>(null);

  const onDecision = async (decision: "approve" | "request_changes") => {
    try {
      setPending(decision);
      await checkpointWorkflow(decision);
      setResultLabel(
        decision === "approve"
          ? "Checkpoint approved. You can continue with the next stage."
          : "Checkpoint rejected. Continue this stage with requested adjustments.",
      );
    } finally {
      setPending(null);
    }
  };

  return (
    <div className="mx-3 mb-2 rounded-xl border border-blue-500/35 bg-blue-500/8 px-3 py-3">
      <div className="flex items-start gap-2">
        <div className="mt-0.5 flex size-8 shrink-0 items-center justify-center rounded-full bg-blue-500/15 text-blue-700 dark:text-blue-200">
          <WorkflowIcon className="size-4" />
        </div>
        <div className="min-w-0 flex-1">
          <div className="flex flex-wrap items-center gap-1.5 text-[11px] text-muted-foreground">
            <span className="rounded-full bg-blue-500/15 px-2 py-0.5 font-medium uppercase tracking-wide text-blue-700 dark:text-blue-200">
              Workflow Checkpoint
            </span>
            <span>{checkpoint.workflowType}</span>
            <span className="rounded-full border border-border/70 px-2 py-0.5">
              {checkpoint.stage}
            </span>
          </div>
          <div className="mt-1 font-medium text-sm text-foreground">
            Stage completed. Continue to next stage?
          </div>
          <div className="mt-2 text-muted-foreground text-xs">{checkpoint.message}</div>
          <div className="mt-3 flex flex-wrap gap-1.5">
            <button
              type="button"
              onClick={() => void onDecision("approve")}
              disabled={pending !== null}
              className="inline-flex items-center gap-1 rounded-md border border-border bg-background px-2.5 py-1.5 text-xs text-foreground transition-colors hover:bg-muted disabled:cursor-not-allowed disabled:opacity-60"
            >
              <CheckCircle2Icon className="size-3.5" />
              {pending === "approve" ? "Approving..." : "Approve Stage"}
            </button>
            <button
              type="button"
              onClick={() => void onDecision("request_changes")}
              disabled={pending !== null}
              className="inline-flex items-center gap-1 rounded-md border border-border bg-background px-2.5 py-1.5 text-xs text-foreground transition-colors hover:bg-muted disabled:cursor-not-allowed disabled:opacity-60"
            >
              <RefreshCwIcon className="size-3.5" />
              {pending === "request_changes"
                ? "Saving..."
                : "Request Changes"}
            </button>
          </div>
          {resultLabel ? (
            <div className="mt-2 text-muted-foreground text-xs">{resultLabel}</div>
          ) : null}
        </div>
      </div>
    </div>
  );
};
