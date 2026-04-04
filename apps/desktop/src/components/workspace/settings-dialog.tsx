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
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Switch } from "@/components/ui/switch";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { Textarea } from "@/components/ui/textarea";
import { useSettingsStore } from "@/stores/settings-store";
import {
  agentSmokeTest,
  settingsTestProviderConnectivity,
  type AgentSmokeResult,
  type ProviderConnectivityResult,
  type SettingsImportMode,
} from "@/lib/settings-api";

interface SettingsDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  projectRoot: string | null;
}

type QueryMode = "fast" | "balanced" | "deep";
type AgentProvider = "openai" | "minimax" | "deepseek";
type AgentRuntimeKind = "claude_cli" | "local_agent";
const panelClass = "space-y-3 rounded-xl border border-sidebar-border/70 bg-sidebar-accent/20 p-4";

const AGENT_PROVIDER_DEFAULTS: Record<
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

const QUERY_MODE_PRESETS: Record<
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

function inferNearestQueryMode(input: {
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

  const agentApiKeyConfigured = secretsMeta.integrations.agent.apiKeyConfigured;
  const agentRuntimeMode =
    agentRuntime === "claude_cli"
      ? "claude_cli"
      : agentProvider === "openai"
        ? "responses"
        : "chat_completions";
  const llmApiKeyConfigured =
    secretsMeta.integrations.llmQuery.apiKeyConfigured;

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
            <TabsList className="grid w-full shrink-0 grid-cols-5 rounded-lg border border-sidebar-border/70 bg-sidebar-accent/35 p-1">
              <TabsTrigger value="general" className="text-xs">
                General
              </TabsTrigger>
              <TabsTrigger value="citation" className="text-xs">
                Citation
              </TabsTrigger>
              <TabsTrigger value="integrations" className="text-xs">
                Providers
              </TabsTrigger>
              <TabsTrigger value="advanced" className="text-xs">
                Advanced
              </TabsTrigger>
              <TabsTrigger value="io" className="text-xs">
                Import/Export
              </TabsTrigger>
            </TabsList>

            <TabsContent
              value="general"
              className="mt-4 min-h-0 flex-1 overflow-y-auto pr-1"
            >
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
            </TabsContent>

            <TabsContent
              value="citation"
              className="mt-4 min-h-0 flex-1 overflow-y-auto pr-1"
            >
              <div className="space-y-4">
                <div className="space-y-2">
                  <Label>Citation Style Policy</Label>
                  <Select
                    value={effective.citation.stylePolicy}
                    onValueChange={(value: string) =>
                      void setCitationStylePolicy(
                        value as "auto" | "cite" | "citep" | "autocite",
                        projectRoot,
                      )
                    }
                  >
                    <SelectTrigger className="w-full">
                      <SelectValue />
                    </SelectTrigger>
                    <SelectContent>
                      <SelectItem value="auto">Auto</SelectItem>
                      <SelectItem value="cite">\cite</SelectItem>
                      <SelectItem value="citep">\citep</SelectItem>
                      <SelectItem value="autocite">\autocite</SelectItem>
                    </SelectContent>
                  </Select>
                </div>

                <div className="grid grid-cols-1 gap-3 md:grid-cols-3">
                  <div className="space-y-2">
                    <Label>
                      Auto Threshold{" "}
                      <span className="text-muted-foreground text-xs">
                        ({autoThresholdUsesProject ? "Project" : "Global"})
                      </span>
                    </Label>
                    <Input
                      value={autoThreshold}
                      onChange={(e) => setAutoThreshold(e.target.value)}
                      onBlur={() => void saveCitationAutoThreshold()}
                      inputMode="decimal"
                    />
                    {hasProjectRoot && (
                      <div className="flex items-center justify-between">
                        <span className="text-muted-foreground text-xs">
                          Use project override
                        </span>
                        <Switch
                          checked={autoThresholdUsesProject}
                          onCheckedChange={(checked) =>
                            void toggleProjectOverride("auto", checked)
                          }
                        />
                      </div>
                    )}
                  </div>
                  <div className="space-y-2">
                    <Label>
                      Review Threshold{" "}
                      <span className="text-muted-foreground text-xs">
                        ({reviewThresholdUsesProject ? "Project" : "Global"})
                      </span>
                    </Label>
                    <Input
                      value={reviewThreshold}
                      onChange={(e) => setReviewThreshold(e.target.value)}
                      onBlur={() => void saveCitationReviewThreshold()}
                      inputMode="decimal"
                    />
                    {hasProjectRoot && (
                      <div className="flex items-center justify-between">
                        <span className="text-muted-foreground text-xs">
                          Use project override
                        </span>
                        <Switch
                          checked={reviewThresholdUsesProject}
                          onCheckedChange={(checked) =>
                            void toggleProjectOverride("review", checked)
                          }
                        />
                      </div>
                    )}
                  </div>
                  <div className="space-y-2">
                    <Label>
                      Search Limit{" "}
                      <span className="text-muted-foreground text-xs">
                        ({searchLimitUsesProject ? "Project" : "Global"})
                      </span>
                    </Label>
                    <Input
                      value={searchLimit}
                      onChange={(e) => setSearchLimit(e.target.value)}
                      onBlur={() => void saveCitationSearchLimit()}
                      inputMode="numeric"
                    />
                    {hasProjectRoot && (
                      <div className="flex items-center justify-between">
                        <span className="text-muted-foreground text-xs">
                          Use project override
                        </span>
                        <Switch
                          checked={searchLimitUsesProject}
                          onCheckedChange={(checked) =>
                            void toggleProjectOverride("limit", checked)
                          }
                        />
                      </div>
                    )}
                  </div>
                </div>

                <div className={panelClass}>
                  <div>
                    <p className="font-medium text-sm">Search Mode</p>
                    <p className="text-muted-foreground text-xs">
                      快速：更少 query 更快返回；平衡：综合效果最佳；深入：扩大检索覆盖。
                    </p>
                  </div>
                  <Select
                    value={queryMode}
                    onValueChange={(value: string) =>
                      void saveQueryMode(value as QueryMode)
                    }
                  >
                    <SelectTrigger className="w-full">
                      <SelectValue />
                    </SelectTrigger>
                    <SelectContent>
                      <SelectItem value="fast">Fast</SelectItem>
                      <SelectItem value="balanced">Balanced (Recommended)</SelectItem>
                      <SelectItem value="deep">Deep</SelectItem>
                    </SelectContent>
                  </Select>
                </div>

                <div className="flex items-center justify-between rounded-xl border border-sidebar-border/70 bg-sidebar-accent/20 p-4">
                  <div>
                    <p className="font-medium text-sm">LLM Query Rewrite</p>
                    <p className="text-muted-foreground text-xs">
                      使用 LLM 生成更贴近语义的检索词。
                      {!llmApiKeyConfigured
                        ? " 当前未配置 API key，开启后不会生效。"
                        : ""}
                    </p>
                  </div>
                  <Switch
                    checked={llmEnabled}
                    onCheckedChange={(checked) => void saveLlmEnabled(checked)}
                  />
                </div>

                <details className="group rounded-xl border border-sidebar-border/70 bg-sidebar-accent/20 p-4">
                  <summary className="cursor-pointer list-none font-medium text-sm">
                    高级参数（一般不需要调整）
                  </summary>
                  <p className="mt-2 text-muted-foreground text-xs">
                    仅在你明确知道影响时再修改，默认建议使用上面的 Search Mode。
                  </p>
                  <div className="mt-3 grid grid-cols-1 gap-3 md:grid-cols-3">
                    <div className="space-y-2">
                      <Label>Top N</Label>
                      <Input
                        value={queryExecutionTopN}
                        onChange={(e) => setQueryExecutionTopN(e.target.value)}
                        onBlur={() =>
                          void saveQueryExecutionField("topN", queryExecutionTopN)
                        }
                        inputMode="numeric"
                      />
                    </div>
                    <div className="space-y-2">
                      <Label>MMR λ</Label>
                      <Input
                        value={queryExecutionMmrLambda}
                        onChange={(e) => setQueryExecutionMmrLambda(e.target.value)}
                        onBlur={() =>
                          void saveQueryExecutionField(
                            "mmrLambda",
                            queryExecutionMmrLambda,
                          )
                        }
                        inputMode="decimal"
                      />
                    </div>
                    <div className="space-y-2">
                      <Label>Min Quality</Label>
                      <Input
                        value={queryExecutionMinQuality}
                        onChange={(e) => setQueryExecutionMinQuality(e.target.value)}
                        onBlur={() =>
                          void saveQueryExecutionField(
                            "minQuality",
                            queryExecutionMinQuality,
                          )
                        }
                        inputMode="decimal"
                      />
                    </div>
                    <div className="space-y-2">
                      <Label>Min Hit Ratio</Label>
                      <Input
                        value={queryExecutionMinHitRatio}
                        onChange={(e) => setQueryExecutionMinHitRatio(e.target.value)}
                        onBlur={() =>
                          void saveQueryExecutionField(
                            "minHitRatio",
                            queryExecutionMinHitRatio,
                          )
                        }
                        inputMode="decimal"
                      />
                    </div>
                    <div className="space-y-2">
                      <Label>Hit Score Threshold</Label>
                      <Input
                        value={queryExecutionHitScoreThreshold}
                        onChange={(e) =>
                          setQueryExecutionHitScoreThreshold(e.target.value)
                        }
                        onBlur={() =>
                          void saveQueryExecutionField(
                            "hitScoreThreshold",
                            queryExecutionHitScoreThreshold,
                          )
                        }
                        inputMode="decimal"
                      />
                    </div>
                  </div>
                </details>
              </div>
            </TabsContent>

            <TabsContent
              value="integrations"
              className="mt-4 min-h-0 flex-1 overflow-y-auto pr-1"
            >
              <div className="space-y-4">
                <div className={panelClass}>
                  <div>
                    <p className="font-medium text-sm">Agent Runtime</p>
                    <p className="text-muted-foreground text-xs">
                      现在可以显式切换聊天主 runtime。`Claude CLI`
                      更接近你之前熟悉的 Claude Code 体验；`Local Agent`
                      则继续使用本地 agent runtime 与 provider/tooling。
                    </p>
                  </div>
                  <div className="grid grid-cols-1 gap-3 md:grid-cols-2">
                    <div className="space-y-2">
                      <Label>Chat Runtime</Label>
                      <Select
                        value={agentRuntime}
                        onValueChange={(value) =>
                          void saveAgentRuntime(value as AgentRuntimeKind)
                        }
                      >
                        <SelectTrigger className="w-full">
                          <SelectValue />
                        </SelectTrigger>
                        <SelectContent>
                          <SelectItem value="claude_cli">
                            Claude CLI
                          </SelectItem>
                          <SelectItem value="local_agent">
                            Local Agent
                          </SelectItem>
                        </SelectContent>
                      </Select>
                    </div>
                    <div className="space-y-2">
                      <Label>Provider</Label>
                      <Select
                        value={agentProvider}
                        onValueChange={(value) =>
                          void saveAgentProvider(value as AgentProvider)
                        }
                      >
                        <SelectTrigger className="w-full">
                          <SelectValue />
                        </SelectTrigger>
                        <SelectContent>
                          <SelectItem value="openai">OpenAI</SelectItem>
                          <SelectItem value="minimax">MiniMax</SelectItem>
                          <SelectItem value="deepseek">DeepSeek</SelectItem>
                        </SelectContent>
                      </Select>
                    </div>
                    <div className="space-y-2">
                      <Label>Transport</Label>
                      <Input value={agentRuntimeMode} disabled />
                    </div>
                    <div className="space-y-2">
                      <Label>Agent Model</Label>
                      <Input
                        value={agentModel}
                        onChange={(e) => setAgentModel(e.target.value)}
                        onBlur={() => void saveAgentModel()}
                      />
                    </div>
                    <div className="space-y-2 md:col-span-2">
                      <Label>Base URL</Label>
                      <Input
                        value={agentBaseUrl}
                        onChange={(e) => setAgentBaseUrl(e.target.value)}
                        onBlur={() => void saveAgentBaseUrl()}
                        disabled={agentRuntime === "claude_cli"}
                      />
                    </div>
                  </div>
                  {agentRuntime === "claude_cli" ? (
                    <p className="text-muted-foreground text-xs">
                      当前聊天默认走 Claude CLI。下面的 provider / model /
                      sampling profiles 仍会保留，用于你切回 Local Agent 时继续生效。
                    </p>
                  ) : (
                    <p className="text-muted-foreground text-xs">
                      Local Agent 会使用这里的 provider 配置：`openai`
                      走 `responses`，`minimax`/`deepseek` 走
                      `chat_completions`。
                    </p>
                  )}
                  <details className="rounded-lg border border-sidebar-border/60 bg-sidebar/35 p-3">
                    <summary className="cursor-pointer font-medium text-sm">
                      Sampling Profiles
                    </summary>
                    <p className="mt-1 text-muted-foreground text-xs">
                      将任务路由和采样参数正式绑定到 runtime。编辑、分析和普通聊天不再共用同一组默认采样。
                    </p>
                    <div className="mt-3 grid grid-cols-1 gap-4 md:grid-cols-4">
                      <div className="space-y-2 rounded-md border border-sidebar-border/50 p-3">
                        <p className="font-medium text-sm">Edit Stable</p>
                        <p className="text-muted-foreground text-xs">
                          selection/file edit 主路径
                        </p>
                        <Label>Temperature</Label>
                        <Input
                          value={agentEditStableTemperature}
                          onChange={(e) => setAgentEditStableTemperature(e.target.value)}
                          onBlur={() => void saveAgentSamplingProfiles()}
                          inputMode="decimal"
                        />
                        <Label>Top P</Label>
                        <Input
                          value={agentEditStableTopP}
                          onChange={(e) => setAgentEditStableTopP(e.target.value)}
                          onBlur={() => void saveAgentSamplingProfiles()}
                          inputMode="decimal"
                        />
                        <Label>Max Tokens</Label>
                        <Input
                          value={agentEditStableMaxTokens}
                          onChange={(e) => setAgentEditStableMaxTokens(e.target.value)}
                          onBlur={() => void saveAgentSamplingProfiles()}
                          inputMode="numeric"
                        />
                      </div>
                      <div className="space-y-2 rounded-md border border-sidebar-border/50 p-3">
                        <p className="font-medium text-sm">Analysis Balanced</p>
                        <p className="text-muted-foreground text-xs">
                          critique / explain / review-only
                        </p>
                        <Label>Temperature</Label>
                        <Input
                          value={agentAnalysisTemperature}
                          onChange={(e) => setAgentAnalysisTemperature(e.target.value)}
                          onBlur={() => void saveAgentSamplingProfiles()}
                          inputMode="decimal"
                        />
                        <Label>Top P</Label>
                        <Input
                          value={agentAnalysisTopP}
                          onChange={(e) => setAgentAnalysisTopP(e.target.value)}
                          onBlur={() => void saveAgentSamplingProfiles()}
                          inputMode="decimal"
                        />
                        <Label>Max Tokens</Label>
                        <Input
                          value={agentAnalysisMaxTokens}
                          onChange={(e) => setAgentAnalysisMaxTokens(e.target.value)}
                          onBlur={() => void saveAgentSamplingProfiles()}
                          inputMode="numeric"
                        />
                      </div>
                      <div className="space-y-2 rounded-md border border-sidebar-border/50 p-3">
                        <p className="font-medium text-sm">Analysis Deep</p>
                        <p className="text-muted-foreground text-xs">
                          attached resources / literature QA / synthesis
                        </p>
                        <Label>Temperature</Label>
                        <Input
                          value={agentAnalysisDeepTemperature}
                          onChange={(e) => setAgentAnalysisDeepTemperature(e.target.value)}
                          onBlur={() => void saveAgentSamplingProfiles()}
                          inputMode="decimal"
                        />
                        <Label>Top P</Label>
                        <Input
                          value={agentAnalysisDeepTopP}
                          onChange={(e) => setAgentAnalysisDeepTopP(e.target.value)}
                          onBlur={() => void saveAgentSamplingProfiles()}
                          inputMode="decimal"
                        />
                        <Label>Max Tokens</Label>
                        <Input
                          value={agentAnalysisDeepMaxTokens}
                          onChange={(e) => setAgentAnalysisDeepMaxTokens(e.target.value)}
                          onBlur={() => void saveAgentSamplingProfiles()}
                          inputMode="numeric"
                        />
                      </div>
                      <div className="space-y-2 rounded-md border border-sidebar-border/50 p-3">
                        <p className="font-medium text-sm">Chat Flexible</p>
                        <p className="text-muted-foreground text-xs">
                          通用聊天 fallback
                        </p>
                        <Label>Temperature</Label>
                        <Input
                          value={agentChatTemperature}
                          onChange={(e) => setAgentChatTemperature(e.target.value)}
                          onBlur={() => void saveAgentSamplingProfiles()}
                          inputMode="decimal"
                        />
                        <Label>Top P</Label>
                        <Input
                          value={agentChatTopP}
                          onChange={(e) => setAgentChatTopP(e.target.value)}
                          onBlur={() => void saveAgentSamplingProfiles()}
                          inputMode="decimal"
                        />
                        <Label>Max Tokens</Label>
                        <Input
                          value={agentChatMaxTokens}
                          onChange={(e) => setAgentChatMaxTokens(e.target.value)}
                          onBlur={() => void saveAgentSamplingProfiles()}
                          inputMode="numeric"
                        />
                      </div>
                    </div>
                  </details>
                  <div className="space-y-2">
                    <div>
                      <p className="font-medium text-sm">Agent API Key</p>
                      <p className="text-muted-foreground text-xs">
                        当前状态：
                        {agentApiKeyConfigured ? "已配置" : "未配置"}
                        {" · "}留空后保存会清除
                      </p>
                    </div>
                    <div className="flex gap-2">
                      <Input
                        type={showAgentApiKey ? "text" : "password"}
                        placeholder="输入 Agent API key（留空可清空）"
                        value={agentApiKeyInput}
                        onChange={(e) => setAgentApiKeyInput(e.target.value)}
                      />
                      <Button
                        variant="outline"
                        size="sm"
                        onClick={() => setShowAgentApiKey((v) => !v)}
                      >
                        {showAgentApiKey ? "隐藏" : "显示"}
                      </Button>
                      <Button
                        variant="outline"
                        size="sm"
                        onClick={() => void saveAgentApiKey()}
                        disabled={isSaving}
                      >
                        保存 Key
                      </Button>
                    </div>
                  </div>
                  <div className="space-y-2 rounded-lg border border-sidebar-border/60 bg-sidebar/40 p-3">
                    <div className="flex items-start justify-between gap-3">
                      <div>
                        <p className="font-medium text-sm">Agent Smoke Test</p>
                        <p className="text-muted-foreground text-xs">
                          直接验证当前 provider 的文本流、tool loop 与多轮续接，不再只看连通性。
                        </p>
                      </div>
                      <Button
                        variant="outline"
                        size="sm"
                        onClick={() => void handleRunAgentSmoke()}
                        disabled={isSaving || isRunningAgentSmoke || !hasProjectRoot}
                      >
                        {isRunningAgentSmoke ? (
                          <>
                            <LoaderIcon className="mr-1 size-3 animate-spin" />
                            Running
                          </>
                        ) : (
                          "Run Smoke"
                        )}
                      </Button>
                    </div>
                    {!hasProjectRoot ? (
                      <p className="text-muted-foreground text-xs">
                        需要先打开一个项目，才能运行 smoke test。
                      </p>
                    ) : agentSmokeResult ? (
                      <div className="space-y-2 text-xs">
                        <div className="flex items-center justify-between gap-2">
                          <span className="text-muted-foreground">
                            {agentSmokeResult.provider} · {agentSmokeResult.runtimeMode}
                          </span>
                          <span
                            className={
                              agentSmokeResult.ok
                                ? "rounded-full bg-emerald-500/15 px-2 py-0.5 text-emerald-300"
                                : "rounded-full bg-amber-500/15 px-2 py-0.5 text-amber-300"
                            }
                          >
                            {agentSmokeResult.ok ? "PASS" : "PARTIAL"}
                          </span>
                        </div>
                        <div className="space-y-1.5">
                          {agentSmokeResult.steps.map((step) => (
                            <div
                              key={step.name}
                              className="rounded-md border border-sidebar-border/50 px-2 py-1.5"
                            >
                              <div className="flex items-center justify-between gap-2">
                                <span className="font-medium">{step.name}</span>
                                <span
                                  className={
                                    step.ok
                                      ? "text-emerald-300"
                                      : "text-amber-300"
                                  }
                                >
                                  {step.ok ? "ok" : "failed"}
                                </span>
                              </div>
                              <p className="mt-0.5 text-muted-foreground">
                                {step.detail}
                              </p>
                            </div>
                          ))}
                        </div>
                      </div>
                    ) : (
                      <p className="text-muted-foreground text-xs">
                        尚未运行 smoke test。
                      </p>
                    )}
                  </div>
                </div>

                <div className="flex items-center justify-between rounded-xl border border-sidebar-border/70 bg-sidebar-accent/20 p-4">
                  <div>
                    <p className="font-medium text-sm">Semantic Scholar</p>
                    <p className="text-muted-foreground text-xs">
                      开关检索 provider（下次检索生效）
                    </p>
                  </div>
                  <Switch
                    checked={effective.integrations.semanticScholar.enabled}
                    onCheckedChange={(checked: boolean) =>
                      void patchGlobal(
                        {
                          integrations: {
                            semanticScholar: { enabled: checked },
                          },
                        },
                        projectRoot,
                      )
                    }
                  />
                </div>

                <div className={panelClass}>
                  <div>
                    <p className="font-medium text-sm">
                      Semantic Scholar API Key
                    </p>
                    <p className="text-muted-foreground text-xs">
                      当前状态：
                      {secretsMeta.integrations.semanticScholar.apiKeyConfigured
                        ? "已配置"
                        : "未配置"}
                      {" · "}留空后保存会清除
                    </p>
                  </div>
                  <div className="flex gap-2">
                    <Input
                      type={showSemanticApiKey ? "text" : "password"}
                      placeholder="输入新 API key（留空可清空）"
                      value={apiKeyInput}
                      onChange={(e) => setApiKeyInput(e.target.value)}
                    />
                    <Button
                      variant="outline"
                      size="sm"
                      onClick={() => setShowSemanticApiKey((v) => !v)}
                    >
                      {showSemanticApiKey ? "隐藏" : "显示"}
                    </Button>
                    <Button
                      variant="outline"
                      size="sm"
                      onClick={() => void saveApiKey()}
                      disabled={isSaving}
                    >
                      保存 Key
                    </Button>
                  </div>
                </div>

                <div className="flex items-center justify-between rounded-xl border border-sidebar-border/70 bg-sidebar-accent/20 p-4">
                  <div>
                    <p className="font-medium text-sm">
                      Zotero Auto Sync On Apply
                    </p>
                    <p className="text-muted-foreground text-xs">
                      自动把采纳的引用写入 Zotero
                    </p>
                  </div>
                  <Switch
                    checked={effective.integrations.zotero.autoSyncOnApply}
                    onCheckedChange={(checked: boolean) =>
                      void patchGlobal(
                        {
                          integrations: {
                            zotero: { autoSyncOnApply: checked },
                          },
                        },
                        projectRoot,
                      )
                    }
                  />
                </div>

                <div className={panelClass}>
                  <div>
                    <div>
                      <p className="font-medium text-sm">LLM Query 配置</p>
                      <p className="text-muted-foreground text-xs">
                        开关在 Citation 页控制，这里只配置模型、端点与 API key。
                      </p>
                    </div>
                  </div>
                  <div className="text-muted-foreground text-xs">
                    API key：
                    {llmApiKeyConfigured ? "已配置" : "未配置"}
                    {!llmApiKeyConfigured ? "（未配置时开关不会生效）" : ""}
                  </div>
                  <div className="grid grid-cols-1 gap-3 md:grid-cols-2">
                    <div className="space-y-2">
                      <Label>LLM Model</Label>
                      <Input
                        value={llmModel}
                        onChange={(e) => setLlmModel(e.target.value)}
                        onBlur={() => void saveLlmModel()}
                      />
                    </div>
                    <div className="space-y-2">
                      <Label>LLM Timeout (ms)</Label>
                      <Input
                        value={llmTimeoutMs}
                        onChange={(e) => setLlmTimeoutMs(e.target.value)}
                        onBlur={() => void saveLlmTimeoutMs()}
                        inputMode="numeric"
                      />
                    </div>
                    <div className="space-y-2">
                      <Label>LLM Endpoint</Label>
                      <Input
                        value={llmEndpoint}
                        onChange={(e) => setLlmEndpoint(e.target.value)}
                        onBlur={() => void saveLlmEndpoint()}
                      />
                    </div>
                    <div className="space-y-2">
                      <Label>LLM Max Queries</Label>
                      <Input
                        value={llmMaxQueries}
                        onChange={(e) => setLlmMaxQueries(e.target.value)}
                        onBlur={() => void saveLlmMaxQueries()}
                        inputMode="numeric"
                      />
                    </div>
                  </div>
                  <div className="space-y-2">
                    <Label>LLM Query API Key</Label>
                    <p className="text-muted-foreground text-xs">
                      留空后保存会清除
                    </p>
                    <div className="flex gap-2">
                      <Input
                        type={showLlmApiKey ? "text" : "password"}
                        placeholder="输入 LLM API key（留空可清空）"
                        value={llmApiKeyInput}
                        onChange={(e) => setLlmApiKeyInput(e.target.value)}
                      />
                      <Button
                        variant="outline"
                        size="sm"
                        onClick={() => setShowLlmApiKey((v) => !v)}
                      >
                        {showLlmApiKey ? "隐藏" : "显示"}
                      </Button>
                      <Button
                        variant="outline"
                        size="sm"
                        onClick={() => void saveLlmApiKey()}
                        disabled={isSaving}
                      >
                        保存 Key
                      </Button>
                    </div>
                  </div>
                </div>

                <div className={panelClass}>
                  <div className="flex items-center justify-between">
                    <div>
                      <p className="font-medium text-sm">
                        Query Embedding Rerank (Local)
                      </p>
                      <p className="text-muted-foreground text-xs">
                        为检索词打分增加本地向量相似度；失败/超时会自动回退到规则分。
                      </p>
                    </div>
                    <Switch
                      checked={queryEmbeddingProvider !== "none"}
                      onCheckedChange={(checked) =>
                        void saveQueryEmbeddingEnabled(checked)
                      }
                    />
                  </div>
                  <div className="grid grid-cols-1 gap-3 md:grid-cols-2">
                    <div className="space-y-2">
                      <Label>Embedding Provider</Label>
                      <Select
                        value={queryEmbeddingProvider}
                        onValueChange={(value: string) =>
                          void saveQueryEmbeddingProvider(
                            value as "none" | "local_embedding",
                          )
                        }
                      >
                        <SelectTrigger className="w-full">
                          <SelectValue />
                        </SelectTrigger>
                        <SelectContent>
                          <SelectItem value="none">none</SelectItem>
                          <SelectItem value="local_embedding">
                            local_embedding
                          </SelectItem>
                        </SelectContent>
                      </Select>
                    </div>
                    <div className="space-y-2">
                      <Label>Embedding Timeout (ms)</Label>
                      <Input
                        value={queryEmbeddingTimeoutMs}
                        onChange={(e) =>
                          setQueryEmbeddingTimeoutMs(e.target.value)
                        }
                        onBlur={() => void saveQueryEmbeddingTimeoutMs()}
                        inputMode="numeric"
                      />
                    </div>
                  </div>
                </div>

                <div className={panelClass}>
                  <div className="flex items-start justify-between gap-3">
                    <div>
                      <p className="font-medium text-sm">Provider Connectivity</p>
                      <p className="text-muted-foreground text-xs">
                        区分 `Base URL reachable`、`Responses compatible` 与
                        `Chat Completions compatible`，避免 `/models` 的假阳性。
                      </p>
                    </div>
                    <Button
                      variant="outline"
                      size="sm"
                      onClick={() => void handleTestProviderConnectivity()}
                      disabled={isSaving || isTestingProviders}
                    >
                      {isTestingProviders ? (
                        <>
                          <LoaderIcon className="mr-1 size-3 animate-spin" />
                          Testing
                        </>
                      ) : (
                        "Test All"
                      )}
                    </Button>
                  </div>

                  {providerConnectivity.length === 0 ? (
                    <p className="text-muted-foreground text-xs">
                      尚未运行连通性测试。
                    </p>
                  ) : (
                    <div className="space-y-2">
                      {providerConnectivity.map((result) => {
                        const badge = connectivityBadge(result);
                        return (
                          <div
                            key={`${result.provider}-${result.capability}`}
                            className="rounded-md border border-sidebar-border/60 px-2 py-1.5 text-xs"
                          >
                            <div className="flex items-center justify-between gap-2">
                              <span className="font-medium">{result.label}</span>
                              <span className={badge.className}>
                                {badge.label}
                              </span>
                            </div>
                            <p className="mt-0.5 text-muted-foreground">
                              {result.message}
                            </p>
                            <p className="mt-0.5 break-all text-muted-foreground/80">
                              {result.endpoint}
                            </p>
                            <p className="mt-0.5 text-muted-foreground/80">
                              status: {result.status ?? "-"} · {result.latencyMs}
                              ms
                            </p>
                          </div>
                        );
                      })}
                    </div>
                  )}
                </div>
              </div>
            </TabsContent>

            <TabsContent
              value="advanced"
              className="mt-4 min-h-0 flex-1 overflow-y-auto pr-1"
            >
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
            </TabsContent>

            <TabsContent value="io" className="mt-4 min-h-0 flex-1 overflow-y-auto pr-1">
              <div className="space-y-4">
                <div className="flex flex-wrap gap-2">
                  <Button
                    variant="outline"
                    size="sm"
                    onClick={() => void handleExport(false)}
                    disabled={isSaving}
                  >
                    导出 Global（复制到剪贴板）
                  </Button>
                  <Button
                    variant="outline"
                    size="sm"
                    onClick={() => void handleExport(true)}
                    disabled={isSaving}
                  >
                    导出 Global+Project
                  </Button>
                </div>

                <div className="space-y-2">
                  <Label>Import JSON</Label>
                  <Textarea
                    className="min-h-40"
                    placeholder='粘贴 JSON，例如 {"global": {...}, "project": {...}}'
                    value={importText}
                    onChange={(e) => setImportText(e.target.value)}
                  />
                </div>

                <div className="flex gap-2">
                  <Button
                    variant="outline"
                    size="sm"
                    onClick={() => void handleImport("merge")}
                    disabled={isSaving || !importText.trim()}
                  >
                    Merge Import
                  </Button>
                  <Button
                    variant="outline"
                    size="sm"
                    onClick={() => void handleImport("replace")}
                    disabled={isSaving || !importText.trim()}
                  >
                    Replace Import
                  </Button>
                </div>

                <div className="space-y-2 rounded-xl border border-destructive/40 bg-destructive/5 p-4">
                  <p className="font-medium text-sm">Danger Zone</p>
                  <p className="text-muted-foreground text-xs">
                    重置 Global Settings 会影响所有项目配置。
                  </p>
                  <Button
                    variant="destructive"
                    size="sm"
                    onClick={() => setConfirmResetGlobalOpen(true)}
                    disabled={isSaving}
                  >
                    重置 Global Settings
                  </Button>
                </div>
              </div>
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
