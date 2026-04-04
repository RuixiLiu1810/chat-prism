import { create } from "zustand";
import { invoke } from "@tauri-apps/api/core";
import { useDocumentStore } from "./document-store";
import { useHistoryStore } from "./history-store";
import { useSettingsStore } from "./settings-store";
import { createLogger } from "@/lib/debug/logger";
import {
  formatRelevantResourceEvidence,
  findRelevantAttachmentMatches,
  type IngestedResourceMatch,
  type ResourceEvidenceGroup,
} from "@/lib/resource-ingestion";
import {
  adaptAgentStreamMessageForUi,
  getToolResultDisplayTarget,
} from "@/lib/agent-message-adapter";
import type { AgentRuntimeKind } from "@/lib/settings-schema";

const log = createLogger("agent-chat");

export function normalizeAgentError(message: string): string {
  const text = message.trim();

  if (!text) {
    return "Agent runtime failed. Please retry.";
  }

  if (text.includes("Agent run cancelled by user.")) {
    return "Agent run cancelled.";
  }

  if (text.includes("API key is not configured")) {
    return "Agent provider API key is not configured. Please check Settings -> Providers.";
  }

  if (
    text.includes("Unsupported agent provider in settings") ||
    text.includes("not promoted to a working transport")
  ) {
    return "The selected provider is not available in the current local runtime.";
  }

  if (text.includes("request failed with status 401")) {
    return "Provider authorization failed (401). Please check the API key.";
  }

  if (text.includes("request failed with status 403")) {
    return "Provider access was denied (403). Please check account permissions or model access.";
  }

  if (text.includes("request failed with status 404")) {
    return "Provider route was not found (404). Please check the Base URL and runtime mode.";
  }

  if (text.includes("request failed with status 429")) {
    return "Provider rate limit reached (429). Please wait a moment and retry.";
  }

  if (text.includes("Failed to parse") && text.includes("stream")) {
    return "Provider returned an unexpected streaming payload. Please retry or switch provider.";
  }

  if (text.includes("request failed:")) {
    return "Provider request failed. Please check network connectivity and provider settings.";
  }

  if (text.includes("streaming read failed")) {
    return "Agent stream was interrupted. Please retry.";
  }

  return text;
}

function isSuggestionOnlyRequest(prompt: string): boolean {
  const normalized = prompt.toLowerCase();
  return [
    "suggest",
    "propose",
    "give me options",
    "do not modify",
    "don't modify",
    "without modifying",
    "review only",
    "建议",
    "给我几个",
    "不要改文件",
    "只是看看",
    "解释一下",
    "分析一下",
    "只做建议",
    "仅建议",
  ].some((needle) => normalized.includes(needle) || prompt.includes(needle));
}

function isEditIntentRequest(prompt: string): boolean {
  const normalized = prompt.toLowerCase();
  return [
    "refine",
    "rewrite",
    "polish",
    "improve",
    "proofread",
    "fix",
    "revise",
    "edit",
    "tighten",
    "make this better",
    "修改",
    "改成",
    "改为",
    "润色",
    "优化",
    "精简",
    "修正",
    "完善",
    "重写",
    "调整",
    "修一下",
    "帮我改",
    "改一下",
  ].some((needle) => normalized.includes(needle) || prompt.includes(needle));
}

function isAnalysisRequest(prompt: string): boolean {
  const normalized = prompt.toLowerCase();
  return [
    "explain",
    "analyze",
    "analyse",
    "critique",
    "comment on",
    "what does",
    "why is",
    "why does",
    "assess",
    "解释",
    "分析",
    "评估",
    "批评",
    "怎么看",
    "怎么样",
  ].some((needle) => normalized.includes(needle) || prompt.includes(needle));
}

function isDeepAnalysisRequest(prompt: string): boolean {
  const normalized = prompt.toLowerCase();
  return [
    "detailed",
    "deep",
    "deeper",
    "compare",
    "comparison",
    "synthesize",
    "summary",
    "summarize",
    "which paper",
    "which article",
    "evidence",
    "详细",
    "深入",
    "展开",
    "对比",
    "比较",
    "总结",
    "归纳",
    "综述",
    "哪篇",
    "哪一篇",
    "列出",
    "依据",
  ].some((needle) => normalized.includes(needle) || prompt.includes(needle));
}

function resolveAnalysisSamplingProfile(
  prompt: string,
  preferDeep = false,
): AgentSamplingProfile {
  return preferDeep || isDeepAnalysisRequest(prompt)
    ? "analysis_deep"
    : "analysis_balanced";
}

type AgentTaskKind =
  | "general"
  | "selection_edit"
  | "file_edit"
  | "suggestion_only"
  | "analysis";
type AgentSelectionScope = "none" | "selected_span";
type AgentResponseMode = "default" | "reviewable_change" | "suggestion_only";
type AgentSamplingProfile =
  | "default"
  | "edit_stable"
  | "analysis_balanced"
  | "analysis_deep"
  | "chat_flexible";

export interface AgentTurnProfile {
  taskKind: AgentTaskKind;
  selectionScope: AgentSelectionScope;
  responseMode: AgentResponseMode;
  samplingProfile: AgentSamplingProfile;
  sourceHint?: string | null;
}

export type AgentPromptContextKind = "selection" | "file" | "attachment";

export interface AgentPromptContext {
  label: string;
  filePath: string;
  absolutePath?: string;
  selectedText: string;
  imageDataUrl?: string;
  kind: AgentPromptContextKind;
  sourceType?: string;
}

