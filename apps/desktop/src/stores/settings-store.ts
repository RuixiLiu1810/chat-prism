import { create } from "zustand";
import { createLogger } from "@/lib/debug/logger";
import {
  settingsExport,
  settingsGet,
  settingsImport,
  settingsReset,
  settingsSet,
  type SettingsImportMode,
} from "@/lib/settings-api";
import {
  type CitationStylePolicy,
  type EffectiveSettingsV1,
  type GlobalSettingsV1,
  type LogLevel,
  type ProjectSettingsV1,
  type SecretsMeta,
  type SettingsEnvelope,
  type ThemePreference,
} from "@/lib/settings-schema";
import { useCitationStore } from "@/stores/citation-store";

const log = createLogger("settings-store");

const INITIAL_EFFECTIVE: EffectiveSettingsV1 = {
  general: {
    theme: "system",
    language: "zh-CN",
    openInEditor: {
      defaultEditor: "system",
    },
  },
  citation: {
    stylePolicy: "auto",
    autoApplyThreshold: 0.78,
    reviewThreshold: 0.62,
    search: {
      limit: 8,
      llmQuery: {
        enabled: false,
        model: "gpt-4o-mini",
        endpoint: "https://api.openai.com/v1/chat/completions",
        timeoutMs: 6000,
        maxQueries: 3,
      },
      queryEmbedding: {
        provider: "none",
        timeoutMs: 1200,
      },
      queryExecution: {
        topN: 5,
        mmrLambda: 0.72,
        minQuality: 0.24,
        minHitRatio: 0.45,
        hitScoreThreshold: 0.58,
      },
    },
  },
  integrations: {
    agent: {
      runtime: "claude_cli",
      provider: "openai",
      model: "gpt-5.4",
      baseUrl: "https://api.openai.com/v1",
      samplingProfiles: {
        editStable: {
          temperature: 0.2,
          topP: 0.9,
          maxTokens: 8192,
        },
        analysisBalanced: {
          temperature: 0.4,
          topP: 0.9,
          maxTokens: 6144,
        },
        analysisDeep: {
          temperature: 0.3,
          topP: 0.92,
          maxTokens: 12288,
        },
        chatFlexible: {
          temperature: 0.7,
          topP: 0.95,
          maxTokens: 4096,
        },
      },
    },
    semanticScholar: {
      enabled: true,
    },
    zotero: {
      autoSyncOnApply: true,
    },
  },
  advanced: {
    debugEnabled: false,
    logLevel: "info",
  },
};

function defaultSecretsMeta(): SecretsMeta {
  return {
    integrations: {
      agent: {
        apiKeyConfigured: false,
      },
      semanticScholar: {
        apiKeyConfigured: false,
      },
      llmQuery: {
        apiKeyConfigured: false,
      },
    },
  };
}

function normalizeInvokeError(err: unknown): string {
  if (err instanceof Error) return err.message;
  if (typeof err === "string") return err;
  return "Settings operation failed.";
}

function syncDebugFlag(enabled: boolean) {
  const prev = window.localStorage.getItem("debug");
  if (enabled) {
    window.localStorage.setItem("debug", "1");
  } else {
    window.localStorage.removeItem("debug");
  }
  const next = enabled ? "1" : null;
  try {
    window.dispatchEvent(
      new StorageEvent("storage", {
        key: "debug",
        oldValue: prev,
        newValue: next,
        storageArea: window.localStorage,
      }),
    );
  } catch {
    // No-op if StorageEvent ctor is restricted.
  }
}

function syncLogLevel(level: LogLevel) {
  const prev = window.localStorage.getItem("logLevel");
  window.localStorage.setItem("logLevel", level);
  try {
    window.dispatchEvent(
      new StorageEvent("storage", {
        key: "logLevel",
        oldValue: prev,
        newValue: level,
        storageArea: window.localStorage,
      }),
    );
  } catch {
    // No-op if StorageEvent ctor is restricted.
  }
}

