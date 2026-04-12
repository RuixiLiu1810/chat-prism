import {
  type FC,
  useCallback,
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import { createPortal } from "react-dom";
import {
  ArrowUpIcon,
  SquareIcon,
  XIcon,
  FileTextIcon,
  FileCodeIcon,
  FileIcon,
  ImageIcon,
  FileSpreadsheetIcon,
  CheckIcon,
  ChevronDownIcon,
  SparklesIcon,
  RabbitIcon,
} from "lucide-react";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import {
  writeFile,
  mkdir,
  exists,
} from "@tauri-apps/plugin-fs";
import { join } from "@tauri-apps/api/path";
import { invoke } from "@tauri-apps/api/core";
import {
  useAgentChatStore,
  offsetToLineCol,
  type AgentPromptContext,
} from "@/stores/agent-chat-store";
import { useDocumentStore, type ProjectFile } from "@/stores/document-store";
import { getUniqueTargetName } from "@/lib/tauri/fs";
import { TooltipIconButton } from "@/components/assistant-ui/tooltip-icon-button";
import { cn } from "@/lib/utils";
import { SlashCommandPicker, type SlashCommand } from "./slash-command-picker";
import { createLogger } from "@/lib/debug/logger";
import {
  buildPromptContextForProjectFile,
  findMentionedAttachmentPaths,
  findMentionedProjectFiles,
} from "@/lib/agent-prompt-context";

const log = createLogger("chat-composer");

// Re-export for other modules
export type { SlashCommand };

type PinnedContext = AgentPromptContext;

function inferFileTypeFromPath(path: string): ProjectFile["type"] {
  const lower = path.toLowerCase();
  if (lower.endsWith(".pdf")) return "pdf";
  if (/\.(png|jpe?g|gif|webp|svg|bmp|tiff?)$/i.test(lower)) return "image";
  if (lower.endsWith(".tex")) return "tex";
  if (lower.endsWith(".bib")) return "bib";
  if (lower.endsWith(".sty")) return "style";
  return "other";
}

function getFileIcon(file: ProjectFile) {
  if (file.type === "image")
    return <ImageIcon className="size-3.5 shrink-0 text-muted-foreground" />;
  if (file.type === "pdf")
    return (
      <FileSpreadsheetIcon className="size-3.5 shrink-0 text-muted-foreground" />
    );
  if (file.type === "style")
    return <FileCodeIcon className="size-3.5 shrink-0 text-muted-foreground" />;
  if (file.type === "other")
    return <FileIcon className="size-3.5 shrink-0 text-muted-foreground" />;
  return <FileTextIcon className="size-3.5 shrink-0 text-muted-foreground" />;
}

export const ChatComposer: FC<{ isOpen?: boolean }> = ({ isOpen }) => {
  const sendPrompt = useAgentChatStore((s) => s.sendPrompt);
  const cancelExecution = useAgentChatStore((s) => s.cancelExecution);
  const isStreaming = useAgentChatStore((s) => s.isStreaming);
  const selectedModel = useAgentChatStore((s) => s.selectedModel);
  const setSelectedModel = useAgentChatStore((s) => s.setSelectedModel);
  const activeTabId = useAgentChatStore((s) => s.activeTabId);
  const [input, setInput] = useState("");
  const editorRef = useRef<HTMLElement>(null);

  // ── contenteditable helpers ──────────────────────────────────────────────
  /** Get plain text from the contenteditable div */
  const getEditorText = useCallback(() => {
    const el = editorRef.current;
    if (!el) return "";
    return el.innerText.replace(/\n$/, "");
  }, []);

  /** Move cursor to end of contenteditable */
  const moveCursorToEnd = useCallback((el: HTMLElement) => {
    const range = document.createRange();
    const sel = window.getSelection();
    range.selectNodeContents(el);
    range.collapse(false);
    sel?.removeAllRanges();
    sel?.addRange(range);
  }, []);

  /** Programmatically set contenteditable text and move cursor to end */
  const setEditorContent = useCallback((text: string) => {
    const el = editorRef.current;
    if (!el) return;
    el.innerText = text;
    if (text) moveCursorToEnd(el);
  }, [moveCursorToEnd]);

  /** Get text content before the cursor */
  const getTextBeforeCursor = useCallback(() => {
    const el = editorRef.current;
    if (!el) return "";
    const sel = window.getSelection();
    if (!sel || sel.rangeCount === 0) return "";
    const range = sel.getRangeAt(0);
    const pre = document.createRange();
    pre.selectNodeContents(el);
    pre.setEnd(range.startContainer, range.startOffset);
    return pre.toString();
  }, []);

  // Model picker state
  const [modelPickerOpen, setModelPickerOpen] = useState(false);
  const modelPickerRef = useRef<HTMLDivElement>(null);
  const modelButtonRef = useRef<HTMLButtonElement>(null);
  const [pickerPos, setPickerPos] = useState<{ left: number; bottom: number }>({
    left: 0,
    bottom: 0,
  });

  // Recalculate popup position when it opens
  useLayoutEffect(() => {
    if (!modelPickerOpen || !modelButtonRef.current) return;
    const rect = modelButtonRef.current.getBoundingClientRect();
    setPickerPos({
      left: rect.left,
      bottom: window.innerHeight - rect.top + 4,
    });
  }, [modelPickerOpen]);

  // Pinned contexts — supports multiple files/selections
  const [pinnedContexts, setPinnedContexts] = useState<PinnedContext[]>([]);

  const externalDropHintActiveRef = useRef(false);

  const setExternalDropHint = useCallback(
    (active: boolean, fileName?: string) => {
      if (
        externalDropHintActiveRef.current === active &&
        (active || fileName == null)
      ) {
        return;
      }
      externalDropHintActiveRef.current = active;
      window.dispatchEvent(
        new CustomEvent("claudeprism:chat-drop-hover", {
          detail: { active, fileName },
        }),
      );
    },
    [],
  );

  // @ mention state
  const [mentionQuery, setMentionQuery] = useState<string | null>(null);
  const [mentionIndex, setMentionIndex] = useState(0);
  const [mentionFiles, setMentionFiles] = useState<ProjectFile[]>([]);
  const mentionRef = useRef<HTMLDivElement>(null);

  // / slash command state
  const [slashQuery, setSlashQuery] = useState<string | null>(null);
  const slashSelectedRef = useRef(false); // true after user picks a command — suppresses re-open

  // Keep refs to latest input/pinnedContexts so the tab-switch effect can
  // save the draft without depending on these values (which would cause loops).
  const inputRef = useRef(input);
  inputRef.current = input;
  const pinnedContextsRef = useRef(pinnedContexts);
  pinnedContextsRef.current = pinnedContexts;

  // Save draft to previous tab, restore draft from new tab
  const prevTabIdRef = useRef(activeTabId);
  useEffect(() => {
    const prevTabId = prevTabIdRef.current;
    if (prevTabId !== activeTabId) {
      // Save current input to the *previous* tab's draft (using refs for latest values)
      useAgentChatStore.getState().saveDraft(prevTabId, {
        input: inputRef.current,
        pinnedContexts: pinnedContextsRef.current,
      });
    }
    prevTabIdRef.current = activeTabId;

    // Restore draft from the new active tab
    const tab = useAgentChatStore
      .getState()
      .tabs.find((t) => t.id === activeTabId);
    const draft = tab?.draft;
    const newInput = draft?.input ?? "";
    setInput(newInput);
    setPinnedContexts(draft?.pinnedContexts ?? []);
    setMentionQuery(null);
    setSlashQuery(null);
    // Sync contenteditable DOM to restored draft
    if (editorRef.current) {
      editorRef.current.innerText = newInput;
    }
  }, [activeTabId]);
  const [slashCommands, setSlashCommands] = useState<SlashCommand[]>([]);
  const composerRef = useRef<HTMLDivElement>(null);

  // Watch selection changes to auto-pin context
  const selectionRange = useDocumentStore((s) => s.selectionRange);
  const activeFileId = useDocumentStore((s) => s.activeFileId);
  const files = useDocumentStore((s) => s.files);
  const importFiles = useDocumentStore((s) => s.importFiles);
  const refreshFiles = useDocumentStore((s) => s.refreshFiles);
  const projectRoot = useDocumentStore((s) => s.projectRoot);

  // Consume pending attachments from external sources (e.g. PDF capture)
  const pendingAttachments = useAgentChatStore((s) => s.pendingAttachments);
  const consumePendingAttachments = useAgentChatStore(
    (s) => s.consumePendingAttachments,
  );

  // Focus editor when the drawer opens
  const prevOpenRef = useRef(false);
  useEffect(() => {
    if (isOpen && !prevOpenRef.current) {
      setTimeout(() => editorRef.current?.focus(), 0);
    }
    prevOpenRef.current = !!isOpen;
  }, [isOpen]);

  useEffect(() => {
    if (pendingAttachments.length === 0) return;
    const attachments = consumePendingAttachments();
    if (attachments.length === 0) return;
    setPinnedContexts((prev) => {
      const existingLabels = new Set(prev.map((c) => c.label));
      const unique = attachments.filter((a) => !existingLabels.has(a.label));
      return [...prev, ...unique];
    });
    setTimeout(() => editorRef.current?.focus(), 0);
  }, [pendingAttachments, consumePendingAttachments]);

  const currentContextLabel = useMemo(() => {
    if (!selectionRange) return null;
    const file = files.find((f) => f.id === activeFileId);
    if (!file?.content) return null;
    const start = offsetToLineCol(file.content, selectionRange.start);
    const end = offsetToLineCol(file.content, selectionRange.end);
    return `@${file.relativePath}:${start.line}:${start.col}-${end.line}:${end.col}`;
  }, [selectionRange, activeFileId, files]);

  // Auto-pin when a new selection is made
  useEffect(() => {
    if (!selectionRange || !currentContextLabel) return;
    const file = files.find((f) => f.id === activeFileId);
    if (!file?.content) return;
    // Replace any existing selection-based context (keep file contexts)
    setPinnedContexts((prev) => {
      const filtered = prev.filter((c) => c.kind !== "selection");
      return [
        ...filtered,
        {
          label: currentContextLabel,
          filePath: file.relativePath,
          absolutePath: file.absolutePath,
          selectedText: file.content!.slice(
            selectionRange.start,
            selectionRange.end,
          ),
          kind: "selection",
          sourceType: file.type,
        },
      ];
    });
  }, [selectionRange, currentContextLabel, activeFileId, files]);

  // Compute @ mention matches
  useEffect(() => {
    if (mentionQuery === null) {
      setMentionFiles([]);
      return;
    }
    const q = mentionQuery.toLowerCase();
    const matched = files
      .filter(
        (f) =>
          f.relativePath.toLowerCase().includes(q) ||
          f.name.toLowerCase().includes(q),
      )
      .slice(0, 8);
    setMentionFiles(matched);
    setMentionIndex(0);
  }, [mentionQuery, files]);

  // Load slash commands when picker is activated (keep loaded after close for send resolution)
  useEffect(() => {
    if (slashQuery === null) return;
    invoke<SlashCommand[]>("slash_commands_list", {
      projectPath: projectRoot ?? undefined,
    })
      .then(setSlashCommands)
      .catch(() => setSlashCommands([]));
  }, [slashQuery !== null, projectRoot]);

  const selectMention = useCallback(
    async (file: ProjectFile) => {
      const el = editorRef.current;
      if (!el) return;
      const textBefore = getTextBeforeCursor();
      const atIndex = textBefore.lastIndexOf("@");
      if (atIndex === -1) return;
      const fullText = getEditorText();
      const cursorPos = textBefore.length;
      const newText = fullText.slice(0, atIndex) + fullText.slice(cursorPos);
      el.innerText = newText;
      setInput(newText);
      setMentionQuery(null);
      // Pin the whole file as context
      const pinnedContext = await buildPromptContextForProjectFile(file);
      setPinnedContexts((prev) => [...prev, pinnedContext]);
      el.focus();
    },
    [getEditorText, getTextBeforeCursor],
  );

  const selectSlashCommand = useCallback((command: SlashCommand) => {
    const newInput = `${command.full_command} `;
    setInput(newInput);
    setSlashQuery(null);
    slashSelectedRef.current = true;
    setTimeout(() => {
      const el = editorRef.current;
      if (el) {
        setEditorContent(newInput);
        el.focus();
      }
    }, 0);
  }, [setEditorContent]);

  // Handle file drops — guard against duplicate calls from stale HMR listeners
  const isProcessingDropRef = useRef(false);
  const handleFileDropRef = useRef<(paths: string[]) => Promise<void>>(
    async () => {},
  );
  const pinFileContexts = useCallback(async (relativePaths: string[]) => {
    if (relativePaths.length === 0) return;
    const latestFiles = useDocumentStore.getState().files;
    const newContexts = await Promise.all(
      relativePaths.map(async (relativePath): Promise<PinnedContext> => {
        const file = latestFiles.find((f) => f.relativePath === relativePath);
        if (file) {
          return buildPromptContextForProjectFile(file);
        }
        return {
          label: `@${relativePath}`,
          filePath: relativePath,
          selectedText: `[Attached file: ${relativePath}]`,
          kind: "attachment",
          sourceType: "unknown",
        };
      }),
    );

    if (newContexts.length > 0) {
      setPinnedContexts((prev) => {
        const existingLabels = new Set(prev.map((c) => c.label));
        const unique = newContexts.filter((c) => !existingLabels.has(c.label));
        return [...prev, ...unique];
      });
      setTimeout(() => editorRef.current?.focus(), 0);
    }
  }, []);
  const normalizePath = (p: string) =>
    p.replace(/\\/g, "/").replace(/\/+$/, "");
  const resolveProjectRelativePath = (
    absolutePath: string,
    rootPath: string,
  ): string | null => {
    const normAbs = normalizePath(absolutePath);
    const normRoot = normalizePath(rootPath);
    if (normAbs === normRoot) return null;
    if (!normAbs.startsWith(`${normRoot}/`)) return null;
    return normAbs.slice(normRoot.length + 1);
  };
  const isElementInChatDropzone = (el: Element | null): boolean =>
    !!el?.closest(
      "[data-chat-dropzone='true'], [data-chat-composer-dropzone='true']",
    );
  const getHitElements = (position?: { x: number; y: number }): Element[] => {
    if (!position) return [];
    const dpr = window.devicePixelRatio || 1;
    const points = [
      { x: position.x, y: position.y },
      { x: position.x / dpr, y: position.y / dpr },
    ];
    const seenPoints = new Set<string>();
    const elements: Element[] = [];

    for (const p of points) {
      if (!Number.isFinite(p.x) || !Number.isFinite(p.y)) continue;
      const key = `${Math.round(p.x * 1000)}:${Math.round(p.y * 1000)}`;
      if (seenPoints.has(key)) continue;
      seenPoints.add(key);
      const el = document.elementFromPoint(p.x, p.y);
      if (el && !elements.includes(el)) {
        elements.push(el);
      }
    }

    return elements;
  };
  const isInChatDropzoneByPosition = (
    position?: { x: number; y: number },
  ): boolean => getHitElements(position).some((el) => isElementInChatDropzone(el));
  handleFileDropRef.current = async (paths: string[]) => {
    if (!projectRoot || paths.length === 0) return;
    if (isProcessingDropRef.current) return;
    isProcessingDropRef.current = true;

    try {
      // Reuse files that are already inside the project; only external files
      // are copied into attachments/.
      const storeFiles = useDocumentStore.getState().files;
      const existingProjectPaths: string[] = [];
      const externalPaths: string[] = [];

      for (const sourcePath of paths) {
        const normalizedSource = normalizePath(sourcePath);
        const existing = storeFiles.find(
          (f) => normalizePath(f.absolutePath) === normalizedSource,
        );
        if (existing) {
          existingProjectPaths.push(existing.relativePath);
          continue;
        }

        const relativeInProject = resolveProjectRelativePath(
          sourcePath,
          projectRoot,
        );
        if (relativeInProject) {
          existingProjectPaths.push(relativeInProject);
        } else {
          externalPaths.push(sourcePath);
        }
      }

      // Import external files to attachments/ — returns actual (deduplicated) relative paths
      const importedPaths =
        externalPaths.length > 0
          ? await importFiles(externalPaths, "attachments")
          : [];
      const allRelativePaths = Array.from(
        new Set([...existingProjectPaths, ...importedPaths]),
      );

      // Pin each file as chat context
      await pinFileContexts(allRelativePaths);
    } finally {
      isProcessingDropRef.current = false;
    }
  };

  // Internal file-drag attach event from sidebar dnd-kit drag.
  useEffect(() => {
    const handler = (event: Event) => {
      const detail = (event as CustomEvent<{ filePath?: string }>).detail;
      const filePath = detail?.filePath;
      if (!filePath) return;
      void pinFileContexts([filePath]);
    };
    window.addEventListener(
      "claudeprism:attach-file-context",
      handler as EventListener,
    );
    return () => {
      window.removeEventListener(
        "claudeprism:attach-file-context",
        handler as EventListener,
      );
    };
  }, [pinFileContexts]);

  // Listen for Tauri drag-drop events (OS file drops)
  useEffect(() => {
    let unlisten: (() => void) | undefined;
    let cancelled = false;

    getCurrentWebview()
      .onDragDropEvent(async (event) => {
        if (cancelled) return;
        const { type } = event.payload;
        if (type === "enter" || type === "over") {
          const payload = event.payload as {
            position?: { x: number; y: number };
          };
          const inChat = isInChatDropzoneByPosition(payload.position);
          setExternalDropHint(inChat);
        } else if (type === "drop") {
          const payload = event.payload as {
            paths: string[];
            position?: { x: number; y: number };
          };
          const droppedInChat = isInChatDropzoneByPosition(payload.position);
          setExternalDropHint(false);
          if (!droppedInChat) return;
          // Skip if the sidebar already handled this drop (OS file dropped on sidebar file tree)
          if ((window as any).__sidebarHandledDrop) {
            log.debug("skipped — sidebar handled this drop");
            return;
          }
          const paths = payload.paths;
          if (paths?.length > 0) {
            await handleFileDropRef.current?.(paths);
          }
        } else if (type === "leave") {
          setExternalDropHint(false);
        }
      })
      .then((fn) => {
        if (cancelled) {
          fn();
        } else {
          unlisten = fn;
        }
      })
      .catch(() => {
        // Not in Tauri environment (dev mode), ignore
      });

    return () => {
      cancelled = true;
      setExternalDropHint(false);
      unlisten?.();
    };
  }, [setExternalDropHint]);

  // Handle clipboard paste — detect files (screenshots, images) and save to attachments/
  const handlePaste = useCallback(
    async (e: React.ClipboardEvent<HTMLElement>) => {
      const clipboardFiles = e.clipboardData?.files;
      if (!clipboardFiles || clipboardFiles.length === 0 || !projectRoot) {
        // No files — in contentEditable, intercept to paste plain text only
        e.preventDefault();
        const text = e.clipboardData?.getData('text/plain') ?? '';
        if (text) {
          document.execCommand('insertText', false, text);
          const el = editorRef.current;
          if (el) setInput(el.innerText.replace(/\n$/, ''));
        }
        return;
      }

      // Check if there are actual file items (not just text)
      const fileItems = Array.from(clipboardFiles);
      if (fileItems.length === 0) return;

      e.preventDefault();

      const newContexts: PinnedContext[] = [];

      for (const file of fileItems) {
        // Generate a filename — use the original name or a timestamp-based name for screenshots
        let fileName = file.name;
        if (!fileName || fileName === "image.png") {
          const ext = file.type.split("/")[1] || "png";
          fileName = `paste-${Date.now()}.${ext}`;
        }

        const targetName = `attachments/${fileName}`;

        try {
          // Ensure attachments/ directory exists
          const attachmentsDir = await join(projectRoot, "attachments");
          if (!(await exists(attachmentsDir))) {
            await mkdir(attachmentsDir, { recursive: true });
          }

          // Deduplicate filename
          const uniqueName = await getUniqueTargetName(projectRoot, targetName);
          const fullPath = await join(projectRoot, uniqueName);

          // Read file data and write to disk
          const buffer = await file.arrayBuffer();
          await writeFile(fullPath, new Uint8Array(buffer));

          // Determine if it's a text file
          const isText = file.type.startsWith("text/");
          const content = isText
            ? await file.text()
            : `[Attached file: ${uniqueName} (${file.type})]`;

          newContexts.push({
            label: `@${uniqueName}`,
            filePath: uniqueName,
            absolutePath: fullPath,
            selectedText: content,
            kind: isText ? "file" : "attachment",
            sourceType: file.type || "unknown",
          });
        } catch (err) {
          log.error("Failed to save pasted file", {
            fileName,
            error: String(err),
          });
        }
      }

      if (newContexts.length > 0) {
        // Refresh file list so the store knows about new files
        await refreshFiles();
        const latestFiles = useDocumentStore.getState().files;
        const enrichedContexts = await Promise.all(
          newContexts.map(async (ctx) => {
            const file = latestFiles.find((f) => f.relativePath === ctx.filePath);
            return file ? buildPromptContextForProjectFile(file) : ctx;
          }),
        );

        setPinnedContexts((prev) => {
          const existingLabels = new Set(prev.map((c) => c.label));
          const unique = enrichedContexts.filter(
            (c) => !existingLabels.has(c.label),
          );
          return [...prev, ...unique];
        });
      }
    },
    [projectRoot, refreshFiles],
  );

  const handleSend = useCallback(async () => {
    const trimmed = input.trim();
    if (!trimmed || isStreaming) return;

    // Resolve slash commands: if input starts with /command, find the command and substitute $ARGUMENTS
    // Skills (scope === "skill") are passed through as-is — Claude handles them via the Skill tool.
    let finalPrompt = trimmed;
    const slashMatch = trimmed.match(/^\/(\S+)\s*([\s\S]*)/);
    if (slashMatch && slashCommands.length > 0) {
      const cmdName = slashMatch[1];
      const args = slashMatch[2].trim();
      const matched = slashCommands.find(
        (cmd) => cmd.full_command === `/${cmdName}` || cmd.name === cmdName,
      );
      if (matched && matched.scope !== "skill") {
        finalPrompt = matched.content;
        if (matched.accepts_arguments && args) {
          finalPrompt = finalPrompt.replace(/\$ARGUMENTS/g, args);
        }
      }
    }

    const latestFiles = useDocumentStore.getState().files;
    const typedMentionedFiles = findMentionedProjectFiles(finalPrompt, latestFiles);
    const fileByPath = new Map(latestFiles.map((file) => [file.relativePath, file]));
    const attachmentMentionPaths = findMentionedAttachmentPaths(finalPrompt);
    const attachmentMentionFiles = await Promise.all(
      attachmentMentionPaths.map(async (relativePath) => {
        const known = fileByPath.get(relativePath);
        if (known) return known;
        if (!projectRoot) return null;
        const absolutePath = await join(projectRoot, relativePath);
        if (!(await exists(absolutePath))) return null;
        return {
          id: relativePath,
          name: relativePath.split("/").pop() || relativePath,
          relativePath,
          absolutePath,
          type: inferFileTypeFromPath(relativePath),
          isDirty: false,
          content: "",
        } satisfies ProjectFile;
      }),
    );
    const implicitSourceFiles = [
      ...typedMentionedFiles,
      ...attachmentMentionFiles.filter(
        (file): file is ProjectFile =>
          file !== null &&
          !typedMentionedFiles.some(
            (typedFile) => typedFile.relativePath === file.relativePath,
          ),
      ),
    ];
    const existingLabels = new Set(pinnedContexts.map((ctx) => ctx.label));
    const implicitContexts = await Promise.all(
      implicitSourceFiles
        .filter((file) => !existingLabels.has(`@${file.relativePath}`))
        .map((file) => buildPromptContextForProjectFile(file)),
    );
    const contextsToSend = [...pinnedContexts, ...implicitContexts];

    setInput("");
    setMentionQuery(null);
    setSlashQuery(null);
    slashSelectedRef.current = false;
    if (contextsToSend.length > 0) {
      await sendPrompt(finalPrompt, contextsToSend);
    } else {
      await sendPrompt(finalPrompt);
    }
    // Clear contenteditable and pinned contexts after send
    if (editorRef.current) {
      editorRef.current.innerText = "";
    }
    setPinnedContexts([]);
  }, [input, isStreaming, sendPrompt, pinnedContexts, slashCommands]);

  const handleKeyDown = useCallback(
    (e: React.KeyboardEvent<HTMLElement>) => {
      // Slash command picker is open — let the picker handle keyboard events
      // (it uses window.addEventListener for ArrowUp/Down, Enter, Tab, Escape)
      if (slashQuery !== null) {
        if (
          e.key === "Enter" ||
          e.key === "ArrowDown" ||
          e.key === "ArrowUp" ||
          e.key === "Tab" ||
          e.key === "Escape"
        ) {
          e.preventDefault();
          return;
        }
      }

      // @ mention navigation
      if (mentionQuery !== null && mentionFiles.length > 0) {
        if (e.key === "ArrowDown") {
          e.preventDefault();
          setMentionIndex((i) => Math.min(i + 1, mentionFiles.length - 1));
          return;
        }
        if (e.key === "ArrowUp") {
          e.preventDefault();
          setMentionIndex((i) => Math.max(i - 1, 0));
          return;
        }
        if (e.key === "Enter" || e.key === "Tab") {
          e.preventDefault();
          selectMention(mentionFiles[mentionIndex]);
          return;
        }
        if (e.key === "Escape") {
          e.preventDefault();
          setMentionQuery(null);
          return;
        }
      }

      if (e.key === "Enter" && !e.shiftKey) {
        e.preventDefault();
        void handleSend();
      }
      // Backspace at start of empty input removes last pinned context
      if (e.key === "Backspace" && pinnedContexts.length > 0 && input === "") {
        e.preventDefault();
        setPinnedContexts((prev) => prev.slice(0, -1));
      }
    },
    [
      handleSend,
      pinnedContexts,
      input,
      mentionQuery,
      mentionFiles,
      mentionIndex,
      selectMention,
      slashQuery,
    ],
  );

  const handleInput = useCallback(
    (e: React.FormEvent<HTMLElement>) => {
      const el = e.currentTarget;
      const value = el.innerText.replace(/\n$/, "");
      setInput(value);

      // Detect / slash command trigger — only at the very start of input
      const slashMatch = value.match(/^\/(\S*)$/);
      if (slashMatch) {
        slashSelectedRef.current = false;
        setSlashQuery(slashMatch[1]);
        setMentionQuery(null);
      } else if (slashSelectedRef.current) {
        // User already selected a command — don't re-open picker
      } else if (!value.startsWith("/")) {
        setSlashQuery(null);
      }

      // Detect @ mention trigger via Selection API
      if (!value.startsWith("/")) {
        const textBefore = getTextBeforeCursor();
        const atMatch = textBefore.match(/(?:^|[\s])@([^\s]*)$/);
        if (atMatch) {
          setMentionQuery(atMatch[1]);
        } else {
          setMentionQuery(null);
        }
      }
    },
    [getTextBeforeCursor],
  );

  // Scroll active mention into view
  useEffect(() => {
    if (mentionRef.current) {
      const active = mentionRef.current.querySelector("[data-active=true]");
      active?.scrollIntoView({ block: "nearest" });
    }
  }, [mentionIndex]);

  // Close model picker on click outside
  useEffect(() => {
    if (!modelPickerOpen) return;
    const handleClickOutside = (e: MouseEvent) => {
      const target = e.target as Node;
      if (
        modelPickerRef.current &&
        !modelPickerRef.current.contains(target) &&
        modelButtonRef.current &&
        !modelButtonRef.current.contains(target)
      ) {
        setModelPickerOpen(false);
      }
    };
    document.addEventListener("mousedown", handleClickOutside);
    return () => document.removeEventListener("mousedown", handleClickOutside);
  }, [modelPickerOpen]);

  return (
    <div
      ref={composerRef}
      data-chat-composer-dropzone="true"
      className="relative shrink-0 p-3"
    >
      {/* / slash command picker — portal to body to escape all stacking contexts */}
      {slashQuery !== null && (
        <SlashCommandPicker
          projectPath={projectRoot}
          query={slashQuery}
          anchorRef={composerRef}
          onSelect={selectSlashCommand}
          onClose={() => {
            setSlashQuery(null);
          }}
        />
      )}

      {/* Model picker popup — portal to body to escape all stacking contexts */}
      {modelPickerOpen &&
        createPortal(
          <div
            ref={modelPickerRef}
            className="fixed w-64 rounded-lg border border-border bg-background shadow-lg"
            style={{
              left: pickerPos.left,
              bottom: pickerPos.bottom,
              zIndex: 9999,
            }}
          >
            {/* Models */}
            <div className="p-1">
              <div className="px-2 py-1 font-medium text-muted-foreground text-xs">
                Runtime Profile
              </div>
              {[
                {
                  id: "opus" as const,
                  name: "Default",
                  desc: "Use the configured agent model",
                  icon: <SparklesIcon className="size-3.5" />,
                },
                {
                  id: "haiku" as const,
                  name: "Fast",
                  desc: "Use gpt-5.4-mini for quicker replies",
                  icon: <RabbitIcon className="size-3.5" />,
                },
              ].map((m) => (
                <button
                  key={m.id}
                  className={cn(
                    "flex w-full items-center gap-2 rounded-md px-3 py-1.5 text-left text-sm transition-colors",
                    selectedModel === m.id
                      ? "bg-accent text-accent-foreground"
                      : "hover:bg-muted",
                  )}
                  onClick={() => setSelectedModel(m.id)}
                >
                  {m.icon}
                  <div className="min-w-0 flex-1">
                    <div className="font-medium text-xs">{m.name}</div>
                    <div className="truncate text-muted-foreground text-xs">
                      {m.desc}
                    </div>
                  </div>
                  {selectedModel === m.id && (
                    <CheckIcon className="size-3 shrink-0" />
                  )}
                </button>
              ))}
            </div>
          </div>,
          document.body,
        )}

      {/* @ mention dropdown */}
      {slashQuery === null &&
        mentionQuery !== null &&
        mentionFiles.length > 0 && (
          <div
            ref={mentionRef}
            className="absolute right-3 bottom-full left-3 mb-1 max-h-48 overflow-y-auto rounded-lg border border-border bg-background shadow-lg"
          >
            {mentionFiles.map((file, i) => {
              const parts = file.relativePath.split("/");
              const fileName = parts.pop()!;
              const dirPath = parts.length > 0 ? `${parts.join("/")}/` : "";
              return (
                <button
                  key={file.id}
                  data-active={i === mentionIndex}
                  className={cn(
                    "flex w-full items-center gap-2 px-3 py-1.5 text-left transition-colors",
                    i === mentionIndex
                      ? "bg-accent text-accent-foreground"
                      : "hover:bg-muted",
                  )}
                  onMouseDown={(e) => {
                    e.preventDefault(); // prevent textarea blur
                    selectMention(file);
                  }}
                  onMouseEnter={() => setMentionIndex(i)}
                >
                  {getFileIcon(file)}
                  <span className="truncate font-mono text-sm">{fileName}</span>
                  {dirPath && (
                    <span className="ml-auto shrink-0 font-mono text-muted-foreground text-xs">
                      {dirPath}
                    </span>
                  )}
                </button>
              );
            })}
          </div>
        )}

      <div
        className={cn(
          "flex w-full flex-col rounded-2xl border border-input bg-muted/30 transition-colors focus-within:border-ring focus-within:bg-background",
        )}
      >
        {/* Unified content area: chips and text share a single inline flow */}
        {/* Content area — single block container, everything flows inline */}
        <div
          className="relative cursor-text overflow-y-auto px-4 pt-3 pb-1 text-sm leading-6"
          style={{ maxHeight: 160, minHeight: '2.5rem', overflowWrap: 'anywhere' as const }}
          onClick={() => editorRef.current?.focus()}
        >
          {/* Image attachments as a block row above the inline flow */}
          {pinnedContexts.some((c) => c.imageDataUrl) && (
            <div className="mb-2 flex flex-wrap gap-1.5">
              {pinnedContexts.map((ctx, i) =>
                ctx.imageDataUrl ? (
                  <div
                    key={`img-${ctx.label}-${i}`}
                    className="group relative overflow-hidden rounded-lg border border-border bg-muted"
                  >
                    <img
                      src={ctx.imageDataUrl}
                      alt={ctx.label}
                      className="block h-16 w-auto object-contain"
                    />
                    <button
                      aria-label="Remove attachment"
                      onClick={(e) => { e.stopPropagation(); setPinnedContexts((prev) => prev.filter((_, idx) => idx !== i)); }}
                      className="absolute top-0.5 right-0.5 rounded-full bg-background/80 p-0.5 opacity-0 transition-opacity group-hover:opacity-100"
                    >
                      <XIcon className="size-3" />
                    </button>
                  </div>
                ) : null,
              )}
            </div>
          )}

          {/* Placeholder — absolutely positioned, visible only when no text & no text chips */}
          {!input && pinnedContexts.every((c) => !!c.imageDataUrl) && (
            <span className="pointer-events-none absolute top-3 left-4 select-none text-muted-foreground">
              Ask me anything (/ for commands, @ to mention)
            </span>
          )}

          {/* Non-image chips — inline-flex elements in the normal flow */}
          {pinnedContexts.map((ctx, i) =>
            ctx.imageDataUrl ? null : (
              <span
                key={`${ctx.label}-${i}`}
                contentEditable={false}
                className="mb-0.5 mr-1.5 inline-flex items-center gap-1 rounded-md bg-muted px-2 py-0.5 align-baseline font-mono text-muted-foreground text-xs"
              >
                {ctx.label}
                <button
                  aria-label="Remove context"
                  onClick={(e) => { e.stopPropagation(); setPinnedContexts((prev) => prev.filter((_, idx) => idx !== i)); }}
                  className="ml-0.5 rounded-sm p-0.5 transition-colors hover:bg-muted-foreground/20"
                >
                  <XIcon className="size-3" />
                </button>
              </span>
            ),
          )}

          {/* Editable span — display:inline so text shares the same line boxes as the chips.
              When text wraps, it wraps at the block container's left edge, flowing below chips. */}
          <span
            ref={editorRef as React.RefObject<HTMLSpanElement>}
            contentEditable
            suppressContentEditableWarning
            onInput={handleInput}
            onKeyDown={handleKeyDown}
            onPaste={handlePaste}
            role="textbox"
            className="outline-none break-all"
          />
        </div>

        {/* Bottom controls — compact layout with aligned heights */}
        <div className="flex items-center gap-2 px-4 py-2 border-t border-border/40">
          {/* Model & settings selector */}
          <button
            ref={modelButtonRef}
            type="button"
            onClick={() => setModelPickerOpen((v) => !v)}
            className="flex items-center gap-1 rounded-md px-2 py-1 text-muted-foreground text-xs transition-colors hover:bg-muted hover:text-foreground h-7 whitespace-nowrap"
          >
            <span>
              {selectedModel === "haiku" ? "Fast" : "Default"}
            </span>
            <ChevronDownIcon className="size-3" />
          </button>

          {/* Send/Stop button — aligned right */}
          <div className="ml-auto">
            {isStreaming ? (
              <TooltipIconButton
                tooltip="Stop"
                side="top"
                variant="secondary"
                size="icon"
                className="size-7 rounded-full"
                onClick={cancelExecution}
              >
                <SquareIcon className="size-3 fill-current" />
              </TooltipIconButton>
            ) : (
              <TooltipIconButton
                tooltip="Send"
                side="top"
                variant="default"
                size="icon"
                className="size-7 rounded-full"
                onClick={() => {
                  void handleSend();
                }}
                disabled={!input.trim()}
              >
                <ArrowUpIcon className="size-3.5" />
              </TooltipIconButton>
            )}
          </div>
        </div>
      </div>
    </div>
  );
};
