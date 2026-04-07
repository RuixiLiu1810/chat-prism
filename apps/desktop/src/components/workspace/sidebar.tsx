import { useState, useCallback, useMemo, useRef, useEffect } from "react";
import { getVersion } from "@tauri-apps/api/app";
import {
  FileTextIcon,
  FolderIcon,
  FolderPlusIcon,
  HomeIcon,
  PlusIcon,
  UploadIcon,
  RefreshCwIcon,
  ListIcon,
  GithubIcon,
  SettingsIcon,
} from "lucide-react";
import {
  DndContext,
  DragOverlay,
  PointerSensor,
  useSensor,
  useSensors,
  type DragStartEvent,
  type DragEndEvent,
} from "@dnd-kit/core";
import { Panel, PanelGroup, PanelResizeHandle } from "react-resizable-panels";
import { useTheme } from "next-themes";
import { useDocumentStore } from "@/stores/document-store";
import { cn } from "@/lib/utils";
import { ZoteroPanel } from "@/components/workspace/zotero-panel";
import { Button } from "@/components/ui/button";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import {
  ContextMenu,
  ContextMenuContent,
  ContextMenuItem,
  ContextMenuSeparator,
  ContextMenuTrigger,
} from "@/components/ui/context-menu";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { createLogger } from "@/lib/debug/logger";
import { SettingsDialog } from "@/components/workspace/settings-dialog";
import { useSettingsStore } from "@/stores/settings-store";
import { buildFileTree, DroppableRoot, FileTreeNode } from "./sidebar/FileTree";
import { parseTableOfContents, OutlinePanelContent } from "./sidebar/OutlinePanel";
import { NewFileDialog, NewFolderDialog, RenameDialog } from "./sidebar/FileDialogs";
import { EnvironmentSection } from "./sidebar/EnvironmentSection";

const log = createLogger("sidebar");

// ─── App Version (resolved once from Tauri) ───

let _appVersion = "";
getVersion().then((v) => {
  _appVersion = v;
});
function useAppVersion() {
  const [version, setVersion] = useState(_appVersion);
  useEffect(() => {
    if (!version) getVersion().then(setVersion);
  }, [version]);
  return version || "…";
}

// ─── Sidebar ───

