import {
  FileTextIcon,
  FolderIcon,
  FolderPlusIcon,
  ImageIcon,
  PencilIcon,
  Trash2Icon,
  UploadIcon,
  ChevronRightIcon,
  ChevronDownIcon,
  FileCodeIcon,
  FileIcon,
  FileSpreadsheetIcon,
} from "lucide-react";
import {
  useDroppable,
  useDraggable,
} from "@dnd-kit/core";
import {
  ContextMenu,
  ContextMenuContent,
  ContextMenuItem,
  ContextMenuSeparator,
  ContextMenuTrigger,
} from "@/components/ui/context-menu";
import { useHistoryStore } from "@/stores/history-store";
import type { ProjectFile } from "@/stores/document-store";
import { cn } from "@/lib/utils";

// ─── Types ───

export interface TreeNode {
  name: string;
  relativePath: string;
  type: "folder" | "file";
  file?: ProjectFile;
  children: TreeNode[];
}

// ─── File Tree Builder ───

export function buildFileTree(files: ProjectFile[], folders: string[]): TreeNode[] {
  const root: TreeNode[] = [];
  const folderMap = new Map<string, TreeNode>();

  function getOrCreateFolder(path: string): TreeNode[] {
    if (!path) return root;
    if (folderMap.has(path)) return folderMap.get(path)!.children;

    const parts = path.split("/");
    const name = parts[parts.length - 1];
    const parentPath = parts.slice(0, -1).join("/");
    const parentChildren = getOrCreateFolder(parentPath);

    const folder: TreeNode = {
      name,
      relativePath: path,
      type: "folder",
      children: [],
    };
    folderMap.set(path, folder);
    parentChildren.push(folder);
    return folder.children;
  }

  for (const folderPath of folders) {
    getOrCreateFolder(folderPath);
  }

  for (const file of files) {
    const parts = file.relativePath.split("/");
    const fileName = parts[parts.length - 1];
    const folderPath = parts.slice(0, -1).join("/");
    const parentChildren = getOrCreateFolder(folderPath);

    parentChildren.push({
      name: fileName,
      relativePath: file.relativePath,
      type: "file",
      file,
      children: [],
    });
  }

  function sortNodes(nodes: TreeNode[]) {
    nodes.sort((a, b) => {
      if (a.type !== b.type) return a.type === "folder" ? -1 : 1;
      return a.name.localeCompare(b.name);
    });
    for (const node of nodes) {
      if (node.type === "folder") sortNodes(node.children);
    }
  }
  sortNodes(root);

  return root;
}

// ─── File Icon ───

export function getFileIcon(file: ProjectFile) {
  if (file.type === "image") return <ImageIcon className="size-4 shrink-0" />;
  if (file.type === "pdf")
    return <FileSpreadsheetIcon className="size-4 shrink-0" />;
  if (file.type === "style")
    return <FileCodeIcon className="size-4 shrink-0" />;
  if (file.type === "other") return <FileIcon className="size-4 shrink-0" />;
  return <FileTextIcon className="size-4 shrink-0" />;
}

// ─── dnd-kit helpers ───

export function DroppableRoot({
  children,
  nativeDragOver,
}: {
  children: React.ReactNode;
  nativeDragOver?: boolean;
}) {
  const { setNodeRef, isOver } = useDroppable({ id: "__root__" });
  return (
    <div
      ref={setNodeRef}
      data-drop-folder="__root__"
      className={cn(
        "min-h-0 flex-1 overflow-y-auto p-1",
        (isOver || nativeDragOver) && "bg-accent/30",
      )}
    >
      {children}
    </div>
  );
}

function DroppableFolder({
  id,
  children,
  nativeDragOver,
}: {
  id: string;
  children: React.ReactNode;
  nativeDragOver?: boolean;
}) {
  const { setNodeRef, isOver } = useDroppable({ id });
  return (
    <div
      ref={setNodeRef}
      data-drop-folder={id}
      className={cn((isOver || nativeDragOver) && "rounded-md bg-accent/30")}
    >
      {children}
    </div>
  );
}

export function DraggableItem({
  id,
  type,
  name,
  children,
}: {
  id: string;
  type: "file" | "folder";
  name: string;
  children: React.ReactNode;
}) {
  const { attributes, listeners, setNodeRef, isDragging } = useDraggable({
    id,
    data: { type, name },
  });

  const wrappedListeners = listeners
    ? Object.fromEntries(
        Object.entries(listeners).map(([key, handler]) => [
          key,
          (e: React.PointerEvent) => {
            (handler as (e: React.PointerEvent) => void)(e);
          },
        ]),
      )
    : {};

  return (
    <div
      ref={setNodeRef}
      {...wrappedListeners}
      {...attributes}
      style={{ opacity: isDragging ? 0.4 : 1 }}
    >
      {children}
    </div>
  );
}

// ─── File Tree Node ───

