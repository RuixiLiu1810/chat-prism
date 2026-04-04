import { describe, expect, it } from "vitest";
import {
  DEFAULT_GLOBAL_SETTINGS,
  migrateGlobalEnvelope,
  resolveEffectiveSettings,
  sanitizeGlobalSettings,
  sanitizeProjectSettings,
  toSecretsMeta,
  type GlobalSettingsV1,
} from "@/lib/settings-schema";

describe("settings-schema", () => {
  it("sanitizes global settings and enforces threshold invariants", () => {
    const parsed = sanitizeGlobalSettings({
      general: {
        theme: "neon",
        language: "fr-FR",
        openInEditor: { defaultEditor: "vim" },
      },
      citation: {
        autoApplyThreshold: 0.4,
        reviewThreshold: 0.95,
        search: { limit: 99 },
      },
      integrations: {
        semanticScholar: { enabled: "yes" },
      },
      advanced: {
        debugEnabled: true,
        logLevel: "trace",
      },
    });

    expect(parsed.general.theme).toBe("system");
    expect(parsed.general.language).toBe("zh-CN");
    expect(parsed.general.openInEditor.defaultEditor).toBe("system");
    expect(parsed.citation.autoApplyThreshold).toBe(0.4);
    expect(parsed.citation.reviewThreshold).toBe(0.4);
    expect(parsed.citation.search.limit).toBe(20);
    expect(parsed.citation.search.queryEmbedding.provider).toBe("none");
    expect(parsed.citation.search.queryEmbedding.timeoutMs).toBe(1200);
    expect(parsed.citation.search.queryExecution.topN).toBe(5);
    expect(parsed.citation.search.queryExecution.mmrLambda).toBe(0.72);
    expect(parsed.citation.search.queryExecution.minQuality).toBe(0.24);
    expect(parsed.citation.search.queryExecution.minHitRatio).toBe(0.45);
    expect(parsed.citation.search.queryExecution.hitScoreThreshold).toBe(0.58);
    expect(parsed.integrations.agent.runtime).toBe("claude_cli");
    expect(parsed.integrations.agent.provider).toBe("openai");
    expect(parsed.integrations.agent.model).toBe("gpt-5.4");
    expect(parsed.integrations.agent.baseUrl).toBe(
      "https://api.openai.com/v1",
    );
    expect(parsed.integrations.semanticScholar.enabled).toBe(
      DEFAULT_GLOBAL_SETTINGS.integrations.semanticScholar.enabled,
    );
    expect(parsed.advanced.debugEnabled).toBe(true);
    expect(parsed.advanced.logLevel).toBe("info");
  });

  it("sanitizes project override settings", () => {
    const parsed = sanitizeProjectSettings({
      citation: {
        autoApplyThreshold: 0.5,
        reviewThreshold: 0.8,
        search: { limit: 0 },
      },
    });

    expect(parsed.citation?.autoApplyThreshold).toBe(0.5);
    expect(parsed.citation?.reviewThreshold).toBe(0.5);
    expect(parsed.citation?.search?.limit).toBe(1);
  });

  it("migrates flat legacy keys while preserving nested values", () => {
    const migrated = migrateGlobalEnvelope({
      theme: "dark",
      citationStylePolicy: "citep",
      general: {
        language: "en-US",
      },
      citation: {
        search: { limit: 5 },
      },
    });

    expect(migrated.version).toBe(1);
    expect(migrated.data.general.theme).toBe("dark");
    expect(migrated.data.general.language).toBe("en-US");
    expect(migrated.data.citation.stylePolicy).toBe("citep");
    expect(migrated.data.citation.search.limit).toBe(5);
    expect(migrated.data.integrations.agent.runtime).toBe("claude_cli");
  });

  it("migrates older envelope versions from data field", () => {
    const migrated = migrateGlobalEnvelope({
      version: 0,
      data: {
        general: {
          theme: "dark",
          language: "en-US",
        },
      },
    });

    expect(migrated.version).toBe(1);
    expect(migrated.data.general.theme).toBe("dark");
    expect(migrated.data.general.language).toBe("en-US");
  });

  it("resolves effective settings with project priority and safeguards", () => {
    const global: GlobalSettingsV1 = sanitizeGlobalSettings({
      citation: {
        autoApplyThreshold: 0.8,
        reviewThreshold: 0.7,
        search: { limit: 6 },
      },
    });
    const effective = resolveEffectiveSettings(global, {
      citation: {
        autoApplyThreshold: 0.55,
        reviewThreshold: 0.95,
        search: { limit: 12 },
      },
    });

    expect(effective.citation.autoApplyThreshold).toBe(0.55);
    expect(effective.citation.reviewThreshold).toBe(0.55);
    expect(effective.citation.search.limit).toBe(12);
  });

  it("returns secret metadata without exposing key", () => {
    const withKey = toSecretsMeta({
      integrations: {
        agent: { apiKey: "openai-key" },
        semanticScholar: { apiKey: "abc123" },
        llmQuery: { apiKey: "llm-key" },
      },
    });
    const emptyKey = toSecretsMeta({
      integrations: {
        agent: { apiKey: " " },
        semanticScholar: { apiKey: "   " },
        llmQuery: { apiKey: " " },
      },
    });

    expect(withKey.integrations.agent.apiKeyConfigured).toBe(true);
    expect(emptyKey.integrations.agent.apiKeyConfigured).toBe(false);
    expect(withKey.integrations.semanticScholar.apiKeyConfigured).toBe(true);
    expect(emptyKey.integrations.semanticScholar.apiKeyConfigured).toBe(false);
    expect(withKey.integrations.llmQuery.apiKeyConfigured).toBe(true);
    expect(emptyKey.integrations.llmQuery.apiKeyConfigured).toBe(false);
  });
});