function applyRuntimeSettings(effective: EffectiveSettingsV1) {
  const citationStore = useCitationStore.getState();
  citationStore.setCitationStylePolicy(effective.citation.stylePolicy);
  citationStore.setRuntimeConfig({
    autoApplyThreshold: effective.citation.autoApplyThreshold,
    reviewThreshold: effective.citation.reviewThreshold,
    searchLimit: effective.citation.search.limit,
    zoteroAutoSyncOnApply: effective.integrations.zotero.autoSyncOnApply,
  });
  syncDebugFlag(effective.advanced.debugEnabled);
  syncLogLevel(effective.advanced.logLevel);
}

interface SettingsState {
  isLoading: boolean;
  isSaving: boolean;
  error: string | null;
  warnings: string[];
  effective: EffectiveSettingsV1;
  global: SettingsEnvelope<GlobalSettingsV1>;
  project: SettingsEnvelope<ProjectSettingsV1>;
  secretsMeta: SecretsMeta;
  load: (projectRoot?: string | null) => Promise<void>;
  patchGlobal: (
    patch: Record<string, unknown>,
    projectRoot?: string | null,
  ) => Promise<boolean>;
  patchProject: (
    patch: Record<string, unknown>,
    projectRoot?: string | null,
  ) => Promise<boolean>;
  patchSecret: (
    patch: Record<string, unknown>,
    projectRoot?: string | null,
  ) => Promise<boolean>;
  resetScope: (
    scope: "global" | "project" | "secret",
    options?: { keys?: string[]; projectRoot?: string | null },
  ) => Promise<boolean>;
  exportJson: (options?: {
    projectRoot?: string | null;
    includeProject?: boolean;
  }) => Promise<string | null>;
  importJson: (options: {
    jsonText: string;
    mode: SettingsImportMode;
    projectRoot?: string | null;
  }) => Promise<boolean>;
  setCitationStylePolicy: (
    policy: CitationStylePolicy,
    projectRoot?: string | null,
  ) => Promise<boolean>;
  setThemePreference: (
    theme: ThemePreference,
    projectRoot?: string | null,
  ) => Promise<boolean>;
}

