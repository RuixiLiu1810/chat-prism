import { Button } from "@/components/ui/button";
import { Label } from "@/components/ui/label";
import { Textarea } from "@/components/ui/textarea";
import type { SettingsImportMode } from "@/lib/settings-api";

interface IoTabProps {
  isSaving: boolean;
  importText: string;
  setImportText: (v: string) => void;
  handleExport: (includeProject: boolean) => Promise<void>;
  handleImport: (mode: SettingsImportMode) => Promise<void>;
  setConfirmResetGlobalOpen: (v: boolean) => void;
}

export function IoTab({
  isSaving,
  importText,
  setImportText,
  handleExport,
  handleImport,
  setConfirmResetGlobalOpen,
}: IoTabProps) {
  return (
    <div className="space-y-4">
      <div className="flex flex-wrap gap-2">
        <Button
          variant="outline"
          size="sm"
          onClick={() => void handleExport(false)}
          disabled={isSaving}
        >
          导出 Global（复制到剪贴板）
        </Button>
        <Button
          variant="outline"
          size="sm"
          onClick={() => void handleExport(true)}
          disabled={isSaving}
        >
          导出 Global+Project
        </Button>
      </div>

      <div className="space-y-2">
        <Label>Import JSON</Label>
        <Textarea
          className="min-h-40"
          placeholder='粘贴 JSON，例如 {"global": {...}, "project": {...}}'
          value={importText}
          onChange={(e) => setImportText(e.target.value)}
        />
      </div>

      <div className="flex gap-2">
        <Button
          variant="outline"
          size="sm"
          onClick={() => void handleImport("merge")}
          disabled={isSaving || !importText.trim()}
        >
          Merge Import
        </Button>
        <Button
          variant="outline"
          size="sm"
          onClick={() => void handleImport("replace")}
          disabled={isSaving || !importText.trim()}
        >
          Replace Import
        </Button>
      </div>

      <div className="space-y-2 rounded-xl border border-destructive/40 bg-destructive/5 p-4">
        <p className="font-medium text-sm">Danger Zone</p>
        <p className="text-muted-foreground text-xs">
          重置 Global Settings 会影响所有项目配置。
        </p>
        <Button
          variant="destructive"
          size="sm"
          onClick={() => setConfirmResetGlobalOpen(true)}
          disabled={isSaving}
        >
          重置 Global Settings
        </Button>
      </div>
    </div>
  );
}
