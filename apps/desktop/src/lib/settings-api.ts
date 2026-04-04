import { invoke } from "@tauri-apps/api/core";
import { createLogger } from "@/lib/debug/logger";
import type {
  EffectiveSettingsV1,
  GlobalSettingsV1,
  ProjectSettingsV1,
  SecretsMeta,
  SettingsEnvelope,
} from "@/lib/settings-schema";

const log = createLogger("settings");

export type SettingsScope = "global" | "project" | "secret";
export type SettingsImportMode = "merge" | "replace";

export interface SettingsFieldError {
  path: string;
  message: string;
}

export interface SettingsMutationResponse {
  ok: boolean;
  errors: SettingsFieldError[];
  warnings: string[];
}

export interface SettingsGetResponse {
  effective: EffectiveSettingsV1;
  global: SettingsEnvelope<GlobalSettingsV1>;
  project: SettingsEnvelope<ProjectSettingsV1>;
  secretsMeta: SecretsMeta;
  warnings: string[];
}

export interface SettingsExportResponse {
  data: {
    version: number;
    global: GlobalSettingsV1;
    project?: ProjectSettingsV1;
  };
  warnings: string[];
}

export interface ProviderConnectivityResult {
  provider: string;
  label: string;
  capability: string;
  endpoint: string;
  ok: boolean;
  reachable: boolean;
  compatibility: "unknown" | "supported" | "unsupported";
  status: number | null;
  latencyMs: number;
  message: string;
}

export interface AgentSmokeStep {
  name: string;
  ok: boolean;
  detail: string;
}

export interface AgentSmokeResult {
  provider: string;
  runtimeMode: string;
  ok: boolean;
  steps: AgentSmokeStep[];
}

function normalizeInvokeError(err: unknown): Error {
  if (err instanceof Error) return err;
  if (typeof err === "string") return new Error(err);
  return new Error("Settings command failed.");
}

export async function settingsGet(
  projectRoot?: string | null,
): Promise<SettingsGetResponse> {
  try {
    return await invoke<SettingsGetResponse>("settings_get", {
      projectRoot: projectRoot ?? null,
    });
  } catch (err) {
    throw normalizeInvokeError(err);
  }
}

export async function settingsSet(args: {
  scope: SettingsScope;
  patch: Record<string, unknown>;
  projectRoot?: string | null;
}): Promise<SettingsMutationResponse> {
  try {
    return await invoke<SettingsMutationResponse>("settings_set", {
      args: {
        scope: args.scope,
        patch: args.patch,
        projectRoot: args.projectRoot ?? null,
      },
    });
  } catch (err) {
    throw normalizeInvokeError(err);
  }
}

export async function settingsReset(args: {
  scope: SettingsScope;
  keys?: string[];
  projectRoot?: string | null;
}): Promise<SettingsMutationResponse> {
  try {
    return await invoke<SettingsMutationResponse>("settings_reset", {
      args: {
        scope: args.scope,
        keys: args.keys ?? null,
        projectRoot: args.projectRoot ?? null,
      },
    });
  } catch (err) {
    throw normalizeInvokeError(err);
  }
}

export async function settingsExport(args?: {
  projectRoot?: string | null;
  includeProject?: boolean;
}): Promise<SettingsExportResponse> {
  try {
    return await invoke<SettingsExportResponse>("settings_export", {
      args: args
        ? {
            projectRoot: args.projectRoot ?? null,
            includeProject: !!args.includeProject,
          }
        : null,
    });
  } catch (err) {
    throw normalizeInvokeError(err);
  }
}

export async function settingsImport(args: {
  json: Record<string, unknown>;
  mode: SettingsImportMode;
  projectRoot?: string | null;
}): Promise<SettingsMutationResponse> {
  try {
    return await invoke<SettingsMutationResponse>("settings_import", {
      args: {
        json: args.json,
        mode: args.mode,
        projectRoot: args.projectRoot ?? null,
      },
    });
  } catch (err) {
    throw normalizeInvokeError(err);
  }
}

export async function settingsTestProviderConnectivity(
  projectRoot?: string | null,
): Promise<ProviderConnectivityResult[]> {
  try {
    return await invoke<ProviderConnectivityResult[]>(
      "settings_test_provider_connectivity",
      {
        args: {
          projectRoot: projectRoot ?? null,
        },
      },
    );
  } catch (err) {
    throw normalizeInvokeError(err);
  }
}

export async function agentSmokeTest(
  projectRoot: string,
): Promise<AgentSmokeResult> {
  try {
    return await invoke<AgentSmokeResult>("agent_smoke_test", {
      projectPath: projectRoot,
    });
  } catch (err) {
    throw normalizeInvokeError(err);
  }
}

export async function bootstrapSettingsMigration(
  projectRoot?: string | null,
): Promise<void> {
  const loaded = await settingsGet(projectRoot);
  for (const warning of loaded.warnings) {
    log.warn(warning);
  }
}