function makeTurnProfile(
  taskKind: AgentTaskKind,
  selectionScope: AgentSelectionScope,
  responseMode: AgentResponseMode,
  samplingProfile: AgentSamplingProfile,
  sourceHint: string | null,
): AgentTurnProfile {
  return {
    taskKind,
    selectionScope,
    responseMode,
    samplingProfile,
    sourceHint,
  };
}

function resolveTurnProfile(
  prompt: string,
  options: {
    hasSelectionContext: boolean;
    hasActiveFile: boolean;
    hasAttachmentContext?: boolean;
  },
): AgentTurnProfile {
  const { hasSelectionContext, hasActiveFile, hasAttachmentContext } = options;

  if (hasSelectionContext) {
    if (isSuggestionOnlyRequest(prompt)) {
      return makeTurnProfile(
        "suggestion_only",
        "selected_span",
        "suggestion_only",
        "analysis_balanced",
        "selection+explicit_suggestion",
      );
    }
    if (isAnalysisRequest(prompt)) {
      return makeTurnProfile(
        "analysis",
        "selected_span",
        "default",
        resolveAnalysisSamplingProfile(prompt),
        "selection+explicit_analysis",
      );
    }
    return makeTurnProfile(
      "selection_edit",
      "selected_span",
      "reviewable_change",
      "edit_stable",
      "selection+default_edit",
    );
  }

  if (hasActiveFile && isSuggestionOnlyRequest(prompt)) {
    return makeTurnProfile(
      "suggestion_only",
      "none",
      "suggestion_only",
      "analysis_balanced",
      "active_file+explicit_suggestion",
    );
  }
  if (hasActiveFile && isAnalysisRequest(prompt)) {
    return makeTurnProfile(
      "analysis",
      "none",
      "default",
      resolveAnalysisSamplingProfile(prompt),
      "active_file+explicit_analysis",
    );
  }
  if (hasActiveFile && isEditIntentRequest(prompt)) {
    return makeTurnProfile(
      "file_edit",
      "none",
      "reviewable_change",
      "edit_stable",
      "active_file+explicit_edit",
    );
  }

  if (hasAttachmentContext && isSuggestionOnlyRequest(prompt)) {
    return makeTurnProfile(
      "suggestion_only",
      "none",
      "suggestion_only",
      "analysis_balanced",
      "attachment+explicit_suggestion",
    );
  }
  if (hasAttachmentContext && isAnalysisRequest(prompt)) {
    return makeTurnProfile(
      "analysis",
      "none",
      "default",
      resolveAnalysisSamplingProfile(prompt, true),
      "attachment+explicit_analysis",
    );
  }

  if (hasAttachmentContext) {
    return makeTurnProfile(
      "analysis",
      "none",
      "default",
      resolveAnalysisSamplingProfile(prompt, true),
      "attachment+default_analysis",
    );
  }

  return makeTurnProfile("general", "none", "default", "default", null);
}

/** Convert a character offset to 1-based line:col */
export function offsetToLineCol(
  content: string,
  offset: number,
): { line: number; col: number } {
  const before = content.slice(0, offset);
  const lines = before.split("\n");
  return { line: lines.length, col: lines[lines.length - 1].length + 1 };
}

// ─── Types ───

export interface ContentBlock {
  type: "text" | "tool_use" | "tool_result" | "thinking";
  // text block
  text?: string;
  // tool_use block
  id?: string;
  name?: string;
  input?: any;
  // tool_result block
  tool_use_id?: string;
  content?: any;
  is_error?: boolean;
  // thinking block
  thinking?: string;
  signature?: string;
}

export interface AgentStreamMessage {
  type: "system" | "assistant" | "user" | "result";
  subtype?: string;
  session_id?: string;
  model?: string;
  cwd?: string;
  tools?: string[];
  message?: {
    content?: ContentBlock[];
    usage?: { input_tokens: number; output_tokens: number };
  };
  usage?: { input_tokens: number; output_tokens: number };
  cost_usd?: number;
  duration_ms?: number;
  duration_api_ms?: number;
  result?: string;
  is_error?: boolean;
  num_turns?: number;
}

// ─── Tab Types ───

export interface TabDraft {
  input: string;
  pinnedContexts: AgentPromptContext[];
}

export interface TabState {
  id: string;
  title: string;
  sessionId: string | null;
  sessionMeta: SessionIdentity | null;
  currentWorkLabel: string | null;
  recentToolActivity: string | null;
  pendingApproval: PendingApprovalState | null;
  messages: AgentStreamMessage[];
  isStreaming: boolean;
  error: string | null;
  statusStage: string | null;
  statusMessage: string | null;
  totalInputTokens: number;
  totalOutputTokens: number;
  draft: TabDraft;
}

export interface ResumableSessionMeta {
  localSessionId?: string | null;
  title: string;
  provider?: string | null;
  model?: string | null;
  updatedAt?: string | null;
  preview?: string | null;
  messageCount?: number | null;
  currentObjective?: string | null;
  currentTarget?: string | null;
  lastToolActivity?: string | null;
  pendingState?: string | null;
  pendingTarget?: string | null;
}

export interface SessionIdentity {
  localSessionId: string;
  title: string;
  provider?: string | null;
  model?: string | null;
  updatedAt?: string | null;
  preview?: string | null;
  messageCount?: number | null;
  currentObjective?: string | null;
  currentTarget?: string | null;
  lastToolActivity?: string | null;
  pendingState?: string | null;
  pendingTarget?: string | null;
}

