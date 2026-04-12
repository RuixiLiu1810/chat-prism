import { type FC, useEffect } from "react";
import { Check, FileEditIcon, X } from "lucide-react";
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from "@/components/ui/tooltip";
import { type ProposedChange } from "@/stores/proposed-changes-store";

interface ProposedChangesPanelProps {
  change: ProposedChange;
  changeIndex: number;
  totalChanges: number;
  onKeep: () => void;
  onUndo: () => void;
}

export const ProposedChangesPanel: FC<ProposedChangesPanelProps> = ({
  change,
  changeIndex,
  totalChanges,
  onKeep,
  onUndo,
}) => {
  useEffect(() => {
    const handler = (e: KeyboardEvent) => {
      if (!(e.metaKey || e.ctrlKey) || !e.shiftKey) return;
      if (e.key === "k" || e.key === "K") {
        e.preventDefault();
        onKeep();
      } else if (e.key === "z" || e.key === "Z") {
        e.preventDefault();
        onUndo();
      }
    };
    window.addEventListener("keydown", handler);
    return () => window.removeEventListener("keydown", handler);
  }, [onKeep, onUndo]);

  const oldLines = change.oldContent.split("\n").length;
  const newLines = change.newContent.split("\n").length;
  const added = Math.max(0, newLines - oldLines);
  const removed = Math.max(0, oldLines - newLines);

  return (
    <div className="flex items-center justify-between border-border border-t bg-muted/50 px-3 py-1.5">
      {/* Left side: icon + label + count */}
      <Tooltip>
        <TooltipTrigger asChild>
          <div className="flex items-center gap-2 text-sm">
            <FileEditIcon className="size-3.5 text-muted-foreground" />
            <span className="font-medium text-foreground">Proposed Changes</span>
            {totalChanges > 1 && (
              <span className="rounded bg-violet-500/15 px-1.5 py-0.5 font-medium text-violet-600 text-xs dark:text-violet-400">
                {changeIndex + 1}/{totalChanges}
              </span>
            )}
          </div>
        </TooltipTrigger>
        <TooltipContent side="top" className="max-w-xs">
          <p className="font-medium">{change.filePath}</p>
          <p className="mt-0.5 opacity-80">
            {change.toolName}
            {added > 0 ? ` +${added}` : ""}
            {removed > 0 ? ` -${removed}` : ""}
          </p>
        </TooltipContent>
      </Tooltip>

      {/* Right side: prominent action buttons */}
      <div className="flex items-center gap-1.5">
        <Tooltip>
          <TooltipTrigger asChild>
            <button
              onClick={onKeep}
              className="flex items-center gap-1 rounded-md bg-green-600/20 px-2.5 py-1 font-medium text-green-400 text-xs transition-colors hover:bg-green-600/30"
            >
              <Check className="size-3.5" />
              Keep All
            </button>
          </TooltipTrigger>
          <TooltipContent side="top">⌘⇧K</TooltipContent>
        </Tooltip>
        <Tooltip>
          <TooltipTrigger asChild>
            <button
              onClick={onUndo}
              className="flex items-center gap-1 rounded-md bg-red-600/20 px-2.5 py-1 font-medium text-red-400 text-xs transition-colors hover:bg-red-600/30"
            >
              <X className="size-3.5" />
              Undo All
            </button>
          </TooltipTrigger>
          <TooltipContent side="top">⌘⇧Z</TooltipContent>
        </Tooltip>
      </div>
    </div>
  );
};
