import { ThemeProvider } from "next-themes";
import { ErrorBoundary } from "react-error-boundary";
import { Toaster } from "@/components/ui/sonner";
import { useKeyboardShortcuts } from "@/hooks/use-keyboard-shortcuts";

import { useDocumentStore } from "@/stores/document-store";
import { useAgentChatStore } from "@/stores/agent-chat-store";
import { ProjectPicker } from "@/components/project-picker";
import { WorkspaceLayout } from "@/components/workspace/workspace-layout";
import { useEffect, useState } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { TooltipProvider } from "@/components/ui/tooltip";
import {
  ScientificSkillsOnboarding,
  shouldShowOnboarding,
} from "@/components/scientific-skills/scientific-skills-onboarding";
import { useUvSetupStore } from "@/stores/uv-setup-store";
import { useSettingsStore } from "@/stores/settings-store";
import { ErrorFallback } from "@/components/error-fallback";
import { createLogger } from "@/lib/debug/logger";

const log = createLogger("app");

function WorkspaceWithAgent() {
  const projectRoot = useDocumentStore((s) => s.projectRoot);
  const initialized = useDocumentStore((s) => s.initialized);
  const [showSkillsOnboarding, setShowSkillsOnboarding] = useState(false);

  // Update window title
  useEffect(() => {
    if (projectRoot) {
      const name = projectRoot.split(/[/\\]/).pop() || "ClaudePrism";
      getCurrentWindow().setTitle(`${name} - ClaudePrism`);
    }
  }, [projectRoot]);

  // Show scientific skills onboarding on first launch
  useEffect(() => {
    if (!initialized) return;
    if (shouldShowOnboarding()) {
      // Small delay so the workspace renders first
      const timer = setTimeout(() => setShowSkillsOnboarding(true), 800);
      return () => clearTimeout(timer);
    }
  }, [initialized]);

  // Auto-setup Python venv when project opens
  useEffect(() => {
    if (!initialized || !projectRoot) return;
    const uvStore = useUvSetupStore.getState();
    uvStore
      .checkStatus()
      .then(() => {
        const { status } = useUvSetupStore.getState();
        if (status === "ready") {
          return uvStore.setupVenv(projectRoot);
        }
      })
      .catch((err) => {
        log.error("Failed to setup Python venv", { error: String(err) });
      });
  }, [initialized, projectRoot]);

  // Consume pending initial prompt from project wizard
  useEffect(() => {
    if (!initialized) return;
    // Delay to let the chat drawer mount and register event listeners
    const timer = setTimeout(() => {
      const prompt = useAgentChatStore
        .getState()
        .consumePendingInitialPrompt();
      if (prompt) {
        useAgentChatStore.getState().sendPrompt(prompt);
      }
    }, 500);
    return () => clearTimeout(timer);
  }, [initialized]);

  return (
    <>
      <WorkspaceLayout />
      {showSkillsOnboarding && (
        <ScientificSkillsOnboarding
          onClose={() => setShowSkillsOnboarding(false)}
        />
      )}
    </>
  );
}

export function App({ onReady }: { onReady?: () => void }) {
  const projectRoot = useDocumentStore((s) => s.projectRoot);
  const loadSettings = useSettingsStore((s) => s.load);

  // Register global keyboard shortcuts (Cmd+S, Cmd+N) at the app level
  useKeyboardShortcuts();

  useEffect(() => {
    onReady?.();
  }, [onReady]);

  useEffect(() => {
    loadSettings(projectRoot).catch((err) => {
      log.warn("Settings migration bootstrap failed", { error: String(err) });
    });
  }, [loadSettings, projectRoot]);

  return (
    <ErrorBoundary FallbackComponent={ErrorFallback}>
      <ThemeProvider attribute="class" defaultTheme="light" enableSystem>
        <TooltipProvider>
          {/* Global macOS titlebar drag region — sits above all content */}
          <div
            data-tauri-drag-region
            className="fixed inset-x-0 top-0 z-[9999] h-[var(--titlebar-height)]"
          />
          {projectRoot ? <WorkspaceWithAgent /> : <ProjectPicker />}
          <Toaster />
        </TooltipProvider>
      </ThemeProvider>
    </ErrorBoundary>
  );
}