export const useSettingsStore = create<SettingsState>()((set, get) => ({
  isLoading: false,
  isSaving: false,
  error: null,
  warnings: [],
  effective: INITIAL_EFFECTIVE,
  global: {
    version: 1,
    data: INITIAL_EFFECTIVE,
  },
  project: {
    version: 1,
    data: {},
  },
  secretsMeta: defaultSecretsMeta(),

  load: async (projectRoot) => {
    set({ isLoading: true, error: null });
    try {
      const loaded = await settingsGet(projectRoot);
      applyRuntimeSettings(loaded.effective);
      set({
        isLoading: false,
        effective: loaded.effective,
        global: loaded.global,
        project: loaded.project,
        secretsMeta: loaded.secretsMeta,
        warnings: loaded.warnings,
      });
    } catch (err) {
      set({
        isLoading: false,
        error: normalizeInvokeError(err),
      });
    }
  },

  patchGlobal: async (patch, projectRoot) => {
    set({ isSaving: true, error: null });
    try {
      const result = await settingsSet({
        scope: "global",
        patch,
        projectRoot,
      });
      if (!result.ok) {
        set({
          isSaving: false,
          warnings: result.warnings,
          error:
            result.errors.map((e) => `${e.path}: ${e.message}`).join(" | ") ||
            "Invalid global settings patch.",
        });
        return false;
      }
      await get().load(projectRoot);
      set({
        isSaving: false,
        warnings: result.warnings,
      });
      return true;
    } catch (err) {
      set({
        isSaving: false,
        error: normalizeInvokeError(err),
      });
      return false;
    }
  },

  patchProject: async (patch, projectRoot) => {
    set({ isSaving: true, error: null });
    try {
      const result = await settingsSet({
        scope: "project",
        patch,
        projectRoot,
      });
      if (!result.ok) {
        set({
          isSaving: false,
          warnings: result.warnings,
          error:
            result.errors.map((e) => `${e.path}: ${e.message}`).join(" | ") ||
            "Invalid project settings patch.",
        });
        return false;
      }
      await get().load(projectRoot);
      set({
        isSaving: false,
        warnings: result.warnings,
      });
      return true;
    } catch (err) {
      set({
        isSaving: false,
        error: normalizeInvokeError(err),
      });
      return false;
    }
  },

  patchSecret: async (patch, projectRoot) => {
    set({ isSaving: true, error: null });
    try {
      const result = await settingsSet({
        scope: "secret",
        patch,
        projectRoot,
      });
      if (!result.ok) {
        set({
          isSaving: false,
          warnings: result.warnings,
          error:
            result.errors.map((e) => `${e.path}: ${e.message}`).join(" | ") ||
            "Invalid secret settings patch.",
        });
        return false;
      }
      await get().load(projectRoot);
      set({
        isSaving: false,
        warnings: result.warnings,
      });
      return true;
    } catch (err) {
      set({
        isSaving: false,
        error: normalizeInvokeError(err),
      });
      return false;
    }
  },

  resetScope: async (scope, options) => {
    set({ isSaving: true, error: null });
    try {
      const result = await settingsReset({
        scope,
        keys: options?.keys,
        projectRoot: options?.projectRoot,
      });
      if (!result.ok) {
        set({
          isSaving: false,
          warnings: result.warnings,
          error:
            result.errors.map((e) => `${e.path}: ${e.message}`).join(" | ") ||
            "Settings reset failed.",
        });
        return false;
      }
      await get().load(options?.projectRoot);
      set({
        isSaving: false,
        warnings: result.warnings,
      });
      return true;
    } catch (err) {
      set({
        isSaving: false,
        error: normalizeInvokeError(err),
      });
      return false;
    }
  },

  exportJson: async (options) => {
    set({ isSaving: true, error: null });
    try {
      const exported = await settingsExport({
        projectRoot: options?.projectRoot,
        includeProject: options?.includeProject,
      });
      set({
        isSaving: false,
        warnings: exported.warnings,
      });
      return JSON.stringify(exported.data, null, 2);
    } catch (err) {
      set({
        isSaving: false,
        error: normalizeInvokeError(err),
      });
      return null;
    }
  },

  importJson: async ({ jsonText, mode, projectRoot }) => {
    set({ isSaving: true, error: null });
    let parsed: Record<string, unknown>;
    try {
      parsed = JSON.parse(jsonText) as Record<string, unknown>;
    } catch {
      set({
        isSaving: false,
        error: "Import JSON is invalid.",
      });
      return false;
    }

    try {
      const result = await settingsImport({
        json: parsed,
        mode,
        projectRoot,
      });
      if (!result.ok) {
        set({
          isSaving: false,
          warnings: result.warnings,
          error:
            result.errors.map((e) => `${e.path}: ${e.message}`).join(" | ") ||
            "Settings import failed.",
        });
        return false;
      }
      await get().load(projectRoot);
      set({
        isSaving: false,
        warnings: result.warnings,
      });
      return true;
    } catch (err) {
      set({
        isSaving: false,
        error: normalizeInvokeError(err),
      });
      return false;
    }
  },

  setCitationStylePolicy: async (policy, projectRoot) => {
    const ok = await get().patchGlobal(
      {
        citation: {
          stylePolicy: policy,
        },
      },
      projectRoot,
    );
    if (!ok) {
      log.warn("Failed to persist citation style policy");
    }
    return ok;
  },

  setThemePreference: async (theme, projectRoot) => {
    const ok = await get().patchGlobal(
      {
        general: {
          theme,
        },
      },
      projectRoot,
    );
    if (!ok) {
      log.warn("Failed to persist theme preference");
    }
    return ok;
  },
}));
