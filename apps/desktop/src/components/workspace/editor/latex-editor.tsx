import { useCallback, useEffect, useRef, useState, useMemo } from "react";
import { Compartment, EditorState, Prec, Transaction } from "@codemirror/state";
import {
  EditorView,
  keymap,
  lineNumbers,
  highlightActiveLine,
  highlightActiveLineGutter,
  scrollPastEnd,
} from "@codemirror/view";
import {
  defaultKeymap,
  history,
  historyKeymap,
  indentMore,
  indentLess,
  toggleComment,
} from "@codemirror/commands";
import { syntaxHighlighting, syntaxTreeAvailable } from "@codemirror/language";
import { oneDark, oneDarkHighlightStyle } from "@codemirror/theme-one-dark";
import { defaultHighlightStyle } from "@codemirror/language";
import { useTheme } from "next-themes";
import {
  search,
  highlightSelectionMatches,
  SearchQuery,
  setSearchQuery as setSearchQueryEffect,
  findNext,
  findPrevious,
} from "@codemirror/search";
import {
  unifiedMergeView,
  getChunks,
  acceptChunk,
  rejectChunk,
} from "@codemirror/merge";
import { latex, latexLinter } from "codemirror-lang-latex";
import { bibtex } from "./lang-bibtex";
import {
  linter,
  lintGutter,
  forEachDiagnostic,
  type Diagnostic,
} from "@codemirror/lint";
import { useDocumentStore, type ProjectFile } from "@/stores/document-store";
import {
  useProposedChangesStore,
  type ProposedChange,
} from "@/stores/proposed-changes-store";
import {
  useAgentChatStore,
  type AgentTurnProfile,
} from "@/stores/agent-chat-store";
import { useHistoryStore, type FileDiff } from "@/stores/history-store";
import {
  compileLatex,
  resolveCompileTarget,
  formatCompileError,
} from "@/lib/latex-compiler";
import { EditorToolbar } from "./editor-toolbar";
import {
  SelectionToolbar,
  type CitationToolbarCandidate,
  type ToolbarAction,
} from "./selection-toolbar";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
  DialogFooter,
} from "@/components/ui/dialog";
import {
  SpellCheckIcon,
  SearchIcon,
  BugIcon,
  LoaderIcon,
  RotateCcwIcon,
  TagIcon,
  CopyIcon,
  XIcon,
} from "lucide-react";
import { AgentChatDrawer } from "@/components/agent-chat/agent-chat-drawer";
import { ProposedChangesPanel } from "@/components/agent-chat/proposed-changes-panel";
import { ImagePreview } from "./image-preview";
import { MarkdownPreview } from "./markdown-preview";
import { DocxPreview } from "./docx-preview";
import { SearchPanel } from "./search-panel";
import { ProblemsPanel, type DiagnosticItem } from "./problems-panel";
import { PdfViewer } from "@/components/workspace/preview/pdf-viewer";
import { readFile } from "@tauri-apps/plugin-fs";
import { createLogger } from "@/lib/debug/logger";
import { useCitationStore } from "@/stores/citation-store";
import { toast } from "sonner";

const SELECTION_EDIT_PROFILE: AgentTurnProfile = {
  taskKind: "selection_edit",
  selectionScope: "selected_span",
  responseMode: "reviewable_change",
  samplingProfile: "edit_stable",
  sourceHint: "editor_selection_action",
};

const FILE_EDIT_PROFILE: AgentTurnProfile = {
  taskKind: "file_edit",
  selectionScope: "none",
  responseMode: "reviewable_change",
  samplingProfile: "edit_stable",
  sourceHint: "editor_file_action",
};
import type { CitationSearchDebug } from "@/lib/citation-api";
import {
  appendJsonLineToProject,
  createFileOnDisk,
  getUniqueTargetName,
} from "@/lib/tauri/fs";

const log = createLogger("merge-view");

function getActiveFileContent(): string {
  const state = useDocumentStore.getState();
  const activeFile = state.files.find((f) => f.id === state.activeFileId);
  return activeFile?.content ?? "";
}

/** Per-file editor state cache: fileId → { cursor, scrollTop } */
const editorStateCache = new Map<
  string,
  { cursor: number; scrollTop: number }
>();

/** Clear editor state cache (e.g., on project close). */
export function clearEditorStateCache(): void {
  editorStateCache.clear();
}