export interface PendingApprovalState {
  phase: "awaiting_approval" | "review_ready";
  toolName: string;
  approvalToolName?: string | null;
  callId: string;
  targetPath?: string | null;
  reviewReady: boolean;
  canResume: boolean;
  message: string;
}

/** Fields that are projected from the active tab to top-level state */
const TAB_FIELDS = [
  "sessionId",
  "messages",
  "isStreaming",
  "error",
  "statusStage",
  "statusMessage",
  "totalInputTokens",
  "totalOutputTokens",
] as const;

function makeDefaultTab(id: string): TabState {
  return {
    id,
    title: "New Chat",
    sessionId: null,
    sessionMeta: null,
    currentWorkLabel: null,
    recentToolActivity: null,
    pendingApproval: null,
    messages: [],
    isStreaming: false,
    error: null,
    statusStage: null,
    statusMessage: null,
    totalInputTokens: 0,
    totalOutputTokens: 0,
    draft: { input: "", pinnedContexts: [] },
  };
}

let tabCounter = 0;
function nextTabId(): string {
  return `tab-${++tabCounter}`;
}

function getConfiguredChatRuntime(): AgentRuntimeKind {
  return (
    useSettingsStore.getState().effective.integrations.agent.runtime ??
    "claude_cli"
  );
}

function getConfiguredLocalAgentModel(): string {
  return (
    useSettingsStore.getState().effective.integrations.agent.model || "gpt-5.4"
  );
}

function resolveClaudeModel(
  selectedModel: "sonnet" | "opus" | "haiku" | "opusplan",
): string {
  return selectedModel;
}

interface ClaudeSessionInfo {
  session_id: string;
  title: string;
  last_modified: number;
}

interface LocalAgentSessionSummary {
  localSessionId: string;
  title: string;
  updatedAt: string;
  createdAt: string;
  provider: string;
  model: string;
  preview?: string | null;
  messageCount: number;
  currentObjective?: string | null;
  currentTarget?: string | null;
  lastToolActivity?: string | null;
  pendingState?: string | null;
  pendingTarget?: string | null;
}

function buildSessionIdentity(
  sessionId: string,
  metadata?: ResumableSessionMeta | null,
): SessionIdentity {
  return {
    localSessionId: metadata?.localSessionId || sessionId,
    title: metadata?.title || "Session",
    provider: metadata?.provider ?? null,
    model: metadata?.model ?? null,
    updatedAt: metadata?.updatedAt ?? null,
    preview: metadata?.preview ?? null,
    messageCount: metadata?.messageCount ?? null,
    currentObjective: metadata?.currentObjective ?? null,
    currentTarget: metadata?.currentTarget ?? null,
    lastToolActivity: metadata?.lastToolActivity ?? null,
    pendingState: metadata?.pendingState ?? null,
    pendingTarget: metadata?.pendingTarget ?? null,
  };
}

function summarizeToolTarget(input: unknown): string | null {
  if (!input || typeof input !== "object" || Array.isArray(input)) {
    return null;
  }
  const record = input as Record<string, unknown>;
  const candidate = ["path", "file_path", "command", "query"]
    .map((key) => record[key])
    .find((value) => typeof value === "string" && value.trim().length > 0);
  return typeof candidate === "string" ? candidate.trim() : null;
}

function summarizeToolContentTarget(content: unknown): string | null {
  return getToolResultDisplayTarget(content);
}

function deriveToolContextFromMessages(messages: AgentStreamMessage[]): {
  currentWorkLabel: string | null;
  recentToolActivity: string | null;
} {
  for (let messageIndex = messages.length - 1; messageIndex >= 0; messageIndex -= 1) {
    const blocks = messages[messageIndex]?.message?.content ?? [];
    for (let blockIndex = blocks.length - 1; blockIndex >= 0; blockIndex -= 1) {
      const block = blocks[blockIndex];
      if (block.type === "tool_result") {
        const target = summarizeToolContentTarget(block.content);
        return {
          currentWorkLabel: target,
          recentToolActivity: target
            ? `Latest tool result on ${target}`
            : "Latest tool result recorded",
        };
      }
      if (block.type === "tool_use") {
        const target = summarizeToolTarget(block.input);
        const toolName = block.name ?? "tool";
        return {
          currentWorkLabel: target,
          recentToolActivity: target
            ? `Last tool: ${toolName} -> ${target}`
            : `Last tool: ${toolName}`,
        };
      }
    }
  }

  return {
    currentWorkLabel: null,
    recentToolActivity: null,
  };
}

/**
 * Update a specific tab in `tabs[]` and, if that tab is the active tab,
 * also project the changed fields to top-level state for consumer compatibility.
 */
function applyTabUpdate(
  state: AgentChatState,
  tabId: string,
  updates: Partial<TabState>,
): Partial<AgentChatState> {
  const newTabs = state.tabs.map((t) =>
    t.id === tabId ? { ...t, ...updates } : t,
  );
  const result: Partial<AgentChatState> = { tabs: newTabs };
  if (tabId === state.activeTabId) {
    for (const key of TAB_FIELDS) {
      if (key in updates) {
        (result as any)[key] = (updates as any)[key];
      }
    }
  }
  return result;
}

// ─── State Interface ───

const DEFAULT_TAB_ID = nextTabId();

