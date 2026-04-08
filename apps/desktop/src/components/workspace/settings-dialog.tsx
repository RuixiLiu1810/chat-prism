import { useEffect, useMemo, useState } from "react";
import { LoaderIcon } from "lucide-react";
import { useTheme } from "next-themes";
import { toast } from "sonner";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { useSettingsStore } from "@/stores/settings-store";
import {
  agentSmokeTest,
  settingsTestProviderConnectivity,
  type AgentSmokeResult,
  type ProviderConnectivityResult,
  type SettingsImportMode,
} from "@/lib/settings-api";
import {
  GeneralTab,
  CitationTab,
  AIAssistantTab,
  AdvancedTab,
  AGENT_PROVIDER_DEFAULTS,
  QUERY_MODE_PRESETS,
  inferNearestQueryMode,
  type QueryMode,
  type AgentProvider,
  type AgentRuntimeKind,
  type AgentDomain,
  type AgentTerminologyStrictness,
} from "./settings-tabs";

interface SettingsDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  projectRoot: string | null;
}

export function SettingsDialog({
  open,
  onOpenChange,
  projectRoot,
}: SettingsDialogProps) {
  const { theme, setTheme } = useTheme();
  const isLoading = useSettingsStore((s) => s.isLoading);
  const isSaving = useSettingsStore((s) => s.isSaving);
  const error = useSettingsStore((s) => s.error);
  const warnings = useSettingsStore((s) => s.warnings);
  const effective = useSettingsStore((s) => s.effective);
  const project = useSettingsStore((s) => s.project);
  const secretsMeta = useSettingsStore((s) => s.secretsMeta);
  const load = useSettingsStore((s) => s.load);
  const patchGlobal = useSettingsStore((s) => s.patchGlobal);
  const patchProject = useSettingsStore((s) => s.patchProject);
  const patchSecret = useSettingsStore((s) => s.patchSecret);
  const setCitationStylePolicy = useSettingsStore(
    (s) => s.setCitationStylePolicy,
  );
  const setThemePreference = useSettingsStore((s) => s.setThemePreference);
  const resetScope = useSettingsStore((s) => s.resetScope);
  const exportJson = useSettingsStore((s) => s.exportJson);
  const importJson = useSettingsStore((s) => s.importJson);

  const [autoThreshold, setAutoThreshold] = useState("0.78");
  const [reviewThreshold, setReviewThreshold] = useState("0.62");
  const [searchLimit, setSearchLimit] = useState("8");
  const [agentRuntime, setAgentRuntime] =
    useState<AgentRuntimeKind>("claude_cli");
  const [agentProvider, setAgentProvider] = useState<AgentProvider>("openai");
  const [agentModel, setAgentModel] = useState("gpt-5.4");
  const [agentBaseUrl, setAgentBaseUrl] = useState("https://api.openai.com/v1");
  const [agentDomain, setAgentDomain] = useState<AgentDomain>("general");
  const [terminologyStrictness, setTerminologyStrictness] =
    useState<AgentTerminologyStrictness>("moderate");
  const [customDomainInstructions, setCustomDomainInstructions] = useState("");
  const [agentEditStableTemperature, setAgentEditStableTemperature] =
    useState("0.2");
  const [agentEditStableTopP, setAgentEditStableTopP] = useState("0.9");
  const [agentEditStableMaxTokens, setAgentEditStableMaxTokens] =
    useState("8192");
  const [agentAnalysisTemperature, setAgentAnalysisTemperature] =
    useState("0.4");
  const [agentAnalysisTopP, setAgentAnalysisTopP] = useState("0.9");
  const [agentAnalysisMaxTokens, setAgentAnalysisMaxTokens] =
    useState("6144");
  const [agentAnalysisDeepTemperature, setAgentAnalysisDeepTemperature] =
    useState("0.3");
  const [agentAnalysisDeepTopP, setAgentAnalysisDeepTopP] = useState("0.92");
  const [agentAnalysisDeepMaxTokens, setAgentAnalysisDeepMaxTokens] =
    useState("12288");
  const [agentChatTemperature, setAgentChatTemperature] = useState("0.7");
  const [agentChatTopP, setAgentChatTopP] = useState("0.95");
  const [agentChatMaxTokens, setAgentChatMaxTokens] = useState("4096");
  const [llmEnabled, setLlmEnabled] = useState(false);
  const [llmModel, setLlmModel] = useState("gpt-4o-mini");
  const [llmEndpoint, setLlmEndpoint] = useState(
    "https://api.openai.com/v1/chat/completions",
  );
  const [llmTimeoutMs, setLlmTimeoutMs] = useState("6000");
  const [llmMaxQueries, setLlmMaxQueries] = useState("3");
  const [queryEmbeddingProvider, setQueryEmbeddingProvider] = useState<
    "none" | "local_embedding"
  >("none");
  const [queryEmbeddingTimeoutMs, setQueryEmbeddingTimeoutMs] =
    useState("1200");
  const [queryExecutionTopN, setQueryExecutionTopN] = useState("5");
  const [queryExecutionMmrLambda, setQueryExecutionMmrLambda] =
    useState("0.72");
  const [queryExecutionMinQuality, setQueryExecutionMinQuality] =
    useState("0.24");
  const [queryExecutionMinHitRatio, setQueryExecutionMinHitRatio] =
    useState("0.45");
  const [queryExecutionHitScoreThreshold, setQueryExecutionHitScoreThreshold] =
    useState("0.58");
  const [queryMode, setQueryMode] = useState<QueryMode>("balanced");
  const [agentApiKeyInput, setAgentApiKeyInput] = useState("");
  const [apiKeyInput, setApiKeyInput] = useState("");
  const [llmApiKeyInput, setLlmApiKeyInput] = useState("");
  const [showAgentApiKey, setShowAgentApiKey] = useState(false);
  const [showSemanticApiKey, setShowSemanticApiKey] = useState(false);
  const [showLlmApiKey, setShowLlmApiKey] = useState(false);
  const [importText, setImportText] = useState("");
  const [confirmResetGlobalOpen, setConfirmResetGlobalOpen] = useState(false);
  const [isTestingProviders, setIsTestingProviders] = useState(false);
  const [isRunningAgentSmoke, setIsRunningAgentSmoke] = useState(false);
  const [providerConnectivity, setProviderConnectivity] = useState<
    ProviderConnectivityResult[]
  >([]);
  const [agentSmokeResult, setAgentSmokeResult] =
    useState<AgentSmokeResult | null>(null);

  useEffect(() => {
    if (open) {
      void load(projectRoot);
    } else {
      setConfirmResetGlobalOpen(false);
      setShowAgentApiKey(false);
      setShowSemanticApiKey(false);
      setShowLlmApiKey(false);
      setIsTestingProviders(false);
      setIsRunningAgentSmoke(false);
      setAgentSmokeResult(null);
    }
  }, [open, load, projectRoot]);

  useEffect(() => {
    setAutoThreshold(String(effective.citation.autoApplyThreshold));
    setReviewThreshold(String(effective.citation.reviewThreshold));
    setSearchLimit(String(effective.citation.search.limit));
    setAgentRuntime(effective.integrations.agent.runtime);
    setAgentProvider(effective.integrations.agent.provider);
    setAgentModel(effective.integrations.agent.model);
    setAgentBaseUrl(effective.integrations.agent.baseUrl);
    setAgentDomain(effective.integrations.agent.domainConfig.domain);
    setTerminologyStrictness(
      effective.integrations.agent.domainConfig.terminologyStrictness,
    );
    setCustomDomainInstructions(
      effective.integrations.agent.domainConfig.customInstructions ?? "",
    );
    setAgentEditStableTemperature(
      String(effective.integrations.agent.samplingProfiles.editStable.temperature),
    );
    setAgentEditStableTopP(
      String(effective.integrations.agent.samplingProfiles.editStable.topP),
    );
    setAgentEditStableMaxTokens(
      String(effective.integrations.agent.samplingProfiles.editStable.maxTokens),
    );
    setAgentAnalysisTemperature(
      String(
        effective.integrations.agent.samplingProfiles.analysisBalanced.temperature,
      ),
    );
    setAgentAnalysisTopP(
      String(effective.integrations.agent.samplingProfiles.analysisBalanced.topP),
    );
    setAgentAnalysisMaxTokens(
      String(
        effective.integrations.agent.samplingProfiles.analysisBalanced.maxTokens,
      ),
    );
    setAgentAnalysisDeepTemperature(
      String(
        effective.integrations.agent.samplingProfiles.analysisDeep.temperature,
      ),
    );
    setAgentAnalysisDeepTopP(
      String(effective.integrations.agent.samplingProfiles.analysisDeep.topP),
    );
    setAgentAnalysisDeepMaxTokens(
      String(
        effective.integrations.agent.samplingProfiles.analysisDeep.maxTokens,
      ),
    );
    setAgentChatTemperature(
      String(effective.integrations.agent.samplingProfiles.chatFlexible.temperature),
    );
    setAgentChatTopP(
      String(effective.integrations.agent.samplingProfiles.chatFlexible.topP),
    );
    setAgentChatMaxTokens(
      String(effective.integrations.agent.samplingProfiles.chatFlexible.maxTokens),
    );
    setLlmEnabled(effective.citation.search.llmQuery.enabled);
    setLlmModel(effective.citation.search.llmQuery.model);
    setLlmEndpoint(effective.citation.search.llmQuery.endpoint);
    setLlmTimeoutMs(String(effective.citation.search.llmQuery.timeoutMs));
    setLlmMaxQueries(String(effective.citation.search.llmQuery.maxQueries));
    setQueryEmbeddingProvider(effective.citation.search.queryEmbedding.provider);
    setQueryEmbeddingTimeoutMs(
      String(effective.citation.search.queryEmbedding.timeoutMs),
    );
    setQueryExecutionTopN(
      String(effective.citation.search.queryExecution.topN),
    );
    setQueryExecutionMmrLambda(
      String(effective.citation.search.queryExecution.mmrLambda),
    );
    setQueryExecutionMinQuality(
      String(effective.citation.search.queryExecution.minQuality),
    );
    setQueryExecutionMinHitRatio(
      String(effective.citation.search.queryExecution.minHitRatio),
    );
    setQueryExecutionHitScoreThreshold(
      String(effective.citation.search.queryExecution.hitScoreThreshold),
    );
    setQueryMode(
      inferNearestQueryMode({
        topN: effective.citation.search.queryExecution.topN,
        mmrLambda: effective.citation.search.queryExecution.mmrLambda,
        minQuality: effective.citation.search.queryExecution.minQuality,
        minHitRatio: effective.citation.search.queryExecution.minHitRatio,
        hitScoreThreshold:
          effective.citation.search.queryExecution.hitScoreThreshold,
      }),
    );
  }, [
    effective.citation.autoApplyThreshold,
    effective.citation.reviewThreshold,
    effective.citation.search.limit,
    effective.integrations.agent.runtime,
    effective.integrations.agent.provider,
    effective.integrations.agent.model,
    effective.integrations.agent.baseUrl,
    effective.integrations.agent.domainConfig.domain,
    effective.integrations.agent.domainConfig.terminologyStrictness,
    effective.integrations.agent.domainConfig.customInstructions,
    effective.integrations.agent.samplingProfiles.editStable.temperature,
    effective.integrations.agent.samplingProfiles.editStable.topP,
    effective.integrations.agent.samplingProfiles.editStable.maxTokens,
    effective.integrations.agent.samplingProfiles.analysisBalanced.temperature,
    effective.integrations.agent.samplingProfiles.analysisBalanced.topP,
    effective.integrations.agent.samplingProfiles.analysisBalanced.maxTokens,
    effective.integrations.agent.samplingProfiles.analysisDeep.temperature,
    effective.integrations.agent.samplingProfiles.analysisDeep.topP,
    effective.integrations.agent.samplingProfiles.analysisDeep.maxTokens,
    effective.integrations.agent.samplingProfiles.chatFlexible.temperature,
    effective.integrations.agent.samplingProfiles.chatFlexible.topP,
    effective.integrations.agent.samplingProfiles.chatFlexible.maxTokens,
    effective.citation.search.llmQuery.enabled,
    effective.citation.search.llmQuery.model,
    effective.citation.search.llmQuery.endpoint,
    effective.citation.search.llmQuery.timeoutMs,
    effective.citation.search.llmQuery.maxQueries,
    effective.citation.search.queryEmbedding.provider,
    effective.citation.search.queryEmbedding.timeoutMs,
    effective.citation.search.queryExecution.topN,
    effective.citation.search.queryExecution.mmrLambda,
    effective.citation.search.queryExecution.minQuality,
    effective.citation.search.queryExecution.minHitRatio,
    effective.citation.search.queryExecution.hitScoreThreshold,
  ]);

  const targetTheme = effective.general.theme;
  useEffect(() => {
    if (targetTheme && theme !== targetTheme) {
      setTheme(targetTheme);
    }
  }, [setTheme, targetTheme, theme]);

  const warningText = useMemo(
    () => (warnings.length > 0 ? warnings.join(" | ") : ""),
    [warnings],
  );

  const hasProjectRoot = !!projectRoot;
  const autoThresholdUsesProject =
    typeof project.data.citation?.autoApplyThreshold === "number";
  const reviewThresholdUsesProject =
    typeof project.data.citation?.reviewThreshold === "number";
  const searchLimitUsesProject =
    typeof project.data.citation?.search?.limit === "number";

  // ─── Save callbacks ───

  const saveCitationAutoThreshold = async () => {
    const auto = Number(autoThreshold);
    if (!Number.isFinite(auto)) {
      toast.error("Auto Threshold 必须是数字。");
      setAutoThreshold(String(effective.citation.autoApplyThreshold));
      return;
    }
    const patchFn =
      hasProjectRoot && autoThresholdUsesProject ? patchProject : patchGlobal;
    await patchFn(
      {
        citation: {
          autoApplyThreshold: auto,
        },
      },
      projectRoot,
    );
  };

  const saveCitationReviewThreshold = async () => {
    const review = Number(reviewThreshold);
    if (!Number.isFinite(review)) {
      toast.error("Review Threshold 必须是数字。");
      setReviewThreshold(String(effective.citation.reviewThreshold));
      return;
    }
    const patchFn =
      hasProjectRoot && reviewThresholdUsesProject ? patchProject : patchGlobal;
    await patchFn(
      {
        citation: {
          reviewThreshold: review,
        },
      },
      projectRoot,
    );
  };

  const saveCitationSearchLimit = async () => {
    const limit = Number(searchLimit);
    if (!Number.isFinite(limit)) {
      toast.error("Search Limit 必须是数字。");
      setSearchLimit(String(effective.citation.search.limit));
      return;
    }
    const patchFn =
      hasProjectRoot && searchLimitUsesProject ? patchProject : patchGlobal;
    await patchFn(
      {
        citation: {
          search: {
            limit,
          },
        },
      },
      projectRoot,
    );
  };

  const saveLlmEnabled = async (enabled: boolean) => {
    setLlmEnabled(enabled);
    await patchGlobal(
      {
        citation: {
          search: {
            llmQuery: {
              enabled,
            },
          },
        },
      },
      projectRoot,
    );
  };

  const saveAgentProvider = async (provider: AgentProvider) => {
    const defaults = AGENT_PROVIDER_DEFAULTS[provider];
    setAgentProvider(provider);
    setAgentModel(defaults.model);
    setAgentBaseUrl(defaults.baseUrl);
    await patchGlobal(
      {
        integrations: {
          agent: {
            provider,
            model: defaults.model,
            baseUrl: defaults.baseUrl,
          },
        },
      },
      projectRoot,
    );
  };

  const saveAgentRuntime = async (runtime: AgentRuntimeKind) => {
    setAgentRuntime(runtime);
    await patchGlobal(
      {
        integrations: {
          agent: {
            runtime,
          },
        },
      },
      projectRoot,
    );
  };

  const saveAgentSamplingProfiles = async () => {
    const parseProfile = (
      name: string,
      rawTemperature: string,
      rawTopP: string,
      rawMaxTokens: string,
    ) => {
      const temperature = Number(rawTemperature);
      const topP = Number(rawTopP);
      const maxTokens = Number(rawMaxTokens);
      if (!Number.isFinite(temperature) || temperature < 0 || temperature > 2) {
        toast.error(`${name} temperature 必须在 0 到 2 之间。`);
        return null;
      }
      if (!Number.isFinite(topP) || topP < 0 || topP > 1) {
        toast.error(`${name} top_p 必须在 0 到 1 之间。`);
        return null;
      }
      if (!Number.isFinite(maxTokens) || maxTokens < 256 || maxTokens > 16384) {
        toast.error(`${name} max_tokens 必须在 256 到 16384 之间。`);
        return null;
      }
      return {
        temperature,
        topP,
        maxTokens: Math.round(maxTokens),
      };
    };

    const editStable = parseProfile(
      "Edit Stable",
      agentEditStableTemperature,
      agentEditStableTopP,
      agentEditStableMaxTokens,
    );
    if (!editStable) {
      setAgentEditStableTemperature(
        String(effective.integrations.agent.samplingProfiles.editStable.temperature),
      );
      setAgentEditStableTopP(
        String(effective.integrations.agent.samplingProfiles.editStable.topP),
      );
      setAgentEditStableMaxTokens(
        String(effective.integrations.agent.samplingProfiles.editStable.maxTokens),
      );
      return;
    }

    const analysisBalanced = parseProfile(
      "Analysis Balanced",
      agentAnalysisTemperature,
      agentAnalysisTopP,
      agentAnalysisMaxTokens,
    );
    if (!analysisBalanced) {
      setAgentAnalysisTemperature(
        String(
          effective.integrations.agent.samplingProfiles.analysisBalanced.temperature,
        ),
      );
      setAgentAnalysisTopP(
        String(effective.integrations.agent.samplingProfiles.analysisBalanced.topP),
      );
      setAgentAnalysisMaxTokens(
        String(
          effective.integrations.agent.samplingProfiles.analysisBalanced.maxTokens,
        ),
      );
      return;
    }

    const chatFlexible = parseProfile(
      "Chat Flexible",
      agentChatTemperature,
      agentChatTopP,
      agentChatMaxTokens,
    );
    if (!chatFlexible) {
      setAgentChatTemperature(
        String(effective.integrations.agent.samplingProfiles.chatFlexible.temperature),
      );
      setAgentChatTopP(
        String(effective.integrations.agent.samplingProfiles.chatFlexible.topP),
      );
      setAgentChatMaxTokens(
        String(effective.integrations.agent.samplingProfiles.chatFlexible.maxTokens),
      );
      return;
    }

    const analysisDeep = parseProfile(
      "Analysis Deep",
      agentAnalysisDeepTemperature,
      agentAnalysisDeepTopP,
      agentAnalysisDeepMaxTokens,
    );
    if (!analysisDeep) {
      setAgentAnalysisDeepTemperature(
        String(effective.integrations.agent.samplingProfiles.analysisDeep.temperature),
      );
      setAgentAnalysisDeepTopP(
        String(effective.integrations.agent.samplingProfiles.analysisDeep.topP),
      );
      setAgentAnalysisDeepMaxTokens(
        String(effective.integrations.agent.samplingProfiles.analysisDeep.maxTokens),
      );
      return;
    }

    await patchGlobal(
      {
        integrations: {
          agent: {
            samplingProfiles: {
              editStable,
              analysisBalanced,
              analysisDeep,
              chatFlexible,
            },
          },
        },
      },
      projectRoot,
    );
  };

  const saveAgentModel = async () => {
    await patchGlobal(
      {
        integrations: {
          agent: {
            model: agentModel.trim() || "gpt-5.4",
          },
        },
      },
      projectRoot,
    );
  };

  const saveAgentBaseUrl = async () => {
    await patchGlobal(
      {
        integrations: {
          agent: {
            baseUrl: agentBaseUrl.trim() || "https://api.openai.com/v1",
          },
        },
      },
      projectRoot,
    );
  };

  const saveAgentDomain = async (domain: AgentDomain) => {
    setAgentDomain(domain);
    await patchGlobal(
      {
        integrations: {
          agent: {
            domainConfig: {
              domain,
            },
          },
        },
      },
      projectRoot,
    );
  };

  const saveTerminologyStrictness = async (
    value: AgentTerminologyStrictness,
  ) => {
    setTerminologyStrictness(value);
    await patchGlobal(
      {
        integrations: {
          agent: {
            domainConfig: {
              terminologyStrictness: value,
            },
          },
        },
      },
      projectRoot,
    );
  };

  const saveCustomDomainInstructions = async () => {
    const normalized = customDomainInstructions.trim();
    await patchGlobal(
      {
        integrations: {
          agent: {
            domainConfig: {
              customInstructions: normalized.length > 0 ? normalized : null,
            },
          },
        },
      },
      projectRoot,
    );
  };

  const saveLlmModel = async () => {
    await patchGlobal(
      {
        citation: {
          search: {
            llmQuery: {
              model: llmModel.trim() || "gpt-4o-mini",
            },
          },
        },
      },
      projectRoot,
    );
  };

  const saveLlmEndpoint = async () => {
    await patchGlobal(
      {
        citation: {
          search: {
            llmQuery: {
              endpoint:
                llmEndpoint.trim() ||
                "https://api.openai.com/v1/chat/completions",
            },
          },
        },
      },
      projectRoot,
    );
  };

  const saveLlmTimeoutMs = async () => {
    const timeoutMs = Number(llmTimeoutMs);
    if (!Number.isFinite(timeoutMs)) {
      toast.error("LLM Timeout 必须是数字。");
      setLlmTimeoutMs(String(effective.citation.search.llmQuery.timeoutMs));
      return;
    }
    await patchGlobal(
      {
        citation: {
          search: {
            llmQuery: {
              timeoutMs,
            },
          },
        },
      },
      projectRoot,
    );
  };

  const saveLlmMaxQueries = async () => {
    const maxQueries = Number(llmMaxQueries);
    if (!Number.isFinite(maxQueries)) {
      toast.error("LLM Max Queries 必须是数字。");
      setLlmMaxQueries(String(effective.citation.search.llmQuery.maxQueries));
      return;
    }
    await patchGlobal(
      {
        citation: {
          search: {
            llmQuery: {
              maxQueries,
            },
          },
        },
      },
      projectRoot,
    );
  };

  const saveQueryEmbeddingEnabled = async (enabled: boolean) => {
    const provider = enabled ? "local_embedding" : "none";
    setQueryEmbeddingProvider(provider);
    await patchGlobal(
      {
        citation: {
          search: {
            queryEmbedding: {
              provider,
            },
          },
        },
      },
      projectRoot,
    );
  };

  const saveQueryEmbeddingProvider = async (
    provider: "none" | "local_embedding",
  ) => {
    setQueryEmbeddingProvider(provider);
    await patchGlobal(
      {
        citation: {
          search: {
            queryEmbedding: {
              provider,
            },
          },
        },
      },
      projectRoot,
    );
  };

  const saveQueryEmbeddingTimeoutMs = async () => {
    const timeoutMs = Number(queryEmbeddingTimeoutMs);
    if (!Number.isFinite(timeoutMs)) {
      toast.error("Embedding Timeout 必须是数字。");
      setQueryEmbeddingTimeoutMs(
        String(effective.citation.search.queryEmbedding.timeoutMs),
      );
      return;
    }
    await patchGlobal(
      {
        citation: {
          search: {
            queryEmbedding: {
              timeoutMs,
            },
          },
        },
      },
      projectRoot,
    );
  };

  const saveQueryExecutionField = async (
    field:
      | "topN"
      | "mmrLambda"
      | "minQuality"
      | "minHitRatio"
      | "hitScoreThreshold",
    rawValue: string,
  ) => {
    const value = Number(rawValue);
    if (!Number.isFinite(value)) {
      toast.error(`${field} 必须是数字。`);
      if (field === "topN") {
        setQueryExecutionTopN(
          String(effective.citation.search.queryExecution.topN),
        );
      } else if (field === "mmrLambda") {
        setQueryExecutionMmrLambda(
          String(effective.citation.search.queryExecution.mmrLambda),
        );
      } else if (field === "minQuality") {
        setQueryExecutionMinQuality(
          String(effective.citation.search.queryExecution.minQuality),
        );
      } else if (field === "minHitRatio") {
        setQueryExecutionMinHitRatio(
          String(effective.citation.search.queryExecution.minHitRatio),
        );
      } else {
        setQueryExecutionHitScoreThreshold(
          String(effective.citation.search.queryExecution.hitScoreThreshold),
        );
      }
      return;
    }

    await patchGlobal(
      {
        citation: {
          search: {
            queryExecution: {
              [field]: value,
            },
          },
        },
      },
      projectRoot,
    );
  };

  const saveQueryMode = async (mode: QueryMode) => {
    const preset = QUERY_MODE_PRESETS[mode];
    setQueryMode(mode);
    setQueryExecutionTopN(String(preset.topN));
    setQueryExecutionMmrLambda(String(preset.mmrLambda));
    setQueryExecutionMinQuality(String(preset.minQuality));
    setQueryExecutionMinHitRatio(String(preset.minHitRatio));
    setQueryExecutionHitScoreThreshold(String(preset.hitScoreThreshold));
    await patchGlobal(
      {
        citation: {
          search: {
            queryExecution: preset,
          },
        },
      },
      projectRoot,
    );
  };

  const toggleProjectOverride = async (
    field: "auto" | "review" | "limit",
    enabled: boolean,
  ) => {
    if (!hasProjectRoot) return;

    if (!enabled) {
      const key =
        field === "auto"
          ? "citation.autoApplyThreshold"
          : field === "review"
            ? "citation.reviewThreshold"
            : "citation.search.limit";
      await resetScope("project", {
        keys: [key],
        projectRoot,
      });
      return;
    }

    if (field === "auto") {
      const auto = Number(autoThreshold);
      await patchProject(
        {
          citation: {
            autoApplyThreshold: Number.isFinite(auto)
              ? auto
              : effective.citation.autoApplyThreshold,
          },
        },
        projectRoot,
      );
      return;
    }

    if (field === "review") {
      const review = Number(reviewThreshold);
      await patchProject(
        {
          citation: {
            reviewThreshold: Number.isFinite(review)
              ? review
              : effective.citation.reviewThreshold,
          },
        },
        projectRoot,
      );
      return;
    }

    const limit = Number(searchLimit);
    await patchProject(
      {
        citation: {
          search: {
            limit: Number.isFinite(limit) ? limit : effective.citation.search.limit,
          },
        },
      },
      projectRoot,
    );
  };

  const saveApiKey = async () => {
    const nextKey = apiKeyInput.trim() || null;
    const ok = await patchSecret(
      {
        integrations: {
          semanticScholar: {
            apiKey: nextKey,
          },
        },
      },
      projectRoot,
    );
    if (ok) {
      setApiKeyInput("");
      toast.success(
        nextKey
          ? "Semantic Scholar API key 已更新。"
          : "Semantic Scholar API key 已清除。",
      );
    }
  };

  const saveAgentApiKey = async () => {
    const nextKey = agentApiKeyInput.trim() || null;
    const ok = await patchSecret(
      {
        integrations: {
          agent: {
            apiKey: nextKey,
          },
        },
      },
      projectRoot,
    );
    if (ok) {
      setAgentApiKeyInput("");
      toast.success(
        nextKey
          ? "Agent Runtime API key 已更新。"
          : "Agent Runtime API key 已清除。",
      );
    }
  };

  const saveLlmApiKey = async () => {
    const nextKey = llmApiKeyInput.trim() || null;
    const ok = await patchSecret(
      {
        integrations: {
          llmQuery: {
            apiKey: nextKey,
          },
        },
      },
      projectRoot,
    );
    if (ok) {
      setLlmApiKeyInput("");
      toast.success(
        nextKey ? "LLM Query API key 已更新。" : "LLM Query API key 已清除。",
      );
    }
  };

  const agentRuntimeMode =
    agentRuntime === "claude_cli"
      ? "claude_cli"
      : agentProvider === "openai"
        ? "responses"
        : "chat_completions";

  const handleExport = async (includeProject: boolean) => {
    const text = await exportJson({
      projectRoot,
      includeProject,
    });
    if (!text) return;
    try {
      await navigator.clipboard.writeText(text);
      toast.success("设置 JSON 已复制到剪贴板。");
    } catch {
      toast.error("复制失败，请手动复制文本。");
      setImportText(text);
    }
  };

  const handleImport = async (mode: SettingsImportMode) => {
    const ok = await importJson({
      jsonText: importText,
      mode,
      projectRoot,
    });
    if (ok) {
      toast.success("设置导入成功。");
      setImportText("");
    }
  };

  const handleResetGlobal = async () => {
    const ok = await resetScope("global", { projectRoot });
    if (ok) {
      toast.success("Global Settings 已重置。");
      setConfirmResetGlobalOpen(false);
    }
  };

  const handleTestProviderConnectivity = async () => {
    setIsTestingProviders(true);
    try {
      const results = await settingsTestProviderConnectivity(projectRoot);
      setProviderConnectivity(results);
      const failedCount = results.filter((r) => !r.ok).length;
      if (failedCount === 0) {
        toast.success("Provider 连通性测试通过。");
      } else {
        toast.error(
          `${failedCount}/${results.length} 个 provider 需要关注，请查看测试结果。`,
        );
      }
    } catch (err) {
      const message = err instanceof Error ? err.message : String(err);
      toast.error(`Provider 连通性测试失败：${message}`);
    } finally {
      setIsTestingProviders(false);
    }
  };

  const handleRunAgentSmoke = async () => {
    if (!projectRoot) {
      toast.error("需要先打开一个项目，才能运行 Agent smoke test。");
      return;
    }

    try {
      setIsRunningAgentSmoke(true);
      const result = await agentSmokeTest(projectRoot);
      setAgentSmokeResult(result);
      if (result.ok) {
        toast.success("Agent smoke test 通过。");
      } else {
        toast.error("Agent smoke test 未完全通过，请查看步骤结果。");
      }
    } catch (err) {
      const message = err instanceof Error ? err.message : String(err);
      setAgentSmokeResult(null);
      toast.error(message);
    } finally {
      setIsRunningAgentSmoke(false);
    }
  };

  const connectivityBadge = (result: ProviderConnectivityResult) => {
    if (result.compatibility === "supported") {
      return {
        label: result.ok ? "OK" : "Supported",
        className: result.ok ? "text-green-600" : "text-sky-600",
      };
    }
    if (result.compatibility === "unsupported") {
      return {
        label: "Unsupported",
        className: "text-amber-600",
      };
    }
    if (result.reachable) {
      return {
        label: "Reachable",
        className: "text-amber-600",
      };
    }
    return {
      label: "Failed",
      className: "text-destructive",
    };
  };

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="flex h-[80vh] max-h-[84vh] w-[min(760px,calc(100vw-1.5rem))] flex-col gap-0 overflow-hidden border border-sidebar-border/70 bg-sidebar p-0 sm:min-h-[620px] sm:max-w-[760px]">
        <DialogHeader className="shrink-0 border-b bg-sidebar/95 px-5 py-3.5">
          <DialogTitle>Workspace Settings</DialogTitle>
        </DialogHeader>

        <div className="min-h-0 flex-1 overflow-hidden px-5 py-3.5">
          <Tabs defaultValue="general" className="flex h-full min-h-0 flex-col">
            <TabsList className="grid w-full shrink-0 grid-cols-4 rounded-lg border border-sidebar-border/70 bg-sidebar-accent/35 p-1">
              <TabsTrigger value="general" className="text-xs">
                General
              </TabsTrigger>
              <TabsTrigger value="assistant" className="text-xs">
                AI Assistant
              </TabsTrigger>
              <TabsTrigger value="citation" className="text-xs">
                Citation
              </TabsTrigger>
              <TabsTrigger value="advanced" className="text-xs">
                Advanced
              </TabsTrigger>
            </TabsList>

            <TabsContent
              value="general"
              className="mt-4 min-h-0 flex-1 overflow-y-auto pr-1"
            >
              <GeneralTab
                effective={effective}
                setTheme={setTheme}
                setThemePreference={setThemePreference}
                patchGlobal={patchGlobal}
                projectRoot={projectRoot}
              />
            </TabsContent>

            <TabsContent
              value="citation"
              className="mt-4 min-h-0 flex-1 overflow-y-auto pr-1"
            >
              <CitationTab
                effective={effective}
                setCitationStylePolicy={setCitationStylePolicy}
                autoThreshold={autoThreshold}
                setAutoThreshold={setAutoThreshold}
                reviewThreshold={reviewThreshold}
                setReviewThreshold={setReviewThreshold}
                searchLimit={searchLimit}
                setSearchLimit={setSearchLimit}
                autoThresholdUsesProject={autoThresholdUsesProject}
                reviewThresholdUsesProject={reviewThresholdUsesProject}
                searchLimitUsesProject={searchLimitUsesProject}
                hasProjectRoot={hasProjectRoot}
                saveCitationAutoThreshold={saveCitationAutoThreshold}
                saveCitationReviewThreshold={saveCitationReviewThreshold}
                saveCitationSearchLimit={saveCitationSearchLimit}
                queryMode={queryMode}
                saveQueryMode={saveQueryMode}
                llmEnabled={llmEnabled}
                saveLlmEnabled={saveLlmEnabled}
                toggleProjectOverride={toggleProjectOverride}
                queryExecutionTopN={queryExecutionTopN}
                setQueryExecutionTopN={setQueryExecutionTopN}
                queryExecutionMmrLambda={queryExecutionMmrLambda}
                setQueryExecutionMmrLambda={setQueryExecutionMmrLambda}
                queryExecutionMinQuality={queryExecutionMinQuality}
                setQueryExecutionMinQuality={setQueryExecutionMinQuality}
                queryExecutionMinHitRatio={queryExecutionMinHitRatio}
                setQueryExecutionMinHitRatio={setQueryExecutionMinHitRatio}
                queryExecutionHitScoreThreshold={queryExecutionHitScoreThreshold}
                setQueryExecutionHitScoreThreshold={setQueryExecutionHitScoreThreshold}
                saveQueryExecutionField={saveQueryExecutionField}
                patchGlobal={patchGlobal}
                projectRoot={projectRoot}
                secretsMeta={secretsMeta}
                isSaving={isSaving}
                apiKeyInput={apiKeyInput}
                setApiKeyInput={setApiKeyInput}
                showSemanticApiKey={showSemanticApiKey}
                setShowSemanticApiKey={setShowSemanticApiKey}
                saveApiKey={saveApiKey}
                llmModel={llmModel}
                setLlmModel={setLlmModel}
                llmEndpoint={llmEndpoint}
                setLlmEndpoint={setLlmEndpoint}
                llmTimeoutMs={llmTimeoutMs}
                setLlmTimeoutMs={setLlmTimeoutMs}
                llmMaxQueries={llmMaxQueries}
                setLlmMaxQueries={setLlmMaxQueries}
                llmApiKeyInput={llmApiKeyInput}
                setLlmApiKeyInput={setLlmApiKeyInput}
                showLlmApiKey={showLlmApiKey}
                setShowLlmApiKey={setShowLlmApiKey}
                saveLlmModel={saveLlmModel}
                saveLlmEndpoint={saveLlmEndpoint}
                saveLlmTimeoutMs={saveLlmTimeoutMs}
                saveLlmMaxQueries={saveLlmMaxQueries}
                saveLlmApiKey={saveLlmApiKey}
                queryEmbeddingProvider={queryEmbeddingProvider}
                queryEmbeddingTimeoutMs={queryEmbeddingTimeoutMs}
                setQueryEmbeddingTimeoutMs={setQueryEmbeddingTimeoutMs}
                saveQueryEmbeddingEnabled={saveQueryEmbeddingEnabled}
                saveQueryEmbeddingProvider={saveQueryEmbeddingProvider}
                saveQueryEmbeddingTimeoutMs={saveQueryEmbeddingTimeoutMs}
              />
            </TabsContent>

            <TabsContent
              value="assistant"
              className="mt-4 min-h-0 flex-1 overflow-y-auto pr-1"
            >
              <AIAssistantTab
                agentRuntime={agentRuntime}
                agentProvider={agentProvider}
                agentModel={agentModel}
                setAgentModel={setAgentModel}
                agentBaseUrl={agentBaseUrl}
                setAgentBaseUrl={setAgentBaseUrl}
                agentRuntimeMode={agentRuntimeMode}
                saveAgentRuntime={saveAgentRuntime}
                saveAgentProvider={saveAgentProvider}
                saveAgentModel={saveAgentModel}
                saveAgentBaseUrl={saveAgentBaseUrl}
                agentDomain={agentDomain}
                saveAgentDomain={saveAgentDomain}
                terminologyStrictness={terminologyStrictness}
                saveTerminologyStrictness={saveTerminologyStrictness}
                customDomainInstructions={customDomainInstructions}
                setCustomDomainInstructions={setCustomDomainInstructions}
                saveCustomDomainInstructions={saveCustomDomainInstructions}
                agentApiKeyInput={agentApiKeyInput}
                setAgentApiKeyInput={setAgentApiKeyInput}
                showAgentApiKey={showAgentApiKey}
                setShowAgentApiKey={setShowAgentApiKey}
                saveAgentApiKey={saveAgentApiKey}
                agentApiKeyConfigured={secretsMeta.integrations.agent.apiKeyConfigured}
                isSaving={isSaving}
                agentEditStableTemperature={agentEditStableTemperature}
                setAgentEditStableTemperature={setAgentEditStableTemperature}
                agentEditStableTopP={agentEditStableTopP}
                setAgentEditStableTopP={setAgentEditStableTopP}
                agentEditStableMaxTokens={agentEditStableMaxTokens}
                setAgentEditStableMaxTokens={setAgentEditStableMaxTokens}
                agentAnalysisTemperature={agentAnalysisTemperature}
                setAgentAnalysisTemperature={setAgentAnalysisTemperature}
                agentAnalysisTopP={agentAnalysisTopP}
                setAgentAnalysisTopP={setAgentAnalysisTopP}
                agentAnalysisMaxTokens={agentAnalysisMaxTokens}
                setAgentAnalysisMaxTokens={setAgentAnalysisMaxTokens}
                agentAnalysisDeepTemperature={agentAnalysisDeepTemperature}
                setAgentAnalysisDeepTemperature={setAgentAnalysisDeepTemperature}
                agentAnalysisDeepTopP={agentAnalysisDeepTopP}
                setAgentAnalysisDeepTopP={setAgentAnalysisDeepTopP}
                agentAnalysisDeepMaxTokens={agentAnalysisDeepMaxTokens}
                setAgentAnalysisDeepMaxTokens={setAgentAnalysisDeepMaxTokens}
                agentChatTemperature={agentChatTemperature}
                setAgentChatTemperature={setAgentChatTemperature}
                agentChatTopP={agentChatTopP}
                setAgentChatTopP={setAgentChatTopP}
                agentChatMaxTokens={agentChatMaxTokens}
                setAgentChatMaxTokens={setAgentChatMaxTokens}
                saveAgentSamplingProfiles={saveAgentSamplingProfiles}
                hasProjectRoot={hasProjectRoot}
                isRunningAgentSmoke={isRunningAgentSmoke}
                agentSmokeResult={agentSmokeResult}
                handleRunAgentSmoke={handleRunAgentSmoke}
              />
            </TabsContent>

            <TabsContent
              value="advanced"
              className="mt-4 min-h-0 flex-1 overflow-y-auto pr-1"
            >
              <AdvancedTab
                effective={effective}
                patchGlobal={patchGlobal}
                projectRoot={projectRoot}
                isSaving={isSaving}
                isTestingProviders={isTestingProviders}
                providerConnectivity={providerConnectivity}
                handleTestProviderConnectivity={handleTestProviderConnectivity}
                connectivityBadge={connectivityBadge}
                importText={importText}
                setImportText={setImportText}
                handleExport={handleExport}
                handleImport={handleImport}
                setConfirmResetGlobalOpen={setConfirmResetGlobalOpen}
              />
            </TabsContent>
          </Tabs>
        </div>

        {(isLoading || isSaving || error || warningText) && (
          <div className="shrink-0 space-y-1 border-t px-5 py-3 text-xs">
            {(isLoading || isSaving) && (
              <div className="flex items-center gap-2 text-muted-foreground">
                <LoaderIcon className="size-3 animate-spin" />
                {isLoading ? "Loading settings..." : "Saving settings..."}
              </div>
            )}
            {error && <p className="text-destructive">{error}</p>}
            {warningText && (
              <p className="text-muted-foreground">{warningText}</p>
            )}
          </div>
        )}
      </DialogContent>

      <Dialog
        open={confirmResetGlobalOpen}
        onOpenChange={setConfirmResetGlobalOpen}
      >
        <DialogContent className="sm:max-w-md">
          <DialogHeader>
            <DialogTitle>确认重置 Global Settings？</DialogTitle>
          </DialogHeader>
          <p className="text-muted-foreground text-sm">
            该操作会将全部 Global 设置恢复到默认值，且会立即生效。
          </p>
          <DialogFooter>
            <Button
              variant="outline"
              onClick={() => setConfirmResetGlobalOpen(false)}
            >
              取消
            </Button>
            <Button
              variant="destructive"
              onClick={() => void handleResetGlobal()}
              disabled={isSaving}
            >
              确认重置
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </Dialog>
  );
}