function normalizeDoi(doi: string) {
  return doi
    .trim()
    .toLowerCase()
    .replace(/^https?:\/\/doi\.org\//, "")
    .replace(/^doi:/, "");
}

function uniqueKeepOrder(items: string[]) {
  const seen = new Set<string>();
  const out: string[] = [];
  for (const item of items) {
    const trimmed = item.trim();
    if (!trimmed || seen.has(trimmed)) continue;
    seen.add(trimmed);
    out.push(trimmed);
  }
  return out;
}

function buildDebugCandidateKey(
  candidate: CitationSearchDebug["merged_results"][number],
  index: number,
) {
  const paperId =
    typeof candidate.paper_id === "string" ? candidate.paper_id.trim() : "";
  if (paperId) return paperId;
  const doi = typeof candidate.doi === "string" ? normalizeDoi(candidate.doi) : "";
  if (doi) return `doi:${doi}`;
  const title = typeof candidate.title === "string" ? candidate.title.trim() : "";
  return `idx:${index}:${title}`;
}

function buildEvalSample(
  debug: CitationSearchDebug,
  expected: { dois: string[]; titles: string[]; no_match?: boolean },
) {
  return {
    id: `sample_${new Date().toISOString().replace(/[:.]/g, "-")}`,
    selected_text: debug.selected_text,
    latency_ms: debug.latency_ms,
    expected,
    merged_results: debug.merged_results.map((c) => ({
      paper_id: c.paper_id,
      title: c.title,
      year: c.year ?? null,
      venue: c.venue ?? null,
      doi: c.doi ?? null,
      url: c.url ?? null,
      score: c.score,
      score_explain: c.score_explain ?? null,
      evidence_sentences: c.evidence_sentences ?? [],
    })),
    debug: {
      preprocessed_text: debug.preprocessed_text,
      query_plan: debug.query_plan,
      provider_budgets: debug.provider_budgets,
      query_execution: debug.query_execution,
      stop_reason: debug.stop_reason,
      stop_stage: debug.stop_stage,
      stop_hit_ratio: debug.stop_hit_ratio,
      stop_quality_hits: debug.stop_quality_hits,
      stop_attempted_queries: debug.stop_attempted_queries,
      stop_merged_count: debug.stop_merged_count,
    },
  };
}

export function LatexEditor() {
  const containerRef = useRef<HTMLDivElement>(null);
  const viewRef = useRef<EditorView | null>(null);

  const files = useDocumentStore((s) => s.files);
  const activeFileId = useDocumentStore((s) => s.activeFileId);
  const projectRoot = useDocumentStore((s) => s.projectRoot);
  const setContent = useDocumentStore((s) => s.setContent);
  const setCursorPosition = useDocumentStore((s) => s.setCursorPosition);
  const setSelectionRange = useDocumentStore((s) => s.setSelectionRange);
  const jumpToPosition = useDocumentStore((s) => s.jumpToPosition);
  const clearJumpRequest = useDocumentStore((s) => s.clearJumpRequest);
  const refreshFiles = useDocumentStore((s) => s.refreshFiles);
  const setActiveFile = useDocumentStore((s) => s.setActiveFile);

  const setIsCompiling = useDocumentStore((s) => s.setIsCompiling);
  const setPdfData = useDocumentStore((s) => s.setPdfData);
  const setCompileError = useDocumentStore((s) => s.setCompileError);
  const saveAllFiles = useDocumentStore((s) => s.saveAllFiles);

  const activeFile = files.find((f) => f.id === activeFileId);
  const isTextFile =
    activeFile?.type === "tex" ||
    activeFile?.type === "bib" ||
    activeFile?.type === "style" ||
    activeFile?.type === "other";
  const normalizedActivePath = activeFile?.relativePath.toLowerCase() ?? "";
  const isMarkdownFile =
    activeFile?.type === "other" &&
    (normalizedActivePath.endsWith(".md") ||
      normalizedActivePath.endsWith(".markdown"));
  const isEditableMarkdownFile =
    isMarkdownFile && normalizedActivePath.endsWith(".editable.md");
  const isDocxFile =
    activeFile?.type === "other" && normalizedActivePath.endsWith(".docx");
  const useCodeEditor =
    isTextFile && (!isMarkdownFile || isEditableMarkdownFile) && !isDocxFile;
  const activeFileContent = activeFile?.content;
  const isLargeFileNotLoaded =
    (useCodeEditor || (isMarkdownFile && !isEditableMarkdownFile)) &&
    activeFileContent === undefined &&
    !!activeFile;
  const loadFileContent = useDocumentStore((s) => s.loadFileContent);

  // History review state
  const reviewingSnapshot = useHistoryStore((s) => s.reviewingSnapshot);
  const historyDiffResult = useHistoryStore((s) => s.diffResult);

  const [imageScale, setImageScale] = useState(1.0);
  const [cropMode, setCropMode] = useState(false);

  // Reset scale and crop mode when switching files
  useEffect(() => {
    setImageScale(1.0);
    setCropMode(false);
  }, [activeFileId]);

  const [isSearchOpen, setIsSearchOpen] = useState(false);
  const [searchQuery, setSearchQuery] = useState("");
  const [matchCount, setMatchCount] = useState(0);
  const [currentMatch, setCurrentMatch] = useState(0);
  const [mergeChunkInfo, setMergeChunkInfo] = useState({
    total: 0,
    current: 0,
  });
  const [diagnostics, setDiagnostics] = useState<DiagnosticItem[]>([]);
  const [selectionCoords, setSelectionCoords] = useState<{
    top: number;
    left: number;
  } | null>(null);
  const [citationPanelOpen, setCitationPanelOpen] = useState(false);
  const [citationDebugOpen, setCitationDebugOpen] = useState(false);
  const [selectedDebugLabelKeys, setSelectedDebugLabelKeys] = useState<string[]>(
    [],
  );
  const [debugLabelNoMatch, setDebugLabelNoMatch] = useState(false);
  // When the selection toolbar is visible, prevent CM selection changes from clearing it.
  // Only explicit dismiss/send/action should clear the toolbar.
  const toolbarStickyRef = useRef(false);
  const parentRef = useRef<HTMLDivElement>(null);

  const citationIsSearching = useCitationStore((s) => s.isSearching);
  const citationIsApplying = useCitationStore((s) => s.isApplying);
  const citationResults = useCitationStore((s) => s.results);
  const citationReviewCandidates = useCitationStore((s) => s.reviewCandidates);
  const citationLastAutoAppliedTitle = useCitationStore(
    (s) => s.lastAutoAppliedTitle,
  );
  const citationLastInsertedCitekey = useCitationStore(
    (s) => s.lastInsertedCitekey,
  );
  const citationDecisionHint = useCitationStore((s) => s.decisionHint);
  const citationError = useCitationStore((s) => s.error);
  const citationIsDebugSearching = useCitationStore((s) => s.isDebugSearching);
  const citationDebugInfo = useCitationStore((s) => s.debugInfo);
  const searchCitationFromSelection = useCitationStore(
    (s) => s.searchFromSelection,
  );
  const runCitationDebugFromSelection = useCitationStore(
    (s) => s.runDebugFromSelection,
  );
  const applyCitationCandidate = useCitationStore((s) => s.applyCandidate);
  const clearCitationState = useCitationStore((s) => s.clear);

  useEffect(() => {
    if (!citationDebugOpen || !citationDebugInfo) {
      setSelectedDebugLabelKeys([]);
      setDebugLabelNoMatch(false);
    }
  }, [citationDebugInfo, citationDebugOpen]);

  const { resolvedTheme } = useTheme();

  const compileRef = useRef<() => void>(() => {});
  const isSearchOpenRef = useRef(false);
  const themeCompartmentRef = useRef(new Compartment());
  const mergeCompartmentRef = useRef(new Compartment());
  const isMergeActiveRef = useRef(false);
  const pendingChangeRef = useRef<ProposedChange | null>(null);
  const handleKeepAllRef = useRef<() => void>(() => {});
  const handleUndoAllRef = useRef<() => void>(() => {});
  const diagnosticsRef = useRef<DiagnosticItem[]>([]);

  useEffect(() => {
    isSearchOpenRef.current = isSearchOpen;
  }, [isSearchOpen]);

  // Proposed changes for active file
  const proposedChanges = useProposedChangesStore((s) => s.changes);
  const activeFileChange = useMemo(() => {
    if (!activeFile) return null;
    return (
      proposedChanges.find((c) => c.filePath === activeFile.relativePath) ??
      null
    );
  }, [proposedChanges, activeFile]);

  // Keep all changes (⌘Y)
  handleKeepAllRef.current = () => {
    const view = viewRef.current;
    const change = pendingChangeRef.current;
    if (!view || !change) return;
    isMergeActiveRef.current = false;
    setMergeChunkInfo({ total: 0, current: 0 });
    view.dispatch({ effects: mergeCompartmentRef.current.reconfigure([]) });
    setContent(change.newContent);
    useProposedChangesStore.getState().keepChange(change.id);
    pendingChangeRef.current = null;
    // Auto-navigate to next file with pending changes (only if file exists)
    const remaining = useProposedChangesStore.getState().changes;
    if (remaining.length > 0) {
      const docStore = useDocumentStore.getState();
      const nextFile = remaining.find((c) =>
        docStore.files.some((f) => f.relativePath === c.filePath),
      );
      if (nextFile) {
        docStore.setActiveFile(nextFile.filePath);
      }
    }
  };

  // Undo all changes (⌘N)
  handleUndoAllRef.current = () => {
    const view = viewRef.current;
    const change = pendingChangeRef.current;
    if (!view || !change) return;
    isMergeActiveRef.current = false;
    setMergeChunkInfo({ total: 0, current: 0 });
    view.dispatch({ effects: mergeCompartmentRef.current.reconfigure([]) });
    view.dispatch({
      changes: {
        from: 0,
        to: view.state.doc.length,
        insert: change.oldContent,
      },
      annotations: Transaction.addToHistory.of(false),
    });
    setContent(change.oldContent);
    useProposedChangesStore.getState().undoChange(change.id);
    pendingChangeRef.current = null;
    // Auto-navigate to next file with pending changes (only if file exists)
    const remaining = useProposedChangesStore.getState().changes;
    if (remaining.length > 0) {
      const docStore = useDocumentStore.getState();
      const nextFile = remaining.find((c) =>
        docStore.files.some((f) => f.relativePath === c.filePath),
      );
      if (nextFile) {
        docStore.setActiveFile(nextFile.filePath);
      }
    }
  };

  // Navigate to a specific chunk by index
  const goToChunk = (index: number) => {
    const view = viewRef.current;
    if (!view) return;
    const chunks = getChunks(view.state);
    if (!chunks || index < 0 || index >= chunks.chunks.length) return;
    const chunk = chunks.chunks[index];
    view.dispatch({
      selection: { anchor: chunk.fromB },
      effects: EditorView.scrollIntoView(chunk.fromB, { y: "center" }),
    });
    view.focus();
  };

  // After individual accept/reject, navigate to next chunk or auto-resolve
  const afterChunkAction = (view: EditorView, prevIdx: number) => {
    const remaining = getChunks(view.state);
    if (!remaining || remaining.chunks.length === 0) {
      // All chunks resolved — clean up merge view
      const change = pendingChangeRef.current;
      if (change) {
        isMergeActiveRef.current = false;
        setMergeChunkInfo({ total: 0, current: 0 });
        const finalContent = view.state.doc.toString();
        view.dispatch({ effects: mergeCompartmentRef.current.reconfigure([]) });
        setContent(finalContent);
        if (finalContent === change.oldContent) {
          useProposedChangesStore.getState().undoChange(change.id);
        } else {
          useProposedChangesStore.getState().keepChange(change.id);
        }
        pendingChangeRef.current = null;
        // Auto-navigate to next file with pending changes
        const pendingChanges = useProposedChangesStore.getState().changes;
        if (pendingChanges.length > 0) {
          useDocumentStore.getState().setActiveFile(pendingChanges[0].filePath);
        }
      }
    } else {
      // Focus the next remaining chunk
      const nextIdx = Math.min(prevIdx, remaining.chunks.length - 1);
      const next = remaining.chunks[nextIdx];
      view.dispatch({
        selection: { anchor: next.fromB },
        effects: EditorView.scrollIntoView(next.fromB, { y: "center" }),
      });
    }
    view.focus();
  };

  const acceptCurrentChunk = () => {
    const view = viewRef.current;
    if (!view) return;
    const chunks = getChunks(view.state);
    const idx = mergeChunkInfo.current - 1;
    if (!chunks || idx < 0 || idx >= chunks.chunks.length) return;
    acceptChunk(view, chunks.chunks[idx].fromB);
    afterChunkAction(view, idx);
  };

  const rejectCurrentChunk = () => {
    const view = viewRef.current;
    if (!view) return;
    const chunks = getChunks(view.state);
    const idx = mergeChunkInfo.current - 1;
    if (!chunks || idx < 0 || idx >= chunks.chunks.length) return;
    rejectChunk(view, chunks.chunks[idx].fromB);
    afterChunkAction(view, idx);
  };

  useEffect(() => {
    if (!searchQuery || !activeFileContent) {
      setMatchCount(0);
      setCurrentMatch(0);
      return;
    }
    const regex = new RegExp(
      searchQuery.replace(/[.*+?^${}()|[\]\\]/g, "\\$&"),
      "gi",
    );
    const matches = activeFileContent.match(regex);
    setMatchCount(matches?.length ?? 0);
    setCurrentMatch(matches && matches.length > 0 ? 1 : 0);
  }, [searchQuery, activeFileContent]);

  useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      if ((e.metaKey || e.ctrlKey) && e.key === "f") {
        e.preventDefault();
        setIsSearchOpen(true);
      }
    };
    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, []);

  useEffect(() => {
    const view = viewRef.current;
    if (!view) return;
    const query = new SearchQuery({
      search: searchQuery,
      caseSensitive: false,
      literal: true,
    });
    view.dispatch({ effects: setSearchQueryEffect.of(query) });
    if (searchQuery) findNext(view);
  }, [searchQuery]);

  const handleFindNext = () => {
    const view = viewRef.current;
    if (view) {
      findNext(view);
      view.focus();
    }
  };
  const handleFindPrevious = () => {
    const view = viewRef.current;
    if (view) {
      findPrevious(view);
      view.focus();
    }
  };

  // Compile: save all files first, then compile via Tauri command
  compileRef.current = async () => {
    const state = useDocumentStore.getState();
    if (!projectRoot || activeFile?.type !== "tex") return;
    if (state.isCompiling) {
      // Queue a recompile after the current one finishes
      state.setPendingRecompile(true);
      return;
    }
    const { files: allFiles } = state;
    const resolved = resolveCompileTarget(activeFile.id, allFiles);
    if (!resolved) {
      setCompileError(
        "No .tex file found in this project. Create a main.tex file to compile.",
        activeFile.id,
      );
      return;
    }
    const { rootId, targetPath } = resolved;
    useHistoryStore.getState().stopReview();
    setIsCompiling(true);
    state.setPendingRecompile(false);
    const compileStart = Date.now();
    try {
      await saveAllFiles();
      // Pre-compile snapshot (fire-and-forget to avoid blocking compilation start)
      useHistoryStore
        .getState()
        .createSnapshot(projectRoot, "[compile] Pre-compile")
        .catch(() => {});
      const data = await compileLatex(projectRoot, targetPath);
      setPdfData(data, rootId);
    } catch (error) {
      setCompileError(formatCompileError(error), rootId);
    } finally {
      // Ensure the spinner is visible for at least 500ms for visual feedback
      const elapsed = Date.now() - compileStart;
      if (elapsed < 500) {
        await new Promise((r) => setTimeout(r, 500 - elapsed));
      }
      setIsCompiling(false);
      // If a recompile was requested while we were compiling, trigger it now
      // Use setTimeout to avoid unbounded recursion on the call stack
      if (useDocumentStore.getState().pendingRecompile) {
        setTimeout(() => compileRef.current?.(), 0);
      }
    }
  };

  useEffect(() => {
    if (!containerRef.current || !useCodeEditor) return;
    const currentContent = getActiveFileContent();

    const updateListener = EditorView.updateListener.of((update) => {
      if (isMergeActiveRef.current) {
        const chunks = getChunks(update.state);
        if (chunks) {
          const total = chunks.chunks.length;
          // Track current chunk based on cursor position
          const cursorPos = update.state.selection.main.head;
          let current = 0;
          for (let i = 0; i < chunks.chunks.length; i++) {
            if (cursorPos >= chunks.chunks[i].fromB) current = i + 1;
          }
          setMergeChunkInfo({
            total,
            current: Math.min(Math.max(1, current), total),
          });

          // Auto-resolve when all chunks have been individually accepted/rejected
          // Note: acceptChunk doesn't change the main doc (only the original),
          // so we check total === 0 regardless of docChanged
          if (total === 0) {
            const change = pendingChangeRef.current;
            if (change) {
              setTimeout(() => {
                const v = viewRef.current;
                if (!v || !isMergeActiveRef.current) return;
                // Guard: bail if already resolved by afterChunkAction or a new stacked edit arrived
                if (pendingChangeRef.current !== change) return;
                isMergeActiveRef.current = false;
                setMergeChunkInfo({ total: 0, current: 0 });
                const finalContent = v.state.doc.toString();
                v.dispatch({
                  effects: mergeCompartmentRef.current.reconfigure([]),
                });
                setContent(finalContent);
                if (finalContent === change.oldContent) {
                  useProposedChangesStore.getState().undoChange(change.id);
                } else {
                  useProposedChangesStore.getState().keepChange(change.id);
                }
                pendingChangeRef.current = null;
                // Auto-navigate to next file with pending changes
                const remaining = useProposedChangesStore.getState().changes;
                if (remaining.length > 0) {
                  useDocumentStore
                    .getState()
                    .setActiveFile(remaining[0].filePath);
                }
              }, 0);
            }
          }
        }
        return;
      }
      if (update.docChanged) setContent(update.state.doc.toString());
      if (update.selectionSet) {
        const { from, to, head } = update.state.selection.main;
        setCursorPosition(head);

        // Compute toolbar position below the selection end
        // Skip toolbar for "select all" (Cmd+A) to avoid overlay issues
        const isSelectAll = from === 0 && to === update.state.doc.length;
        if (from !== to && !isSelectAll) {
          setCitationPanelOpen(false);
          setCitationDebugOpen(false);
          clearCitationState();
          setSelectionRange({ start: from, end: to });
          const startCoords = update.view.coordsAtPos(from);
          const endCoords = update.view.coordsAtPos(to);
          if (endCoords && startCoords) {
            setSelectionCoords({
              top: endCoords.bottom, // below last line of selection
              left: startCoords.left, // aligned to selection start
            });
          }
          toolbarStickyRef.current = true;
        } else if (!toolbarStickyRef.current) {
          // Only clear selection/coords if the toolbar is not being interacted with.
          // Clicking the toolbar input causes CM to lose focus and collapse the selection,
          // but we want to keep the toolbar visible until explicitly dismissed.
          setSelectionRange(null);
          setSelectionCoords(null);
        }
      }

      // Sync diagnostics for Problems panel
      const diags: DiagnosticItem[] = [];
      forEachDiagnostic(update.state, (d, from) => {
        diags.push({
          from,
          to: d.to,
          severity: d.severity,
          message: d.message,
          line: update.state.doc.lineAt(from).number,
        });
      });
      if (
        diags.length !== diagnosticsRef.current.length ||
        diags.some(
          (d, i) =>
            d.from !== diagnosticsRef.current[i]?.from ||
            d.message !== diagnosticsRef.current[i]?.message,
        )
      ) {
        diagnosticsRef.current = diags;
        setDiagnostics(diags);
      }
    });

    // Wrap selected text with a LaTeX command, or insert empty command at cursor
    const wrapSelection = (view: EditorView, cmd: string): boolean => {
      const { from, to } = view.state.selection.main;
      const selected = view.state.sliceDoc(from, to);
      const wrapped = `\\${cmd}{${selected}}`;
      const cursorPos = selected
        ? from + wrapped.length
        : from + cmd.length + 2;
      view.dispatch({
        changes: { from, to, insert: wrapped },
        selection: { anchor: cursorPos },
      });
      return true;
    };

    const compileKeymap = Prec.highest(
      keymap.of([
        {
          key: "Mod-Enter",
          run: () => {
            compileRef.current();
            return true;
          },
        },
        {
          key: "Mod-s",
          run: () => {
            const state = useDocumentStore.getState();
            state.setIsSaving(true);
            state
              .saveCurrentFile()
              .finally(() => setTimeout(() => state.setIsSaving(false), 500));
            return true;
          },
        },
        {
          key: "Mod-f",
          run: () => {
            setIsSearchOpen(true);
            return true;
          },
        },
        {
          key: "Escape",
          run: () => {
            if (isSearchOpenRef.current) {
              setIsSearchOpen(false);
              return true;
            }
            return false;
          },
        },
        {
          key: "Mod-y",
          run: () => {
            if (isMergeActiveRef.current) {
              handleKeepAllRef.current();
              return true;
            }
            return false;
          },
        },
        {
          key: "Mod-n",
          run: () => {
            if (isMergeActiveRef.current) {
              handleUndoAllRef.current();
              return true;
            }
            return false;
          },
        },
        {
          key: "Mod-b",
          run: (view) => wrapSelection(view, "textbf"),
        },
        {
          key: "Mod-i",
          run: (view) => wrapSelection(view, "textit"),
        },
        {
          key: "Mod-/",
          run: toggleComment,
        },
      ]),
    );

    const state = EditorState.create({
      doc: currentContent,
      extensions: [
        compileKeymap,
        lineNumbers(),
        highlightActiveLine(),
        highlightActiveLineGutter(),
        history(),
        keymap.of([
          { key: "Tab", run: indentMore, shift: indentLess },
          ...defaultKeymap,
          ...historyKeymap,
        ]),
        activeFile?.type === "bib" ? bibtex() : latex({ enableLinting: false }),
        ...(activeFile?.type === "tex"
          ? [
              linter((view) => {
                // Wait until the Lezer parser has fully parsed the document
                // to avoid false positives from incomplete syntax trees
                if (!syntaxTreeAvailable(view.state, view.state.doc.length)) {
                  return [];
                }
                const baseLinter = latexLinter();
                const diagnostics = baseLinter(view);
                return diagnostics.map((d: Diagnostic) => ({
                  ...d,
                  actions: [
                    ...(d.actions ?? []),
                    {
                      name: "Fix with chat",
                      apply: (v: EditorView, from: number, _to: number) => {
                        const line = v.state.doc.lineAt(from);
                        const docState = useDocumentStore.getState();
                        const file = docState.files.find(
                          (f) => f.id === docState.activeFileId,
                        );
                        const fileName = file?.relativePath ?? "main.tex";
                        const ctx = `[Lint error in ${fileName}:${line.number}]\n[Error: ${d.message}]`;
                        useAgentChatStore
                          .getState()
                          .sendPrompt(
                            `${ctx}\n\nFix this lint error.`,
                            undefined,
                            FILE_EDIT_PROFILE,
                          );
                      },
                    },
                  ],
                }));
              }),
              lintGutter(),
            ]
          : []),
        themeCompartmentRef.current.of(
          resolvedTheme === "dark"
            ? [oneDark, syntaxHighlighting(oneDarkHighlightStyle)]
            : [syntaxHighlighting(defaultHighlightStyle)],
        ),
        search(),
        highlightSelectionMatches(),
        mergeCompartmentRef.current.of([]),
        updateListener,
        EditorView.lineWrapping,
        scrollPastEnd(),
        EditorView.theme({
          "&": {
            height: "100%",
            fontSize: "14px",
            color: "var(--foreground)",
            backgroundColor: "var(--background)",
            WebkitBackfaceVisibility: "hidden",
            backfaceVisibility: "hidden",
          },
          ".cm-scroller": {
            overflow: "auto",
            WebkitTransform: "translateZ(0)",
            transform: "translateZ(0)",
          },
          ".cm-gutters": { paddingRight: "4px" },
          ".cm-lineNumbers .cm-gutterElement": {
            paddingLeft: "8px",
            paddingRight: "4px",
          },
          ".cm-content": {
            paddingLeft: "8px",
            paddingRight: "12px",
          },
          ".cm-searchMatch": {
            backgroundColor: "#facc15 !important",
            color: "#000 !important",
            borderRadius: "2px",
            boxShadow: "0 0 0 1px #eab308",
          },
          ".cm-searchMatch-selected": {
            backgroundColor: "#f97316 !important",
            color: "#fff !important",
            borderRadius: "2px",
            boxShadow: "0 0 0 2px #ea580c",
          },
          "&.cm-focused .cm-selectionBackground, .cm-selectionBackground": {
            backgroundColor: "rgba(100, 150, 255, 0.3)",
          },
          ".cm-changedLine": {
            backgroundColor: "rgba(34, 197, 94, 0.08) !important",
          },
          ".cm-deletedChunk": {
            backgroundColor: "rgba(239, 68, 68, 0.12) !important",
            paddingLeft: "6px",
            position: "relative",
          },
          ".cm-insertedLine": {
            backgroundColor: "rgba(34, 197, 94, 0.15) !important",
          },
          ".cm-deletedLine": {
            backgroundColor: "rgba(239, 68, 68, 0.15) !important",
          },
          ".cm-changedText": {
            backgroundColor: "rgba(34, 197, 94, 0.25) !important",
          },
          ".cm-chunkButtons": {
            position: "absolute",
            insetInlineEnd: "5px",
            top: "2px",
            zIndex: "10",
          },
          ".cm-chunkButtons button": {
            border: "none",
            cursor: "pointer",
            color: "white",
            margin: "0 2px",
            borderRadius: "3px",
            padding: "2px 8px",
            fontSize: "12px",
            lineHeight: "1.4",
          },
          ".cm-chunkButtons button[name=accept]": {
            backgroundColor: "#22c55e",
          },
          ".cm-chunkButtons button[name=reject]": {
            backgroundColor: "#ef4444",
          },
          ".cm-changeGutter": { width: "3px", minWidth: "3px" },
          ".cm-changedLineGutter": { backgroundColor: "#22c55e" },
          ".cm-deletedLineGutter": { backgroundColor: "#ef4444" },
          ".cm-diagnostic": {
            padding: "8px 10px",
          },
          ".cm-diagnosticAction": {
            display: "inline-block",
            padding: "4px 12px",
            borderRadius: "6px",
            fontSize: "12px",
            fontWeight: "500",
            cursor: "pointer",
            backgroundColor: "var(--muted, rgba(255,255,255,0.08))",
            color: "var(--foreground, #e5e5e5)",
            border: "1px solid var(--border, rgba(255,255,255,0.1))",
            marginTop: "8px",
            transition: "background-color 0.15s, border-color 0.15s",
          },
          ".cm-diagnosticAction:hover": {
            backgroundColor: "var(--accent, rgba(255,255,255,0.15))",
            borderColor: "var(--foreground, rgba(255,255,255,0.3))",
          },
        }),
      ],
    });

    const view = new EditorView({ state, parent: containerRef.current });
    viewRef.current = view;

    // Restore per-file cursor + scroll from cache
    const cached = editorStateCache.get(activeFileId);
    if (cached) {
      const pos = Math.min(cached.cursor, view.state.doc.length);
      view.dispatch({ selection: { anchor: pos, head: pos } });
      // Scroll restoration needs layout to settle
      requestAnimationFrame(() => {
        view.scrollDOM.scrollTop = cached.scrollTop;
      });
    }

    return () => {
      // Save per-file cursor + scroll before destroying
      editorStateCache.set(activeFileId, {
        cursor: view.state.selection.main.head,
        scrollTop: view.scrollDOM.scrollTop,
      });
      view.destroy();
      viewRef.current = null;
    };
  }, [
    activeFileId,
    useCodeEditor,
    setContent,
    setCursorPosition,
    setSelectionRange,
  ]);

  // Dynamically switch editor theme when resolvedTheme changes
  useEffect(() => {
    const view = viewRef.current;
    if (!view) return;
    const extensions =
      resolvedTheme === "dark"
        ? [oneDark, syntaxHighlighting(oneDarkHighlightStyle)]
        : [syntaxHighlighting(defaultHighlightStyle)];
    view.dispatch({
      effects: themeCompartmentRef.current.reconfigure(extensions),
    });
  }, [resolvedTheme]);

  useEffect(() => {
    const view = viewRef.current;
    if (!view || !useCodeEditor || isMergeActiveRef.current) return;
    const content = activeFileContent ?? "";
    const currentContent = view.state.doc.toString();
    if (currentContent !== content) {
      view.dispatch({
        changes: { from: 0, to: currentContent.length, insert: content },
      });
    }
  }, [activeFileContent, useCodeEditor]);

  // Watch for proposed changes → activate/deactivate/update merge view
  useEffect(() => {
    const view = viewRef.current;
    log.debug("effect fired", {
      hasView: !!view,
      isTextFile: useCodeEditor,
      activeFileChange: activeFileChange
        ? { id: activeFileChange.id, filePath: activeFileChange.filePath }
        : null,
      isMergeActive: isMergeActiveRef.current,
      pendingId: pendingChangeRef.current?.id,
    });
    if (!view || !useCodeEditor) return;

    if (activeFileChange && !isMergeActiveRef.current) {
      // Activate merge view: load newContent + enable merge extension in ONE atomic dispatch
      log.debug(`ACTIVATING merge view for: ${activeFileChange.filePath}`);
      pendingChangeRef.current = activeFileChange;
      isMergeActiveRef.current = true;
      try {
        const scrollTop = view.scrollDOM.scrollTop;
        view.dispatch({
          changes: {
            from: 0,
            to: view.state.doc.length,
            insert: activeFileChange.newContent,
          },
          effects: mergeCompartmentRef.current.reconfigure(
            unifiedMergeView({
              original: activeFileChange.oldContent,
              highlightChanges: true,
              gutter: true,
              mergeControls: true,
            }),
          ),
          annotations: Transaction.addToHistory.of(false),
        });
        view.scrollDOM.scrollTop = scrollTop;
        log.debug("merge view activated successfully");
        // Auto-scroll to first chunk
        setTimeout(() => goToChunk(0), 50);
      } catch (err) {
        log.error("failed to activate merge view", { error: String(err) });
        isMergeActiveRef.current = false;
        pendingChangeRef.current = null;
      }
    } else if (
      activeFileChange &&
      isMergeActiveRef.current &&
      pendingChangeRef.current?.id !== activeFileChange.id
    ) {
      // Stacked edit: the change was updated while merge was already active.
      // Re-dispatch the merge view with the accumulated diff (original → latest).
      log.debug(
        `UPDATING merge view (stacked edit) for: ${activeFileChange.filePath}`,
      );
      pendingChangeRef.current = activeFileChange;
      try {
        const scrollTop = view.scrollDOM.scrollTop;
        view.dispatch({
          changes: {
            from: 0,
            to: view.state.doc.length,
            insert: activeFileChange.newContent,
          },
          effects: mergeCompartmentRef.current.reconfigure(
            unifiedMergeView({
              original: activeFileChange.oldContent,
              highlightChanges: true,
              gutter: true,
              mergeControls: true,
            }),
          ),
          annotations: Transaction.addToHistory.of(false),
        });
        view.scrollDOM.scrollTop = scrollTop;
        log.debug("merge view updated successfully (stacked edit)");
      } catch (err) {
        log.error("failed to update merge view", { error: String(err) });
      }
    } else if (!activeFileChange && isMergeActiveRef.current) {
      // Deactivate merge view (externally resolved)
      log.debug("DEACTIVATING merge view");
      view.dispatch({ effects: mergeCompartmentRef.current.reconfigure([]) });
      isMergeActiveRef.current = false;
      pendingChangeRef.current = null;
    }
  }, [activeFileChange, useCodeEditor]);

  useEffect(() => {
    const view = viewRef.current;
    if (!view || jumpToPosition === null) return;
    view.dispatch({
      selection: { anchor: jumpToPosition },
      effects: EditorView.scrollIntoView(jumpToPosition, { y: "center" }),
    });
    view.focus();
    clearJumpRequest();
  }, [jumpToPosition, clearJumpRequest]);

  // Selection toolbar: compute context label and container-relative position
  const selectionRange = useDocumentStore((s) => s.selectionRange);
  const selectionLabel = useMemo(() => {
    const view = viewRef.current;
    if (!selectionRange || !view || !activeFile) return null;
    try {
      const startLine = view.state.doc.lineAt(selectionRange.start);
      const endLine = view.state.doc.lineAt(selectionRange.end);
      const startCol = selectionRange.start - startLine.from + 1;
      const endCol = selectionRange.end - endLine.from + 1;
      const fileName = activeFile.relativePath;
      return `@${fileName}:${startLine.number}:${startCol}-${endLine.number}:${endCol}`;
    } catch {
      return null;
    }
  }, [selectionRange, activeFile]);

  const toolbarPosition = useMemo(() => {
    if (!selectionCoords || !parentRef.current) return null;
    const parentRect = parentRef.current.getBoundingClientRect();
    const relTop = selectionCoords.top - parentRect.top + 4; // 4px gap below selection
    const relLeft = Math.max(
      8,
      Math.min(
        selectionCoords.left - parentRect.left,
        parentRect.width - 272, // 264px toolbar + 8px margin
      ),
    );
    return { top: relTop, left: relLeft };
  }, [selectionCoords]);

  const handleToolbarSendPrompt = useCallback(
    (prompt: string) => {
      toolbarStickyRef.current = false;
      setCitationPanelOpen(false);
      setCitationDebugOpen(false);
      clearCitationState();
      setSelectionCoords(null);
      setSelectionRange(null);
      useAgentChatStore.getState().sendPrompt(prompt);
    },
    [clearCitationState, setSelectionRange],
  );

  const editorToolbarActions: ToolbarAction[] = useMemo(
    () => [
      {
        id: "citation-search",
        label: "Search Citation",
        icon: <SearchIcon className="size-4" />,
      },
      {
        id: "citation-debug",
        label: "Debug Citation",
        icon: <BugIcon className="size-4" />,
      },
      {
        id: "proofread",
        label: "Proofread",
        icon: <SpellCheckIcon className="size-4" />,
      },
    ],
    [],
  );

  const citationToolbarCandidates: CitationToolbarCandidate[] = useMemo(() => {
    const pool =
      citationReviewCandidates.length > 0
        ? citationReviewCandidates
        : citationResults.slice(0, 5);
    return pool.map((candidate, index) => ({
      key:
        candidate.paper_id ||
        candidate.doi ||
        `${candidate.title}-${candidate.year ?? "nd"}-${index}`,
      title: candidate.title,
      year: candidate.year,
      score: candidate.score,
    }));
  }, [citationResults, citationReviewCandidates]);

  const citationCandidateByKey = useMemo(() => {
    const pool =
      citationReviewCandidates.length > 0
        ? citationReviewCandidates
        : citationResults.slice(0, 5);
    const map = new Map<string, (typeof pool)[number]>();
    for (let i = 0; i < pool.length; i += 1) {
      const candidate = pool[i];
      const key =
        candidate.paper_id ||
        candidate.doi ||
        `${candidate.title}-${candidate.year ?? "nd"}-${i}`;
      map.set(key, candidate);
    }
    return map;
  }, [citationResults, citationReviewCandidates]);

  const copyCitationDebugPayload = useCallback(async (payload: unknown) => {
    const text = JSON.stringify(payload, null, 2);
    try {
      await navigator.clipboard.writeText(text);
      toast.success("Copied to clipboard.");
    } catch {
      toast.error("Failed to copy.");
    }
  }, []);

  const copyCitationDebugRaw = useCallback(async () => {
    if (!citationDebugInfo) return;
    await copyCitationDebugPayload(citationDebugInfo);
  }, [citationDebugInfo, copyCitationDebugPayload]);

  const copyCitationEvalSample = useCallback(async () => {
    if (!citationDebugInfo) return;
    const sample = buildEvalSample(citationDebugInfo, { dois: [], titles: [] });
    await copyCitationDebugPayload(sample);
  }, [citationDebugInfo, copyCitationDebugPayload]);

  const debugLabeledExpected = useMemo(() => {
    if (!citationDebugInfo) {
      return { dois: [], titles: [], no_match: false };
    }
    const selected = citationDebugInfo.merged_results.filter((candidate, index) =>
      selectedDebugLabelKeys.includes(buildDebugCandidateKey(candidate, index)),
    );
    const dois = uniqueKeepOrder(
      selected
        .map((candidate) =>
          typeof candidate.doi === "string" ? normalizeDoi(candidate.doi) : "",
        )
        .filter((doi) => doi.length > 0),
    );
    const titles = uniqueKeepOrder(
      selected
        .map((candidate) =>
          typeof candidate.title === "string" ? candidate.title.trim() : "",
        )
        .filter((title) => title.length > 0),
    );
    return { dois, titles, no_match: debugLabelNoMatch };
  }, [citationDebugInfo, debugLabelNoMatch, selectedDebugLabelKeys]);

  const toggleDebugCandidateLabel = useCallback(
    (candidate: CitationSearchDebug["merged_results"][number], index: number) => {
      const key = buildDebugCandidateKey(candidate, index);
      setDebugLabelNoMatch(false);
      setSelectedDebugLabelKeys((prev) =>
        prev.includes(key)
          ? prev.filter((item) => item !== key)
          : [...prev, key],
      );
    },
    [],
  );

  const selectDebugTop1 = useCallback(() => {
    if (!citationDebugInfo || citationDebugInfo.merged_results.length === 0) return;
    const top1Key = buildDebugCandidateKey(citationDebugInfo.merged_results[0], 0);
    setDebugLabelNoMatch(false);
    setSelectedDebugLabelKeys([top1Key]);
  }, [citationDebugInfo]);

  const markDebugNoMatch = useCallback(() => {
    setSelectedDebugLabelKeys([]);
    setDebugLabelNoMatch(true);
  }, []);

  const clearDebugLabelSelection = useCallback(() => {
    setSelectedDebugLabelKeys([]);
    setDebugLabelNoMatch(false);
  }, []);

  const copyCitationLabeledSelected = useCallback(async () => {
    if (!citationDebugInfo) return;
    if (
      debugLabeledExpected.dois.length === 0 &&
      debugLabeledExpected.titles.length === 0 &&
      !debugLabeledExpected.no_match
    ) {
      toast.error("Select at least one candidate or mark no_match first.");
      return;
    }
    const sample = buildEvalSample(citationDebugInfo, debugLabeledExpected);
    await copyCitationDebugPayload(sample);
  }, [citationDebugInfo, copyCitationDebugPayload, debugLabeledExpected]);

  const appendCitationLabeledToLocalDataset = useCallback(async () => {
    if (!projectRoot) {
      toast.error("Open a project first.");
      return;
    }
    if (!citationDebugInfo) {
      toast.error("No debug sample available.");
      return;
    }
    if (
      debugLabeledExpected.dois.length === 0 &&
      debugLabeledExpected.titles.length === 0 &&
      !debugLabeledExpected.no_match
    ) {
      toast.error("Select at least one candidate or mark no_match first.");
      return;
    }
    const sample = buildEvalSample(citationDebugInfo, debugLabeledExpected);
    try {
      const savedPath = await appendJsonLineToProject(
        projectRoot,
        ".workflow-local/citation_eval_samples.jsonl",
        sample,
      );
      toast.success(`Saved to ${savedPath}`);
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error);
      toast.error(`Failed to save sample: ${message}`);
    }
  }, [citationDebugInfo, debugLabeledExpected, projectRoot]);

  const handleToolbarAction = useCallback(
    (actionId: string) => {
      if (actionId === "citation-search") {
        // Keep selection available until search reads it from store.
        toolbarStickyRef.current = true;
        setCitationPanelOpen(true);
        setCitationDebugOpen(false);
        void searchCitationFromSelection();
        return;
      }
      if (actionId === "citation-debug") {
        toolbarStickyRef.current = true;
        setCitationPanelOpen(false);
        setCitationDebugOpen(true);
        void runCitationDebugFromSelection();
        return;
      }
      toolbarStickyRef.current = false;
      setCitationPanelOpen(false);
      setCitationDebugOpen(false);
      clearCitationState();
      setSelectionCoords(null);
      setSelectionRange(null);
      if (actionId === "proofread") {
        useAgentChatStore
          .getState()
          .sendPrompt(
            "Proofread and fix any errors in this text",
            undefined,
            SELECTION_EDIT_PROFILE,
          );
      }
    },
    [
      clearCitationState,
      runCitationDebugFromSelection,
      searchCitationFromSelection,
      setSelectionRange,
    ],
  );

  const handleToolbarDismiss = useCallback(() => {
    if (citationPanelOpen && (citationIsSearching || citationIsApplying)) return;
    toolbarStickyRef.current = false;
    setCitationPanelOpen(false);
    setCitationDebugOpen(false);
    clearCitationState();
    setSelectionCoords(null);
    setSelectionRange(null);
  }, [
    citationIsApplying,
    citationIsSearching,
    citationPanelOpen,
    clearCitationState,
    setSelectionRange,
  ]);

  // History review action handlers
  const handleHistoryRestore = useCallback(async () => {
    if (!reviewingSnapshot || !projectRoot) return;
    useHistoryStore.getState().stopReview();
    await useHistoryStore
      .getState()
      .restoreSnapshot(projectRoot, reviewingSnapshot.id);
    await useDocumentStore.getState().openProject(projectRoot);
    await useHistoryStore.getState().loadSnapshots(projectRoot);
  }, [reviewingSnapshot, projectRoot]);

  const [historyLabelDialogOpen, setHistoryLabelDialogOpen] = useState(false);
  const [historyLabelValue, setHistoryLabelValue] = useState("");

  const handleHistoryAddLabel = useCallback(async () => {
    const label = historyLabelValue.trim();
    if (!label || !reviewingSnapshot || !projectRoot) return;
    await useHistoryStore
      .getState()
      .addLabel(projectRoot, reviewingSnapshot.id, label);
    setHistoryLabelDialogOpen(false);
    setHistoryLabelValue("");
  }, [reviewingSnapshot, projectRoot, historyLabelValue]);

  const handleHistoryCopySha = useCallback(() => {
    if (!reviewingSnapshot) return;
    navigator.clipboard.writeText(reviewingSnapshot.id);
  }, [reviewingSnapshot]);

  const handleHistoryClose = useCallback(() => {
    useHistoryStore.getState().stopReview();
  }, []);

  const isPdf = activeFile?.type === "pdf";
  const isImage = !isTextFile && !isPdf && !!activeFile;
  const handleImportDocxAsEditable = useCallback(
    async (content: string, sourceFile: ProjectFile) => {
      if (!projectRoot) {
        throw new Error("Project root is missing");
      }
      const sourceRelativePath = sourceFile.relativePath;
      const lastSlash = sourceRelativePath.lastIndexOf("/");
      const folder = lastSlash >= 0 ? sourceRelativePath.slice(0, lastSlash) : "";
      const sourceName = sourceFile.name.replace(/\.docx$/i, "");
      const targetBaseName = `${sourceName || "imported"}.editable.md`;
      const targetRelativePath = folder
        ? `${folder}/${targetBaseName}`
        : targetBaseName;
      const uniqueRelativePath = await getUniqueTargetName(
        projectRoot,
        targetRelativePath,
      );
      await createFileOnDisk(projectRoot, uniqueRelativePath, content);
      await refreshFiles();
      setActiveFile(uniqueRelativePath);
      toast.success("Imported editable copy", {
        description: uniqueRelativePath,
      });
    },
    [projectRoot, refreshFiles, setActiveFile],
  );

  return (
    <div className="flex h-full flex-col bg-background">
      {/* Toolbar — adapts to file type */}
      <EditorToolbar
        editorView={viewRef}
        fileType={isPdf || isImage ? "image" : undefined}
        imageScale={isPdf || isImage ? imageScale : undefined}
        onImageScaleChange={isPdf || isImage ? setImageScale : undefined}
        cropMode={isImage ? cropMode : undefined}
        onCropToggle={isImage ? () => setCropMode((v) => !v) : undefined}
      />
      {/* Text-editor-only panels */}
      {useCodeEditor && !isLargeFileNotLoaded && isSearchOpen && (
        <SearchPanel
          searchQuery={searchQuery}
          onSearchQueryChange={setSearchQuery}
          onClose={() => {
            setIsSearchOpen(false);
            setSearchQuery("");
            viewRef.current?.focus();
          }}
          onFindNext={handleFindNext}
          onFindPrevious={handleFindPrevious}
          matchCount={matchCount}
          currentMatch={currentMatch}
        />
      )}
      {useCodeEditor && !isLargeFileNotLoaded && reviewingSnapshot && (
        <div className="flex h-9 shrink-0 items-center justify-between border-border border-b bg-amber-500/10 px-3">
          <div className="flex items-center gap-2 text-xs">
            <RotateCcwIcon className="size-3.5 text-amber-600 dark:text-amber-400" />
            <span className="font-medium text-amber-700 dark:text-amber-300">
              Reviewing history
            </span>
            <span className="text-muted-foreground">
              {reviewingSnapshot.message.replace(/^\[.*?\]\s*/, "")} &middot;{" "}
              {reviewingSnapshot.id.slice(0, 7)}
            </span>
          </div>
          <div className="flex items-center gap-1">
            <Button
              variant="ghost"
              size="sm"
              className="h-6 gap-1 px-2 text-xs"
              onClick={handleHistoryRestore}
            >
              <RotateCcwIcon className="size-3" />
              Restore
            </Button>
            <Button
              variant="ghost"
              size="sm"
              className="h-6 gap-1 px-2 text-xs"
              onClick={() => {
                setHistoryLabelDialogOpen(true);
                setHistoryLabelValue("");
              }}
            >
              <TagIcon className="size-3" />
              Label
            </Button>
            <Button
              variant="ghost"
              size="sm"
              className="h-6 gap-1 px-2 text-xs"
              onClick={handleHistoryCopySha}
            >
              <CopyIcon className="size-3" />
              SHA
            </Button>
            <div className="mx-0.5 h-4 w-px bg-border" />
            <Button
              variant="ghost"
              size="icon"
              className="size-6"
              onClick={handleHistoryClose}
            >
              <XIcon className="size-3.5" />
            </Button>
          </div>
        </div>
      )}
      {/* Main content area — single wrapper keeps AgentChatDrawer stable */}
      <div
        ref={
          isPdf ||
          isImage ||
          (isMarkdownFile && !isEditableMarkdownFile) ||
          isDocxFile
            ? undefined
            : parentRef
        }
        className="relative flex min-h-0 flex-1 flex-col overflow-hidden"
      >
        {/* PDF content */}
        {isPdf && activeFile && (
          <InlinePdfContent
            file={activeFile}
            imageScale={imageScale}
            onImageScaleChange={setImageScale}
          />
        )}
        {/* Image content */}
        {isImage && activeFile && (
          <ImagePreview
            file={activeFile}
            scale={imageScale}
            onScaleChange={setImageScale}
            cropMode={cropMode}
            onCropModeChange={setCropMode}
          />
        )}
        {/* Markdown preview */}
        {isMarkdownFile && !isEditableMarkdownFile && activeFile && (
          <MarkdownPreview content={activeFileContent ?? ""} />
        )}
        {/* DOCX preview */}
        {isDocxFile && activeFile && (
          <DocxPreview
            file={activeFile}
            onImportEditable={async (content) =>
              handleImportDocxAsEditable(content, activeFile)
            }
          />
        )}
        {/* Large file warning */}
        {isLargeFileNotLoaded && activeFile && (
          <div className="flex flex-1 flex-col items-center justify-center gap-4 p-8 text-center">
            <div className="max-w-md rounded-lg border border-border bg-card/50 p-6 shadow-sm">
              <p className="mb-1 font-medium text-foreground text-sm">
                {activeFile.name}
              </p>
              <p className="mb-4 text-muted-foreground text-xs">
                This file is large (
                {activeFile.fileSize != null
                  ? `${(activeFile.fileSize / (1024 * 1024)).toFixed(1)} MB`
                  : "unknown size"}
                ). Opening it may slow down the editor.
              </p>
              <Button
                variant="outline"
                size="sm"
                onClick={() => loadFileContent(activeFile.id)}
              >
                Open Anyway
              </Button>
            </div>
          </div>
        )}
        {/* Text editor content */}
        {useCodeEditor && !isLargeFileNotLoaded && (
          <>
            <div
              ref={containerRef}
              className={reviewingSnapshot ? "hidden" : "absolute inset-0"}
            />
            {reviewingSnapshot && historyDiffResult && (
              <HistoryDiffView diffs={historyDiffResult} />
            )}
            {toolbarPosition &&
              selectionLabel &&
              !isMergeActiveRef.current &&
              !isSearchOpen &&
              !citationDebugOpen && (
                <SelectionToolbar
                  position={toolbarPosition}
                  contextLabel={selectionLabel}
                  actions={editorToolbarActions}
                  citation={{
                    active: citationPanelOpen,
                    isSearching: citationIsSearching,
                    isApplying: citationIsApplying,
                    error: citationError,
                    decisionHint: citationDecisionHint,
                    lastAutoAppliedTitle: citationLastAutoAppliedTitle,
                    lastInsertedCitekey: citationLastInsertedCitekey,
                    candidates: citationToolbarCandidates,
                    onCite: (key) => {
                      const candidate = citationCandidateByKey.get(key);
                      if (!candidate) return;
                      void applyCitationCandidate(candidate);
                    },
                    onRetry: () => {
                      void searchCitationFromSelection();
                    },
                    onClose: () => {
                      toolbarStickyRef.current = false;
                      setCitationPanelOpen(false);
                      clearCitationState();
                      setSelectionCoords(null);
                      setSelectionRange(null);
                    },
                  }}
                  onSendPrompt={handleToolbarSendPrompt}
                  onAction={handleToolbarAction}
                  onDismiss={handleToolbarDismiss}
                />
              )}
            {activeFileChange && mergeChunkInfo.total > 0 && (
              <div className="absolute top-3 right-3 z-20 flex items-center gap-1 rounded-lg border border-border bg-background/95 px-2 py-1 shadow-lg backdrop-blur-sm">
                <span className="px-1 font-mono text-muted-foreground text-xs">
                  ±&nbsp;{mergeChunkInfo.current}/{mergeChunkInfo.total}
                </span>
                <div className="mx-0.5 h-4 w-px bg-border" />
                <button
                  onClick={() =>
                    goToChunk(
                      mergeChunkInfo.current <= 1
                        ? mergeChunkInfo.total - 1
                        : mergeChunkInfo.current - 2,
                    )
                  }
                  className="rounded p-0.5 text-muted-foreground transition-colors hover:bg-white/10 hover:text-foreground"
                  title="Previous change"
                  aria-label="Previous change"
                >
                  <svg
                    width="14"
                    height="14"
                    viewBox="0 0 24 24"
                    fill="none"
                    stroke="currentColor"
                    strokeWidth="2"
                    strokeLinecap="round"
                    strokeLinejoin="round"
                  >
                    <polyline points="18 15 12 9 6 15" />
                  </svg>
                </button>
                <button
                  onClick={() =>
                    goToChunk(
                      mergeChunkInfo.current >= mergeChunkInfo.total
                        ? 0
                        : mergeChunkInfo.current,
                    )
                  }
                  className="rounded p-0.5 text-muted-foreground transition-colors hover:bg-white/10 hover:text-foreground"
                  title="Next change"
                  aria-label="Next change"
                >
                  <svg
                    width="14"
                    height="14"
                    viewBox="0 0 24 24"
                    fill="none"
                    stroke="currentColor"
                    strokeWidth="2"
                    strokeLinecap="round"
                    strokeLinejoin="round"
                  >
                    <polyline points="6 9 12 15 18 9" />
                  </svg>
                </button>
                <div className="mx-0.5 h-4 w-px bg-border" />
                <button
                  onClick={acceptCurrentChunk}
                  className="rounded p-0.5 text-green-400 transition-colors hover:bg-green-600/20"
                  title="Accept this change"
                  aria-label="Accept this change"
                >
                  <svg
                    width="14"
                    height="14"
                    viewBox="0 0 24 24"
                    fill="none"
                    stroke="currentColor"
                    strokeWidth="2"
                    strokeLinecap="round"
                    strokeLinejoin="round"
                  >
                    <polyline points="20 6 9 17 4 12" />
                  </svg>
                </button>
                <button
                  onClick={rejectCurrentChunk}
                  className="rounded p-0.5 text-red-400 transition-colors hover:bg-red-600/20"
                  title="Reject this change"
                  aria-label="Reject this change"
                >
                  <svg
                    width="14"
                    height="14"
                    viewBox="0 0 24 24"
                    fill="none"
                    stroke="currentColor"
                    strokeWidth="2"
                    strokeLinecap="round"
                    strokeLinejoin="round"
                  >
                    <line x1="18" y1="6" x2="6" y2="18" />
                    <line x1="6" y1="6" x2="18" y2="18" />
                  </svg>
                </button>
              </div>
            )}
          </>
        )}
        {/* Chat drawer — single stable instance across all file types */}
        <AgentChatDrawer />
      </div>
      {/* Text-editor-only bottom panels */}
      {!isPdf &&
        useCodeEditor &&
        !isLargeFileNotLoaded &&
        diagnostics.length > 0 && (
          <ProblemsPanel
            diagnostics={diagnostics}
            fileName={activeFile?.relativePath ?? "main.tex"}
            onNavigate={(from) => {
              const view = viewRef.current;
              if (!view) return;
              view.dispatch({
                selection: { anchor: from },
                effects: EditorView.scrollIntoView(from, { y: "center" }),
              });
              view.focus();
            }}
            onFixWithChat={(message, line) => {
              const fileName = activeFile?.relativePath ?? "main.tex";
              const ctx = `[Lint error in ${fileName}:${line}]\n[Error: ${message}]`;
              useAgentChatStore
                .getState()
                .sendPrompt(
                  `${ctx}\n\nFix this lint error.`,
                  undefined,
                  FILE_EDIT_PROFILE,
                );
            }}
            onFixAllWithChat={() => {
              const fileName = activeFile?.relativePath ?? "main.tex";
              const errorList = diagnostics
                .map((d) => `- ${fileName}:${d.line} — ${d.message}`)
                .join("\n");
              useAgentChatStore
                .getState()
                .sendPrompt(
                  `[Lint errors in ${fileName}]\n${errorList}\n\nFix all these lint errors.`,
                  undefined,
                  FILE_EDIT_PROFILE,
                );
            }}
          />
        )}
      {useCodeEditor && !isLargeFileNotLoaded && activeFileChange && (
        <ProposedChangesPanel
          change={activeFileChange}
          changeIndex={proposedChanges.findIndex(
            (c) => c.filePath === activeFile?.relativePath,
          )}
          totalChanges={proposedChanges.length}
          onKeep={() => handleKeepAllRef.current()}
          onUndo={() => handleUndoAllRef.current()}
        />
      )}
      {/* Citation debug dialog */}
      <Dialog
        open={citationDebugOpen}
        onOpenChange={(open) => {
          setCitationDebugOpen(open);
          if (!open) {
            toolbarStickyRef.current = false;
            clearCitationState();
            setSelectionCoords(null);
            setSelectionRange(null);
          }
        }}
      >
        <DialogContent className="max-h-[80vh] overflow-y-auto sm:max-w-3xl">
          <DialogHeader>
            <DialogTitle>Citation Debug</DialogTitle>
          </DialogHeader>
          {citationIsDebugSearching && (
            <div className="flex items-center gap-2 text-muted-foreground text-sm">
              <LoaderIcon className="size-4 animate-spin" />
              Running citation debug search...
            </div>
          )}
          {!citationIsDebugSearching && !citationDebugInfo && (
            <p className="text-muted-foreground text-sm">No debug data yet.</p>
          )}
          {!citationIsDebugSearching && citationDebugInfo && (
            <div className="space-y-3 text-xs">
              <div className="flex flex-wrap items-center gap-1.5">
                <Button
                  variant="outline"
                  size="sm"
                  className="h-7 text-xs"
                  onClick={() => void copyCitationDebugRaw()}
                >
                  <CopyIcon className="mr-1 size-3.5" />
                  Export Raw (Copy)
                </Button>
                <Button
                  variant="outline"
                  size="sm"
                  className="h-7 text-xs"
                  onClick={() => void copyCitationEvalSample()}
                >
                  Export Eval (Copy)
                </Button>
                <Button
                  variant="outline"
                  size="sm"
                  className="h-7 text-xs"
                  onClick={selectDebugTop1}
                  disabled={citationDebugInfo.merged_results.length === 0}
                >
                  Select Top1
                </Button>
                <Button
                  variant="outline"
                  size="sm"
                  className="h-7 text-xs"
                  onClick={markDebugNoMatch}
                  disabled={debugLabelNoMatch}
                >
                  Mark No Match
                </Button>
                <Button
                  variant="outline"
                  size="sm"
                  className="h-7 text-xs"
                  onClick={clearDebugLabelSelection}
                  disabled={
                    selectedDebugLabelKeys.length === 0 && !debugLabelNoMatch
                  }
                >
                  Clear
                </Button>
                <Button
                  variant="outline"
                  size="sm"
                  className="h-7 text-xs"
                  onClick={() => void copyCitationLabeledSelected()}
                  disabled={
                    debugLabeledExpected.dois.length === 0 &&
                    debugLabeledExpected.titles.length === 0 &&
                    !debugLabeledExpected.no_match
                  }
                >
                  Export Labeled (Copy)
                </Button>
                <Button
                  variant="outline"
                  size="sm"
                  className="h-7 text-xs"
                  onClick={() => void appendCitationLabeledToLocalDataset()}
                  disabled={
                    !projectRoot ||
                    (debugLabeledExpected.dois.length === 0 &&
                      debugLabeledExpected.titles.length === 0 &&
                      !debugLabeledExpected.no_match)
                  }
                >
                  Append Local Dataset
                </Button>
              </div>

              <div className="rounded border border-border px-2 py-1.5">
                <div className="font-medium">Summary</div>
                <div className="text-muted-foreground">
                  latency={citationDebugInfo.latency_ms}ms · merged=
                  {citationDebugInfo.merged_results.length} · stop=
                  {citationDebugInfo.stop_reason ?? "n/a"}
                </div>
                <div className="mt-1 text-muted-foreground">
                  need={citationDebugInfo.need_decision.level} · type=
                  {citationDebugInfo.need_decision.claim_type} · refs≈
                  {citationDebugInfo.need_decision.recommended_refs} · score=
                  {citationDebugInfo.need_decision.score.toFixed(2)}
                </div>
                <div className="mt-1 text-muted-foreground break-words">
                  why: {citationDebugInfo.need_decision.reasons.join("; ")}
                </div>
                <div className="mt-1 text-muted-foreground">
                  exec: topN={citationDebugInfo.query_execution_top_n}, selected=
                  {citationDebugInfo.query_execution_selected_count}, minQ=
                  {citationDebugInfo.query_execution_min_quality.toFixed(2)}, hitRatio=
                  {citationDebugInfo.query_execution_min_hit_ratio.toFixed(2)}
                </div>
              </div>

              <div className="rounded border border-border px-2 py-1.5">
                <div className="font-medium">Query Plan</div>
                <div className="mt-1 space-y-1">
                  {citationDebugInfo.query_plan.length === 0 ? (
                    <div className="text-muted-foreground">No query plan.</div>
                  ) : (
                    citationDebugInfo.query_plan.map((item, index) => (
                      <div key={`${index}-${item.query}`}>
                        {index + 1}. [{item.source}/{item.strategy}] q=
                        {item.quality.total.toFixed(3)} · {item.query}
                      </div>
                    ))
                  )}
                </div>
              </div>

              <div className="rounded border border-border px-2 py-1.5">
                <div className="font-medium">Provider Attempts</div>
                <div className="mt-1 space-y-1">
                  {citationDebugInfo.attempts.length === 0 ? (
                    <div className="text-muted-foreground">No attempts recorded.</div>
                  ) : (
                    citationDebugInfo.attempts.map((attempt, index) => (
                      <div key={`${index}-${attempt.provider}-${attempt.query}`}>
                        [{attempt.provider}] {attempt.query} ·{" "}
                        {attempt.ok
                          ? `${attempt.result_count} results`
                          : `failed${attempt.error ? `: ${attempt.error}` : ""}`}
                      </div>
                    ))
                  )}
                </div>
              </div>

              <div className="rounded border border-border px-2 py-1.5">
                <div className="font-medium">Merged Results</div>
                <div className="mt-1 space-y-1">
                  {citationDebugInfo.merged_results.length === 0 ? (
                    <div className="text-muted-foreground">No merged results.</div>
                  ) : (
                    citationDebugInfo.merged_results.map((candidate, index) => (
                      <div
                        key={`${index}-${candidate.paper_id}`}
                        className="rounded border border-border/60 px-2 py-1"
                      >
                        <div className="flex items-start justify-between gap-2">
                          <div>
                            {index + 1}. {candidate.title} ·{" "}
                            {Math.round(candidate.score * 100)}%
                            {candidate.doi
                              ? ` · ${normalizeDoi(candidate.doi)}`
                              : ""}
                          </div>
                          <button
                            type="button"
                            className="rounded border border-border px-2 py-0.5 text-[11px] text-muted-foreground transition-colors hover:bg-muted hover:text-foreground"
                            onClick={() =>
                              toggleDebugCandidateLabel(candidate, index)
                            }
                          >
                            {selectedDebugLabelKeys.includes(
                              buildDebugCandidateKey(candidate, index),
                            )
                              ? "Selected"
                              : "Select"}
                          </button>
                        </div>
                      </div>
                    ))
                  )}
                </div>
              </div>

              <div className="rounded border border-border px-2 py-1.5">
                <div className="font-medium">Evaluation Labeling</div>
                <div className="mt-1 text-muted-foreground">
                  selected candidates: {selectedDebugLabelKeys.length}
                </div>
                <div className="mt-1 text-muted-foreground">
                  expected.no_match: {debugLabeledExpected.no_match ? "true" : "false"}
                </div>
                <div className="mt-1 text-muted-foreground break-words">
                  expected.dois:{" "}
                  {debugLabeledExpected.dois.length > 0
                    ? debugLabeledExpected.dois.join(", ")
                    : "(empty)"}
                </div>
                <div className="mt-1 text-muted-foreground break-words">
                  expected.titles:{" "}
                  {debugLabeledExpected.titles.length > 0
                    ? debugLabeledExpected.titles.join(" | ")
                    : "(empty)"}
                </div>
              </div>
            </div>
          )}
        </DialogContent>
      </Dialog>
      {/* History label dialog */}
      <Dialog
        open={historyLabelDialogOpen}
        onOpenChange={setHistoryLabelDialogOpen}
      >
        <DialogContent className="sm:max-w-sm">
          <DialogHeader>
            <DialogTitle>Add Label</DialogTitle>
          </DialogHeader>
          <div className="py-4">
            <Input
              placeholder="e.g. Draft v1"
              value={historyLabelValue}
              onChange={(e) => setHistoryLabelValue(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "Enter") handleHistoryAddLabel();
              }}
              autoFocus
            />
          </div>
          <DialogFooter>
            <Button
              variant="outline"
              onClick={() => setHistoryLabelDialogOpen(false)}
            >
              Cancel
            </Button>
            <Button
              onClick={handleHistoryAddLabel}
              disabled={!historyLabelValue.trim()}
            >
              Add
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </div>
  );
}