interface AgentChatState {
  // ── Projected fields (from active tab — read by consumers) ──
  messages: AgentStreamMessage[];
  sessionId: string | null;
  isStreaming: boolean;
  error: string | null;
  statusStage: string | null;
  statusMessage: string | null;
  totalInputTokens: number;
  totalOutputTokens: number;

  // ── Tab state ──
  tabs: TabState[];
  activeTabId: string;

  /** Deferred prompt to send once the workspace is ready (set by project wizard) */
  pendingInitialPrompt: string | null;
  setPendingInitialPrompt: (prompt: string | null) => void;
  consumePendingInitialPrompt: () => string | null;

  /** Pending attachments from external sources (e.g. PDF capture) */
  pendingAttachments: AgentPromptContext[];
  addPendingAttachment: (attachment: AgentPromptContext) => void;
  consumePendingAttachments: () => AgentPromptContext[];

  /** Currently selected runtime profile, resolved per prompt for the agent backend */
  selectedModel: "sonnet" | "opus" | "haiku" | "opusplan";
  setSelectedModel: (model: "sonnet" | "opus" | "haiku" | "opusplan") => void;

  /** Reserved effort knob for future agent runtime support */
  effortLevel: "low" | "medium" | "high";
  setEffortLevel: (level: "low" | "medium" | "high") => void;

  // Actions
  sendPrompt: (
    userPrompt: string,
    contextOverride?: AgentPromptContext | AgentPromptContext[],
    turnProfileOverride?: AgentTurnProfile,
  ) => Promise<void>;
  cancelExecution: () => Promise<void>;
  clearMessages: () => void;
  newSession: () => void;
  resumeSession: (
    sessionId: string,
    metadata?: ResumableSessionMeta,
  ) => Promise<void>;
  setToolApproval: (
    toolName: string,
    decision: "allow_once" | "allow_session" | "deny_session",
  ) => Promise<void>;
  continueAfterApproval: (
    toolName: string,
    targetLabel?: string,
  ) => Promise<void>;
  resetToolApprovals: () => Promise<void>;

  // Tab actions
  createTab: () => string;
  closeTab: (tabId: string) => void;
  setActiveTab: (tabId: string) => void;
  saveDraft: (tabId: string, draft: TabDraft) => void;

  /** True when any tab is streaming */
  anyStreaming: () => boolean;
  refreshSessionMeta: (tabId: string, localSessionId: string) => Promise<void>;

  // Internal actions (called by event hook, routed by tabId)
  _appendMessage: (tabId: string, msg: AgentStreamMessage) => void;
  _appendAssistantTextDelta: (tabId: string, delta: string) => void;
  _setSessionId: (tabId: string, id: string) => void;
  _setSessionMeta: (tabId: string, meta: SessionIdentity | null) => void;
  _setStreaming: (tabId: string, streaming: boolean) => void;
  _setError: (tabId: string, error: string | null) => void;
  _setStatus: (
    tabId: string,
    stage: string | null,
    message: string | null,
  ) => void;
  _setWorkContext: (
    tabId: string,
    currentWorkLabel: string | null,
    recentToolActivity: string | null,
  ) => void;
  _setPendingApproval: (
    tabId: string,
    pendingApproval: PendingApprovalState | null,
  ) => void;
  _cancelledByUser: boolean;
}

// ─── Store ───

