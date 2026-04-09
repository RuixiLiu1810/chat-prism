import { useMemo, useState, type FC } from "react";
import { SearchIcon, MicroscopeIcon, LayersIcon } from "lucide-react";

import { useAgentChatStore, type AgentTurnProfile } from "@/stores/agent-chat-store";
import { useSettingsStore } from "@/stores/settings-store";

const LITERATURE_PROFILE: AgentTurnProfile = {
  taskKind: "literature_review",
  selectionScope: "none",
  responseMode: "default",
  samplingProfile: "analysis_deep",
  sourceHint: "literature_panel",
};

export const LiteratureReviewPanel: FC = () => {
  const runtime = useSettingsStore(
    (s) => s.effective.integrations.agent.runtime ?? "claude_cli",
  );
  const activeTabId = useAgentChatStore((s) => s.activeTabId);
  const activeTab = useAgentChatStore((s) => s.tabs.find((tab) => tab.id === activeTabId));
  const sendPrompt = useAgentChatStore((s) => s.sendPrompt);
  const [question, setQuestion] = useState("");
  const [population, setPopulation] = useState("");
  const [intervention, setIntervention] = useState("");
  const [comparator, setComparator] = useState("");
  const [outcome, setOutcome] = useState("");
  const [paperPaths, setPaperPaths] = useState("");

  const parsedPaths = useMemo(
    () =>
      paperPaths
        .split("\n")
        .map((line) => line.trim())
        .filter((line) => line.length > 0),
    [paperPaths],
  );

  if (runtime !== "local_agent") {
    return null;
  }

  const isStreaming = activeTab?.isStreaming ?? false;

  const submitPico = async () => {
    const lines = [
      "Run literature_review workflow for this research question.",
      question.trim() ? `Question: ${question.trim()}` : null,
      population.trim() ? `Population/Problem: ${population.trim()}` : null,
      intervention.trim() ? `Intervention/Exposure: ${intervention.trim()}` : null,
      comparator.trim() ? `Comparator: ${comparator.trim()}` : null,
      outcome.trim() ? `Outcome: ${outcome.trim()}` : null,
      "Stage goal: perform PICO scoping then continue with search/screening.",
    ].filter(Boolean);
    await sendPrompt(lines.join("\n"), undefined, LITERATURE_PROFILE);
  };

  const analyzeSelected = async () => {
    if (parsedPaths.length === 0) {
      return;
    }
    const prompt = [
      "Literature review workflow: analyze selected papers.",
      `Selected papers (${parsedPaths.length}):`,
      ...parsedPaths.map((path) => `- ${path}`),
      "For each paper, extract objective, methods, key findings, limitations, and relevance.",
    ].join("\n");
    await sendPrompt(prompt, undefined, LITERATURE_PROFILE);
  };

  const synthesize = async () => {
    if (parsedPaths.length === 0) {
      return;
    }
    const prompt = [
      "Literature review workflow: synthesize evidence from selected papers.",
      `Selected papers (${parsedPaths.length}):`,
      ...parsedPaths.map((path) => `- ${path}`),
      "Produce theme-organized synthesis with source-linked evidence and uncertainty notes.",
    ].join("\n");
    await sendPrompt(prompt, undefined, LITERATURE_PROFILE);
  };

  return (
    <div className="mx-3 mb-2 rounded-xl border border-emerald-500/35 bg-emerald-500/8 px-3 py-3">
      <div className="mb-2 text-[11px] text-emerald-700 dark:text-emerald-200 uppercase tracking-wide font-medium">
        Literature Workflow
      </div>
      <div className="grid gap-2">
        <input
          value={question}
          onChange={(event) => setQuestion(event.target.value)}
          placeholder="Research question"
          className="h-8 rounded-md border border-border bg-background px-2 text-xs text-foreground"
        />
        <div className="grid grid-cols-1 gap-2 md:grid-cols-2">
          <input
            value={population}
            onChange={(event) => setPopulation(event.target.value)}
            placeholder="Population / Problem (P)"
            className="h-8 rounded-md border border-border bg-background px-2 text-xs text-foreground"
          />
          <input
            value={intervention}
            onChange={(event) => setIntervention(event.target.value)}
            placeholder="Intervention (I)"
            className="h-8 rounded-md border border-border bg-background px-2 text-xs text-foreground"
          />
          <input
            value={comparator}
            onChange={(event) => setComparator(event.target.value)}
            placeholder="Comparator (C)"
            className="h-8 rounded-md border border-border bg-background px-2 text-xs text-foreground"
          />
          <input
            value={outcome}
            onChange={(event) => setOutcome(event.target.value)}
            placeholder="Outcome (O)"
            className="h-8 rounded-md border border-border bg-background px-2 text-xs text-foreground"
          />
        </div>
        <textarea
          value={paperPaths}
          onChange={(event) => setPaperPaths(event.target.value)}
          placeholder={"Selected paper paths (one per line)\nattachments/paper-a.pdf\nattachments/paper-b.pdf"}
          className="min-h-16 rounded-md border border-border bg-background px-2 py-1.5 text-xs text-foreground"
        />
      </div>
      <div className="mt-3 flex flex-wrap gap-1.5">
        <button
          type="button"
          onClick={() => void submitPico()}
          disabled={isStreaming || question.trim().length === 0}
          className="inline-flex items-center gap-1 rounded-md border border-border bg-background px-2 py-1.5 text-xs text-foreground transition-colors hover:bg-muted disabled:cursor-not-allowed disabled:opacity-60"
        >
          <SearchIcon className="size-3.5" />
          Submit PICO
        </button>
        <button
          type="button"
          onClick={() => void analyzeSelected()}
          disabled={isStreaming || parsedPaths.length === 0}
          className="inline-flex items-center gap-1 rounded-md border border-border bg-background px-2 py-1.5 text-xs text-foreground transition-colors hover:bg-muted disabled:cursor-not-allowed disabled:opacity-60"
        >
          <MicroscopeIcon className="size-3.5" />
          Analyze Papers
        </button>
        <button
          type="button"
          onClick={() => void synthesize()}
          disabled={isStreaming || parsedPaths.length === 0}
          className="inline-flex items-center gap-1 rounded-md border border-border bg-background px-2 py-1.5 text-xs text-foreground transition-colors hover:bg-muted disabled:cursor-not-allowed disabled:opacity-60"
        >
          <LayersIcon className="size-3.5" />
          Synthesize
        </button>
      </div>
    </div>
  );
};
