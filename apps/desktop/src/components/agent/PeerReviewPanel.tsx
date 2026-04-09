import { useMemo, useState, type FC } from "react";
import { ClipboardCheckIcon, SigmaIcon, ReplyIcon } from "lucide-react";

import { useAgentChatStore, type AgentTurnProfile } from "@/stores/agent-chat-store";
import { useSettingsStore } from "@/stores/settings-store";

const PEER_REVIEW_PROFILE: AgentTurnProfile = {
  taskKind: "peer_review",
  selectionScope: "none",
  responseMode: "default",
  samplingProfile: "analysis_deep",
  sourceHint: "peer_review_panel",
};

export const PeerReviewPanel: FC = () => {
  const runtime = useSettingsStore(
    (s) => s.effective.integrations.agent.runtime ?? "claude_cli",
  );
  const activeTabId = useAgentChatStore((s) => s.activeTabId);
  const activeTab = useAgentChatStore((s) => s.tabs.find((tab) => tab.id === activeTabId));
  const sendPrompt = useAgentChatStore((s) => s.sendPrompt);

  const [manuscriptPath, setManuscriptPath] = useState("");
  const [focus, setFocus] = useState("");
  const [reviewerComments, setReviewerComments] = useState("");

  const parsedComments = useMemo(
    () =>
      reviewerComments
        .split("\n")
        .map((line) => line.trim())
        .filter((line) => line.length > 0),
    [reviewerComments],
  );

  if (runtime !== "local_agent") {
    return null;
  }

  const isStreaming = activeTab?.isStreaming ?? false;
  const hasPath = manuscriptPath.trim().length > 0;

  const runPeerReview = async () => {
    if (!hasPath) {
      return;
    }
    const lines = [
      "Run peer_review workflow on this manuscript.",
      `Path: ${manuscriptPath.trim()}`,
      focus.trim().length > 0 ? `Focus: ${focus.trim()}` : null,
      "Stage goal: perform scope setup and section-level review with severity-tagged findings.",
    ].filter(Boolean);
    await sendPrompt(lines.join("\n"), undefined, PEER_REVIEW_PROFILE);
  };

  const runStatsCheck = async () => {
    if (!hasPath) {
      return;
    }
    const lines = [
      "Peer review workflow: run statistical reporting checks.",
      `Path: ${manuscriptPath.trim()}`,
      "Call check_statistics and summarize major/critical issues first.",
    ];
    await sendPrompt(lines.join("\n"), undefined, PEER_REVIEW_PROFILE);
  };

  const draftResponseLetter = async () => {
    if (parsedComments.length === 0) {
      return;
    }
    const lines = [
      "Peer review workflow: draft response letter.",
      `Manuscript path: ${manuscriptPath.trim() || "(not provided)"}`,
      "Reviewer comments:",
      ...parsedComments.map((line) => `- ${line}`),
      "Call generate_response_letter and provide a point-by-point response.",
    ];
    await sendPrompt(lines.join("\n"), undefined, PEER_REVIEW_PROFILE);
  };

  return (
    <div className="mx-3 mb-2 rounded-xl border border-sky-500/35 bg-sky-500/8 px-3 py-3">
      <div className="mb-2 text-[11px] text-sky-700 dark:text-sky-200 uppercase tracking-wide font-medium">
        Peer Review Workflow
      </div>
      <div className="grid gap-2">
        <input
          value={manuscriptPath}
          onChange={(event) => setManuscriptPath(event.target.value)}
          placeholder="Manuscript path (e.g., attachments/paper.pdf or docs/manuscript.md)"
          className="h-8 rounded-md border border-border bg-background px-2 text-xs text-foreground"
        />
        <input
          value={focus}
          onChange={(event) => setFocus(event.target.value)}
          placeholder="Optional review focus (novelty, methods rigor, clarity...)"
          className="h-8 rounded-md border border-border bg-background px-2 text-xs text-foreground"
        />
        <textarea
          value={reviewerComments}
          onChange={(event) => setReviewerComments(event.target.value)}
          placeholder={"Reviewer comments (one per line)\nMajor concern: ...\nMinor concern: ..."}
          className="min-h-16 rounded-md border border-border bg-background px-2 py-1.5 text-xs text-foreground"
        />
      </div>
      <div className="mt-3 flex flex-wrap gap-1.5">
        <button
          type="button"
          onClick={() => void runPeerReview()}
          disabled={isStreaming || !hasPath}
          className="inline-flex items-center gap-1 rounded-md border border-border bg-background px-2 py-1.5 text-xs text-foreground transition-colors hover:bg-muted disabled:cursor-not-allowed disabled:opacity-60"
        >
          <ClipboardCheckIcon className="size-3.5" />
          Review Manuscript
        </button>
        <button
          type="button"
          onClick={() => void runStatsCheck()}
          disabled={isStreaming || !hasPath}
          className="inline-flex items-center gap-1 rounded-md border border-border bg-background px-2 py-1.5 text-xs text-foreground transition-colors hover:bg-muted disabled:cursor-not-allowed disabled:opacity-60"
        >
          <SigmaIcon className="size-3.5" />
          Check Statistics
        </button>
        <button
          type="button"
          onClick={() => void draftResponseLetter()}
          disabled={isStreaming || parsedComments.length === 0}
          className="inline-flex items-center gap-1 rounded-md border border-border bg-background px-2 py-1.5 text-xs text-foreground transition-colors hover:bg-muted disabled:cursor-not-allowed disabled:opacity-60"
        >
          <ReplyIcon className="size-3.5" />
          Draft Response Letter
        </button>
      </div>
    </div>
  );
};
