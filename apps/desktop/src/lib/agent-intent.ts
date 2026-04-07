import { getToolResultDisplayTarget } from "@/lib/agent-message-adapter";

// ─── Type Definitions ───

export type AgentTaskKind =
  | "general"
  | "selection_edit"
  | "file_edit"
  | "suggestion_only"
  | "analysis";
export type AgentSelectionScope = "none" | "selected_span";
export type AgentResponseMode = "default" | "reviewable_change" | "suggestion_only";
export type AgentSamplingProfile =
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

// ─── Pure Functions ───

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

export function makeTurnProfile(
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

export function resolveTurnProfile(
  _prompt: string,
  options: {
    hasSelectionContext: boolean;
    hasActiveFile: boolean;
    hasAttachmentContext?: boolean;
  },
): AgentTurnProfile {
  const { hasSelectionContext, hasAttachmentContext } = options;

  // Intent detection (keyword matching) is handled exclusively by the backend
  // resolve_turn_profile(). The frontend only provides structural context hints.

  if (hasSelectionContext) {
    return makeTurnProfile(
      "selection_edit",
      "selected_span",
      "default",
      "default",
      "selection",
    );
  }

  if (hasAttachmentContext) {
    return makeTurnProfile(
      "analysis",
      "none",
      "default",
      "default",
      "attachment",
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

export interface SessionIdentity {
  localSessionId: string;
  title: string;
  provider: string | null;
  model: string | null;
  updatedAt: string | null;
  preview: string | null;
  messageCount: number | null;
  currentObjective: string | null;
  currentTarget: string | null;
  lastToolActivity: string | null;
  pendingState: string | null;
  pendingTarget: string | null;
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

export function buildSessionIdentity(
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

export function summarizeToolTarget(input: unknown): string | null {
  if (!input || typeof input !== "object" || Array.isArray(input)) {
    return null;
  }
  const record = input as Record<string, unknown>;
  const candidate = ["path", "file_path", "command", "query"]
    .map((key) => record[key])
    .find((value) => typeof value === "string" && value.trim().length > 0);
  return typeof candidate === "string" ? candidate.trim() : null;
}

export function summarizeToolContentTarget(content: unknown): string | null {
  return getToolResultDisplayTarget(content);
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

export interface ContentBlock {
  type: "text" | "tool_use" | "tool_result" | "thinking";
  text?: string;
  id?: string;
  name?: string;
  input?: any;
  tool_use_id?: string;
  content?: any;
  is_error?: boolean;
  thinking?: string;
  signature?: string;
}

export function deriveToolContextFromMessages(messages: AgentStreamMessage[]): {
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
