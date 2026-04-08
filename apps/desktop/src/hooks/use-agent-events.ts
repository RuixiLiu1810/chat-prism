import { useEffect, useRef } from "react";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { invoke } from "@tauri-apps/api/core";
import {
  useAgentChatStore,
  type AgentStreamMessage,
  normalizeAgentError,
} from "@/stores/agent-chat-store";
import { useDocumentStore } from "@/stores/document-store";
import { useHistoryStore } from "@/stores/history-store";
import { useProposedChangesStore } from "@/stores/proposed-changes-store";
import { readTexFileContent } from "@/lib/tauri/fs";
import {
  compileLatex,
  formatCompileError,
  resolveCompileTarget,
} from "@/lib/latex-compiler";
import { adaptToolResultDisplayContent } from "@/lib/agent-message-adapter";
import { createLogger } from "@/lib/debug/logger";

const log = createLogger("agent-event");

type AgentEventPayload =
  | { type: "status"; stage: string; message: string }
  | { type: "message_delta"; delta: string }
  | { type: "tool_call"; toolName: string; callId: string; input: unknown }
  | {
      type: "tool_result";
      toolName: string;
      callId: string;
      isError: boolean;
      preview: string;
      content: unknown;
      display?: unknown;
    }
  | {
      type: "tool_interrupt";
      phase: "awaiting_approval" | "review_ready" | "resumed" | "cleared";
      toolName?: string | null;
      approvalToolName?: string | null;
      callId?: string | null;
      targetPath?: string | null;
      reviewReady: boolean;
      canResume: boolean;
      message: string;
    }
  | {
      type: "approval_requested";
      toolName: string;
      callId: string;
      targetPath?: string | null;
      reviewReady: boolean;
      message: string;
    }
  | {
      type: "review_artifact_ready";
      toolName: string;
      callId: string;
      targetPath: string;
      summary?: string | null;
      written: boolean;
    }
  | {
      type: "tool_resumed";
      toolName: string;
      targetPath?: string | null;
      message: string;
    }
  | {
      type: "turn_resumed";
      localSessionId?: string | null;
      message: string;
    }
  | {
      type: "workflow_checkpoint_requested";
      workflowType: string;
      stage: string;
      message: string;
    }
  | {
      type: "workflow_checkpoint_approved";
      workflowType: string;
      fromStage: string;
      toStage: string;
      completed: boolean;
      message: string;
    }
  | {
      type: "workflow_checkpoint_rejected";
      workflowType: string;
      stage: string;
      message: string;
    }
  | { type: "error"; code: string; message: string };

interface AgentEventEnvelope {
  tabId: string;
  payload: AgentEventPayload;
}

interface AgentCompletePayload {
  tabId: string;
  outcome: string;
}

interface ClaudeOutputPayload {
  tab_id: string;
  data: string;
}

interface ClaudeCompletePayload {
  tab_id: string;
  success: boolean;
}

interface ClaudeErrorPayload {
  tab_id: string;
  data: string;
}

function summarizeTarget(input: unknown): string | null {
  if (!input || typeof input !== "object" || Array.isArray(input)) {
    return null;
  }
  const record = input as Record<string, unknown>;
  const candidate = ["path", "file_path", "command", "query"]
    .map((key) => record[key])
    .find((value) => typeof value === "string" && value.trim().length > 0);
  return typeof candidate === "string" ? candidate.trim() : null;
}

function summarizeToolActivity(
  toolName: string,
  phase: "running" | "result",
  target: string | null,
  isError = false,
): string {
  const prefix = phase === "running" ? "Running" : isError ? "Result" : "Completed";
  return target ? `${prefix} ${toolName} on ${target}` : `${prefix} ${toolName}`;
}

function isReviewableEditTool(toolName: string): boolean {
  return ["write_file", "replace_selected_text", "apply_text_patch"].includes(
    toolName,
  );
}

