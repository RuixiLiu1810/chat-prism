import { LoaderIcon } from "lucide-react";
import { Button } from "@/components/ui/button";
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
import type { AgentSmokeResult, ProviderConnectivityResult } from "@/lib/settings-api";
import {
  panelClass,
  type AgentProvider,
  type AgentRuntimeKind,
  type EffectiveSettings,
  type SecretsMeta,
} from "./types";

interface ProvidersTabProps {
  effective: EffectiveSettings;
  secretsMeta: SecretsMeta;
  isSaving: boolean;
  projectRoot: string | null;
  hasProjectRoot: boolean;
  patchGlobal: (patch: any, projectRoot: string | null) => Promise<unknown>;
  patchSecret: (patch: any, projectRoot: string | null) => Promise<boolean>;
  // Agent runtime
  agentRuntime: AgentRuntimeKind;
  agentProvider: AgentProvider;
  agentModel: string;
  setAgentModel: (v: string) => void;
  agentBaseUrl: string;
  setAgentBaseUrl: (v: string) => void;
  agentRuntimeMode: string;
  saveAgentRuntime: (runtime: AgentRuntimeKind) => Promise<void>;
  saveAgentProvider: (provider: AgentProvider) => Promise<void>;
  saveAgentModel: () => Promise<void>;
  saveAgentBaseUrl: () => Promise<void>;
  // Agent API key
  agentApiKeyInput: string;
  setAgentApiKeyInput: (v: string) => void;
  showAgentApiKey: boolean;
  setShowAgentApiKey: (v: boolean | ((prev: boolean) => boolean)) => void;
  saveAgentApiKey: () => Promise<void>;
  // Sampling profiles
  agentEditStableTemperature: string;
  setAgentEditStableTemperature: (v: string) => void;
  agentEditStableTopP: string;
  setAgentEditStableTopP: (v: string) => void;
  agentEditStableMaxTokens: string;
  setAgentEditStableMaxTokens: (v: string) => void;
  agentAnalysisTemperature: string;
  setAgentAnalysisTemperature: (v: string) => void;
  agentAnalysisTopP: string;
  setAgentAnalysisTopP: (v: string) => void;
  agentAnalysisMaxTokens: string;
  setAgentAnalysisMaxTokens: (v: string) => void;
  agentAnalysisDeepTemperature: string;
  setAgentAnalysisDeepTemperature: (v: string) => void;
  agentAnalysisDeepTopP: string;
  setAgentAnalysisDeepTopP: (v: string) => void;
  agentAnalysisDeepMaxTokens: string;
  setAgentAnalysisDeepMaxTokens: (v: string) => void;
  agentChatTemperature: string;
  setAgentChatTemperature: (v: string) => void;
  agentChatTopP: string;
  setAgentChatTopP: (v: string) => void;
  agentChatMaxTokens: string;
  setAgentChatMaxTokens: (v: string) => void;
  saveAgentSamplingProfiles: () => Promise<void>;
  // Smoke test
  isRunningAgentSmoke: boolean;
  agentSmokeResult: AgentSmokeResult | null;
  handleRunAgentSmoke: () => Promise<void>;
  // Semantic Scholar
  apiKeyInput: string;
  setApiKeyInput: (v: string) => void;
  showSemanticApiKey: boolean;
  setShowSemanticApiKey: (v: boolean | ((prev: boolean) => boolean)) => void;
  saveApiKey: () => Promise<void>;
  // LLM Query
  llmModel: string;
  setLlmModel: (v: string) => void;
  llmEndpoint: string;
  setLlmEndpoint: (v: string) => void;
  llmTimeoutMs: string;
  setLlmTimeoutMs: (v: string) => void;
  llmMaxQueries: string;
  setLlmMaxQueries: (v: string) => void;
  llmApiKeyInput: string;
  setLlmApiKeyInput: (v: string) => void;
  showLlmApiKey: boolean;
  setShowLlmApiKey: (v: boolean | ((prev: boolean) => boolean)) => void;
  saveLlmModel: () => Promise<void>;
  saveLlmEndpoint: () => Promise<void>;
  saveLlmTimeoutMs: () => Promise<void>;
  saveLlmMaxQueries: () => Promise<void>;
  saveLlmApiKey: () => Promise<void>;
  // Query Embedding
  queryEmbeddingProvider: "none" | "local_embedding";
  queryEmbeddingTimeoutMs: string;
  setQueryEmbeddingTimeoutMs: (v: string) => void;
  saveQueryEmbeddingEnabled: (enabled: boolean) => Promise<void>;
  saveQueryEmbeddingProvider: (provider: "none" | "local_embedding") => Promise<void>;
  saveQueryEmbeddingTimeoutMs: () => Promise<void>;
  // Connectivity
  isTestingProviders: boolean;
  providerConnectivity: ProviderConnectivityResult[];
  handleTestProviderConnectivity: () => Promise<void>;
  connectivityBadge: (result: ProviderConnectivityResult) => { label: string; className: string };
}

