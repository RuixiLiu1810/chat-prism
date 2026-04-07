import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
  DialogFooter,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";

// ─── New File Dialog ───

interface NewFileDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  folder?: string;
  fileName: string;
  onFileNameChange: (name: string) => void;
  nameError: string;
  onNameErrorChange: (error: string) => void;
  onSubmit: () => void;
}

export function NewFileDialog({
  open,
  onOpenChange,
  folder,
  fileName,
  onFileNameChange,
  nameError,
  onNameErrorChange,
  onSubmit,
}: NewFileDialogProps) {
  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-md">
        <DialogHeader>
          <DialogTitle>
            New File{folder ? ` in ${folder}` : ""}
          </DialogTitle>
        </DialogHeader>
        <div className="space-y-2 py-4">
          <Input
            placeholder="filename.tex"
            value={fileName}
            onChange={(e) => {
              onFileNameChange(e.target.value);
              onNameErrorChange("");
            }}
            onKeyDown={(e) => {
              if (e.key === "Enter") onSubmit();
            }}
            autoFocus
          />
          {nameError && (
            <p className="text-destructive text-xs">{nameError}</p>
          )}
        </div>
        <DialogFooter>
          <Button variant="outline" onClick={() => onOpenChange(false)}>
            Cancel
          </Button>
          <Button onClick={onSubmit} disabled={!fileName.trim()}>
            Create
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

// ─── New Folder Dialog ───

interface NewFolderDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  parent?: string;
  folderName: string;
  onFolderNameChange: (name: string) => void;
  nameError: string;
  onNameErrorChange: (error: string) => void;
  onSubmit: () => void;
}

export function NewFolderDialog({
  open,
  onOpenChange,
  parent,
  folderName,
  onFolderNameChange,
  nameError,
  onNameErrorChange,
  onSubmit,
}: NewFolderDialogProps) {
  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-md">
        <DialogHeader>
          <DialogTitle>
            New Folder{parent ? ` in ${parent}` : ""}
          </DialogTitle>
        </DialogHeader>
        <div className="space-y-2 py-4">
          <Input
            placeholder="folder name"
            value={folderName}
            onChange={(e) => {
              onFolderNameChange(e.target.value);
              onNameErrorChange("");
            }}
            onKeyDown={(e) => {
              if (e.key === "Enter") onSubmit();
            }}
            autoFocus
          />
          {nameError && (
            <p className="text-destructive text-xs">{nameError}</p>
          )}
        </div>
        <DialogFooter>
          <Button variant="outline" onClick={() => onOpenChange(false)}>
            Cancel
          </Button>
          <Button onClick={onSubmit} disabled={!folderName.trim()}>
            Create
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

// ─── Rename Dialog ───

interface RenameDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  renameValue: string;
  onRenameValueChange: (value: string) => void;
  nameError: string;
  onNameErrorChange: (error: string) => void;
  onSubmit: () => void;
}

export function RenameDialog({
  open,
  onOpenChange,
  renameValue,
  onRenameValueChange,
  nameError,
  onNameErrorChange,
  onSubmit,
}: RenameDialogProps) {
  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-md">
        <DialogHeader>
          <DialogTitle>Rename</DialogTitle>
        </DialogHeader>
        <div className="space-y-2 py-4">
          <Input
            value={renameValue}
            onChange={(e) => {
              onRenameValueChange(e.target.value);
              onNameErrorChange("");
            }}
            onKeyDown={(e) => {
              if (e.key === "Enter") onSubmit();
            }}
            autoFocus
          />
          {nameError && (
            <p className="text-destructive text-xs">{nameError}</p>
          )}
        </div>
        <DialogFooter>
          <Button variant="outline" onClick={() => onOpenChange(false)}>
            Cancel
          </Button>
          <Button onClick={onSubmit}>Rename</Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
