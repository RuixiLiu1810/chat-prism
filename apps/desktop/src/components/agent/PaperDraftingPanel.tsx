import { useMemo, useState, type FC } from "react";
import { FileTextIcon, WandSparklesIcon, ListTreeIcon } from "lucide-react";

import { useAgentChatStore, type AgentTurnProfile } from "@/stores/agent-chat-store";
import { useSettingsStore } from "@/stores/settings-store";

const PAPER_DRAFTING_PROFILE: AgentTurnProfile = {
  taskKind: "paper_drafting",
  selectionScope: "none",
  responseMode: "default",
  samplingProfile: "edit_stable",
  sourceHint: "paper_drafting_panel",
};

export const PaperDraftingPanel: FC = () => {
  const runtime = useSettingsStore(
    (s) => s.effective.integrations.agent.runtime ?? "claude_cli",
  );
  const activeTabId = useAgentChatStore((s) => s.activeTabId);
  const activeTab = useAgentChatStore((s) => s.tabs.find((tab) => tab.id === activeTabId));
  const sendPrompt = useAgentChatStore((s) => s.sendPrompt);

  const [manuscriptType, setManuscriptType] = useState("imrad");
  const [sectionType, setSectionType] = useState("Introduction");
  const [outline, setOutline] = useState("");
  const [keyPoints, setKeyPoints] = useState("");
  const [citationKeys, setCitationKeys] = useState("");

  const parsedOutline = useMemo(
    () =>
      outline
        .split("\n")
        .map((line) => line.trim())
        .filter((line) => line.length > 0),
    [outline],
  );
  const parsedKeyPoints = useMemo(
    () =>
      keyPoints
        .split("\n")
        .map((line) => line.trim())
        .filter((line) => line.length > 0),
    [keyPoints],
  );
  const parsedCitationKeys = useMemo(
    () =>
      citationKeys
        .split(",")
        .map((line) => line.trim())
        .filter((line) => line.length > 0),
    [citationKeys],
  );

  if (runtime !== "local_agent") {
    return null;
  }

  const isStreaming = activeTab?.isStreaming ?? false;

  const startPaperWorkflow = async () => {
    const prompt = [
      "Run paper_drafting workflow for this manuscript.",
      `Manuscript type: ${manuscriptType}`,
      parsedOutline.length > 0 ? "Current outline:" : null,
      ...parsedOutline.map((line) => `- ${line}`),
      "Stage goal: confirm outline then continue section drafting with checkpoints.",
    ]
      .filter(Boolean)
      .join("\n");
    await sendPrompt(prompt, undefined, PAPER_DRAFTING_PROFILE);
  };

  const draftSection = async () => {
    if (parsedKeyPoints.length === 0) {
      return;
    }
    const prompt = [
      "Paper drafting workflow: draft a manuscript section.",
      `Section type: ${sectionType}`,
      `Manuscript type: ${manuscriptType}`,
      "Key points:",
      ...parsedKeyPoints.map((line) => `- ${line}`),
      parsedCitationKeys.length > 0 ? `Citation keys: ${parsedCitationKeys.join(", ")}` : null,
      "Call draft_section and keep claims evidence-calibrated.",
    ]
      .filter(Boolean)
      .join("\n");
    await sendPrompt(prompt, undefined, PAPER_DRAFTING_PROFILE);
  };

  const restructure = async () => {
    if (parsedOutline.length === 0) {
      return;
    }
    const prompt = [
      "Paper drafting workflow: restructure manuscript outline.",
      `Manuscript type: ${manuscriptType}`,
      "Outline:",
      ...parsedOutline.map((line) => `- ${line}`),
      "Call restructure_outline and return the revised section plan.",
    ].join("\n");
    await sendPrompt(prompt, undefined, PAPER_DRAFTING_PROFILE);
  };

  return (
    <div className="mx-3 mb-2 rounded-xl border border-violet-500/35 bg-violet-500/8 px-3 py-3">
      <div className="mb-2 text-[11px] text-violet-700 dark:text-violet-200 uppercase tracking-wide font-medium">
        Paper Drafting Workflow
      </div>
      <div className="grid gap-2">
        <div className="grid grid-cols-1 gap-2 md:grid-cols-2">
          <select
            value={manuscriptType}
            onChange={(event) => setManuscriptType(event.target.value)}
            className="h-8 rounded-md border border-border bg-background px-2 text-xs text-foreground"
          >
            <option value="imrad">IMRaD</option>
            <option value="review">Review Article</option>
            <option value="case_report">Case Report</option>
            <option value="methods_paper">Methods Paper</option>
          </select>
          <input
            value={sectionType}
            onChange={(event) => setSectionType(event.target.value)}
            placeholder="Section type (Introduction/Methods/...)"
            className="h-8 rounded-md border border-border bg-background px-2 text-xs text-foreground"
          />
        </div>

        <textarea
          value={outline}
          onChange={(event) => setOutline(event.target.value)}
          placeholder={"Outline sections (one per line)\nIntroduction\nMethods\nResults\nDiscussion"}
          className="min-h-16 rounded-md border border-border bg-background px-2 py-1.5 text-xs text-foreground"
        />

        <textarea
          value={keyPoints}
          onChange={(event) => setKeyPoints(event.target.value)}
          placeholder={"Section key points (one per line)\nPoint 1\nPoint 2"}
          className="min-h-14 rounded-md border border-border bg-background px-2 py-1.5 text-xs text-foreground"
        />

        <input
          value={citationKeys}
          onChange={(event) => setCitationKeys(event.target.value)}
          placeholder="Citation keys (comma-separated, optional)"
          className="h-8 rounded-md border border-border bg-background px-2 text-xs text-foreground"
        />
      </div>

      <div className="mt-3 flex flex-wrap gap-1.5">
        <button
          type="button"
          onClick={() => void startPaperWorkflow()}
          disabled={isStreaming}
          className="inline-flex items-center gap-1 rounded-md border border-border bg-background px-2 py-1.5 text-xs text-foreground transition-colors hover:bg-muted disabled:cursor-not-allowed disabled:opacity-60"
        >
          <FileTextIcon className="size-3.5" />
          Start Drafting
        </button>
        <button
          type="button"
          onClick={() => void draftSection()}
          disabled={isStreaming || parsedKeyPoints.length === 0}
          className="inline-flex items-center gap-1 rounded-md border border-border bg-background px-2 py-1.5 text-xs text-foreground transition-colors hover:bg-muted disabled:cursor-not-allowed disabled:opacity-60"
        >
          <WandSparklesIcon className="size-3.5" />
          Draft Section
        </button>
        <button
          type="button"
          onClick={() => void restructure()}
          disabled={isStreaming || parsedOutline.length === 0}
          className="inline-flex items-center gap-1 rounded-md border border-border bg-background px-2 py-1.5 text-xs text-foreground transition-colors hover:bg-muted disabled:cursor-not-allowed disabled:opacity-60"
        >
          <ListTreeIcon className="size-3.5" />
          Restructure Outline
        </button>
      </div>
    </div>
  );
};
