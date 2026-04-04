export const SETTINGS_SCHEMA_VERSION = 1;

export type ThemePreference = "system" | "light" | "dark";
export type LanguagePreference = "zh-CN" | "en-US";
export type EditorPreference =
  | "cursor"
  | "vscode"
  | "zed"
  | "sublime"
  | "system";
export type CitationStylePolicy = "auto" | "cite" | "citep" | "autocite";
export type QueryEmbeddingProvider = "none" | "local_embedding";
export type LogLevel = "info" | "debug" | "warn" | "error";
export type AgentProvider = "openai" | "minimax" | "deepseek";
export type AgentRuntimeKind = "claude_cli" | "local_agent";

export interface AgentSamplingPreset {
  temperature: number;
  topP: number;
  maxTokens: number;
}

export interface GlobalSettingsV1 {
  general: {
    theme: ThemePreference;
    language: LanguagePreference;
    openInEditor: {
      defaultEditor: EditorPreference;
    };
  };
  citation: {
    stylePolicy: CitationStylePolicy;
    autoApplyThreshold: number;
    reviewThreshold: number;
    search: {
      limit: number;
      llmQuery: {
        enabled: boolean;
        model: string;
        endpoint: string;
        timeoutMs: number;
        maxQueries: number;
      };
      queryEmbedding: {
        provider: QueryEmbeddingProvider;
        timeoutMs: number;
      };
      queryExecution: {
        topN: number;
        mmrLambda: number;
        minQuality: number;
        minHitRatio: number;
        hitScoreThreshold: number;
      };
    };
  };
  integrations: {
    agent: {
      runtime: AgentRuntimeKind;
      provider: AgentProvider;
      model: string;
      baseUrl: string;
      samplingProfiles: {
        editStable: AgentSamplingPreset;
        analysisBalanced: AgentSamplingPreset;
        analysisDeep: AgentSamplingPreset;
        chatFlexible: AgentSamplingPreset;
      };
    };
    semanticScholar: {
      enabled: boolean;
    };
    zotero: {
      autoSyncOnApply: boolean;
    };
  };
  advanced: {
    debugEnabled: boolean;
    logLevel: LogLevel;
  };
}

export interface ProjectSettingsV1 {
  citation?: {
    autoApplyThreshold?: number;
    reviewThreshold?: number;
    search?: {
      limit?: number;
    };
  };
}

export interface SecretSettingsV1 {
  integrations: {
    agent: {
      apiKey: string | null;
    };
    semanticScholar: {
      apiKey: string | null;
    };
    llmQuery: {
      apiKey: string | null;
    };
  };
}

export interface SettingsEnvelope<T> {
  version: number;
  data: T;
}

export interface EffectiveSettingsV1
  extends Omit<GlobalSettingsV1, "citation"> {
  citation: {
    stylePolicy: CitationStylePolicy;
    autoApplyThreshold: number;
    reviewThreshold: number;
    search: {
      limit: number;
      llmQuery: {
        enabled: boolean;
        model: string;
        endpoint: string;
        timeoutMs: number;
        maxQueries: number;
      };
      queryEmbedding: {
        provider: QueryEmbeddingProvider;
        timeoutMs: number;
      };
      queryExecution: {
        topN: number;
        mmrLambda: number;
        minQuality: number;
        minHitRatio: number;
        hitScoreThreshold: number;
      };
    };
  };
}

export interface SecretsMeta {
  integrations: {
    agent: {
      apiKeyConfigured: boolean;
    };
    semanticScholar: {
      apiKeyConfigured: boolean;
    };
    llmQuery: {
      apiKeyConfigured: boolean;
    };
  };
}

