import {
  useCallback,
  useEffect,
  useRef,
  useState,
  type ReactNode,
} from "react";
import { ArrowUpIcon, LoaderIcon } from "lucide-react";

export interface ToolbarAction {
  id: string;
  label: string;
  icon: ReactNode;
  hint?: string; // e.g. "Double-click also works"
}

export interface CitationToolbarCandidate {
  key: string;
  title: string;
  year?: number | null;
  score: number;
}

export interface CitationToolbarState {
  active: boolean;
  isSearching: boolean;
  isApplying: boolean;
  error: string | null;
  decisionHint: string | null;
  lastAutoAppliedTitle: string | null;
  lastInsertedCitekey: string | null;
  candidates: CitationToolbarCandidate[];
  onCite: (key: string) => void;
  onRetry: () => void;
  onClose: () => void;
}

interface SelectionToolbarProps {
  position: { top: number; left: number };
  contextLabel: string;
  actions: ToolbarAction[];
  citation?: CitationToolbarState;
  onSendPrompt: (prompt: string) => void;
  onAction: (actionId: string) => void;
  onDismiss: () => void;
}

export function SelectionToolbar({
  position,
  contextLabel,
  actions,
  citation,
  onSendPrompt,
  onAction,
  onDismiss,
}: SelectionToolbarProps) {
  const [input, setInput] = useState("");
  const toolbarRef = useRef<HTMLDivElement>(null);
  const inputRef = useRef<HTMLInputElement>(null);

  const handleSend = useCallback(() => {
    const trimmed = input.trim();
    if (!trimmed) return;
    setInput("");
    onSendPrompt(trimmed);
  }, [input, onSendPrompt]);

  const handleKeyDown = useCallback(
    (e: React.KeyboardEvent<HTMLInputElement>) => {
      if (e.key === "Enter" && !e.shiftKey) {
        e.preventDefault();
        handleSend();
      }
      if (e.key === "Escape") {
        e.preventDefault();
        onDismiss();
      }
    },
    [handleSend, onDismiss],
  );

  const citationActive = citation?.active ?? false;

  // Dismiss on click outside or Escape
  useEffect(() => {
    const handleMouseDown = (e: MouseEvent) => {
      if (citationActive && (citation?.isSearching || citation?.isApplying)) {
        return;
      }
      if (
        toolbarRef.current &&
        !toolbarRef.current.contains(e.target as Node)
      ) {
        onDismiss();
      }
    };
    const handleKeyDown = (e: KeyboardEvent) => {
      if (citationActive && (citation?.isSearching || citation?.isApplying)) {
        return;
      }
      if (e.key === "Escape") onDismiss();
    };
    // Delay attaching to avoid dismissing on the same click that created the selection
    const timer = setTimeout(() => {
      document.addEventListener("mousedown", handleMouseDown);
      document.addEventListener("keydown", handleKeyDown);
    }, 100);
    return () => {
      clearTimeout(timer);
      document.removeEventListener("mousedown", handleMouseDown);
      document.removeEventListener("keydown", handleKeyDown);
    };
  }, [citation?.isSearching, citationActive, onDismiss]);

  const citationBody = citation ? (
    <div className="space-y-2 p-2">
      <div className="flex items-center justify-between">
        <span className="font-medium text-sm">Citation Search</span>
        <button
          type="button"
          className="rounded px-1.5 py-0.5 text-muted-foreground text-xs hover:bg-muted hover:text-foreground disabled:cursor-not-allowed disabled:opacity-40"
          onClick={citation.onClose}
          disabled={citation.isSearching || citation.isApplying}
        >
          Close
        </button>
      </div>
      {citation.isSearching ? (
        <div className="flex items-center gap-2 rounded-md border border-border px-2 py-2 text-muted-foreground text-xs">
          <LoaderIcon className="size-3.5 animate-spin" />
          Searching related papers...
        </div>
      ) : citation.isApplying ? (
        <div className="flex items-center gap-2 rounded-md border border-border px-2 py-2 text-muted-foreground text-xs">
          <LoaderIcon className="size-3.5 animate-spin" />
          Applying citation...
        </div>
      ) : citation.error ? (
        <div className="space-y-2">
          <div className="rounded-md border border-destructive/40 bg-destructive/10 px-2 py-1.5 text-destructive text-xs">
            {citation.error}
          </div>
          <button
            type="button"
            className="rounded border border-border px-2 py-1 text-xs transition-colors hover:bg-muted"
            onClick={citation.onRetry}
          >
            Retry Search
          </button>
        </div>
      ) : citation.lastAutoAppliedTitle ? (
        <div className="space-y-1 rounded-md border border-border px-2 py-2">
          <p className="text-xs">
            Auto-applied citation:{" "}
            <span className="font-medium">{citation.lastAutoAppliedTitle}</span>
          </p>
          {citation.lastInsertedCitekey && (
            <p className="text-muted-foreground text-xs">
              Inserted: <code>{citation.lastInsertedCitekey}</code>
            </p>
          )}
        </div>
      ) : citation.candidates.length > 0 ? (
        <div className="max-h-44 space-y-1 overflow-y-auto pr-1">
          {citation.candidates.map((candidate) => (
            <div
              key={candidate.key}
              className="rounded-md border border-border bg-background px-2 py-1.5"
            >
              <div className="line-clamp-2 text-xs">{candidate.title}</div>
              <div className="mt-1 flex items-center justify-between text-muted-foreground text-xs">
                <span>
                  {candidate.year ?? "n.d."} · {Math.round(candidate.score * 100)}%
                </span>
                <button
                  type="button"
                  className="rounded border border-border px-1.5 py-0.5 text-xs transition-colors hover:bg-muted disabled:cursor-not-allowed disabled:opacity-40"
                  onClick={() => citation.onCite(candidate.key)}
                  disabled={citation.isApplying}
                >
                  Cite
                </button>
              </div>
            </div>
          ))}
        </div>
      ) : (
        <div className="rounded-md border border-border px-2 py-2 text-muted-foreground text-xs">
          No matching papers found.
        </div>
      )}
      {citation.decisionHint && !citation.isSearching && (
        <p className="text-muted-foreground text-xs">{citation.decisionHint}</p>
      )}
    </div>
  ) : null;

  return (
    <div
      ref={toolbarRef}
      className={`absolute z-30 rounded-lg border border-border bg-background shadow-xl ${citationActive ? "w-80" : "w-64"}`}
      style={{
        top: position.top,
        left: position.left,
      }}
    >
      {citationActive ? (
        citationBody
      ) : (
        <>
          {/* Prompt input */}
          <div className="flex items-center gap-1 border-border border-b px-2 py-1.5">
            <input
              ref={inputRef}
              type="text"
              value={input}
              onChange={(e) => setInput(e.target.value)}
              onKeyDown={handleKeyDown}
              placeholder="Enter prompt..."
              className="min-w-0 flex-1 bg-transparent text-sm outline-none placeholder:text-muted-foreground"
            />
            <button
              aria-label="Send prompt"
              onClick={handleSend}
              disabled={!input.trim()}
              className="flex size-6 shrink-0 items-center justify-center rounded-full bg-primary text-primary-foreground transition-opacity disabled:opacity-30"
            >
              <ArrowUpIcon className="size-3.5" />
            </button>
          </div>

          {/* Action buttons */}
          {actions.length > 0 && (
            <div className="flex flex-col py-1">
              {actions.map((action) => (
                <button
                  key={action.id}
                  onClick={() => onAction(action.id)}
                  className="flex items-center gap-2.5 px-3 py-1.5 text-left text-foreground text-sm transition-colors hover:bg-muted"
                >
                  <span className="size-4 text-muted-foreground">
                    {action.icon}
                  </span>
                  {action.label}
                  {action.hint && (
                    <span className="ml-auto text-muted-foreground text-xs">
                      {action.hint}
                    </span>
                  )}
                </button>
              ))}
            </div>
          )}
        </>
      )}

      {/* Context label */}
      <div className="border-border border-t px-3 py-1.5">
        <span className="font-mono text-muted-foreground text-xs">
          {contextLabel}
        </span>
      </div>
    </div>
  );
}
