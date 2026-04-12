import { useRef, useState, useCallback, useEffect } from "react";
import {
  ChevronDownIcon,
  Maximize2Icon,
  MessageCircleIcon,
  Minimize2Icon,
  PaperclipIcon,
} from "lucide-react";

import { cn } from "@/lib/utils";
import { useAgentChatStore } from "@/stores/agent-chat-store";
import { useAgentEvents } from "@/hooks/use-agent-events";
import { ApprovalCard } from "./approval-card";
import { ChatMessages } from "./chat-messages";
import { ChatComposer } from "./chat-composer";
import { ChatTabBar } from "./chat-tab-bar";
import { WorkflowCheckpointCard } from "./workflow-checkpoint-card";
import { toast } from "sonner";

const MIN_HEIGHT = 150;
const DEFAULT_HEIGHT = 420;
/** Maximum ratio of parent height the drawer can occupy */
const MAX_HEIGHT_RATIO = 0.85;
/** Snap threshold in px — if within this distance of a snap point, snap to it */
const SNAP_THRESHOLD = 30;
/** Predefined snap ratios relative to parent height */
const SNAP_RATIOS = [1 / 3, 1 / 2, 2 / 3];

/** Compute the maximum allowed height from the parent element */
function getMaxHeight(container: HTMLDivElement | null): number {
  const parent = container?.parentElement;
  return parent ? parent.clientHeight * MAX_HEIGHT_RATIO : 600;
}

/** Snap a raw height to the nearest snap point if within threshold */
function snapHeight(raw: number, parentHeight: number): number {
  for (const ratio of SNAP_RATIOS) {
    const target = parentHeight * ratio;
    if (Math.abs(raw - target) < SNAP_THRESHOLD) {
      return target;
    }
  }
  return raw;
}