// ─── Inline PDF Content (data loading + MuPDF PdfViewer) ───

function InlinePdfContent({
  file,
  imageScale,
  onImageScaleChange,
}: {
  file: ProjectFile;
  imageScale: number;
  onImageScaleChange: (scale: number) => void;
}) {
  const [pdfData, setPdfData] = useState<Uint8Array | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [fitted, setFitted] = useState(false);
  const wrapperRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    let cancelled = false;
    setPdfData(null);
    setError(null);
    setFitted(false);

    readFile(file.absolutePath)
      .then((data) => {
        if (!cancelled) setPdfData(new Uint8Array(data));
      })
      .catch((err) => {
        if (!cancelled) setError(String(err));
      });

    return () => {
      cancelled = true;
    };
  }, [file.absolutePath]);

  const handleFirstPageSize = useCallback(
    (pageWidth: number) => {
      const containerWidth = wrapperRef.current?.clientWidth;
      if (!containerWidth || !onImageScaleChange) return;
      const fitScale = (containerWidth - 32) / pageWidth; // 32px padding
      onImageScaleChange(Math.max(0.25, Math.min(2, fitScale)));
      setFitted(true);
    },
    [onImageScaleChange],
  );

  if (pdfData) {
    return (
      <div
        ref={wrapperRef}
        className="flex min-h-0 flex-1 flex-col"
        style={{ opacity: fitted ? 1 : 0 }}
      >
        <PdfViewer
          data={pdfData}
          scale={imageScale}
          onScaleChange={onImageScaleChange}
          onFirstPageSize={handleFirstPageSize}
        />
      </div>
    );
  }

  if (error) {
    return (
      <div className="flex h-full items-center justify-center text-muted-foreground text-sm">
        Failed to load PDF: {error}
      </div>
    );
  }

  return (
    <div className="flex h-full items-center justify-center text-muted-foreground text-sm">
      Loading PDF...
    </div>
  );
}

