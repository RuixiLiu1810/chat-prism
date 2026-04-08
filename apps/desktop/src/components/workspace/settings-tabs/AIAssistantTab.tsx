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
import type { AgentSmokeResult } from "@/lib/settings-api";
import {
  panelClass,
  type AgentProvider,
  type AgentRuntimeKind,
} from "./types";

interface AIAssistantTabProps {
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
  // API key
  agentApiKeyInput: string;
  setAgentApiKeyInput: (v: string) => void;
  showAgentApiKey: boolean;
  setShowAgentApiKey: (v: boolean | ((prev: boolean) => boolean)) => void;
  saveAgentApiKey: () => Promise<void>;
  agentApiKeyConfigured: boolean;
  isSaving: boolean;
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
  hasProjectRoot: boolean;
  isRunningAgentSmoke: boolean;
  agentSmokeResult: AgentSmokeResult | null;
  handleRunAgentSmoke: () => Promise<void>;
}

export function AIAssistantTab({
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
  agentApiKeyConfigured,
  isSaving,
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
  hasProjectRoot,
  isRunningAgentSmoke,
  agentSmokeResult,
  handleRunAgentSmoke,
}: AIAssistantTabProps) {
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

        {/* API Key */}
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

        {/* Sampling Profiles */}
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
      </div>

      {/* Smoke Test */}
      <div className="space-y-2 rounded-xl border border-sidebar-border/60 bg-sidebar/40 p-4">
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
  );
}