export function AgentChatDrawer() {
  // Initialize event listeners for the active agent runtime stream.
  useAgentEvents();

  const anyStreaming = useAgentChatStore((s) =>
    s.tabs.some((t) => t.isStreaming),
  );
  const error = useAgentChatStore((s) => s.error);
  const statusStage = useAgentChatStore((s) => {
    const active = s.tabs.find((t) => t.id === s.activeTabId);
    return active?.statusStage ?? null;
  });
  const statusMessage = useAgentChatStore((s) => {
    const active = s.tabs.find((t) => t.id === s.activeTabId);
    return active?.statusMessage ?? null;
  });
  const pendingApproval = useAgentChatStore((s) => {
    const active = s.tabs.find((t) => t.id === s.activeTabId);
    return active?.pendingApproval ?? null;
  });
  const pendingWorkflowCheckpoint = useAgentChatStore((s) => {
    const active = s.tabs.find((t) => t.id === s.activeTabId);
    return active?.pendingWorkflowCheckpoint ?? null;
  });

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
  /** Remember the user's preferred height so re-opening restores it */
  const preferredHeightRef = useRef(DEFAULT_HEIGHT);

  const pendingAttachments = useAgentChatStore((s) => s.pendingAttachments);

  // Surface status changes as toasts instead of a persistent banner
  const prevStatusRef = useRef<string | null>(null);
  useEffect(() => {
    const key = statusStage ?? null;
    if (key === prevStatusRef.current) return;
    prevStatusRef.current = key;
    if (!statusMessage) return;

    if (statusStage === "failed") {
      toast.error(statusMessage, { duration: Infinity });
    } else if (statusStage === "cancelled") {
      toast.warning(statusMessage, { duration: 5000 });
    } else if (statusStage === "awaiting_approval") {
      toast.warning(statusMessage, { duration: 5000 });
    } else if (statusStage === "resuming_after_approval") {
      toast.info(statusMessage, { duration: 3000 });
    } else if (statusStage === "review_ready") {
      toast.info(statusMessage, { duration: 3000 });
    }
  }, [statusStage, statusMessage]);

  // Auto-open when streaming starts or a new attachment is added.
  // Restores the user's preferred height instead of always jumping to max.
  useEffect(() => {
    const shouldOpen = anyStreaming || pendingAttachments.length > 0;
    if (shouldOpen && !isOpen) {
      setIsOpen(true);
      const maxHeight = getMaxHeight(containerRef.current);
      const restored = Math.min(preferredHeightRef.current, maxHeight);
      setHeight(restored);
      heightRef.current = restored;
      if (panelRef.current) {
        panelRef.current.style.height = `${restored}px`;
      }
    }
  }, [anyStreaming, isOpen, pendingAttachments]);

  // Clamp height when the window resizes so the panel never overflows
  useEffect(() => {
    const onResize = () => {
      if (!isOpen || isExpanded) return;
      const maxHeight = getMaxHeight(containerRef.current);
      if (heightRef.current > maxHeight) {
        const clamped = maxHeight;
        heightRef.current = clamped;
        setHeight(clamped);
        if (panelRef.current) {
          panelRef.current.style.height = `${clamped}px`;
        }
      }
    };
    window.addEventListener("resize", onResize);
    return () => window.removeEventListener("resize", onResize);
  }, [isOpen, isExpanded]);

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

  /** Apply a height value — updates refs, DOM, and React state */
  const applyHeight = useCallback((h: number) => {
    heightRef.current = h;
    preferredHeightRef.current = h;
    setHeight(h);
    if (panelRef.current) {
      panelRef.current.style.height = `${h}px`;
    }
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
        const maxHeight = getMaxHeight(containerRef.current);
        const delta = startY - e.clientY;
        const raw = Math.min(
          Math.max(startHeight + delta, MIN_HEIGHT),
          maxHeight,
        );
        heightRef.current = raw;
        if (panelRef.current) {
          panelRef.current.style.height = `${raw}px`;
        }
      };

      const handleMouseUp = () => {
        setIsDragging(false);
        // Snap to the nearest ergonomic anchor on release
        const parentH = containerRef.current?.parentElement?.clientHeight ?? 0;
        const snapped = parentH > 0
          ? Math.min(Math.max(snapHeight(heightRef.current, parentH), MIN_HEIGHT), parentH * MAX_HEIGHT_RATIO)
          : heightRef.current;
        applyHeight(snapped);
        document.removeEventListener("mousemove", handleMouseMove);
        document.removeEventListener("mouseup", handleMouseUp);
      };

      document.addEventListener("mousemove", handleMouseMove);
      document.addEventListener("mouseup", handleMouseUp);
    },
    [isExpanded, applyHeight],
  );

  /** Double-click the drag handle to toggle between 1/3 and 2/3 height */
  const handleDoubleClick = useCallback(() => {
    if (isExpanded) return;
    const parentH = containerRef.current?.parentElement?.clientHeight ?? 0;
    if (parentH === 0) return;
    const thirdH = parentH / 3;
    const twoThirdH = (parentH * 2) / 3;
    // If currently closer to 2/3, snap down to 1/3, and vice versa
    const target = heightRef.current > parentH / 2 ? thirdH : twoThirdH;
    applyHeight(target);
  }, [isExpanded, applyHeight]);

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
            {/* Header: Maximize | Drag Handle | Minimize — flex layout for breathing room */}
            <div
              className="flex items-center justify-between gap-2 px-3 py-2 transition-colors"
              style={{ minHeight: 32 }}
            >
              <button
                type="button"
                onClick={() => setIsExpanded(true)}
                className="flex size-7 items-center justify-center rounded-md text-muted-foreground transition-colors hover:bg-muted hover:text-foreground flex-shrink-0"
                aria-label="Fullscreen"
              >
                <Maximize2Icon className="size-4" />
              </button>

              {/* Drag handle in the center — drag to resize, double-click to snap toggle */}
              <div
                className="group flex cursor-row-resize flex-1 items-center justify-center py-2 transition-colors hover:bg-muted/50 rounded-md"
                onMouseDown={handleMouseDown}
                onDoubleClick={handleDoubleClick}
              >
                <div className="h-1 w-10 rounded-full bg-muted-foreground/30 transition-all group-hover:w-16 group-hover:bg-muted-foreground/50" />
              </div>

              <button
                type="button"
                onClick={() => setIsOpen(false)}
                className="flex size-7 items-center justify-center rounded-md text-muted-foreground transition-colors hover:bg-muted hover:text-foreground flex-shrink-0"
                aria-label="Minimize chat"
              >
                <ChevronDownIcon className="size-4" />
              </button>
            </div>
            <ChatTabBar />
          </>
        )}

        {/* Status / error — shown inline only for persistent errors */}
        {error ? (
          <div className="mx-3 mb-1 rounded-lg border border-destructive/50 bg-destructive/10 px-3 py-1.5 text-destructive text-xs">
            {error}
          </div>
        ) : null}

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