// ─── History Diff View (git-diff style combined view) ───

function HistoryDiffView({ diffs }: { diffs: FileDiff[] }) {
  return (
    <div className="absolute inset-0 overflow-y-auto bg-background font-mono text-xs leading-relaxed">
      {diffs.map((diff) => (
        <div key={diff.file_path} className="border-border border-b">
          {/* File header */}
          <div className="sticky top-0 z-10 flex items-center gap-2 border-border border-b bg-muted/80 px-4 py-1.5 backdrop-blur-sm">
            <span
              className={
                diff.status === "added"
                  ? "font-bold text-green-600 dark:text-green-400"
                  : diff.status === "deleted"
                    ? "font-bold text-red-600 dark:text-red-400"
                    : "font-bold text-blue-600 dark:text-blue-400"
              }
            >
              {diff.status === "added"
                ? "+"
                : diff.status === "deleted"
                  ? "−"
                  : "~"}
            </span>
            <span className="font-medium text-foreground">
              {diff.file_path}
            </span>
            <span className="text-muted-foreground">({diff.status})</span>
          </div>
          {/* Diff lines */}
          <DiffLines diff={diff} />
        </div>
      ))}
      {diffs.length === 0 && (
        <div className="flex h-full items-center justify-center text-muted-foreground">
          No changes in this snapshot
        </div>
      )}
    </div>
  );
}