function proposedChangeToolLabel(toolName: string): string {
  switch (toolName) {
    case "replace_selected_text":
      return "Replace Selection";
    case "apply_text_patch":
      return "Apply Patch";
    case "write_file":
    default:
      return "Write";
  }
}

function applyToolInterruptState(
  tabId: string,
  payload: Extract<AgentEventPayload, { type: "tool_interrupt" }>,
) {
  const chatStore = useAgentChatStore.getState();
  const toolName = payload.toolName ?? payload.approvalToolName ?? "tool";
  const target = payload.targetPath ?? null;

  if (payload.phase === "cleared" || payload.phase === "resumed") {
    chatStore._setPendingApproval(tabId, null);
    if (payload.phase === "resumed") {
      chatStore._setWorkContext(
        tabId,
        target,
        target ? `Resumed ${toolName} on ${target}` : `Resumed ${toolName}`,
      );
      chatStore._setStatus(tabId, "tool_resumed", payload.message);
    }
    return;
  }

  chatStore._setWorkContext(
    tabId,
    target,
    target
      ? `Awaiting ${payload.phase === "review_ready" ? "review" : "approval"} for ${toolName} on ${target}`
      : `Awaiting ${payload.phase === "review_ready" ? "review" : "approval"} for ${toolName}`,
  );
  chatStore._setStatus(tabId, payload.phase, payload.message);
  chatStore._setPendingApproval(tabId, {
    phase: payload.phase,
    toolName,
    approvalToolName: payload.approvalToolName ?? null,
    callId: payload.callId ?? `${toolName}-interrupt`,
    targetPath: target,
    reviewReady: payload.reviewReady,
    canResume: payload.canResume,
    message: payload.message,
  });
}

function makeToolCallMessage(
  toolName: string,
  callId: string,
  input: unknown,
): AgentStreamMessage {
  return {
    type: "assistant",
    message: {
      content: [
        {
          type: "tool_use",
          id: callId,
          name: toolName,
          input:
            input && typeof input === "object" && !Array.isArray(input)
              ? input
              : {},
        },
      ],
    },
  };
}

function makeToolResultMessage(
  callId: string,
  preview: string,
  isError: boolean,
  content: unknown,
  display?: unknown,
): AgentStreamMessage {
  return {
    type: "user",
    message: {
      content: [
        {
          type: "tool_result",
          tool_use_id: callId,
          content: adaptToolResultDisplayContent(
            display ?? (content !== undefined ? content : preview),
            { preview, isError },
          ),
          is_error: isError,
        },
      ],
    },
  };
}

function maybeRecordProposedChange(
  payload: Extract<AgentEventPayload, { type: "tool_result" }>,
) {
  if (!isReviewableEditTool(payload.toolName)) {
    return;
  }

  if (!payload.content || typeof payload.content !== "object") {
    return;
  }

  const content = payload.content as Record<string, unknown>;
  const artifact =
    content.reviewArtifactPayload &&
    typeof content.reviewArtifactPayload === "object" &&
    !Array.isArray(content.reviewArtifactPayload)
      ? (content.reviewArtifactPayload as Record<string, unknown>)
      : content;
  const filePath =
    typeof artifact.targetPath === "string"
      ? artifact.targetPath.trim()
      : typeof content.path === "string"
        ? content.path.trim()
        : "";
  const absolutePath =
    typeof artifact.absolutePath === "string"
      ? artifact.absolutePath.trim()
      : typeof content.absolutePath === "string"
        ? content.absolutePath.trim()
        : "";
  const oldContent =
    typeof artifact.oldContent === "string"
      ? artifact.oldContent
      : typeof content.oldContent === "string"
        ? content.oldContent
        : "";
  const newContent =
    typeof artifact.newContent === "string"
      ? artifact.newContent
      : typeof content.newContent === "string"
        ? content.newContent
        : "";

  if (!filePath || !absolutePath) {
    return;
  }

  if (oldContent === newContent) {
    return;
  }

  useProposedChangesStore.getState().addChange({
    id: payload.callId,
    filePath,
    absolutePath,
    oldContent,
    newContent,
    toolName: proposedChangeToolLabel(payload.toolName),
  });
}

