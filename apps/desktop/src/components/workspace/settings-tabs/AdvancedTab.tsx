import { Label } from "@/components/ui/label";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Switch } from "@/components/ui/switch";
import type { EffectiveSettings } from "./types";

interface AdvancedTabProps {
  effective: EffectiveSettings;
  patchGlobal: (patch: any, projectRoot: string | null) => Promise<unknown>;
  projectRoot: string | null;
}

export function AdvancedTab({
  effective,
  patchGlobal,
  projectRoot,
}: AdvancedTabProps) {
  return (
    <div className="space-y-4">
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
    </div>
  );
}