function DiffLines({ diff }: { diff: FileDiff }) {
  const oldLines = diff.old_content?.split("\n") ?? [];
  const newLines = diff.new_content?.split("\n") ?? [];

  if (diff.status === "added") {
    return (
      <div className="px-1">
        {newLines.map((line, i) => (
          <div key={i} className="flex bg-green-500/10">
            <span className="w-12 shrink-0 select-none pr-2 text-right text-green-500/50">
              {i + 1}
            </span>
            <span className="mr-1 select-none text-green-500/50">+</span>
            <span className="text-green-700 dark:text-green-400">
              {line || " "}
            </span>
          </div>
        ))}
      </div>
    );
  }

  if (diff.status === "deleted") {
    return (
      <div className="px-1">
        {oldLines.map((line, i) => (
          <div key={i} className="flex bg-red-500/10">
            <span className="w-12 shrink-0 select-none pr-2 text-right text-red-500/50">
              {i + 1}
            </span>
            <span className="mr-1 select-none text-red-500/50">−</span>
            <span className="text-red-700 dark:text-red-400">
              {line || " "}
            </span>
          </div>
        ))}
      </div>
    );
  }

  // Modified: compute unified diff with context
  const hunks = computeUnifiedHunks(oldLines, newLines, 3);

  return (
    <div className="px-1">
      {hunks.map((hunk, hi) => (
        <div key={hi}>
          {/* Hunk header */}
          <div className="bg-blue-500/10 px-1 text-blue-600 dark:text-blue-400">
            @@ -{hunk.oldStart},{hunk.oldCount} +{hunk.newStart},{hunk.newCount}{" "}
            @@
          </div>
          {hunk.lines.map((line, li) => (
            <div
              key={li}
              className={
                line.type === "del"
                  ? "flex bg-red-500/10"
                  : line.type === "add"
                    ? "flex bg-green-500/10"
                    : "flex"
              }
            >
              <span
                className={`w-12 shrink-0 select-none pr-2 text-right ${
                  line.type === "del"
                    ? "text-red-500/50"
                    : line.type === "add"
                      ? "text-green-500/50"
                      : "text-muted-foreground/50"
                }`}
              >
                {line.type !== "add" ? line.oldNum : ""}
              </span>
              <span
                className={`w-12 shrink-0 select-none pr-2 text-right ${
                  line.type === "del"
                    ? "text-red-500/50"
                    : line.type === "add"
                      ? "text-green-500/50"
                      : "text-muted-foreground/50"
                }`}
              >
                {line.type !== "del" ? line.newNum : ""}
              </span>
              <span
                className={`mr-1 select-none ${
                  line.type === "del"
                    ? "text-red-500/50"
                    : line.type === "add"
                      ? "text-green-500/50"
                      : "text-muted-foreground/30"
                }`}
              >
                {line.type === "del" ? "−" : line.type === "add" ? "+" : " "}
              </span>
              <span
                className={
                  line.type === "del"
                    ? "text-red-700 dark:text-red-400"
                    : line.type === "add"
                      ? "text-green-700 dark:text-green-400"
                      : "text-muted-foreground"
                }
              >
                {line.text || " "}
              </span>
            </div>
          ))}
        </div>
      ))}
    </div>
  );
}

