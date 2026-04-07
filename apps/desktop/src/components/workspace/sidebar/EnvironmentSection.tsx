import { useState, useCallback, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import {
  AppWindowIcon,
  FlaskConicalIcon,
  TerminalIcon,
} from "lucide-react";
import { useUvSetupStore } from "@/stores/uv-setup-store";
import { UvSetupDialog } from "@/components/uv-setup";
import { cn } from "@/lib/utils";

interface SkillsStatus {
  installed: boolean;
  skill_count: number;
  location: string;
}

export function EnvironmentSection({ projectPath }: { projectPath: string | null }) {
  const venvReady = useUvSetupStore((s) => s.venvReady);
  const uvStatus = useUvSetupStore((s) => s.status);
  const [showUvDialog, setShowUvDialog] = useState(false);

  const [skillsStatus, setSkillsStatus] = useState<SkillsStatus | null>(null);
  const [showOnboarding, setShowOnboarding] = useState(false);

  const checkSkillsStatus = useCallback(async () => {
    try {
      const globalStatus = await invoke<SkillsStatus>(
        "check_skills_installed",
        {
          projectPath: null,
        },
      );
      if (globalStatus.installed) {
        setSkillsStatus(globalStatus);
        return;
      }
      if (projectPath) {
        const projectStatus = await invoke<SkillsStatus>(
          "check_skills_installed",
          {
            projectPath,
          },
        );
        setSkillsStatus(projectStatus);
      } else {
        setSkillsStatus(globalStatus);
      }
    } catch {
      // Ignore errors silently
    }
  }, [projectPath]);

  useEffect(() => {
    checkSkillsStatus();
  }, [checkSkillsStatus]);

  const [OnboardingComponent, setOnboardingComponent] =
    useState<React.ComponentType<{
      onClose: () => void;
    }> | null>(null);

  useEffect(() => {
    if (showOnboarding && !OnboardingComponent) {
      import(
        "@/components/scientific-skills/scientific-skills-onboarding"
      ).then((mod) =>
        setOnboardingComponent(() => mod.ScientificSkillsOnboarding),
      );
    }
  }, [showOnboarding, OnboardingComponent]);

  const pythonLabel = venvReady
    ? "Active"
    : uvStatus === "not-installed"
      ? "Not installed"
      : uvStatus === "ready"
        ? "No venv"
        : "";
  const skillsLabel = skillsStatus?.installed
    ? `${skillsStatus.skill_count} skills`
    : "Not installed";

  return (
    <>
      <div className="border-sidebar-border border-t">
        <div className="flex h-8 shrink-0 items-center justify-center gap-2 px-3">
          <AppWindowIcon className="size-3.5 text-muted-foreground" />
          <span className="font-medium text-xs">Environment</span>
        </div>
        <div className="space-y-0.5 px-1 pb-1.5">
          <button
            className="flex w-full min-w-0 items-center gap-2 rounded-md px-2 py-1 text-left text-sm transition-colors hover:bg-sidebar-accent/50"
            onClick={() => setShowUvDialog(true)}
          >
            <TerminalIcon
              className={cn(
                "size-3.5 shrink-0",
                venvReady ? "text-foreground" : "text-muted-foreground",
              )}
            />
            <span className="min-w-0 flex-1 truncate text-xs">Python</span>
            <span
              className={cn(
                "shrink-0 text-xs",
                venvReady ? "text-foreground" : "text-muted-foreground",
              )}
            >
              {pythonLabel}
            </span>
          </button>
          <button
            className="flex w-full min-w-0 items-center gap-2 rounded-md px-2 py-1 text-left text-sm transition-colors hover:bg-sidebar-accent/50"
            onClick={() => setShowOnboarding(true)}
          >
            <FlaskConicalIcon
              className={cn(
                "size-3.5 shrink-0",
                skillsStatus?.installed
                  ? "text-foreground"
                  : "text-muted-foreground",
              )}
            />
            <span className="min-w-0 flex-1 truncate text-xs">Skills</span>
            <span
              className={cn(
                "shrink-0 text-xs",
                skillsStatus?.installed
                  ? "text-foreground"
                  : "text-muted-foreground",
              )}
            >
              {skillsLabel}
            </span>
          </button>
        </div>
      </div>

      <UvSetupDialog
        open={showUvDialog}
        onClose={() => setShowUvDialog(false)}
      />

      {showOnboarding && OnboardingComponent && (
        <OnboardingComponent
          onClose={() => {
            setShowOnboarding(false);
            checkSkillsStatus();
          }}
        />
      )}
    </>
  );
}
