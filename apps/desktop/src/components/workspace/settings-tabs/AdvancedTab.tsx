import { LoaderIcon } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Label } from "@/components/ui/label";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Switch } from "@/components/ui/switch";
import { Textarea } from "@/components/ui/textarea";
import type { ProviderConnectivityResult, SettingsImportMode } from "@/lib/settings-api";
import { panelClass, type EffectiveSettings } from "./types";

interface AdvancedTabProps {
  effective: EffectiveSettings;
  patchGlobal: (patch: any, projectRoot: string | null) => Promise<unknown>;
  projectRoot: string | null;
  // Connectivity (moved from ProvidersTab)
  isSaving: boolean;
  isTestingProviders: boolean;
  providerConnectivity: ProviderConnectivityResult[];
  handleTestProviderConnectivity: () => Promise<void>;
  connectivityBadge: (result: ProviderConnectivityResult) => { label: string; className: string };
  // Import/Export (moved from IoTab)
  importText: string;
  setImportText: (v: string) => void;
  handleExport: (includeProject: boolean) => Promise<void>;
  handleImport: (mode: SettingsImportMode) => Promise<void>;
  setConfirmResetGlobalOpen: (v: boolean) => void;
}

export function AdvancedTab({
  effective,
  patchGlobal,
  projectRoot,
  isSaving,
  isTestingProviders,
  providerConnectivity,
  handleTestProviderConnectivity,
  connectivityBadge,
  importText,
  setImportText,
  handleExport,
  handleImport,
  setConfirmResetGlobalOpen,
}: AdvancedTabProps) {
  return (
    <div className="space-y-4">
      {/* Debug Logging */}
      <div className="flex items-center justify-between rounded-xl border border-sidebar-border/70 bg-sidebar-accent/20 p-4">
        <div>
          <p className="font-medium text-sm">Debug Logging</p>
          <p className="text-muted-foreground text-xs">
            开启后输出 debug 级日志
          </p>
        </div>
        <Switch
          checked={effective.advanced.debugEnabled}
          onCheckedChange={(checked: boolean) =>
            void patchGlobal(
              { advanced: { debugEnabled: checked } },
              projectRoot,
            )
          }
        />
      </div>

      <div className="space-y-2">
        <Label>Log Level</Label>
        <Select
          value={effective.advanced.logLevel}
          onValueChange={(value: string) =>
            void patchGlobal(
              { advanced: { logLevel: value } },
              projectRoot,
            )
          }
        >
          <SelectTrigger className="w-full">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            <SelectItem value="info">info</SelectItem>
            <SelectItem value="debug">debug</SelectItem>
            <SelectItem value="warn">warn</SelectItem>
            <SelectItem value="error">error</SelectItem>
          </SelectContent>
        </Select>
      </div>

      {/* Provider Connectivity */}
      <div className={panelClass}>
        <div className="flex items-start justify-between gap-3">
          <div>
            <p className="font-medium text-sm">Provider Connectivity</p>
            <p className="text-muted-foreground text-xs">
              区分 `Base URL reachable`、`Responses compatible` 与
              `Chat Completions compatible`，避免 `/models` 的假阳性。
            </p>
          </div>
          <Button
            variant="outline"
            size="sm"
            onClick={() => void handleTestProviderConnectivity()}
            disabled={isSaving || isTestingProviders}
          >
            {isTestingProviders ? (
              <>
                <LoaderIcon className="mr-1 size-3 animate-spin" />
                Testing
              </>
            ) : (
              "Test All"
            )}
          </Button>
        </div>

        {providerConnectivity.length === 0 ? (
          <p className="text-muted-foreground text-xs">
            尚未运行连通性测试。
          </p>
        ) : (
          <div className="space-y-2">
            {providerConnectivity.map((result) => {
              const badge = connectivityBadge(result);
              return (
                <div
                  key={`${result.provider}-${result.capability}`}
                  className="rounded-md border border-sidebar-border/60 px-2 py-1.5 text-xs"
                >
                  <div className="flex items-center justify-between gap-2">
                    <span className="font-medium">{result.label}</span>
                    <span className={badge.className}>
                      {badge.label}
                    </span>
                  </div>
                  <p className="mt-0.5 text-muted-foreground">
                    {result.message}
                  </p>
                  <p className="mt-0.5 break-all text-muted-foreground/80">
                    {result.endpoint}
                  </p>
                  <p className="mt-0.5 text-muted-foreground/80">
                    status: {result.status ?? "-"} · {result.latencyMs}
                    ms
                  </p>
                </div>
              );
            })}
          </div>
        )}
      </div>

      {/* Import / Export */}
      <div className={panelClass}>
        <p className="font-medium text-sm">Import / Export</p>
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
      </div>

      {/* Danger Zone */}
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
