import { useRef, useState, useCallback, useEffect } from "react";
import {
  AlertCircleIcon,
  CheckCircle2Icon,
  ChevronDownIcon,
  HistoryIcon,
  LoaderCircleIcon,
  Maximize2Icon,
  MessageCircleIcon,
  Minimize2Icon,
  PaperclipIcon,
  SquareIcon,
} from "lucide-react";

import { cn } from "@/lib/utils";
import { useAgentChatStore } from "@/stores/agent-chat-store";
import { useAgentEvents } from "@/hooks/use-agent-events";
import { ApprovalCard } from "./approval-card";
import { ChatMessages } from "./chat-messages";
import { ChatComposer } from "./chat-composer";
import { ChatTabBar } from "./chat-tab-bar";
import { WorkflowCheckpointCard } from "./workflow-checkpoint-card";
import { LiteratureReviewPanel } from "@/components/agent/LiteratureReviewPanel";
import { PeerReviewPanel } from "@/components/agent/PeerReviewPanel";
import { PaperDraftingPanel } from "@/components/agent/PaperDraftingPanel";

const MIN_HEIGHT = 150;
const DEFAULT_HEIGHT = 360;

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

export function AgentChatDrawer() {
  // Initialize event listeners for the active agent runtime stream.
  useAgentEvents();

  const anyStreaming = useAgentChatStore((s) =>
    s.tabs.some((t) => t.isStreaming),
  );
  const error = useAgentChatStore((s) => s.error);
  const activeSessionMeta = useAgentChatStore((s) => {
    const active = s.tabs.find((t) => t.id === s.activeTabId);
    return active?.sessionMeta ?? null;
  });
  const activeTabTitle = useAgentChatStore((s) => {
    const active = s.tabs.find((t) => t.id === s.activeTabId);
    return active?.title ?? "New Chat";
  });
  const statusStage = useAgentChatStore((s) => {
    const active = s.tabs.find((t) => t.id === s.activeTabId);
    return active?.statusStage ?? null;
  });
  const statusMessage = useAgentChatStore((s) => {
    const active = s.tabs.find((t) => t.id === s.activeTabId);
    return active?.statusMessage ?? null;
  });
  const currentWorkLabel = useAgentChatStore((s) => {
    const active = s.tabs.find((t) => t.id === s.activeTabId);
    return active?.currentWorkLabel ?? null;
  });
  const recentToolActivity = useAgentChatStore((s) => {
    const active = s.tabs.find((t) => t.id === s.activeTabId);
    return active?.recentToolActivity ?? null;
  });
  const pendingApproval = useAgentChatStore((s) => {
    const active = s.tabs.find((t) => t.id === s.activeTabId);
    return active?.pendingApproval ?? null;
  });
  const pendingWorkflowCheckpoint = useAgentChatStore((s) => {
    const active = s.tabs.find((t) => t.id === s.activeTabId);
    return active?.pendingWorkflowCheckpoint ?? null;
  });
  const resolvedWorkLabel = currentWorkLabel || activeSessionMeta?.currentTarget || activeSessionMeta?.pendingTarget || null;
  const resolvedToolActivity =
    recentToolActivity || activeSessionMeta?.lastToolActivity || null;
  const sessionPendingState = activeSessionMeta?.pendingState ?? null;

  const [isOpen, setIsOpen] = useState(false);
  const [isExpanded, setIsExpanded] = useState(false);
  const [height, setHeight] = useState(DEFAULT_HEIGHT);
  const [isDragging, setIsDragging] = useState(false);
  const [chatDropHint, setChatDropHint] = useState<{
    active: boolean;
    fileName?: string;
  }>({ active: false });
  const containerRef = useRef<HTMLDivElement>(null);
  const panelRef = useRef<HTMLDivElement>(null);
  const hasDraggedRef = useRef(false);
  const heightRef = useRef(height);
  heightRef.current = height;

  const pendingAttachments = useAgentChatStore((s) => s.pendingAttachments);

  // Auto-open when streaming starts or a new attachment is added
  useEffect(() => {
    const shouldOpen = anyStreaming || pendingAttachments.length > 0;
    if (shouldOpen && !isOpen) {
      setIsOpen(true);
      const parent = containerRef.current?.parentElement;
      const maxHeight = parent ? parent.clientHeight * 0.5 : 400;
      setHeight(maxHeight);
      heightRef.current = maxHeight;
      if (panelRef.current) {
        panelRef.current.style.height = `${maxHeight}px`;
      }
    }
  }, [anyStreaming, isOpen, pendingAttachments]);

  // Show a ChatGPT-style overlay when dragging sidebar files over the chat panel.
  useEffect(() => {
    const handler = (event: Event) => {
      const detail = (
        event as CustomEvent<{ active?: boolean; fileName?: string }>
      ).detail;
      setChatDropHint({
        active: !!detail?.active,
        fileName: detail?.fileName,
      });
    };
    window.addEventListener(
      "claudeprism:chat-drop-hover",
      handler as EventListener,
    );
    return () => {
      window.removeEventListener(
        "claudeprism:chat-drop-hover",
        handler as EventListener,
      );
    };
  }, []);

  const handleMouseDown = useCallback(
    (e: React.MouseEvent) => {
      if (isExpanded) return;

      e.preventDefault();
      setIsDragging(true);
      hasDraggedRef.current = false;

      const startY = e.clientY;
      const startHeight = heightRef.current;

      const handleMouseMove = (e: MouseEvent) => {
        hasDraggedRef.current = true;
        const parent = containerRef.current?.parentElement;
        const maxHeight = parent ? parent.clientHeight * 0.5 : 400;
        const delta = startY - e.clientY;
        const newHeight = Math.min(
          Math.max(startHeight + delta, MIN_HEIGHT),
          maxHeight,
        );
        heightRef.current = newHeight;
        if (panelRef.current) {
          panelRef.current.style.height = `${newHeight}px`;
        }
      };

      const handleMouseUp = () => {
        setIsDragging(false);
        setHeight(heightRef.current);
        document.removeEventListener("mousemove", handleMouseMove);
        document.removeEventListener("mouseup", handleMouseUp);
      };

      document.addEventListener("mousemove", handleMouseMove);
      document.addEventListener("mouseup", handleMouseUp);
    },
    [isExpanded],
  );

  // Compute expanded dimensions from parent
  const getExpandedDimensions = useCallback(() => {
    const parent = containerRef.current?.parentElement;
    return {
      height: parent?.clientHeight ?? 600,
      width: parent?.clientWidth ?? 800,
    };
  }, []);

  const panelStyle = (): React.CSSProperties => {
    if (!isOpen && !isExpanded) {
      return { height: 0, maxWidth: 672, borderRadius: 24 };
    }
    if (isExpanded) {
      const dims = getExpandedDimensions();
      return { height: dims.height, maxWidth: dims.width, borderRadius: 0 };
    }
    return { height, maxWidth: 672, borderRadius: 24 };
  };

  const statusTone = statusStage === "failed"
    ? "error"
    : statusStage === "cancelled"
      ? "muted"
      : statusStage === "awaiting_approval"
        ? "pending"
        : statusStage === "review_ready"
          ? "ready"
          : statusStage
            ? "active"
            : "idle";

  const StatusIcon =
    statusTone === "error"
      ? AlertCircleIcon
      : statusTone === "muted"
        ? SquareIcon
        : statusTone === "pending"
          ? AlertCircleIcon
          : statusTone === "ready"
            ? CheckCircle2Icon
            : statusTone === "active"
          ? LoaderCircleIcon
          : CheckCircle2Icon;

  const statusClassName =
    statusTone === "error"
      ? "border-destructive/50 bg-destructive/10 text-destructive"
      : statusTone === "muted"
        ? "border-border bg-muted/40 text-muted-foreground"
        : statusTone === "pending"
          ? "border-amber-500/40 bg-amber-500/10 text-amber-700 dark:text-amber-200"
          : statusTone === "ready"
            ? "border-blue-500/35 bg-blue-500/10 text-blue-700 dark:text-blue-200"
        : statusTone === "active"
          ? "border-primary/30 bg-primary/5 text-foreground"
          : "border-border bg-muted/30 text-muted-foreground";
  const showCompactSessionInfo = Boolean(activeSessionMeta);
  const showStatusBanner = Boolean(
    statusStage &&
      statusMessage &&
      (statusTone === "error" ||
        statusTone === "muted" ||
        statusStage === "resuming_after_approval"),
  );

  return (
    <div
      ref={containerRef}
      className={cn(
        "pointer-events-none absolute inset-0 z-10 flex items-end justify-center transition-[padding] duration-300 ease-out",
        isExpanded ? "p-0" : "px-4 pt-4 pb-6",
      )}
    >
      {/* Floating toggle button */}
      <button
        type="button"
        onClick={() => setIsOpen(true)}
        className={cn(
          "pointer-events-auto absolute right-4 bottom-6 flex size-12 items-center justify-center rounded-full border border-border bg-background shadow-lg transition-all duration-300 ease-out hover:scale-105 hover:shadow-xl",
          isOpen
            ? "pointer-events-none scale-50 opacity-0"
            : "scale-100 opacity-100",
        )}
        aria-label="Open AI Assistant"
      >
        <MessageCircleIcon className="size-5 text-foreground" />
      </button>

      {/* Chat panel */}
      <div
        ref={panelRef}
        data-chat-dropzone="true"
        className={cn(
          "pointer-events-auto relative flex w-full flex-col overflow-hidden border bg-background transition-[height,max-width,border-radius,border-color,box-shadow,opacity,transform] duration-300 ease-out",
          isExpanded
            ? "border-transparent shadow-none"
            : "border-border shadow-2xl",
          isOpen
            ? "scale-100 opacity-100"
            : "pointer-events-none origin-bottom scale-95 opacity-0",
          isDragging && "!transition-none",
        )}
        style={panelStyle()}
      >
        {isOpen && (
          <div
            className={cn(
              "pointer-events-none absolute inset-0 z-30 flex items-center justify-center bg-white/45 backdrop-blur-sm transition-opacity duration-200 ease-out",
              chatDropHint.active ? "opacity-100" : "opacity-0",
            )}
          >
            <div
              className={cn(
                "mx-6 flex flex-col items-center gap-2 text-center text-foreground transition-all duration-200 ease-out",
                chatDropHint.active
                  ? "translate-y-0 opacity-100"
                  : "translate-y-1 opacity-0",
              )}
            >
              <PaperclipIcon className="size-6 text-foreground/90" />
              <div className="font-medium text-sm">Drop File To Attach</div>
              <div className="max-w-md truncate text-foreground/70 text-xs">
                {chatDropHint.fileName || "Add file reference to this chat"}
              </div>
            </div>
          </div>
        )}

        {/* Header with drag handle, tab bar, and session selector */}
        {isExpanded ? (
          <>
            <div className="flex items-center justify-start border-border border-b px-2 py-1">
              <button
                type="button"
                onClick={() => setIsExpanded(false)}
                className="flex size-7 items-center justify-center rounded-md text-muted-foreground transition-colors hover:bg-muted hover:text-foreground"
                aria-label="Exit fullscreen"
              >
                <Minimize2Icon className="size-4" />
              </button>
            </div>
            <ChatTabBar />
          </>
        ) : (
          <>
            <div className="relative">
              <div
                className="group flex cursor-row-resize items-center justify-center gap-2 py-2 transition-colors hover:bg-muted/50"
                onMouseDown={handleMouseDown}
                onClick={() => {
                  if (!hasDraggedRef.current) {
                    setIsOpen(false);
                  }
                }}
              >
                <div className="h-1 w-10 rounded-full bg-muted-foreground/30 transition-all group-hover:w-8" />
                <ChevronDownIcon className="size-4 text-muted-foreground opacity-0 transition-opacity group-hover:opacity-100" />
              </div>
              <div className="absolute top-1/2 left-2 flex -translate-y-1/2 items-center gap-1">
                <button
                  type="button"
                  onClick={() => setIsExpanded(true)}
                  className="flex size-7 items-center justify-center rounded-md text-muted-foreground transition-colors hover:bg-muted hover:text-foreground"
                  aria-label="Fullscreen"
                >
                  <Maximize2Icon className="size-4" />
                </button>
              </div>
            </div>
            <ChatTabBar />
          </>
        )}

        {showCompactSessionInfo ? (
          <div className="mx-3 mb-1 flex min-w-0 flex-wrap items-center gap-1.5 border-border/60 border-b pb-2 text-[11px] text-muted-foreground">
            <span className="inline-flex items-center gap-1 rounded-full bg-muted px-2 py-0.5 font-medium text-foreground">
              <HistoryIcon className="size-3" />
              <span className="uppercase tracking-wide">
                {activeSessionMeta?.provider || "session"}
              </span>
            </span>
            <span className="max-w-[16rem] truncate font-medium text-foreground">
              {activeSessionMeta?.title || activeTabTitle}
            </span>
            {activeSessionMeta?.model ? (
              <span className="truncate rounded-full border border-border/70 px-2 py-0.5">
                {activeSessionMeta.model}
              </span>
            ) : null}
            {sessionPendingState ? (
              <span className="rounded-full bg-amber-500/10 px-2 py-0.5 uppercase tracking-wide text-amber-700 dark:text-amber-200">
                {sessionPendingState}
              </span>
            ) : null}
            {resolvedWorkLabel ? (
              <span className="max-w-[18rem] truncate">Working on {resolvedWorkLabel}</span>
            ) : null}
            {!resolvedWorkLabel && resolvedToolActivity ? (
              <span className="max-w-[18rem] truncate">{resolvedToolActivity}</span>
            ) : null}
            {activeSessionMeta?.updatedAt ? (
              <span>Updated {formatRelativeTime(activeSessionMeta.updatedAt)}</span>
            ) : null}
          </div>
        ) : null}

        {/* Status / error banner */}
        {showStatusBanner ? (
          <div
            className={cn(
              "mx-3 mb-1 flex items-center gap-2 rounded-lg border px-3 py-1.5 text-xs",
              statusClassName,
            )}
          >
            <StatusIcon
              className={cn(
                "size-3.5 shrink-0",
                statusTone === "active" && "animate-spin",
              )}
            />
            <span className="truncate">{statusMessage}</span>
          </div>
        ) : error ? (
          <div className="mx-3 mb-1 rounded-lg border border-destructive/50 bg-destructive/10 px-3 py-1.5 text-destructive text-xs">
            {error}
          </div>
        ) : null}

        <LiteratureReviewPanel />
        <PaperDraftingPanel />
        <PeerReviewPanel />

        {/* Messages area */}
        <div className="relative min-h-0 flex-1 overflow-hidden">
          <ChatMessages />
        </div>

        {pendingWorkflowCheckpoint ? (
          <WorkflowCheckpointCard checkpoint={pendingWorkflowCheckpoint} />
        ) : null}
        {pendingApproval ? <ApprovalCard approval={pendingApproval} /> : null}

        {/* Composer */}
        <ChatComposer isOpen={isOpen} />
      </div>
    </div>
  );
}