export interface FileTreeNodeProps {
  node: TreeNode;
  depth: number;
  activeFileId: string;
  expandedFolders: Set<string>;
  onToggleFolder: (path: string) => void;
  onSelectFile: (id: string) => void;
  onNewFile: (folder?: string) => void;
  onNewFolder: (parent?: string) => void;
  onImport: (folder?: string) => void;
  onRename: (id: string, name: string) => void;
  onDelete: (id: string) => void;
  onDeleteFolder: (folderPath: string) => void;
  fileCount: number;
  nativeDragOver?: string | null;
}

export function FileTreeNode({
  node,
  depth,
  activeFileId,
  expandedFolders,
  onToggleFolder,
  onSelectFile,
  onNewFile,
  onNewFolder,
  onImport,
  onRename,
  onDelete,
  onDeleteFolder,
  fileCount,
  nativeDragOver,
}: FileTreeNodeProps) {
  const isExpanded = expandedFolders.has(node.relativePath);

  if (node.type === "folder") {
    return (
      <DroppableFolder
        id={node.relativePath}
        nativeDragOver={nativeDragOver === node.relativePath}
      >
        <DraggableItem id={node.relativePath} type="folder" name={node.name}>
          <ContextMenu>
            <ContextMenuTrigger asChild>
              <button
                className="flex w-full items-center gap-1.5 rounded-md px-2 py-1 text-left text-sm transition-colors hover:bg-sidebar-accent/50"
                style={{ paddingLeft: `${depth * 16 + 4}px` }}
                onClick={() => onToggleFolder(node.relativePath)}
              >
                {isExpanded ? (
                  <ChevronDownIcon className="size-3.5 shrink-0 text-muted-foreground" />
                ) : (
                  <ChevronRightIcon className="size-3.5 shrink-0 text-muted-foreground" />
                )}
                <FolderIcon className="size-4 shrink-0" />
                <span className="truncate">{node.name}</span>
              </button>
            </ContextMenuTrigger>
            <ContextMenuContent>
              <ContextMenuItem onClick={() => onNewFile(node.relativePath)}>
                <FileTextIcon className="mr-2 size-4" />
                New File Here
              </ContextMenuItem>
              <ContextMenuItem onClick={() => onNewFolder(node.relativePath)}>
                <FolderPlusIcon className="mr-2 size-4" />
                New Folder
              </ContextMenuItem>
              <ContextMenuItem onClick={() => onImport(node.relativePath)}>
                <UploadIcon className="mr-2 size-4" />
                Import File Here
              </ContextMenuItem>
              <ContextMenuSeparator />
              <ContextMenuItem
                onClick={() => onRename(node.relativePath, node.name)}
              >
                <PencilIcon className="mr-2 size-4" />
                Rename
              </ContextMenuItem>
              <ContextMenuItem
                variant="destructive"
                onClick={() => onDeleteFolder(node.relativePath)}
              >
                <Trash2Icon className="mr-2 size-4" />
                Delete
              </ContextMenuItem>
            </ContextMenuContent>
          </ContextMenu>
        </DraggableItem>
        {isExpanded &&
          node.children.map((child) => (
            <FileTreeNode
              key={child.relativePath}
              node={child}
              depth={depth + 1}
              activeFileId={activeFileId}
              expandedFolders={expandedFolders}
              onToggleFolder={onToggleFolder}
              onSelectFile={onSelectFile}
              onNewFile={onNewFile}
              onNewFolder={onNewFolder}
              onImport={onImport}
              onRename={onRename}
              onDelete={onDelete}
              onDeleteFolder={onDeleteFolder}
              fileCount={fileCount}
              nativeDragOver={nativeDragOver}
            />
          ))}
      </DroppableFolder>
    );
  }

  // File node
  const file = node.file!;
  return (
    <DraggableItem id={file.relativePath} type="file" name={node.name}>
      <ContextMenu>
        <ContextMenuTrigger asChild>
          <button
            className={cn(
              "flex w-full items-center gap-2 rounded-md px-2 py-1.5 text-left text-sm transition-colors",
              file.id === activeFileId
                ? "bg-sidebar-accent text-sidebar-accent-foreground"
                : "hover:bg-sidebar-accent/50",
            )}
            style={{ paddingLeft: `${depth * 16 + 8}px` }}
            onClick={() => {
              useHistoryStore.getState().stopReview();
              onSelectFile(file.id);
            }}
          >
            {getFileIcon(file)}
            <span className="min-w-0 flex-1 truncate">{node.name}</span>
            {file.isDirty && (
              <span
                className="ml-auto size-2 shrink-0 rounded-full bg-blue-500"
                title="Modified"
              />
            )}
          </button>
        </ContextMenuTrigger>
        <ContextMenuContent>
          <ContextMenuItem onClick={() => onRename(file.id, file.name)}>
            <PencilIcon className="mr-2 size-4" />
            Rename
          </ContextMenuItem>
          <ContextMenuItem
            variant="destructive"
            onClick={() => onDelete(file.id)}
            disabled={fileCount <= 1}
          >
            <Trash2Icon className="mr-2 size-4" />
            Delete
          </ContextMenuItem>
        </ContextMenuContent>
      </ContextMenu>
    </DraggableItem>
  );
}