interface DiffLine {
  type: "ctx" | "del" | "add";
  text: string;
  oldNum?: number;
  newNum?: number;
}

interface Hunk {
  oldStart: number;
  oldCount: number;
  newStart: number;
  newCount: number;
  lines: DiffLine[];
}

function computeUnifiedHunks(
  oldLines: string[],
  newLines: string[],
  context: number,
): Hunk[] {
  // Simple line-by-line diff to find changed regions
  const ops: {
    type: "eq" | "del" | "add";
    oldIdx?: number;
    newIdx?: number;
    text: string;
  }[] = [];
  let i = 0;
  let j = 0;

  while (i < oldLines.length || j < newLines.length) {
    if (
      i < oldLines.length &&
      j < newLines.length &&
      oldLines[i] === newLines[j]
    ) {
      ops.push({ type: "eq", oldIdx: i, newIdx: j, text: oldLines[i] });
      i++;
      j++;
    } else {
      // Find the next matching line
      let foundOld = -1;
      let foundNew = -1;
      const searchLimit = Math.min(
        50,
        Math.max(oldLines.length - i, newLines.length - j),
      );
      for (let look = 1; look <= searchLimit; look++) {
        if (
          i + look < oldLines.length &&
          j < newLines.length &&
          oldLines[i + look] === newLines[j]
        ) {
          foundOld = i + look;
          break;
        }
        if (
          j + look < newLines.length &&
          i < oldLines.length &&
          newLines[j + look] === oldLines[i]
        ) {
          foundNew = j + look;
          break;
        }
      }

      if (foundOld >= 0) {
        // Delete lines from old until match
        while (i < foundOld) {
          ops.push({ type: "del", oldIdx: i, text: oldLines[i] });
          i++;
        }
      } else if (foundNew >= 0) {
        // Add lines from new until match
        while (j < foundNew) {
          ops.push({ type: "add", newIdx: j, text: newLines[j] });
          j++;
        }
      } else {
        // No match found nearby, emit del+add
        if (i < oldLines.length) {
          ops.push({ type: "del", oldIdx: i, text: oldLines[i] });
          i++;
        }
        if (j < newLines.length) {
          ops.push({ type: "add", newIdx: j, text: newLines[j] });
          j++;
        }
      }
    }
  }

  // Group into hunks with context lines
  const changedIndices = new Set<number>();
  ops.forEach((op, idx) => {
    if (op.type !== "eq") {
      for (
        let c = Math.max(0, idx - context);
        c <= Math.min(ops.length - 1, idx + context);
        c++
      ) {
        changedIndices.add(c);
      }
    }
  });

  const hunks: Hunk[] = [];
  let currentHunk: Hunk | null = null;

  for (let idx = 0; idx < ops.length; idx++) {
    if (!changedIndices.has(idx)) {
      if (currentHunk) {
        hunks.push(currentHunk);
        currentHunk = null;
      }
      continue;
    }

    const op = ops[idx];
    if (!currentHunk) {
      const oldStart =
        op.type !== "add"
          ? (op.oldIdx ?? 0) + 1
          : (ops[idx + 1]?.oldIdx ?? 0) + 1;
      const newStart =
        op.type !== "del"
          ? (op.newIdx ?? 0) + 1
          : (ops[idx + 1]?.newIdx ?? 0) + 1;
      currentHunk = { oldStart, oldCount: 0, newStart, newCount: 0, lines: [] };
    }

    if (op.type === "eq") {
      currentHunk.lines.push({
        type: "ctx",
        text: op.text,
        oldNum: (op.oldIdx ?? 0) + 1,
        newNum: (op.newIdx ?? 0) + 1,
      });
      currentHunk.oldCount++;
      currentHunk.newCount++;
    } else if (op.type === "del") {
      currentHunk.lines.push({
        type: "del",
        text: op.text,
        oldNum: (op.oldIdx ?? 0) + 1,
      });
      currentHunk.oldCount++;
    } else {
      currentHunk.lines.push({
        type: "add",
        text: op.text,
        newNum: (op.newIdx ?? 0) + 1,
      });
      currentHunk.newCount++;
    }
  }
  if (currentHunk) hunks.push(currentHunk);

  return hunks;
}