export function ProvidersTab({
  effective,
  secretsMeta,
  isSaving,
  projectRoot,
  hasProjectRoot,
  patchGlobal,
  agentRuntime,
  agentProvider,
  agentModel,
  setAgentModel,
  agentBaseUrl,
  setAgentBaseUrl,
  agentRuntimeMode,
  saveAgentRuntime,
  saveAgentProvider,
  saveAgentModel,
  saveAgentBaseUrl,
  agentApiKeyInput,
  setAgentApiKeyInput,
  showAgentApiKey,
  setShowAgentApiKey,
  saveAgentApiKey,
  agentEditStableTemperature,
  setAgentEditStableTemperature,
  agentEditStableTopP,
  setAgentEditStableTopP,
  agentEditStableMaxTokens,
  setAgentEditStableMaxTokens,
  agentAnalysisTemperature,
  setAgentAnalysisTemperature,
  agentAnalysisTopP,
  setAgentAnalysisTopP,
  agentAnalysisMaxTokens,
  setAgentAnalysisMaxTokens,
  agentAnalysisDeepTemperature,
  setAgentAnalysisDeepTemperature,
  agentAnalysisDeepTopP,
  setAgentAnalysisDeepTopP,
  agentAnalysisDeepMaxTokens,
  setAgentAnalysisDeepMaxTokens,
  agentChatTemperature,
  setAgentChatTemperature,
  agentChatTopP,
  setAgentChatTopP,
  agentChatMaxTokens,
  setAgentChatMaxTokens,
  saveAgentSamplingProfiles,
  isRunningAgentSmoke,
  agentSmokeResult,
  handleRunAgentSmoke,
  apiKeyInput,
  setApiKeyInput,
  showSemanticApiKey,
  setShowSemanticApiKey,
  saveApiKey,
  llmModel,
  setLlmModel,
  llmEndpoint,
  setLlmEndpoint,
  llmTimeoutMs,
  setLlmTimeoutMs,
  llmMaxQueries,
  setLlmMaxQueries,
  llmApiKeyInput,
  setLlmApiKeyInput,
  showLlmApiKey,
  setShowLlmApiKey,
  saveLlmModel,
  saveLlmEndpoint,
  saveLlmTimeoutMs,
  saveLlmMaxQueries,
  saveLlmApiKey,
  queryEmbeddingProvider,
  queryEmbeddingTimeoutMs,
  setQueryEmbeddingTimeoutMs,
  saveQueryEmbeddingEnabled,
  saveQueryEmbeddingProvider,
  saveQueryEmbeddingTimeoutMs,
  isTestingProviders,
  providerConnectivity,
  handleTestProviderConnectivity,
  connectivityBadge,
}: ProvidersTabProps) {
  const agentApiKeyConfigured = secretsMeta.integrations.agent.apiKeyConfigured;
  const llmApiKeyConfigured = secretsMeta.integrations.llmQuery.apiKeyConfigured;

  return (
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
  );
}
