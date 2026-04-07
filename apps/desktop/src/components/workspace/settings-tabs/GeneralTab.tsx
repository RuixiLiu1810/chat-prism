import { Label } from "@/components/ui/label";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import type { EffectiveSettings } from "./types";

interface GeneralTabProps {
  effective: EffectiveSettings;
  setTheme: (theme: string) => void;
  setThemePreference: (theme: "system" | "light" | "dark", projectRoot: string | null) => Promise<unknown>;
  patchGlobal: (patch: any, projectRoot: string | null) => Promise<unknown>;
  projectRoot: string | null;
}

export function GeneralTab({
  effective,
  setTheme,
  setThemePreference,
  patchGlobal,
  projectRoot,
}: GeneralTabProps) {
  return (
    <div className="space-y-4">
      <div className="space-y-2">
        <Label>Theme</Label>
        <Select
          value={effective.general.theme}
          onValueChange={async (value: string) => {
            const next = value as "system" | "light" | "dark";
            setTheme(next);
            await setThemePreference(next, projectRoot);
          }}
        >
          <SelectTrigger className="w-full">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            <SelectItem value="system">System</SelectItem>
            <SelectItem value="light">Light</SelectItem>
            <SelectItem value="dark">Dark</SelectItem>
          </SelectContent>
        </Select>
      </div>

      <div className="space-y-2">
        <Label>Language</Label>
        <Select
          value={effective.general.language}
          onValueChange={(value: string) =>
            void patchGlobal(
              { general: { language: value } },
              projectRoot,
            )
          }
        >
          <SelectTrigger className="w-full">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            <SelectItem value="zh-CN">zh-CN</SelectItem>
            <SelectItem value="en-US">en-US</SelectItem>
          </SelectContent>
        </Select>
      </div>

      <div className="space-y-2">
        <Label>Open In Editor</Label>
        <Select
          value={effective.general.openInEditor.defaultEditor}
          onValueChange={(value: string) =>
            void patchGlobal(
              { general: { openInEditor: { defaultEditor: value } } },
              projectRoot,
            )
          }
        >
          <SelectTrigger className="w-full">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            <SelectItem value="system">System</SelectItem>
            <SelectItem value="cursor">Cursor</SelectItem>
            <SelectItem value="vscode">VS Code</SelectItem>
            <SelectItem value="zed">Zed</SelectItem>
            <SelectItem value="sublime">Sublime</SelectItem>
          </SelectContent>
        </Select>
      </div>
    </div>
  );
}