export const DEFAULT_GLOBAL_SETTINGS: GlobalSettingsV1 = {
  general: {
    theme: "system",
    language: "zh-CN",
    openInEditor: {
      defaultEditor: "system",
    },
  },
  citation: {
    stylePolicy: "auto",
    autoApplyThreshold: 0.64,
    reviewThreshold: 0.5,
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

export const DEFAULT_PROJECT_SETTINGS: ProjectSettingsV1 = {};

export const DEFAULT_SECRET_SETTINGS: SecretSettingsV1 = {
  integrations: {
    agent: {
      apiKey: null,
    },
    semanticScholar: {
      apiKey: null,
    },
    llmQuery: {
      apiKey: null,
    },
  },
};

type RecordLike = Record<string, unknown>;

function isRecord(value: unknown): value is RecordLike {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function asRecord(value: unknown): RecordLike {
  return isRecord(value) ? value : {};
}

function pickEnum<T extends string>(
  value: unknown,
  allowed: readonly T[],
  fallback: T,
): T {
  return typeof value === "string" && allowed.includes(value as T)
    ? (value as T)
    : fallback;
}

function pickBoolean(value: unknown, fallback: boolean): boolean {
  return typeof value === "boolean" ? value : fallback;
}

function pickNumberInRange(
  value: unknown,
  fallback: number,
  min: number,
  max: number,
): number {
  if (typeof value !== "number" || !Number.isFinite(value)) return fallback;
  return Math.min(max, Math.max(min, value));
}

function pickStringOrNull(value: unknown): string | null {
  if (value == null) return null;
  if (typeof value !== "string") return null;
  const trimmed = value.trim();
  return trimmed ? trimmed : null;
}

function pickNonEmptyString(value: unknown, fallback: string): string {
  if (typeof value !== "string") return fallback;
  const trimmed = value.trim();
  return trimmed || fallback;
}

function normalizeThresholdPair(
  autoApplyThreshold: number,
  reviewThreshold: number,
): { autoApplyThreshold: number; reviewThreshold: number } {
  const auto = pickNumberInRange(autoApplyThreshold, 0.64, 0, 1);
  const review = pickNumberInRange(reviewThreshold, 0.5, 0, 1);
  return {
    autoApplyThreshold: auto,
    reviewThreshold: Math.min(review, auto),
  };
}

function legacyGlobalData(raw: unknown): RecordLike {
  const root = asRecord(raw);
  const migrated: RecordLike = {};

  if (typeof root.theme === "string") {
    migrated.general = { theme: root.theme };
  }
  if (typeof root.language === "string") {
    migrated.general = {
      ...(asRecord(migrated.general)),
      language: root.language,
    };
  }
  if (typeof root.defaultEditor === "string") {
    migrated.general = {
      ...(asRecord(migrated.general)),
      openInEditor: { defaultEditor: root.defaultEditor },
    };
  }
  if (typeof root.debugEnabled === "boolean") {
    migrated.advanced = { debugEnabled: root.debugEnabled };
  }
  if (typeof root.logLevel === "string") {
    migrated.advanced = {
      ...(asRecord(migrated.advanced)),
      logLevel: root.logLevel,
    };
  }
  if (typeof root.citationStylePolicy === "string") {
    migrated.citation = { stylePolicy: root.citationStylePolicy };
  }
  if (typeof root.autoApplyThreshold === "number") {
    migrated.citation = {
      ...(asRecord(migrated.citation)),
      autoApplyThreshold: root.autoApplyThreshold,
    };
  }
  if (typeof root.reviewThreshold === "number") {
    migrated.citation = {
      ...(asRecord(migrated.citation)),
      reviewThreshold: root.reviewThreshold,
    };
  }
  if (typeof root.searchLimit === "number") {
    migrated.citation = {
      ...(asRecord(migrated.citation)),
      search: { limit: root.searchLimit },
    };
  }
  if (typeof root.semanticScholarEnabled === "boolean") {
    migrated.integrations = {
      semanticScholar: { enabled: root.semanticScholarEnabled },
    };
  }
  if (typeof root.zoteroAutoSyncOnApply === "boolean") {
    migrated.integrations = {
      ...(asRecord(migrated.integrations)),
      zotero: { autoSyncOnApply: root.zoteroAutoSyncOnApply },
    };
  }

  return migrated;
}

function mergeGlobalMigrationSource(raw: unknown): RecordLike {
  const source = asRecord(raw);
  const legacy = legacyGlobalData(raw);

  const sourceGeneral = asRecord(source.general);
  const legacyGeneral = asRecord(legacy.general);
  const sourceGeneralOpenInEditor = asRecord(sourceGeneral.openInEditor);
  const legacyGeneralOpenInEditor = asRecord(legacyGeneral.openInEditor);

  const sourceCitation = asRecord(source.citation);
  const legacyCitation = asRecord(legacy.citation);
  const sourceCitationSearch = asRecord(sourceCitation.search);
  const legacyCitationSearch = asRecord(legacyCitation.search);
  const sourceCitationSearchLlmQuery = asRecord(sourceCitationSearch.llmQuery);
  const legacyCitationSearchLlmQuery = asRecord(legacyCitationSearch.llmQuery);
  const sourceCitationSearchQueryEmbedding = asRecord(
    sourceCitationSearch.queryEmbedding,
  );
  const legacyCitationSearchQueryEmbedding = asRecord(
    legacyCitationSearch.queryEmbedding,
  );
  const sourceCitationSearchQueryExecution = asRecord(
    sourceCitationSearch.queryExecution,
  );
  const legacyCitationSearchQueryExecution = asRecord(
    legacyCitationSearch.queryExecution,
  );

  const sourceIntegrations = asRecord(source.integrations);
  const legacyIntegrations = asRecord(legacy.integrations);
  const sourceAgent = asRecord(sourceIntegrations.agent);
  const legacyAgent = asRecord(legacyIntegrations.agent);
  const sourceSemanticScholar = asRecord(sourceIntegrations.semanticScholar);
  const legacySemanticScholar = asRecord(legacyIntegrations.semanticScholar);
  const sourceLlmQuery = asRecord(sourceIntegrations.llmQuery);
  const legacyLlmQuery = asRecord(legacyIntegrations.llmQuery);
  const sourceZotero = asRecord(sourceIntegrations.zotero);
  const legacyZotero = asRecord(legacyIntegrations.zotero);

  const sourceAdvanced = asRecord(source.advanced);
  const legacyAdvanced = asRecord(legacy.advanced);

  return {
    ...source,
    general: {
      ...sourceGeneral,
      ...legacyGeneral,
      openInEditor: {
        ...sourceGeneralOpenInEditor,
        ...legacyGeneralOpenInEditor,
      },
    },
    citation: {
      ...sourceCitation,
      ...legacyCitation,
      search: {
        ...sourceCitationSearch,
        ...legacyCitationSearch,
        llmQuery: {
          ...sourceCitationSearchLlmQuery,
          ...legacyCitationSearchLlmQuery,
        },
        queryEmbedding: {
          ...sourceCitationSearchQueryEmbedding,
          ...legacyCitationSearchQueryEmbedding,
        },
        queryExecution: {
          ...sourceCitationSearchQueryExecution,
          ...legacyCitationSearchQueryExecution,
        },
      },
    },
    integrations: {
      ...sourceIntegrations,
      ...legacyIntegrations,
      agent: {
        ...sourceAgent,
        ...legacyAgent,
      },
      semanticScholar: {
        ...sourceSemanticScholar,
        ...legacySemanticScholar,
      },
      llmQuery: {
        ...sourceLlmQuery,
        ...legacyLlmQuery,
      },
      zotero: {
        ...sourceZotero,
        ...legacyZotero,
      },
    },
    advanced: {
      ...sourceAdvanced,
      ...legacyAdvanced,
    },
  };
}

export function sanitizeGlobalSettings(input: unknown): GlobalSettingsV1 {
  const root = asRecord(input);
  const general = asRecord(root.general);
  const openInEditor = asRecord(general.openInEditor);
  const citation = asRecord(root.citation);
  const citationSearch = asRecord(citation.search);
  const citationSearchLlmQuery = asRecord(citationSearch.llmQuery);
  const citationSearchQueryEmbedding = asRecord(citationSearch.queryEmbedding);
  const citationSearchQueryExecution = asRecord(citationSearch.queryExecution);
  const integrations = asRecord(root.integrations);
  const agent = asRecord(integrations.agent);
  const agentSamplingProfiles = asRecord(agent.samplingProfiles);
  const agentEditStable = asRecord(agentSamplingProfiles.editStable);
  const agentAnalysisBalanced = asRecord(agentSamplingProfiles.analysisBalanced);
  const agentAnalysisDeep = asRecord(agentSamplingProfiles.analysisDeep);
  const agentChatFlexible = asRecord(agentSamplingProfiles.chatFlexible);
  const semanticScholar = asRecord(integrations.semanticScholar);
  const zotero = asRecord(integrations.zotero);
  const advanced = asRecord(root.advanced);

  const thresholds = normalizeThresholdPair(
    pickNumberInRange(
      citation.autoApplyThreshold,
      DEFAULT_GLOBAL_SETTINGS.citation.autoApplyThreshold,
      0,
      1,
    ),
    pickNumberInRange(
      citation.reviewThreshold,
      DEFAULT_GLOBAL_SETTINGS.citation.reviewThreshold,
      0,
      1,
    ),
  );

  return {
    general: {
      theme: pickEnum(
        general.theme,
        ["system", "light", "dark"],
        DEFAULT_GLOBAL_SETTINGS.general.theme,
      ),
      language: pickEnum(
        general.language,
        ["zh-CN", "en-US"],
        DEFAULT_GLOBAL_SETTINGS.general.language,
      ),
      openInEditor: {
        defaultEditor: pickEnum(
          openInEditor.defaultEditor,
          ["cursor", "vscode", "zed", "sublime", "system"],
          DEFAULT_GLOBAL_SETTINGS.general.openInEditor.defaultEditor,
        ),
      },
    },
    citation: {
      stylePolicy: pickEnum(
        citation.stylePolicy,
        ["auto", "cite", "citep", "autocite"],
        DEFAULT_GLOBAL_SETTINGS.citation.stylePolicy,
      ),
      autoApplyThreshold: thresholds.autoApplyThreshold,
      reviewThreshold: thresholds.reviewThreshold,
      search: {
        limit: Math.round(
          pickNumberInRange(
            citationSearch.limit,
            DEFAULT_GLOBAL_SETTINGS.citation.search.limit,
            1,
            20,
          ),
        ),
        llmQuery: {
          enabled: pickBoolean(
            citationSearchLlmQuery.enabled,
            DEFAULT_GLOBAL_SETTINGS.citation.search.llmQuery.enabled,
          ),
          model: pickNonEmptyString(
            citationSearchLlmQuery.model,
            DEFAULT_GLOBAL_SETTINGS.citation.search.llmQuery.model,
          ),
          endpoint: pickNonEmptyString(
            citationSearchLlmQuery.endpoint,
            DEFAULT_GLOBAL_SETTINGS.citation.search.llmQuery.endpoint,
          ),
          timeoutMs: Math.round(
            pickNumberInRange(
              citationSearchLlmQuery.timeoutMs,
              DEFAULT_GLOBAL_SETTINGS.citation.search.llmQuery.timeoutMs,
              2000,
              20000,
            ),
          ),
          maxQueries: Math.round(
            pickNumberInRange(
              citationSearchLlmQuery.maxQueries,
              DEFAULT_GLOBAL_SETTINGS.citation.search.llmQuery.maxQueries,
              1,
              6,
            ),
          ),
        },
        queryEmbedding: {
          provider: pickEnum(
            citationSearchQueryEmbedding.provider,
            ["none", "local_embedding"],
            DEFAULT_GLOBAL_SETTINGS.citation.search.queryEmbedding.provider,
          ),
          timeoutMs: Math.round(
            pickNumberInRange(
              citationSearchQueryEmbedding.timeoutMs,
              DEFAULT_GLOBAL_SETTINGS.citation.search.queryEmbedding.timeoutMs,
              100,
              10000,
            ),
          ),
        },
        queryExecution: {
          topN: Math.round(
            pickNumberInRange(
              citationSearchQueryExecution.topN,
              DEFAULT_GLOBAL_SETTINGS.citation.search.queryExecution.topN,
              1,
              10,
            ),
          ),
          mmrLambda: pickNumberInRange(
            citationSearchQueryExecution.mmrLambda,
            DEFAULT_GLOBAL_SETTINGS.citation.search.queryExecution.mmrLambda,
            0,
            1,
          ),
          minQuality: pickNumberInRange(
            citationSearchQueryExecution.minQuality,
            DEFAULT_GLOBAL_SETTINGS.citation.search.queryExecution.minQuality,
            0,
            1,
          ),
          minHitRatio: pickNumberInRange(
            citationSearchQueryExecution.minHitRatio,
            DEFAULT_GLOBAL_SETTINGS.citation.search.queryExecution.minHitRatio,
            0,
            1,
          ),
          hitScoreThreshold: pickNumberInRange(
            citationSearchQueryExecution.hitScoreThreshold,
            DEFAULT_GLOBAL_SETTINGS.citation.search.queryExecution.hitScoreThreshold,
            0,
            1,
          ),
        },
      },
    },
    integrations: {
      agent: {
        runtime: pickEnum(
          agent.runtime,
          ["claude_cli", "local_agent"],
          DEFAULT_GLOBAL_SETTINGS.integrations.agent.runtime,
        ),
        provider: pickEnum(
          agent.provider,
          ["openai", "minimax", "deepseek"],
          DEFAULT_GLOBAL_SETTINGS.integrations.agent.provider,
        ),
        model: pickNonEmptyString(
          agent.model,
          DEFAULT_GLOBAL_SETTINGS.integrations.agent.model,
        ),
        baseUrl: pickNonEmptyString(
          agent.baseUrl,
          DEFAULT_GLOBAL_SETTINGS.integrations.agent.baseUrl,
        ),
        samplingProfiles: {
          editStable: {
            temperature: pickNumberInRange(
              agentEditStable.temperature,
              DEFAULT_GLOBAL_SETTINGS.integrations.agent.samplingProfiles
                .editStable.temperature,
              0,
              2,
            ),
            topP: pickNumberInRange(
              agentEditStable.topP,
              DEFAULT_GLOBAL_SETTINGS.integrations.agent.samplingProfiles
                .editStable.topP,
              0,
              1,
            ),
            maxTokens: Math.round(
              pickNumberInRange(
                agentEditStable.maxTokens,
                DEFAULT_GLOBAL_SETTINGS.integrations.agent.samplingProfiles
                  .editStable.maxTokens,
                256,
                16384,
              ),
            ),
          },
          analysisBalanced: {
            temperature: pickNumberInRange(
              agentAnalysisBalanced.temperature,
              DEFAULT_GLOBAL_SETTINGS.integrations.agent.samplingProfiles
                .analysisBalanced.temperature,
              0,
              2,
            ),
            topP: pickNumberInRange(
              agentAnalysisBalanced.topP,
              DEFAULT_GLOBAL_SETTINGS.integrations.agent.samplingProfiles
                .analysisBalanced.topP,
              0,
              1,
            ),
            maxTokens: Math.round(
              pickNumberInRange(
                agentAnalysisBalanced.maxTokens,
                DEFAULT_GLOBAL_SETTINGS.integrations.agent.samplingProfiles
                  .analysisBalanced.maxTokens,
                256,
                16384,
              ),
            ),
          },
          analysisDeep: {
            temperature: pickNumberInRange(
              agentAnalysisDeep.temperature,
              DEFAULT_GLOBAL_SETTINGS.integrations.agent.samplingProfiles
                .analysisDeep.temperature,
              0,
              2,
            ),
            topP: pickNumberInRange(
              agentAnalysisDeep.topP,
              DEFAULT_GLOBAL_SETTINGS.integrations.agent.samplingProfiles
                .analysisDeep.topP,
              0,
              1,
            ),
            maxTokens: Math.round(
              pickNumberInRange(
                agentAnalysisDeep.maxTokens,
                DEFAULT_GLOBAL_SETTINGS.integrations.agent.samplingProfiles
                  .analysisDeep.maxTokens,
                256,
                16384,
              ),
            ),
          },
          chatFlexible: {
            temperature: pickNumberInRange(
              agentChatFlexible.temperature,
              DEFAULT_GLOBAL_SETTINGS.integrations.agent.samplingProfiles
                .chatFlexible.temperature,
              0,
              2,
            ),
            topP: pickNumberInRange(
              agentChatFlexible.topP,
              DEFAULT_GLOBAL_SETTINGS.integrations.agent.samplingProfiles
                .chatFlexible.topP,
              0,
              1,
            ),
            maxTokens: Math.round(
              pickNumberInRange(
                agentChatFlexible.maxTokens,
                DEFAULT_GLOBAL_SETTINGS.integrations.agent.samplingProfiles
                  .chatFlexible.maxTokens,
                256,
                16384,
              ),
            ),
          },
        },
      },
      semanticScholar: {
        enabled: pickBoolean(
          semanticScholar.enabled,
          DEFAULT_GLOBAL_SETTINGS.integrations.semanticScholar.enabled,
        ),
      },
      zotero: {
        autoSyncOnApply: pickBoolean(
          zotero.autoSyncOnApply,
          DEFAULT_GLOBAL_SETTINGS.integrations.zotero.autoSyncOnApply,
        ),
      },
    },
    advanced: {
      debugEnabled: pickBoolean(
        advanced.debugEnabled,
        DEFAULT_GLOBAL_SETTINGS.advanced.debugEnabled,
      ),
      logLevel: pickEnum(
        advanced.logLevel,
        ["info", "debug", "warn", "error"],
        DEFAULT_GLOBAL_SETTINGS.advanced.logLevel,
      ),
    },
  };
}

export function sanitizeProjectSettings(input: unknown): ProjectSettingsV1 {
  const root = asRecord(input);
  const citationRaw = asRecord(root.citation);
  const citationSearchRaw = asRecord(citationRaw.search);

  const auto = pickNumberInRange(citationRaw.autoApplyThreshold, NaN, 0, 1);
  const review = pickNumberInRange(citationRaw.reviewThreshold, NaN, 0, 1);
  const limit = pickNumberInRange(citationSearchRaw.limit, NaN, 1, 20);

  const citation: NonNullable<ProjectSettingsV1["citation"]> = {};
  if (!Number.isNaN(auto)) {
    citation.autoApplyThreshold = auto;
  }
  if (!Number.isNaN(review)) {
    citation.reviewThreshold = review;
  }
  if (!Number.isNaN(limit)) {
    citation.search = { limit: Math.round(limit) };
  }

  if (citation.autoApplyThreshold != null && citation.reviewThreshold != null) {
    citation.reviewThreshold = Math.min(
      citation.reviewThreshold,
      citation.autoApplyThreshold,
    );
  }

  return Object.keys(citation).length > 0 ? { citation } : {};
}

export function sanitizeSecretSettings(input: unknown): SecretSettingsV1 {
  const root = asRecord(input);
  const integrations = asRecord(root.integrations);
  const agent = asRecord(integrations.agent);
  const semanticScholar = asRecord(integrations.semanticScholar);
  const llmQuery = asRecord(integrations.llmQuery);

  return {
    integrations: {
      agent: {
        apiKey: pickStringOrNull(agent.apiKey),
      },
      semanticScholar: {
        apiKey: pickStringOrNull(semanticScholar.apiKey),
      },
      llmQuery: {
        apiKey: pickStringOrNull(llmQuery.apiKey),
      },
    },
  };
}

export function migrateGlobalEnvelope(
  raw: unknown,
): SettingsEnvelope<GlobalSettingsV1> {
  if (isRecord(raw) && isRecord(raw.data) && typeof raw.version === "number") {
    return {
      version: SETTINGS_SCHEMA_VERSION,
      data: sanitizeGlobalSettings(raw.data),
    };
  }

  const migrated = mergeGlobalMigrationSource(raw);
  return {
    version: SETTINGS_SCHEMA_VERSION,
    data: sanitizeGlobalSettings(migrated),
  };
}

export function migrateProjectEnvelope(
  raw: unknown,
): SettingsEnvelope<ProjectSettingsV1> {
  if (isRecord(raw) && isRecord(raw.data) && typeof raw.version === "number") {
    return {
      version: SETTINGS_SCHEMA_VERSION,
      data: sanitizeProjectSettings(raw.data),
    };
  }
  return {
    version: SETTINGS_SCHEMA_VERSION,
    data: sanitizeProjectSettings(raw),
  };
}

export function migrateSecretEnvelope(
  raw: unknown,
): SettingsEnvelope<SecretSettingsV1> {
  if (isRecord(raw) && isRecord(raw.data) && typeof raw.version === "number") {
    return {
      version: SETTINGS_SCHEMA_VERSION,
      data: sanitizeSecretSettings(raw.data),
    };
  }
  return {
    version: SETTINGS_SCHEMA_VERSION,
    data: sanitizeSecretSettings(raw),
  };
}

export function resolveEffectiveSettings(
  globalInput: GlobalSettingsV1,
  projectInput?: ProjectSettingsV1 | null,
): EffectiveSettingsV1 {
  const global = sanitizeGlobalSettings(globalInput);
  const project = sanitizeProjectSettings(projectInput ?? {});

  const autoApplyThreshold =
    project.citation?.autoApplyThreshold ?? global.citation.autoApplyThreshold;
  const reviewThreshold = Math.min(
    project.citation?.reviewThreshold ?? global.citation.reviewThreshold,
    autoApplyThreshold,
  );
  const searchLimit =
    project.citation?.search?.limit ?? global.citation.search.limit;

  return {
    general: global.general,
    citation: {
      stylePolicy: global.citation.stylePolicy,
      autoApplyThreshold,
      reviewThreshold,
      search: {
        limit: searchLimit,
        llmQuery: global.citation.search.llmQuery,
        queryEmbedding: global.citation.search.queryEmbedding,
        queryExecution: global.citation.search.queryExecution,
      },
    },
    integrations: global.integrations,
    advanced: global.advanced,
  };
}

export function toSecretsMeta(secretInput: SecretSettingsV1): SecretsMeta {
  const secret = sanitizeSecretSettings(secretInput);
  return {
    integrations: {
      agent: {
        apiKeyConfigured: !!secret.integrations.agent.apiKey,
      },
      semanticScholar: {
        apiKeyConfigured: !!secret.integrations.semanticScholar.apiKey,
      },
      llmQuery: {
        apiKeyConfigured: !!secret.integrations.llmQuery.apiKey,
      },
    },
  };
}
