import type { AgentSmokeResult, ProviderConnectivityResult, SettingsImportMode } from "@/lib/settings-api";

export type QueryMode = "fast" | "balanced" | "deep";
export type AgentProvider = "openai" | "minimax" | "deepseek";
export type AgentRuntimeKind = "claude_cli" | "local_agent";
export type AgentDomain = "general" | "biomedical" | "chemistry" | "custom";
export type AgentTerminologyStrictness = "strict" | "moderate" | "relaxed";

export const panelClass = "space-y-3 rounded-xl border border-sidebar-border/70 bg-sidebar-accent/20 p-4";

export const AGENT_PROVIDER_DEFAULTS: Record<
  AgentProvider,
  { model: string; baseUrl: string }
> = {
  openai: {
    model: "gpt-5.4",
    baseUrl: "https://api.openai.com/v1",
  },
  minimax: {
    model: "MiniMax-M2.5",
    baseUrl: "https://api.minimax.io/v1",
  },
  deepseek: {
    model: "deepseek-chat",
    baseUrl: "https://api.deepseek.com",
  },
};

export const QUERY_MODE_PRESETS: Record<
  QueryMode,
  {
    topN: number;
    mmrLambda: number;
    minQuality: number;
    minHitRatio: number;
    hitScoreThreshold: number;
  }
> = {
  fast: {
    topN: 3,
    mmrLambda: 0.8,
    minQuality: 0.28,
    minHitRatio: 0.55,
    hitScoreThreshold: 0.62,
  },
  balanced: {
    topN: 5,
    mmrLambda: 0.72,
    minQuality: 0.24,
    minHitRatio: 0.45,
    hitScoreThreshold: 0.58,
  },
  deep: {
    topN: 8,
    mmrLambda: 0.64,
    minQuality: 0.18,
    minHitRatio: 0.33,
    hitScoreThreshold: 0.5,
  },
};

export function inferNearestQueryMode(input: {
  topN: number;
  mmrLambda: number;
  minQuality: number;
  minHitRatio: number;
  hitScoreThreshold: number;
}): QueryMode {
  const entries = Object.entries(QUERY_MODE_PRESETS) as [
    QueryMode,
    (typeof QUERY_MODE_PRESETS)[QueryMode],
  ][];
  let bestMode: QueryMode = "balanced";
  let bestDistance = Number.POSITIVE_INFINITY;
  for (const [mode, preset] of entries) {
    const distance =
      Math.abs(input.topN - preset.topN) +
      Math.abs(input.mmrLambda - preset.mmrLambda) * 2 +
      Math.abs(input.minQuality - preset.minQuality) * 2 +
      Math.abs(input.minHitRatio - preset.minHitRatio) * 2 +
      Math.abs(input.hitScoreThreshold - preset.hitScoreThreshold) * 2;
    if (distance < bestDistance) {
      bestDistance = distance;
      bestMode = mode;
    }
  }
  return bestMode;
}

/** Effective settings shape — re-exported from the store for convenience. */
export type EffectiveSettings = ReturnType<
  typeof import("@/stores/settings-store").useSettingsStore.getState
>["effective"];

export type SecretsMeta = ReturnType<
  typeof import("@/stores/settings-store").useSettingsStore.getState
>["secretsMeta"];