export function useAgentEvents() {
  const listenersRef = useRef<UnlistenFn[]>([]);

  useEffect(() => {
    async function handleComplete(payload: AgentCompletePayload) {
      const { tabId, outcome } = payload;
      const chatStore = useAgentChatStore.getState();
      const tab = chatStore.tabs.find((entry) => entry.id === tabId);
      if (!tab?.isStreaming) {
        return;
      }

      if (
        outcome === "error" &&
        !tab.error &&
        !chatStore._cancelledByUser
      ) {
        chatStore._setError(
          tabId,
          normalizeAgentError(
            "Agent runtime exited unexpectedly. Please review the logs or retry.",
          ),
        );
        chatStore._setStatus(
          tabId,
          "failed",
          normalizeAgentError(
            "Agent runtime exited unexpectedly. Please review the logs or retry.",
          ),
        );
      }

      if (outcome === "cancelled") {
        chatStore._setPendingApproval(tabId, null);
        chatStore._setPendingWorkflowCheckpoint(tabId, null);
        chatStore._setStatus(tabId, "cancelled", "Agent run cancelled.");
      } else if (outcome === "completed") {
        chatStore._setPendingApproval(tabId, null);
        chatStore._setPendingWorkflowCheckpoint(tabId, null);
        chatStore._setStatus(tabId, null, null);
      } else if (outcome === "suspended") {
        chatStore._setStatus(
          tabId,
          tab.pendingApproval?.phase ??
            (tab.pendingApproval?.reviewReady ? "review_ready" : "awaiting_approval"),
          tab.pendingApproval?.message ?? "Agent run suspended for approval.",
        );
      }

      chatStore._setStreaming(tabId, false);

      if (outcome === "suspended") {
        return;
      }

      const projectPath = useDocumentStore.getState().projectRoot;
      if (projectPath) {
        try {
          await useHistoryStore
            .getState()
            .createSnapshot(projectPath, "[agent] After agent edit");
        } catch {
          // Snapshot failure should not break the flow.
        }
      }

      const docStore = useDocumentStore.getState();
      await docStore.refreshFiles();

      const {
        projectRoot,
        files,
        activeFileId,
        isCompiling: alreadyCompiling,
      } = useDocumentStore.getState();
      if (projectRoot && !alreadyCompiling) {
        const resolved = resolveCompileTarget(activeFileId, files);
        if (resolved) {
          const { rootId, targetPath } = resolved;
          useDocumentStore.getState().setIsCompiling(true);
          useDocumentStore.getState().setPendingRecompile(false);
          try {
            await useDocumentStore.getState().saveAllFiles();
            const pdfData = await compileLatex(projectRoot, targetPath);
            useDocumentStore.getState().setPdfData(pdfData, rootId);
          } catch (err) {
            useDocumentStore
              .getState()
              .setCompileError(formatCompileError(err), rootId);
          } finally {
            useDocumentStore.getState().setIsCompiling(false);
          }
        }
      } else if (alreadyCompiling) {
        useDocumentStore.getState().setPendingRecompile(true);
        log.info("queued post-agent recompile — already compiling");
      }
    }

    function handleAgentEvent({ tabId, payload }: AgentEventEnvelope) {
      const chatStore = useAgentChatStore.getState();
      const tab = chatStore.tabs.find((entry) => entry.id === tabId);
      if (!tab?.isStreaming) {
        return;
      }

      switch (payload.type) {
        case "status":
          log.debug(`[${tabId}] status=${payload.stage} ${payload.message}`);
          chatStore._setStatus(tabId, payload.stage, payload.message);
          break;
        case "message_delta":
          chatStore._appendAssistantTextDelta(tabId, payload.delta);
          break;
        case "tool_call":
          log.debug(`[${tabId}] tool_call=${payload.toolName}`);
          chatStore._setWorkContext(
            tabId,
            summarizeTarget(payload.input),
            summarizeToolActivity(
              payload.toolName,
              "running",
              summarizeTarget(payload.input),
            ),
          );
          chatStore._appendMessage(
            tabId,
            makeToolCallMessage(payload.toolName, payload.callId, payload.input),
          );
          break;
        case "tool_result":
          log.debug(
            `[${tabId}] tool_result=${payload.toolName} err=${payload.isError}`,
          );
          chatStore._setWorkContext(
            tabId,
            summarizeTarget(payload.content),
            summarizeToolActivity(
              payload.toolName,
              "result",
              summarizeTarget(payload.content),
              payload.isError,
            ),
          );
          maybeRecordProposedChange(payload);
          if (!payload.isError) {
            chatStore._setPendingApproval(tabId, null);
          }
          chatStore._appendMessage(
            tabId,
            makeToolResultMessage(
              payload.callId,
              payload.preview,
              payload.isError,
              payload.content,
              payload.display,
            ),
          );
          break;
        case "tool_interrupt":
          log.debug(
            `[${tabId}] tool_interrupt phase=${payload.phase} tool=${payload.toolName ?? payload.approvalToolName ?? "(none)"}`,
          );
          applyToolInterruptState(tabId, payload);
          break;
        case "approval_requested":
          log.debug(
            `[${tabId}] approval_requested=${payload.toolName} reviewReady=${payload.reviewReady}`,
          );
          chatStore._setWorkContext(
            tabId,
            payload.targetPath ?? null,
            payload.targetPath
              ? `Awaiting approval for ${payload.toolName} on ${payload.targetPath}`
              : `Awaiting approval for ${payload.toolName}`,
          );
          chatStore._setStatus(
            tabId,
            payload.reviewReady ? "review_ready" : "awaiting_approval",
            payload.message,
          );
          chatStore._setPendingApproval(tabId, {
            phase: payload.reviewReady ? "review_ready" : "awaiting_approval",
            toolName: payload.toolName,
            approvalToolName: payload.toolName,
            callId: payload.callId,
            targetPath: payload.targetPath ?? null,
            reviewReady: payload.reviewReady,
            canResume: true,
            message: payload.message,
          });
          break;
        case "review_artifact_ready":
          log.debug(
            `[${tabId}] review_artifact_ready=${payload.toolName} target=${payload.targetPath}`,
          );
          chatStore._setWorkContext(
            tabId,
            payload.targetPath,
            payload.written
              ? `Review artifact captured for ${payload.targetPath}`
              : `Review ready for ${payload.targetPath}`,
          );
          chatStore._setPendingApproval(tabId, {
            phase: "review_ready",
            toolName: payload.toolName,
            approvalToolName: payload.toolName,
            callId: payload.callId,
            targetPath: payload.targetPath,
            reviewReady: true,
            canResume: true,
            message: payload.summary || `Review ready for ${payload.targetPath}.`,
          });
          break;
        case "tool_resumed":
          log.debug(
            `[${tabId}] tool_resumed=${payload.toolName} target=${payload.targetPath ?? "(none)"}`,
          );
          applyToolInterruptState(tabId, {
            type: "tool_interrupt",
            phase: "resumed",
            toolName: payload.toolName,
            approvalToolName: payload.toolName,
            callId: null,
            targetPath: payload.targetPath ?? null,
            reviewReady: false,
            canResume: false,
            message: payload.message,
          });
          break;
        case "turn_resumed":
          log.debug(`[${tabId}] turn_resumed`);
          applyToolInterruptState(tabId, {
            type: "tool_interrupt",
            phase: "cleared",
            toolName: null,
            approvalToolName: null,
            callId: null,
            targetPath: null,
            reviewReady: false,
            canResume: false,
            message: payload.message,
          });
          chatStore._setStatus(tabId, "turn_resumed", payload.message);
          break;
        case "workflow_checkpoint_requested":
          chatStore._setStatus(tabId, "workflow_checkpoint_requested", payload.message);
          chatStore._setPendingWorkflowCheckpoint(tabId, {
            workflowType: payload.workflowType,
            stage: payload.stage,
            message: payload.message,
          });
          chatStore._setWorkflowState(tabId, {
            workflowType: payload.workflowType,
            stage: payload.stage,
            completed: false,
          });
          break;
        case "workflow_checkpoint_approved":
          chatStore._setStatus(tabId, "workflow_checkpoint_approved", payload.message);
          chatStore._setPendingWorkflowCheckpoint(tabId, null);
          chatStore._setWorkflowState(tabId, {
            workflowType: payload.workflowType,
            stage: payload.toStage,
            completed: payload.completed,
          });
          break;
        case "workflow_checkpoint_rejected":
          chatStore._setStatus(tabId, "workflow_checkpoint_rejected", payload.message);
          chatStore._setPendingWorkflowCheckpoint(tabId, null);
          chatStore._setWorkflowState(tabId, {
            workflowType: payload.workflowType,
            stage: payload.stage,
            completed: false,
          });
          break;
        case "error":
          log.warn(`[${tabId}] agent error ${payload.code}: ${payload.message}`);
          chatStore._setPendingApproval(tabId, null);
          chatStore._setPendingWorkflowCheckpoint(tabId, null);
          chatStore._setError(tabId, payload.message);
          chatStore._setStatus(tabId, "failed", payload.message);
          break;
      }
    }

    let cancelled = false;
    (async () => {
      const unlistenEvent = await listen<AgentEventEnvelope>(
        "agent-event",
        (event) => {
          if (!cancelled) handleAgentEvent(event.payload);
        },
      );
      if (cancelled) {
        unlistenEvent();
        return;
      }
      listenersRef.current.push(unlistenEvent);

      const unlistenComplete = await listen<AgentCompletePayload>(
        "agent-complete",
        (event) => {
          if (!cancelled) {
            void handleComplete(event.payload);
          }
        },
      );
      if (cancelled) {
        unlistenComplete();
        return;
      }
      listenersRef.current.push(unlistenComplete);
    })();

    return () => {
      cancelled = true;
      for (const unlisten of listenersRef.current) {
        unlisten();
      }
      listenersRef.current = [];
    };
  }, []);

  useEffect(() => {
    const pendingToolUsesRef = { current: new Map<string, Map<string, { name: string; input: any }>>() };
    const hasTexChangesRef = { current: new Map<string, boolean>() };
    const cancelledForAskRef = { current: new Map<string, boolean>() };
    const msgCountRef = { current: new Map<string, number>() };
    const streamStartTimeRef = { current: new Map<string, number>() };
    const lastMsgTimeRef = { current: new Map<string, number>() };

    async function registerProposedChange(
      filePath: string,
      toolUseId: string,
      toolName: string,
    ) {
      const docState = useDocumentStore.getState();
      const projectRoot = docState.projectRoot;
      let relativePath = filePath;
      if (projectRoot && filePath.startsWith(projectRoot)) {
        relativePath = filePath.slice(projectRoot.length).replace(/^\//, "");
      }
      const file = docState.files.find(
        (f) => f.relativePath === relativePath || f.absolutePath === filePath,
      );
      if (!file) return;

      const oldContent = file.content ?? "";
      try {
        const newContent = await readTexFileContent(file.absolutePath);
        if (oldContent !== newContent) {
          useProposedChangesStore.getState().addChange({
            id: toolUseId,
            filePath: file.relativePath,
            absolutePath: file.absolutePath,
            oldContent,
            newContent,
            toolName,
          });
        }
      } catch {
        // best-effort only
      }
    }

    function elapsed(tabId: string) {
      const start = streamStartTimeRef.current.get(tabId);
      if (!start) return "";
      return `+${((performance.now() - start) / 1000).toFixed(1)}s`;
    }

    function handleClaudeStreamMessage(payload: ClaudeOutputPayload) {
      const { tab_id: tabId, data } = payload;

      let msg: AgentStreamMessage;
      try {
        msg = JSON.parse(data);
      } catch {
        return;
      }

      const chatStore = useAgentChatStore.getState();
      const tab = chatStore.tabs.find((entry) => entry.id === tabId);
      if (!tab?.isStreaming) return;

      const count = (msgCountRef.current.get(tabId) ?? 0) + 1;
      msgCountRef.current.set(tabId, count);
      const now = performance.now();
      if (count === 1) streamStartTimeRef.current.set(tabId, now);
      const lastTime = lastMsgTimeRef.current.get(tabId);
      const gap = lastTime ? ((now - lastTime) / 1000).toFixed(1) : "0";
      lastMsgTimeRef.current.set(tabId, now);

      const contentTypes =
        msg.message?.content?.map((block: any) => block.type).join(",") ?? "";
      const gapWarning = Number(gap) > 10 ? ` GAP ${gap}s` : "";
      log.debug(
        `[${tabId}] ${elapsed(tabId)} Claude type=${msg.type} sub=${msg.subtype ?? ""} content=[${contentTypes}] gap=${gap}s${gapWarning}`,
      );

      if (msg.type === "system" && msg.subtype === "init" && msg.session_id) {
        chatStore._setSessionId(tabId, msg.session_id);
        void chatStore.refreshSessionMeta(tabId, msg.session_id);
      }

      if ((msg as any).type === "rate_limit_event") {
        const info = (msg as any).rate_limit_info;
        if (info && info.status !== "allowed") {
          const resetsAt = info.resetsAt
            ? new Date(info.resetsAt * 1000).toLocaleTimeString()
            : "unknown";
          chatStore._setError(
            tabId,
            `Rate limited (${info.rateLimitType}). Resets at ${resetsAt}`,
          );
        }
        return;
      }

      const tabToolUses = pendingToolUsesRef.current.get(tabId) ?? new Map();
      if (msg.type === "assistant" && msg.message?.content) {
        for (const block of msg.message.content) {
          if (block.type === "tool_use" && block.id && block.name) {
            tabToolUses.set(block.id, { name: block.name, input: block.input });
          }
        }
        pendingToolUsesRef.current.set(tabId, tabToolUses);
      }

      if (msg.type === "user" && msg.message?.content) {
        for (const block of msg.message.content) {
          if (block.type === "tool_result" && block.tool_use_id) {
            const toolUse = tabToolUses.get(block.tool_use_id);
            if (
              toolUse &&
              !block.is_error &&
              /^(Write|write|Edit|edit|MultiEdit|multiedit)$/.test(toolUse.name)
            ) {
              const fp = toolUse.input?.file_path || toolUse.input?.path;
              if (fp) {
                void registerProposedChange(fp, block.tool_use_id, toolUse.name);
                if (/\.(tex|bib|sty|cls|dtx)$/i.test(fp)) {
                  hasTexChangesRef.current.set(tabId, true);
                }
              }
            }
          }
        }
      }

      if (
        msg.type === "user" &&
        msg.message?.content?.length === 1 &&
        msg.message.content[0].type === "text"
      ) {
        return;
      }

      chatStore._appendMessage(tabId, msg);

      if (msg.type === "assistant" && msg.message?.content) {
        const hasAskUser = msg.message.content.some(
          (block: any) => block.type === "tool_use" && block.name === "AskUserQuestion",
        );
        if (hasAskUser) {
          cancelledForAskRef.current.set(tabId, true);
          void invoke("cancel_claude_execution", { tabId }).catch(() => {});
        }
      }
    }

    async function handleClaudeComplete(payload: ClaudeCompletePayload) {
      const { tab_id: tabId, success } = payload;
      const count = msgCountRef.current.get(tabId) ?? 0;
      const chatStore = useAgentChatStore.getState();
      const tab = chatStore.tabs.find((entry) => entry.id === tabId);
      if (!tab?.isStreaming) {
        return;
      }

      if (
        !success &&
        count > 0 &&
        !tab.error &&
        !cancelledForAskRef.current.get(tabId) &&
        !chatStore._cancelledByUser
      ) {
        chatStore._setError(
          tabId,
          "Claude process exited unexpectedly. This may be due to rate limiting or an API error.",
        );
      }

      pendingToolUsesRef.current.delete(tabId);
      hasTexChangesRef.current.delete(tabId);
      cancelledForAskRef.current.delete(tabId);

      chatStore._setPendingApproval(tabId, null);
      chatStore._setStatus(tabId, success ? null : "failed", success ? null : tab.error);
      chatStore._setStreaming(tabId, false);

      const projectPath = useDocumentStore.getState().projectRoot;
      if (projectPath) {
        try {
          await useHistoryStore
            .getState()
            .createSnapshot(projectPath, "[claude] After Claude edit");
        } catch {
          // snapshot failure should not break the flow
        }
      }

      const docStore = useDocumentStore.getState();
      await docStore.refreshFiles();

      const {
        projectRoot,
        files,
        activeFileId,
        isCompiling: alreadyCompiling,
      } = useDocumentStore.getState();
      if (projectRoot && !alreadyCompiling) {
        const resolved = resolveCompileTarget(activeFileId, files);
        if (resolved) {
          const { rootId, targetPath } = resolved;
          useDocumentStore.getState().setIsCompiling(true);
          useDocumentStore.getState().setPendingRecompile(false);
          try {
            await useDocumentStore.getState().saveAllFiles();
            const pdfData = await compileLatex(projectRoot, targetPath);
            useDocumentStore.getState().setPdfData(pdfData, rootId);
          } catch (err) {
            useDocumentStore
              .getState()
              .setCompileError(formatCompileError(err), rootId);
          } finally {
            useDocumentStore.getState().setIsCompiling(false);
          }
        }
      } else if (alreadyCompiling) {
        useDocumentStore.getState().setPendingRecompile(true);
        log.info("queued post-Claude recompile — already compiling");
      }
    }

    let cancelled = false;
    (async () => {
      const unlistenOutput = await listen<ClaudeOutputPayload>(
        "claude-output",
        (event) => {
          if (!cancelled) handleClaudeStreamMessage(event.payload);
        },
      );
      if (cancelled) {
        unlistenOutput();
        return;
      }
      listenersRef.current.push(unlistenOutput);

      const unlistenComplete = await listen<ClaudeCompletePayload>(
        "claude-complete",
        (event) => {
          if (!cancelled) {
            void handleClaudeComplete(event.payload);
          }
        },
      );
      if (cancelled) {
        unlistenComplete();
        return;
      }
      listenersRef.current.push(unlistenComplete);

      const unlistenError = await listen<ClaudeErrorPayload>(
        "claude-error",
        (event) => {
          if (!cancelled) {
            const { tab_id: tabId, data } = event.payload;
            log.warn(`[${tabId}] Claude stderr: ${data}`);
            if (
              data.includes("Error") ||
              data.includes("error") ||
              data.includes("ECONNREFUSED") ||
              data.includes("timeout")
            ) {
              useAgentChatStore.getState()._setError(
                tabId,
                normalizeAgentError(data),
              );
            }
          }
        },
      );
      if (cancelled) {
        unlistenError();
        return;
      }
      listenersRef.current.push(unlistenError);
    })();

    return () => {
      cancelled = true;
      for (const unlisten of listenersRef.current) {
        unlisten();
      }
      listenersRef.current = [];
    };
  }, []);
}