export const useAgentChatStore = create<AgentChatState>()((set, get) => ({
  // Projected fields (initialized from default tab)
  messages: [],
  sessionId: null,
  isStreaming: false,
  error: null,
  statusStage: null,
  statusMessage: null,
  _cancelledByUser: false,
  totalInputTokens: 0,
  totalOutputTokens: 0,

  // Tab state
  tabs: [makeDefaultTab(DEFAULT_TAB_ID)],
  activeTabId: DEFAULT_TAB_ID,

  selectedModel: "opus",
  setSelectedModel: (model) => set({ selectedModel: model }),

  effortLevel: "medium",
  setEffortLevel: (level) => set({ effortLevel: level }),

  pendingInitialPrompt: null,
  setPendingInitialPrompt: (prompt) => set({ pendingInitialPrompt: prompt }),
  consumePendingInitialPrompt: () => {
    const { pendingInitialPrompt } = get();
    if (pendingInitialPrompt) {
      set({ pendingInitialPrompt: null });
    }
    return pendingInitialPrompt;
  },

  pendingAttachments: [],
  addPendingAttachment: (attachment) => {
    set((state) => ({
      pendingAttachments: [...state.pendingAttachments, attachment],
    }));
  },
  consumePendingAttachments: () => {
    const { pendingAttachments } = get();
    if (pendingAttachments.length > 0) {
      set({ pendingAttachments: [] });
    }
    return pendingAttachments;
  },

  anyStreaming: () => get().tabs.some((t) => t.isStreaming),

  refreshSessionMeta: async (tabId: string, localSessionId: string) => {
    try {
      const projectPath = useDocumentStore.getState().projectRoot;
      if (!projectPath) return;
      const runtime = getConfiguredChatRuntime();
      if (runtime === "claude_cli") {
        const sessions = await invoke<ClaudeSessionInfo[]>("list_claude_sessions", {
          projectPath,
        });
        const session = sessions.find((entry) => entry.session_id === localSessionId);
        if (!session) return;
        set((state) =>
          applyTabUpdate(state, tabId, {
            title: session.title || state.tabs.find((t) => t.id === tabId)?.title,
            sessionMeta: buildSessionIdentity(localSessionId, {
              localSessionId,
              title: session.title,
              provider: "claude-cli",
              model: get().selectedModel,
              updatedAt: new Date(session.last_modified).toISOString(),
              preview: null,
              messageCount: null,
            }),
          }),
        );
        return;
      }

      const session = await invoke<LocalAgentSessionSummary | null>(
        "agent_get_session_summary",
        {
          localSessionId,
        },
      );
      if (!session) return;
      set((state) =>
        applyTabUpdate(state, tabId, {
          title: session.title || state.tabs.find((t) => t.id === tabId)?.title,
          sessionMeta: buildSessionIdentity(localSessionId, session),
        }),
      );
    } catch (err) {
      log.warn("Failed to refresh session metadata", {
        error: String(err),
        tabId,
        localSessionId,
      });
    }
  },

  sendPrompt: async (
    userPrompt: string,
    contextOverride?: AgentPromptContext | AgentPromptContext[],
    turnProfileOverride?: AgentTurnProfile,
  ) => {
    const state = get();
    const { activeTabId } = state;
    const activeTab = state.tabs.find((t) => t.id === activeTabId);
    // Guard: prevent sending from a tab that's already streaming
    if (activeTab?.isStreaming) return;

    const { sessionId, selectedModel, effortLevel } = state;
    const claudeModel = resolveClaudeModel(selectedModel);
    const runtime = getConfiguredChatRuntime();

    const sendStart = performance.now();
    log.info("sendPrompt start", {
      sessionId: !!sessionId,
      hasContext: !!contextOverride,
      tab: activeTabId,
    });

    const docState = useDocumentStore.getState();
    const projectPath = docState.projectRoot;
    if (!projectPath) {
      set((s) => applyTabUpdate(s, activeTabId, { error: "No project open" }));
      return;
    }

    // Compute context label for display in chat history
    const activeFile = docState.files.find(
      (f) => f.id === docState.activeFileId,
    );
    const promptContexts = Array.isArray(contextOverride)
      ? contextOverride
      : contextOverride
        ? [contextOverride]
        : [];
    const primaryContext = promptContexts[0] ?? null;
    let contextLabel: string | null = null;

    if (promptContexts.length > 0) {
      contextLabel = promptContexts.map((ctx) => ctx.label).join(", ");
    } else if (activeFile) {
      const selRange = docState.selectionRange;
      if (selRange && activeFile.content) {
        const content = activeFile.content;
        const startLC = offsetToLineCol(content, selRange.start);
        const endLC = offsetToLineCol(content, selRange.end);
        contextLabel = `@${activeFile.relativePath}:${startLC.line}:${startLC.col}-${endLC.line}:${endLC.col}`;
      }
    }

    // Add user message to the list for display (with context label visible)
    const displayText = contextLabel
      ? `${contextLabel}\n${userPrompt}`
      : userPrompt;
    const userMessage: AgentStreamMessage = {
      type: "user",
      message: {
        content: [{ type: "text", text: displayText }],
      },
    };

    // Auto-set tab title from first prompt
    const isFirstMessage = activeTab && activeTab.messages.length === 0;
    const tabTitle = isFirstMessage
      ? userPrompt.slice(0, 40) + (userPrompt.length > 40 ? "..." : "")
      : undefined;

    set((s) => {
      const tabUpdates: Partial<TabState> = {
        messages: [
          ...(s.tabs.find((t) => t.id === activeTabId)?.messages ?? []),
          userMessage,
        ],
        currentWorkLabel: primaryContext?.filePath || activeFile?.relativePath || null,
        pendingApproval: null,
        isStreaming: true,
        error: null,
        statusStage: "starting",
        statusMessage: "Starting agent run...",
      };
      if (tabTitle) tabUpdates.title = tabTitle;
      return {
        ...applyTabUpdate(s, activeTabId, tabUpdates),
        _cancelledByUser: false,
      };
    });

    // Flush unsaved edits to disk so the agent reads the latest content
    if (docState.files.some((f) => f.isDirty)) {
      log.debug("saving dirty files...");
      await docState.saveAllFiles();
      log.debug("saveAllFiles done");
    }

    // Snapshot before agent edit
    if (projectPath) {
      try {
        log.debug("creating snapshot...");
        await useHistoryStore
          .getState()
          .createSnapshot(projectPath, "[agent] Before agent edit");
        log.debug("snapshot done");
      } catch {
        /* snapshot failure should not block the agent */
      }
    }

    // Build prompt with full context for the agent
    let prompt = userPrompt;
    let hasSelectionContext = promptContexts.some(
      (ctx) => ctx.kind === "selection",
    );
    const hasAttachmentContext = promptContexts.some(
      (ctx) => ctx.kind === "attachment" || ctx.kind === "file",
    );
    const hasActiveFileContext = promptContexts.length === 0 && !!activeFile;
    if (promptContexts.length > 0) {
      const promptLines: string[] = [];
      type AttachmentEvidenceCandidate = {
        filePath: string;
        sourceType?: string;
        matches: IngestedResourceMatch[];
      };
      const attachmentMatches: Array<AttachmentEvidenceCandidate | null> =
        await Promise.all(
        promptContexts.map(async (ctx) => {
          if (ctx.kind !== "attachment") return null;
          const file =
            ctx.absolutePath != null
              ? docState.files.find((candidate) => candidate.absolutePath === ctx.absolutePath) ?? null
              : docState.files.find((candidate) => candidate.relativePath === ctx.filePath) ?? null;
          const matches = await findRelevantAttachmentMatches(userPrompt, {
            absolutePath: ctx.absolutePath,
            file,
          });
          return { filePath: ctx.filePath, sourceType: ctx.sourceType, matches };
        }),
      );
      const evidenceGroups: ResourceEvidenceGroup[] = attachmentMatches.filter(
        (
          entry,
        ): entry is AttachmentEvidenceCandidate =>
          entry !== null && entry.matches.length > 0,
      );
      const evidenceLines = formatRelevantResourceEvidence(evidenceGroups);

      if (activeFile && hasSelectionContext) {
        promptLines.push(`[Currently open file: ${activeFile.relativePath}]`);
      }

      for (const ctx of promptContexts) {
        if (ctx.kind === "selection") {
          promptLines.push(`[Selection: ${ctx.label}]`);
          promptLines.push(`[Selected text:\n${ctx.selectedText}\n]`);
          continue;
        }

        const sourceType = ctx.sourceType ? ` (${ctx.sourceType})` : "";
        promptLines.push(`[Attached resource: ${ctx.label}${sourceType}]`);
        promptLines.push(`[Resource path: ${ctx.filePath}]`);
        promptLines.push(`[Attached excerpt:\n${ctx.selectedText}\n]`);
      }

      if (evidenceLines.length > 0) {
        promptLines.push(...evidenceLines);
      }

      prompt = `${promptLines.join("\n")}\n\n${userPrompt}`;
    } else if (activeFile) {
      const selRange = docState.selectionRange;
      const selectedText =
        selRange && activeFile.content
          ? activeFile.content.slice(selRange.start, selRange.end)
          : null;
      let ctx = `[Currently open file: ${activeFile.relativePath}]`;
      if (selectedText && selRange) {
        const content = activeFile.content ?? "";
        const startLC = offsetToLineCol(content, selRange.start);
        const endLC = offsetToLineCol(content, selRange.end);
        ctx += `\n[Selection: @${activeFile.relativePath}:${startLC.line}:${startLC.col}-${endLC.line}:${endLC.col}]`;
        ctx += `\n[Selected text:\n${selectedText}\n]`;
      }
      hasSelectionContext = selectedText !== null;
      prompt = `${ctx}\n\n${userPrompt}`;
    }
    const turnProfile =
      turnProfileOverride ??
      resolveTurnProfile(userPrompt, {
        hasSelectionContext,
        hasActiveFile: hasActiveFileContext,
        hasAttachmentContext,
      });

    log.info("invoking chat runtime", {
      promptLength: prompt.length,
      mode: sessionId ? "resume" : "new",
      runtime,
    });

    try {
      if (runtime === "claude_cli") {
        if (sessionId) {
          await invoke("resume_claude_code", {
            projectPath,
            sessionId,
            prompt,
            tabId: activeTabId,
            model: claudeModel,
            effortLevel,
          });
        } else {
          await invoke("execute_claude_code", {
            projectPath,
            prompt,
            tabId: activeTabId,
            model: claudeModel,
            effortLevel,
          });
        }
      } else {
        const localSessionId = sessionId
          ? await invoke<string>("agent_continue_turn", {
              projectPath,
              prompt,
              tabId: activeTabId,
              model: null,
              localSessionId: sessionId,
              previousResponseId: null,
              turnProfile,
            })
          : await invoke<string>("agent_start_turn", {
              projectPath,
              prompt,
              tabId: activeTabId,
              model: null,
              turnProfile,
            });
        if (localSessionId) {
          get()._setSessionId(activeTabId, localSessionId);
          await get().refreshSessionMeta(activeTabId, localSessionId);
        }
      }
      log.info(
        `sendPrompt complete in ${(performance.now() - sendStart).toFixed(0)}ms`,
      );
    } catch (err: any) {
      const message = err?.message || String(err);
      if (
        get()._cancelledByUser ||
        message.includes("Agent run cancelled by user.")
      ) {
        log.info("sendPrompt cancelled by user", { tab: activeTabId });
        return;
      }
      const normalizedMessage = normalizeAgentError(message);
      log.error(
        `sendPrompt failed after ${(performance.now() - sendStart).toFixed(0)}ms`,
        { error: String(err) },
      );
      set((s) =>
        applyTabUpdate(s, activeTabId, {
          isStreaming: false,
          error: normalizedMessage,
          statusStage: "failed",
          statusMessage: normalizedMessage,
        }),
      );
    }
  },

  cancelExecution: async () => {
    const { activeTabId } = get();
    const runtime = getConfiguredChatRuntime();
    set({ _cancelledByUser: true });
    try {
      if (runtime === "claude_cli") {
        await invoke("cancel_claude_execution", {
          tabId: activeTabId,
        });
      } else {
        await invoke("agent_cancel_turn", {
          tabId: activeTabId,
          responseId: null,
        });
      }
    } catch {
      // ignore
    }
    set((s) =>
      applyTabUpdate(s, activeTabId, {
        isStreaming: false,
        pendingApproval: null,
        statusStage: "cancelled",
        statusMessage: "Agent run cancelled.",
      }),
    );
  },

  clearMessages: () => {
    const { activeTabId } = get();
    set((s) =>
      applyTabUpdate(s, activeTabId, {
        messages: [],
        currentWorkLabel: null,
        recentToolActivity: null,
        pendingApproval: null,
        error: null,
        statusStage: null,
        statusMessage: null,
        totalInputTokens: 0,
        totalOutputTokens: 0,
      }),
    );
  },

  newSession: () => {
    log.info("Starting new session");
    if (getConfiguredChatRuntime() === "local_agent") {
      void get().resetToolApprovals();
    }
    const { activeTabId } = get();
    set((s) =>
      applyTabUpdate(s, activeTabId, {
        messages: [],
        sessionId: null,
        sessionMeta: null,
        currentWorkLabel: null,
        recentToolActivity: null,
        pendingApproval: null,
        error: null,
        isStreaming: false,
        statusStage: null,
        statusMessage: null,
        totalInputTokens: 0,
        totalOutputTokens: 0,
        title: "New Chat",
      }),
    );
  },

  resumeSession: async (sessionId: string, metadata) => {
    log.info(`Resuming session: ${sessionId.slice(0, 8)}`);
    const { activeTabId } = get();
    const projectPath = useDocumentStore.getState().projectRoot;
    const runtime = getConfiguredChatRuntime();
    const resumeLabel = [metadata?.provider, metadata?.model]
      .filter(Boolean)
      .join(" · ");

    // Reset state with new session ID
    set((s) =>
      applyTabUpdate(s, activeTabId, {
        messages: [],
        sessionId,
        sessionMeta: metadata ? buildSessionIdentity(sessionId, metadata) : null,
        currentWorkLabel: null,
        recentToolActivity: null,
        pendingApproval: null,
        error: null,
        isStreaming: false,
        statusStage: "ready",
        statusMessage: resumeLabel
          ? `Resumed ${resumeLabel}`
          : "Resumed session history.",
        totalInputTokens: 0,
        totalOutputTokens: 0,
        title: metadata?.title || "Resumed Chat",
      }),
    );

    // Load session history from local agent runtime state
    if (projectPath) {
      try {
        const history =
          runtime === "claude_cli"
            ? await invoke<any[]>("load_session_history", {
                projectPath,
                sessionId,
              })
            : await invoke<any[]>("agent_load_session_history", {
                localSessionId: sessionId,
              });

        const messages: AgentStreamMessage[] = [];
        for (const entry of history) {
          const type = entry.type;
          if (type === "user" || type === "assistant" || type === "result") {
            messages.push(adaptAgentStreamMessageForUi(entry as AgentStreamMessage));
          }
        }

        set((s) =>
          applyTabUpdate(s, activeTabId, {
            messages,
            ...deriveToolContextFromMessages(messages),
          }),
        );
        await get().refreshSessionMeta(activeTabId, sessionId);
      } catch (err) {
        log.error("Failed to load session history", { error: String(err) });
      }
    }
  },

  setToolApproval: async (toolName, decision) => {
    if (getConfiguredChatRuntime() === "claude_cli") {
      log.debug("setToolApproval ignored in Claude CLI mode", {
        toolName,
        decision,
      });
      return;
    }
    const { activeTabId } = get();
    await invoke("agent_set_tool_approval", {
      tabId: activeTabId,
      toolName,
      decision,
    });
  },

  continueAfterApproval: async (toolName, targetLabel) => {
    if (getConfiguredChatRuntime() === "claude_cli") {
      log.debug("continueAfterApproval ignored in Claude CLI mode", {
        toolName,
        targetLabel,
      });
      return;
    }
    const { activeTabId } = get();
    const tab = get().tabs.find((entry) => entry.id === activeTabId);
    if (!tab?.pendingApproval || !tab.pendingApproval.canResume) {
      return;
    }
    set((state) =>
      applyTabUpdate(state, activeTabId, {
        isStreaming: true,
        error: null,
        statusStage: "resuming_after_approval",
        statusMessage: targetLabel
          ? `Resuming ${toolName} on ${targetLabel}...`
          : `Resuming ${toolName}...`,
      }),
    );
    const localSessionId = await invoke<string>("agent_resume_pending_turn", {
      tabId: activeTabId,
    });
    if (localSessionId) {
      get()._setSessionId(activeTabId, localSessionId);
      await get().refreshSessionMeta(activeTabId, localSessionId);
    }
  },

  resetToolApprovals: async () => {
    if (getConfiguredChatRuntime() === "claude_cli") {
      log.debug("resetToolApprovals ignored in Claude CLI mode");
      return;
    }
    const { activeTabId } = get();
    await invoke("agent_reset_tool_approvals", {
      tabId: activeTabId,
    });
  },

  // ─── Tab Actions ───

  createTab: () => {
    log.debug("Creating new tab");
    const id = nextTabId();
    const newTab = makeDefaultTab(id);
    set((s) => ({
      tabs: [...s.tabs, newTab],
      activeTabId: id,
      // Project new tab fields to top-level
      messages: newTab.messages,
      sessionId: newTab.sessionId,
      isStreaming: newTab.isStreaming,
      error: newTab.error,
      statusStage: newTab.statusStage,
      statusMessage: newTab.statusMessage,
      totalInputTokens: newTab.totalInputTokens,
      totalOutputTokens: newTab.totalOutputTokens,
    }));
    return id;
  },

  closeTab: (tabId: string) => {
    const state = get();
    const tab = state.tabs.find((t) => t.id === tabId);
    // Prevent closing a streaming tab
    if (tab?.isStreaming) return;
    // Prevent closing the last tab
    if (state.tabs.length <= 1) return;

    const idx = state.tabs.findIndex((t) => t.id === tabId);
    if (idx === -1) return;

    const newTabs = state.tabs.filter((t) => t.id !== tabId);

    if (tabId === state.activeTabId) {
      // Switch to adjacent tab
      const newIdx = Math.min(idx, newTabs.length - 1);
      const newActive = newTabs[newIdx];
      set({
        tabs: newTabs,
        activeTabId: newActive.id,
        // Project new active tab
        messages: newActive.messages,
        sessionId: newActive.sessionId,
        isStreaming: newActive.isStreaming,
        error: newActive.error,
        statusStage: newActive.statusStage,
        statusMessage: newActive.statusMessage,
        totalInputTokens: newActive.totalInputTokens,
        totalOutputTokens: newActive.totalOutputTokens,
      });
    } else {
      set({ tabs: newTabs });
    }
  },

  setActiveTab: (tabId: string) => {
    const state = get();
    if (tabId === state.activeTabId) return;
    const targetTab = state.tabs.find((t) => t.id === tabId);
    if (!targetTab) return;

    // Project the target tab's fields to top-level
    set({
      activeTabId: tabId,
      messages: targetTab.messages,
      sessionId: targetTab.sessionId,
      isStreaming: targetTab.isStreaming,
      error: targetTab.error,
      statusStage: targetTab.statusStage,
      statusMessage: targetTab.statusMessage,
      totalInputTokens: targetTab.totalInputTokens,
      totalOutputTokens: targetTab.totalOutputTokens,
    });
  },

  saveDraft: (tabId: string, draft: TabDraft) => {
    set((s) => ({
      tabs: s.tabs.map((t) => (t.id === tabId ? { ...t, draft } : t)),
    }));
  },

  // ─── Internal Actions (routed by explicit tabId) ───

  _appendMessage: (tabId: string, msg: AgentStreamMessage) => {
    const adaptedMessage = adaptAgentStreamMessageForUi(msg);
    set((state) => {
      let inputDelta = 0;
      let outputDelta = 0;
      const usage = adaptedMessage.usage || adaptedMessage.message?.usage;
      if (usage) {
        inputDelta = usage.input_tokens || 0;
        outputDelta = usage.output_tokens || 0;
      }

      const tab = state.tabs.find((t) => t.id === tabId);
      if (!tab) return {};

      return applyTabUpdate(state, tabId, {
        messages: [...tab.messages, adaptedMessage],
        totalInputTokens: tab.totalInputTokens + inputDelta,
        totalOutputTokens: tab.totalOutputTokens + outputDelta,
      });
    });
  },

  _appendAssistantTextDelta: (tabId: string, delta: string) => {
    if (!delta) return;
    set((state) => {
      const tab = state.tabs.find((t) => t.id === tabId);
      if (!tab) return {};

      const messages = [...tab.messages];
      const last = messages.length > 0 ? messages[messages.length - 1] : null;
      if (
        last?.type === "assistant" &&
        last.message?.content?.length === 1 &&
        last.message.content[0].type === "text"
      ) {
        const existing = last.message.content[0].text ?? "";
        messages[messages.length - 1] = {
          ...last,
          message: {
            ...last.message,
            content: [
              {
                ...last.message.content[0],
                text: `${existing}${delta}`,
              },
            ],
          },
        };
      } else {
        messages.push({
          type: "assistant",
          message: {
            content: [{ type: "text", text: delta }],
          },
        });
      }

      return applyTabUpdate(state, tabId, { messages });
    });
  },

  _setSessionId: (tabId: string, id: string) => {
    set((state) => {
      const tab = state.tabs.find((t) => t.id === tabId);
      const runtime = getConfiguredChatRuntime();
      return applyTabUpdate(state, tabId, {
        sessionId: id,
        sessionMeta: tab?.sessionMeta
          ? { ...tab.sessionMeta, localSessionId: id }
          : buildSessionIdentity(id, {
              localSessionId: id,
              title:
                tab?.title || (runtime === "claude_cli" ? "Claude Session" : "Agent Session"),
              provider: runtime === "claude_cli" ? "claude-cli" : "local-agent",
              model:
                runtime === "claude_cli"
                  ? state.selectedModel
                  : getConfiguredLocalAgentModel(),
              updatedAt: new Date().toISOString(),
            }),
      });
    });
  },

  _setSessionMeta: (tabId: string, meta: SessionIdentity | null) => {
    set((state) => applyTabUpdate(state, tabId, { sessionMeta: meta }));
  },

  _setStreaming: (tabId: string, streaming: boolean) => {
    set((state) => applyTabUpdate(state, tabId, { isStreaming: streaming }));
  },

  _setError: (tabId: string, error: string | null) => {
    set((state) => applyTabUpdate(state, tabId, { error }));
  },

  _setStatus: (tabId: string, stage: string | null, message: string | null) => {
    set((state) =>
      applyTabUpdate(state, tabId, {
        statusStage: stage,
        statusMessage: message,
      }),
    );
  },

  _setWorkContext: (
    tabId: string,
    currentWorkLabel: string | null,
    recentToolActivity: string | null,
  ) => {
    set((state) =>
      applyTabUpdate(state, tabId, {
        currentWorkLabel,
        recentToolActivity,
      }),
    );
  },

  _setPendingApproval: (tabId, pendingApproval) => {
    set((state) => applyTabUpdate(state, tabId, { pendingApproval }));
  },
}));