export function Sidebar() {
  const appVersion = useAppVersion();
  const files = useDocumentStore((s) => s.files);
  const activeFileId = useDocumentStore((s) => s.activeFileId);
  const setActiveFile = useDocumentStore((s) => s.setActiveFile);
  const deleteFile = useDocumentStore((s) => s.deleteFile);
  const deleteFolder = useDocumentStore((s) => s.deleteFolder);
  const renameFile = useDocumentStore((s) => s.renameFile);
  const createNewFile = useDocumentStore((s) => s.createNewFile);
  const createFolder = useDocumentStore((s) => s.createFolder);
  const importFiles = useDocumentStore((s) => s.importFiles);
  const activeFileContent = useDocumentStore((s) => {
    const active = s.files.find((f) => f.id === s.activeFileId);
    return active?.content ?? "";
  });
  const requestJumpToPosition = useDocumentStore(
    (s) => s.requestJumpToPosition,
  );
  const moveFile = useDocumentStore((s) => s.moveFile);
  const moveFolder = useDocumentStore((s) => s.moveFolder);
  const closeProject = useDocumentStore((s) => s.closeProject);
  const refreshFiles = useDocumentStore((s) => s.refreshFiles);
  const projectRoot = useDocumentStore((s) => s.projectRoot);
  const folders = useDocumentStore((s) => s.folders);
  const { theme, setTheme } = useTheme();
  const preferredTheme = useSettingsStore((s) => s.effective.general.theme);
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [isRefreshingFiles, setIsRefreshingFiles] = useState(false);

  useEffect(() => {
    if (preferredTheme && theme !== preferredTheme) {
      setTheme(preferredTheme);
    }
  }, [preferredTheme, setTheme, theme]);

  // ─── Native OS file drop (Tauri onDragDropEvent) ───
  const sidebarFilesRef = useRef<HTMLDivElement>(null);
  const nativeDropTargetRef = useRef<string | null>(null);
  const [nativeDragOver, setNativeDragOver] = useState<string | null>(null);

  useEffect(() => {
    let unlisten: (() => void) | undefined;
    let cancelled = false;
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
    const isInChatDropzone = (el: Element | null): boolean =>
      !!el?.closest(
        "[data-chat-dropzone='true'], [data-chat-composer-dropzone='true']",
      );

    getCurrentWebview()
      .onDragDropEvent(async (event) => {
        if (cancelled) return;
        const { type } = event.payload;

        if (type === "over" || type === "enter") {
          const payload = event.payload as {
            position?: { x: number; y: number };
          };
          const hitElements = getHitElements(payload.position);
          if (hitElements.some((el) => isInChatDropzone(el))) {
            if (nativeDropTargetRef.current !== null) {
              nativeDropTargetRef.current = null;
              setNativeDragOver(null);
            }
            return;
          }
          const filesArea = sidebarFilesRef.current;
          const el =
            filesArea == null
              ? null
              : (hitElements.find((candidate) => filesArea.contains(candidate)) ??
                null);

          if (!filesArea || !el || !filesArea.contains(el)) {
            if (nativeDropTargetRef.current !== null) {
              nativeDropTargetRef.current = null;
              setNativeDragOver(null);
            }
            return;
          }

          const folderEl = el.closest(
            "[data-drop-folder]",
          ) as HTMLElement | null;
          const folder = folderEl?.dataset.dropFolder ?? null;
          nativeDropTargetRef.current = folder;
          setNativeDragOver(folder);
        } else if (type === "drop") {
          const payload = event.payload as {
            paths: string[];
            position?: { x: number; y: number };
          };
          const { paths } = payload;
          const hitElements = getHitElements(payload.position);
          if (hitElements.some((el) => isInChatDropzone(el))) {
            setNativeDragOver(null);
            nativeDropTargetRef.current = null;
            return;
          }
          const filesArea = sidebarFilesRef.current;
          const el =
            filesArea == null
              ? null
              : (hitElements.find((candidate) => filesArea.contains(candidate)) ??
                null);

          if (!filesArea || !el || !filesArea.contains(el)) {
            setNativeDragOver(null);
            nativeDropTargetRef.current = null;
            return;
          }

          const folderEl = el.closest(
            "[data-drop-folder]",
          ) as HTMLElement | null;
          const dropFolder = folderEl?.dataset.dropFolder ?? null;
          if (!dropFolder) {
            setNativeDragOver(null);
            nativeDropTargetRef.current = null;
            return;
          }

          const targetFolder =
            dropFolder === "__root__"
              ? undefined
              : dropFolder;

          (window as any).__sidebarHandledDrop = true;
          setTimeout(() => {
            (window as any).__sidebarHandledDrop = false;
          }, 200);

          try {
            await importFiles(paths, targetFolder);
          } catch (err) {
            log.error("Native drop import failed", { error: String(err) });
          }

          setNativeDragOver(null);
          nativeDropTargetRef.current = null;
        } else if (type === "leave") {
          setNativeDragOver(null);
          nativeDropTargetRef.current = null;
        }
      })
      .then((fn) => {
        if (cancelled) fn();
        else unlisten = fn;
      })
      .catch(() => {
        // Not in Tauri environment (dev mode)
      });

    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, [importFiles]);

  // Track selected folder for paste target
  const [pasteTargetFolder, setPasteTargetFolder] = useState<
    string | undefined
  >();

  // ─── Cmd+V paste files from OS clipboard ───
  useEffect(() => {
    const handleKeyDown = async (e: KeyboardEvent) => {
      if (!(e.metaKey || e.ctrlKey) || e.key !== "v") return;

      const active = document.activeElement;
      if (
        active &&
        (active.tagName === "INPUT" ||
          active.tagName === "TEXTAREA" ||
          (active as HTMLElement).isContentEditable)
      )
        return;

      try {
        const { invoke } = await import("@tauri-apps/api/core");
        const paths = await invoke<string[]>("read_clipboard_file_paths");
        if (paths.length > 0) {
          e.preventDefault();
          await importFiles(paths, pasteTargetFolder);
        }
      } catch (err) {
        log.error("Read clipboard failed", { error: String(err) });
      }
    };

    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, [importFiles, pasteTargetFolder]);

  // dnd-kit drag-and-drop
  const sensors = useSensors(
    useSensor(PointerSensor, { activationConstraint: { distance: 5 } }),
  );
  const lastPointerRef = useRef<{ x: number; y: number } | null>(null);
  const pointerListenerRef = useRef<((e: PointerEvent) => void) | null>(null);
  const chatHoverRef = useRef(false);
  const [activeDrag, setActiveDrag] = useState<{
    id: string;
    type: "file" | "folder";
    name: string;
  } | null>(null);

  const isPointInChatDropzone = useCallback((x: number, y: number) => {
    const hitElements = document.elementsFromPoint(x, y);
    return hitElements.some((el) =>
      !!el.closest(
        "[data-chat-dropzone='true'], [data-chat-composer-dropzone='true']",
      ),
    );
  }, []);

  const handleDragStart = useCallback((event: DragStartEvent) => {
    const { type, name } = event.active.data.current as {
      type: "file" | "folder";
      name: string;
    };
    setActiveDrag({ id: event.active.id as string, type, name });

    const activator = event.activatorEvent as MouseEvent | PointerEvent | null;
    if (activator && typeof activator.clientX === "number") {
      lastPointerRef.current = { x: activator.clientX, y: activator.clientY };
    }
    chatHoverRef.current = false;
    window.dispatchEvent(
      new CustomEvent("claudeprism:chat-drop-hover", {
        detail: { active: false },
      }),
    );
    const onPointerMove = (e: PointerEvent) => {
      lastPointerRef.current = { x: e.clientX, y: e.clientY };
      if (type !== "file") return;
      const inChat = isPointInChatDropzone(e.clientX, e.clientY);
      if (inChat !== chatHoverRef.current) {
        chatHoverRef.current = inChat;
        window.dispatchEvent(
          new CustomEvent("claudeprism:chat-drop-hover", {
            detail: {
              active: inChat,
              fileName: name,
            },
          }),
        );
      }
    };
    pointerListenerRef.current = onPointerMove;
    window.addEventListener("pointermove", onPointerMove);
    window.addEventListener("pointerup", onPointerMove);
  }, [isPointInChatDropzone]);

  const handleDragEnd = useCallback(
    async (event: DragEndEvent) => {
      if (pointerListenerRef.current) {
        window.removeEventListener("pointermove", pointerListenerRef.current);
        window.removeEventListener("pointerup", pointerListenerRef.current);
        pointerListenerRef.current = null;
      }
      if (chatHoverRef.current) {
        chatHoverRef.current = false;
      }
      window.dispatchEvent(
        new CustomEvent("claudeprism:chat-drop-hover", {
          detail: { active: false },
        }),
      );

      setActiveDrag(null);
      const { active, over } = event;
      const draggedPath = active.id as string;
      const draggedType = (active.data.current as { type: string }).type;

      if (draggedType === "file") {
        const point = lastPointerRef.current;
        if (point) {
          const inComposer = isPointInChatDropzone(point.x, point.y);
          if (inComposer) {
            window.dispatchEvent(
              new CustomEvent("claudeprism:attach-file-context", {
                detail: { filePath: draggedPath },
              }),
            );
            return;
          }
        }
      }

      if (!over) return;

      const targetId = over.id as string;
      const targetFolder = targetId === "__root__" ? null : targetId;

      const draggedParent = draggedPath.includes("/")
        ? draggedPath.substring(0, draggedPath.lastIndexOf("/"))
        : null;
      if (targetFolder === draggedParent) return;

      if (draggedType === "folder" && targetFolder) {
        if (
          targetFolder === draggedPath ||
          targetFolder.startsWith(`${draggedPath}/`)
        )
          return;
      }

      try {
        if (draggedType === "file") await moveFile(draggedPath, targetFolder);
        else await moveFolder(draggedPath, targetFolder);
      } catch (err) {
        log.error("DnD move failed", { error: String(err) });
      }
    },
    [isPointInChatDropzone, moveFile, moveFolder],
  );

  const handleDragCancel = useCallback(() => {
    if (pointerListenerRef.current) {
      window.removeEventListener("pointermove", pointerListenerRef.current);
      window.removeEventListener("pointerup", pointerListenerRef.current);
      pointerListenerRef.current = null;
    }
    if (chatHoverRef.current) {
      chatHoverRef.current = false;
    }
    window.dispatchEvent(
      new CustomEvent("claudeprism:chat-drop-hover", {
        detail: { active: false },
      }),
    );
    setActiveDrag(null);
  }, []);

  useEffect(() => {
    return () => {
      if (pointerListenerRef.current) {
        window.removeEventListener("pointermove", pointerListenerRef.current);
        window.removeEventListener("pointerup", pointerListenerRef.current);
        pointerListenerRef.current = null;
      }
      if (chatHoverRef.current) {
        chatHoverRef.current = false;
      }
      window.dispatchEvent(
        new CustomEvent("claudeprism:chat-drop-hover", {
          detail: { active: false },
        }),
      );
    };
  }, []);

  // Dialog state
  const [addDialogOpen, setAddDialogOpen] = useState(false);
  const [addDialogFolder, setAddDialogFolder] = useState<string | undefined>();
  const [folderDialogOpen, setFolderDialogOpen] = useState(false);
  const [folderDialogParent, setFolderDialogParent] = useState<
    string | undefined
  >();
  const [renameDialogOpen, setRenameDialogOpen] = useState(false);
  const [renameFileId, setRenameFileId] = useState<string | null>(null);
  const [renameValue, setRenameValue] = useState("");
  const [newFileName, setNewFileName] = useState("");
  const [newFolderName, setNewFolderName] = useState("");

  // Folder expand/collapse
  const [expandedFolders, setExpandedFolders] = useState<Set<string>>(
    new Set(),
  );
  const tree = useMemo(() => buildFileTree(files, folders), [files, folders]);

  useEffect(() => {
    if (!activeFileId) return;
    const parts = activeFileId.split("/");
    if (parts.length <= 1) return;
    setExpandedFolders((prev) => {
      const next = new Set(prev);
      let changed = false;
      for (let i = 1; i < parts.length; i++) {
        const folder = parts.slice(0, i).join("/");
        if (!next.has(folder)) {
          next.add(folder);
          changed = true;
        }
      }
      return changed ? next : prev;
    });
  }, [activeFileId]);

  const toggleFolder = useCallback((path: string) => {
    setPasteTargetFolder(path);
    setExpandedFolders((prev) => {
      const next = new Set(prev);
      if (next.has(path)) next.delete(path);
      else next.add(path);
      return next;
    });
  }, []);

  // Outline
  const toc = useMemo(
    () => parseTableOfContents(activeFileContent),
    [activeFileContent],
  );
  const handleTocClick = useCallback(
    (line: number) => {
      const lines = activeFileContent.split("\n");
      let position = 0;
      for (let i = 0; i < line - 1 && i < lines.length; i++) {
        position += lines[i].length + 1;
      }
      requestJumpToPosition(position);
    },
    [activeFileContent, requestJumpToPosition],
  );

  // Name collision check
  const isCaseInsensitiveFs =
    navigator.platform.startsWith("Mac") ||
    navigator.platform.startsWith("Win");
  const nameExistsIn = useCallback(
    (name: string, folder?: string) => {
      const targetPath = folder ? `${folder}/${name}` : name;
      const cmp = (a: string, b: string) =>
        isCaseInsensitiveFs ? a.toLowerCase() === b.toLowerCase() : a === b;
      const existsAsFile = files.some((f) => cmp(f.relativePath, targetPath));
      const existsAsFolder = folders.some((f) => cmp(f, targetPath));
      return existsAsFile || existsAsFolder;
    },
    [files, folders, isCaseInsensitiveFs],
  );

  // Handlers
  const [nameError, setNameError] = useState("");

  const handleAddFile = () => {
    const name = newFileName.trim();
    if (!name) return;
    if (nameExistsIn(name, addDialogFolder)) {
      setNameError("A file or folder with this name already exists");
      return;
    }
    const finalName = /\.\w+$/.test(name) ? name : `${name}.tex`;
    const lower = finalName.toLowerCase();
    const type: "tex" | "image" = /\.(png|jpg|jpeg|gif|svg|bmp|webp)$/.test(
      lower,
    )
      ? "image"
      : "tex";
    createNewFile(finalName, type, addDialogFolder);
    setNewFileName("");
    setNameError("");
    setAddDialogOpen(false);
    setAddDialogFolder(undefined);
  };

  const handleCreateFolder = () => {
    const name = newFolderName.trim();
    if (!name) return;
    if (nameExistsIn(name, folderDialogParent)) {
      setNameError("A file or folder with this name already exists");
      return;
    }
    createFolder(name, folderDialogParent);
    setNewFolderName("");
    setNameError("");
    setFolderDialogOpen(false);
    setFolderDialogParent(undefined);
  };

  const handleImport = async (targetFolder?: string) => {
    const selected = await openDialog({
      multiple: true,
      filters: [
        {
          name: "All Files",
          extensions: [
            "tex",
            "bib",
            "sty",
            "cls",
            "bst",
            "png",
            "jpg",
            "jpeg",
            "gif",
            "svg",
            "bmp",
            "webp",
            "pdf",
            "txt",
            "md",
          ],
        },
      ],
    });
    if (selected && projectRoot) {
      const paths = Array.isArray(selected) ? selected : [selected];
      await importFiles(paths, targetFolder);
    }
  };

  const openRenameDialog = (id: string, name: string) => {
    setRenameFileId(id);
    setRenameValue(name);
    setNameError("");
    setRenameDialogOpen(true);
  };

  const handleRename = () => {
    const name = renameValue.trim();
    if (!renameFileId || !name) return;
    const file = files.find((f) => f.id === renameFileId);
    const parentFolder = file?.relativePath.includes("/")
      ? file.relativePath.substring(0, file.relativePath.lastIndexOf("/"))
      : undefined;
    const isSameName = isCaseInsensitiveFs
      ? name.toLowerCase() === file?.name.toLowerCase()
      : name === file?.name;
    if (nameExistsIn(name, parentFolder) && !isSameName) {
      setNameError("A file or folder with this name already exists");
      return;
    }
    renameFile(renameFileId, name);
    setRenameDialogOpen(false);
    setRenameFileId(null);
    setRenameValue("");
    setNameError("");
  };

  const openNewFileDialog = (folder?: string) => {
    setAddDialogFolder(folder);
    setNewFileName("");
    setNameError("");
    setAddDialogOpen(true);
  };

  const openNewFolderDialog = (parent?: string) => {
    setFolderDialogParent(parent);
    setNewFolderName("");
    setNameError("");
    setFolderDialogOpen(true);
  };

  const handleRefreshFiles = useCallback(async () => {
    if (isRefreshingFiles) return;
    setIsRefreshingFiles(true);
    const startedAt = Date.now();
    try {
      await refreshFiles();
    } finally {
      const minSpinMs = 450;
      const elapsed = Date.now() - startedAt;
      if (elapsed < minSpinMs) {
        await new Promise((resolve) => setTimeout(resolve, minSpinMs - elapsed));
      }
      setIsRefreshingFiles(false);
    }
  }, [isRefreshingFiles, refreshFiles]);

  // ─── Render ───

  return (
    <div className="flex h-full flex-col bg-sidebar text-sidebar-foreground">
      {/* Header */}
      <div className="relative flex h-[calc(48px+var(--titlebar-height))] items-center justify-center border-sidebar-border border-b px-3 pt-[var(--titlebar-height)]">
        <div className="flex flex-col items-center">
          <span className="font-semibold text-sm">ClaudePrism</span>
          <span className="text-muted-foreground text-xs">
            {projectRoot?.split(/[/\\]/).pop() || "Desktop"}
          </span>
        </div>
        <div className="absolute right-3 flex items-center gap-0.5">
          <Button
            variant="ghost"
            size="icon"
            className="size-6"
            onClick={closeProject}
            title="Close Project"
          >
            <HomeIcon className="size-3.5" />
          </Button>
        </div>
      </div>

      {/* Resizable sections */}
      <PanelGroup direction="vertical" className="min-h-0 flex-1">
        {/* Files */}
        <Panel defaultSize={50} minSize={15}>
          <div
            ref={sidebarFilesRef}
            className="flex h-full flex-col"
            data-sidebar-files
          >
            <div className="relative flex h-8 shrink-0 items-center justify-center border-sidebar-border border-b px-3">
              <div className="flex items-center gap-2">
                <FolderIcon className="size-3.5 text-muted-foreground" />
                <span className="font-medium text-xs">Files</span>
              </div>
              <div className="absolute right-3 flex items-center gap-0.5">
                <Button
                  variant="ghost"
                  size="icon"
                  className="size-5"
                  title={isRefreshingFiles ? "Refreshing..." : "Refresh"}
                  onClick={handleRefreshFiles}
                  disabled={isRefreshingFiles}
                >
                  <RefreshCwIcon
                    className={cn("size-3", isRefreshingFiles && "animate-spin")}
                  />
                </Button>
                <DropdownMenu>
                  <DropdownMenuTrigger asChild>
                    <Button
                      variant="ghost"
                      size="icon"
                      className="size-5"
                      title="Add"
                    >
                      <PlusIcon className="size-3" />
                    </Button>
                  </DropdownMenuTrigger>
                  <DropdownMenuContent align="end">
                    <DropdownMenuItem onClick={() => openNewFileDialog()}>
                      <FileTextIcon className="mr-2 size-4" />
                      New LaTeX File
                    </DropdownMenuItem>
                    <DropdownMenuItem onClick={() => openNewFolderDialog()}>
                      <FolderPlusIcon className="mr-2 size-4" />
                      New Folder
                    </DropdownMenuItem>
                    <DropdownMenuSeparator />
                    <DropdownMenuItem onClick={() => handleImport()}>
                      <UploadIcon className="mr-2 size-4" />
                      Import File
                    </DropdownMenuItem>
                  </DropdownMenuContent>
                </DropdownMenu>
              </div>
            </div>
            <DndContext
              sensors={sensors}
              onDragStart={handleDragStart}
              onDragEnd={handleDragEnd}
              onDragCancel={handleDragCancel}
            >
              <ContextMenu>
                <ContextMenuTrigger asChild>
                  <DroppableRoot nativeDragOver={nativeDragOver === "__root__"}>
                    {tree.map((node) => (
                      <FileTreeNode
                        key={node.relativePath}
                        node={node}
                        depth={0}
                        activeFileId={activeFileId}
                        expandedFolders={expandedFolders}
                        onToggleFolder={toggleFolder}
                        onSelectFile={(id: string) => {
                          const parent = id.includes("/")
                            ? id.substring(0, id.lastIndexOf("/"))
                            : undefined;
                          setPasteTargetFolder(parent);
                          setActiveFile(id);
                        }}
                        onNewFile={openNewFileDialog}
                        onNewFolder={openNewFolderDialog}
                        onImport={handleImport}
                        onRename={openRenameDialog}
                        onDelete={deleteFile}
                        onDeleteFolder={deleteFolder}
                        fileCount={files.length}
                        nativeDragOver={nativeDragOver}
                      />
                    ))}
                  </DroppableRoot>
                </ContextMenuTrigger>
                <ContextMenuContent>
                  <ContextMenuItem onClick={() => openNewFileDialog()}>
                    <FileTextIcon className="mr-2 size-4" />
                    New File
                  </ContextMenuItem>
                  <ContextMenuItem onClick={() => openNewFolderDialog()}>
                    <FolderPlusIcon className="mr-2 size-4" />
                    New Folder
                  </ContextMenuItem>
                  <ContextMenuSeparator />
                  <ContextMenuItem onClick={() => handleImport()}>
                    <UploadIcon className="mr-2 size-4" />
                    Import File
                  </ContextMenuItem>
                </ContextMenuContent>
              </ContextMenu>
              <DragOverlay dropAnimation={null}>
                {activeDrag && (
                  <div className="flex items-center gap-2 rounded-md bg-sidebar px-2 py-1 text-sm shadow-lg ring-1 ring-ring">
                    {activeDrag.type === "folder" ? (
                      <FolderIcon className="size-4 shrink-0" />
                    ) : (
                      <FileTextIcon className="size-4 shrink-0" />
                    )}
                    <span className="truncate">{activeDrag.name}</span>
                  </div>
                )}
              </DragOverlay>
            </DndContext>
          </div>
        </Panel>

        <PanelResizeHandle className="h-px bg-sidebar-border transition-colors hover:bg-ring data-resize-handle-active:bg-ring" />

        {/* Outline */}
        <Panel defaultSize={20} minSize={10}>
          <div className="flex h-full flex-col">
            <div className="flex h-8 shrink-0 items-center justify-center gap-2 px-3">
              <ListIcon className="size-3.5 text-muted-foreground" />
              <span className="font-medium text-xs">Outline</span>
            </div>
            <div className="min-h-0 flex-1 overflow-y-auto p-1">
              <OutlinePanelContent toc={toc} onTocClick={handleTocClick} />
            </div>
          </div>
        </Panel>

        <PanelResizeHandle className="h-px bg-sidebar-border transition-colors hover:bg-ring data-resize-handle-active:bg-ring" />

        {/* Zotero */}
        <Panel defaultSize={15} minSize={10}>
          <div className="h-full overflow-hidden">
            <ZoteroPanel />
          </div>
        </Panel>
      </PanelGroup>

      {/* Environment section */}
      <EnvironmentSection projectPath={projectRoot} />

      {/* Footer */}
      <div className="flex items-center justify-between border-sidebar-border border-t px-3 py-2 text-muted-foreground text-xs">
        <span className="truncate">ClaudePrism v{appVersion}</span>
        <div className="flex shrink-0 items-center gap-1">
          <Button variant="ghost" size="icon" className="size-6" asChild>
            <a
              href="https://github.com/delibae/claude-prism"
              target="_blank"
              rel="noopener noreferrer"
              title="GitHub"
            >
              <GithubIcon className="size-3.5" />
            </a>
          </Button>
          <Button
            variant="ghost"
            size="icon"
            className="size-6"
            onClick={() => setSettingsOpen(true)}
            title="Settings"
          >
            <SettingsIcon className="size-3.5" />
          </Button>
        </div>
      </div>

      {/* Dialogs */}
      <NewFileDialog
        open={addDialogOpen}
        onOpenChange={setAddDialogOpen}
        folder={addDialogFolder}
        fileName={newFileName}
        onFileNameChange={setNewFileName}
        nameError={nameError}
        onNameErrorChange={setNameError}
        onSubmit={handleAddFile}
      />

      <NewFolderDialog
        open={folderDialogOpen}
        onOpenChange={setFolderDialogOpen}
        parent={folderDialogParent}
        folderName={newFolderName}
        onFolderNameChange={setNewFolderName}
        nameError={nameError}
        onNameErrorChange={setNameError}
        onSubmit={handleCreateFolder}
      />

      <RenameDialog
        open={renameDialogOpen}
        onOpenChange={setRenameDialogOpen}
        renameValue={renameValue}
        onRenameValueChange={setRenameValue}
        nameError={nameError}
        onNameErrorChange={setNameError}
        onSubmit={handleRename}
      />

      <SettingsDialog
        open={settingsOpen}
        onOpenChange={setSettingsOpen}
        projectRoot={projectRoot}
      />
    </div>
  );
}
