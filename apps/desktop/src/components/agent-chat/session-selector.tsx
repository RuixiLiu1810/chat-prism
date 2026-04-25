import { useState, useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";
import { HistoryIcon, PlusIcon, CheckIcon, Loader2Icon } from "lucide-react";
import { toast } from "sonner";
import {
  DropdownMenu,
  DropdownMenuTrigger,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuLabel,
} from "@/components/ui/dropdown-menu";
import { useAgentChatStore } from "@/stores/agent-chat-store";
import { useDocumentStore } from "@/stores/document-store";
import { useSettingsStore } from "@/stores/settings-store";
import { createLogger } from "@/lib/debug/logger";

const log = createLogger("session-selector");

interface AgentSessionInfo {
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
  workflowType?: string | null;
  workflowStage?: string | null;
  collectedReferenceCount?: number | null;
  reviewFindingCount?: number | null;
  hasRevisionTracker?: boolean;
}

interface ClaudeSessionInfo {
  session_id: string;
  title: string;
  last_modified: number;
}

function formatRelativeTime(isoTime: string): string {
  const timestamp = Date.parse(isoTime);
  if (Number.isNaN(timestamp)) return "unknown";
  const delta = Date.now() - timestamp;

  if (delta < 60_000) return "just now";
  if (delta < 3_600_000) return `${Math.floor(delta / 60_000)}m ago`;
  if (delta < 86_400_000) return `${Math.floor(delta / 3_600_000)}h ago`;
  if (delta < 604_800_000) return `${Math.floor(delta / 86_400_000)}d ago`;

  const date = new Date(timestamp);
  return date.toLocaleDateString(undefined, { month: "short", day: "numeric" });
}

export function SessionSelector() {
  const [sessions, setSessions] = useState<AgentSessionInfo[]>([]);
  const [isLoading, setIsLoading] = useState(false);
  const sessionId = useAgentChatStore((s) => s.sessionId);
  const isStreaming = useAgentChatStore((s) => s.isStreaming);
  const newSession = useAgentChatStore((s) => s.newSession);
  const resumeSession = useAgentChatStore((s) => s.resumeSession);
  const projectRoot = useDocumentStore((s) => s.projectRoot);
  const runtime = useSettingsStore(
    (s) => s.effective.integrations.agent.runtime,
  );

  const loadSessions = useCallback(async () => {
    if (!projectRoot) return;
    setIsLoading(true);
    log.debug(`loading sessions for projectRoot: ${projectRoot}`);
    try {
      if (runtime === "claude_cli") {
        const result = await invoke<ClaudeSessionInfo[]>("list_claude_sessions", {
          projectPath: projectRoot,
        });
        log.debug("loaded Claude sessions", { count: result.length });
        setSessions(
          result.map((session) => ({
            localSessionId: session.session_id,
            title: session.title,
            updatedAt: new Date(session.last_modified).toISOString(),
            createdAt: new Date(session.last_modified).toISOString(),
            provider: "claude-cli",
            model: "Claude",
            preview: null,
            messageCount: 0,
            currentObjective: null,
            currentTarget: null,
            lastToolActivity: null,
            pendingState: null,
            pendingTarget: null,
            workflowType: null,
            workflowStage: null,
            collectedReferenceCount: null,
            reviewFindingCount: null,
            hasRevisionTracker: false,
          })),
        );
      } else {
        const result = await invoke<AgentSessionInfo[]>(
          "list_local_agent_sessions",
          {
            projectPath: projectRoot,
          },
        );
        log.debug("loaded local agent sessions", { count: result.length });
        setSessions(result);
      }
    } catch (err) {
      log.error("Failed to load sessions", { error: String(err) });
      toast.error("Failed to load sessions");
      setSessions([]);
    } finally {
      setIsLoading(false);
    }
  }, [projectRoot, runtime]);

  const handleOpenChange = useCallback(
    (open: boolean) => {
      if (open) {
        loadSessions();
      }
    },
    [loadSessions],
  );

  const handleSelectSession = useCallback(
    (session: AgentSessionInfo) => {
      if (isStreaming) return;
      if (session.localSessionId === sessionId) return;
      log.debug(`selecting session: ${session.localSessionId}`);
      resumeSession(session.localSessionId, {
        localSessionId: session.localSessionId,
        title: session.title,
        provider: session.provider,
        model: session.model,
        updatedAt: session.updatedAt,
        preview: session.preview ?? null,
        messageCount: session.messageCount,
        currentObjective: session.currentObjective ?? null,
        currentTarget: session.currentTarget ?? null,
        lastToolActivity: session.lastToolActivity ?? null,
        pendingState: session.pendingState ?? null,
        pendingTarget: session.pendingTarget ?? null,
        workflowType: session.workflowType ?? null,
        workflowStage: session.workflowStage ?? null,
        collectedReferenceCount: session.collectedReferenceCount ?? null,
        reviewFindingCount: session.reviewFindingCount ?? null,
        hasRevisionTracker: session.hasRevisionTracker ?? false,
      });
    },
    [isStreaming, sessionId, resumeSession],
  );

  const handleNewChat = useCallback(() => {
    if (isStreaming) return;
    newSession();
  }, [isStreaming, newSession]);

  return (
    <DropdownMenu onOpenChange={handleOpenChange}>
      <DropdownMenuTrigger asChild>
        <button
          type="button"
          className="flex size-7 items-center justify-center rounded-md text-muted-foreground transition-colors hover:bg-muted hover:text-foreground"
          aria-label="Session history"
          onMouseDown={(e) => e.stopPropagation()}
          onClick={(e) => e.stopPropagation()}
        >
          <HistoryIcon className="size-4" />
        </button>
      </DropdownMenuTrigger>

      <DropdownMenuContent
        align="end"
        side="bottom"
        className="max-h-80 w-72 overflow-y-auto"
      >
        <DropdownMenuLabel>Sessions</DropdownMenuLabel>

        <DropdownMenuItem onSelect={handleNewChat} disabled={isStreaming}>
          <PlusIcon className="size-4" />
          <span>New Chat</span>
        </DropdownMenuItem>

        <DropdownMenuSeparator />

        {isLoading ? (
          <div className="flex items-center justify-center py-4">
            <Loader2Icon className="size-4 animate-spin text-muted-foreground" />
          </div>
        ) : sessions.length === 0 ? (
          <div className="px-2 py-4 text-center text-muted-foreground text-sm">
            No previous sessions
          </div>
        ) : (
          sessions.map((session) => (
            <DropdownMenuItem
              key={session.localSessionId}
              onSelect={() => handleSelectSession(session)}
              disabled={isStreaming}
              className="flex items-start gap-2 py-2"
            >
              <div className="flex min-w-0 flex-1 flex-col">
                <div className="flex items-center gap-1.5">
                  <span className="truncate text-sm">{session.title}</span>
                  <span className="rounded bg-muted px-1.5 py-0.5 text-[10px] uppercase tracking-wide text-muted-foreground">
                    {session.provider}
                  </span>
                  {session.pendingState && (
                    <span className="rounded bg-amber-500/10 px-1.5 py-0.5 text-[10px] uppercase tracking-wide text-amber-700 dark:text-amber-200">
                      {session.pendingState}
                    </span>
                  )}
                </div>
                {session.preview && (
                  <span className="mt-0.5 line-clamp-2 text-muted-foreground text-xs">
                    {session.preview}
                  </span>
                )}
                {session.currentObjective && (
                  <span className="mt-0.5 line-clamp-1 text-[11px] text-muted-foreground/80">
                    Objective: {session.currentObjective}
                  </span>
                )}
                <div className="mt-1 flex flex-wrap items-center gap-1.5 text-[11px] text-muted-foreground">
                  <span>{formatRelativeTime(session.updatedAt)}</span>
                  <span>·</span>
                  <span>{session.model}</span>
                  <span>·</span>
                  <span>{session.messageCount} items</span>
                  {session.pendingTarget && (
                    <>
                      <span>·</span>
                      <span className="truncate">{session.pendingTarget}</span>
                    </>
                  )}
                </div>
              </div>
              {session.localSessionId === sessionId && (
                <CheckIcon className="size-4 shrink-0 text-primary" />
              )}
            </DropdownMenuItem>
          ))
        )}
      </DropdownMenuContent>
    </DropdownMenu>
  );
}
