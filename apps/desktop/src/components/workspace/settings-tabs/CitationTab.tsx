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
import { panelClass, type EffectiveSettings, type QueryMode } from "./types";

interface CitationTabProps {
  effective: EffectiveSettings;
  setCitationStylePolicy: (policy: "auto" | "cite" | "citep" | "autocite", projectRoot: string | null) => Promise<unknown>;
  autoThreshold: string;
  setAutoThreshold: (v: string) => void;
  reviewThreshold: string;
  setReviewThreshold: (v: string) => void;
  searchLimit: string;
  setSearchLimit: (v: string) => void;
  autoThresholdUsesProject: boolean;
  reviewThresholdUsesProject: boolean;
  searchLimitUsesProject: boolean;
  hasProjectRoot: boolean;
  saveCitationAutoThreshold: () => Promise<void>;
  saveCitationReviewThreshold: () => Promise<void>;
  saveCitationSearchLimit: () => Promise<void>;
  queryMode: QueryMode;
  saveQueryMode: (mode: QueryMode) => Promise<void>;
  llmEnabled: boolean;
  saveLlmEnabled: (enabled: boolean) => Promise<void>;
  llmApiKeyConfigured: boolean;
  toggleProjectOverride: (field: "auto" | "review" | "limit", enabled: boolean) => Promise<void>;
  queryExecutionTopN: string;
  setQueryExecutionTopN: (v: string) => void;
  queryExecutionMmrLambda: string;
  setQueryExecutionMmrLambda: (v: string) => void;
  queryExecutionMinQuality: string;
  setQueryExecutionMinQuality: (v: string) => void;
  queryExecutionMinHitRatio: string;
  setQueryExecutionMinHitRatio: (v: string) => void;
  queryExecutionHitScoreThreshold: string;
  setQueryExecutionHitScoreThreshold: (v: string) => void;
  saveQueryExecutionField: (field: "topN" | "mmrLambda" | "minQuality" | "minHitRatio" | "hitScoreThreshold", rawValue: string) => Promise<void>;
}

export function CitationTab({
  effective,
  setCitationStylePolicy,
  autoThreshold,
  setAutoThreshold,
  reviewThreshold,
  setReviewThreshold,
  searchLimit,
  setSearchLimit,
  autoThresholdUsesProject,
  reviewThresholdUsesProject,
  searchLimitUsesProject,
  hasProjectRoot,
  saveCitationAutoThreshold,
  saveCitationReviewThreshold,
  saveCitationSearchLimit,
  queryMode,
  saveQueryMode,
  llmEnabled,
  saveLlmEnabled,
  llmApiKeyConfigured,
  toggleProjectOverride,
  queryExecutionTopN,
  setQueryExecutionTopN,
  queryExecutionMmrLambda,
  setQueryExecutionMmrLambda,
  queryExecutionMinQuality,
  setQueryExecutionMinQuality,
  queryExecutionMinHitRatio,
  setQueryExecutionMinHitRatio,
  queryExecutionHitScoreThreshold,
  setQueryExecutionHitScoreThreshold,
  saveQueryExecutionField,
}: CitationTabProps) {
  return (
    <div className="space-y-4">
      <div className="space-y-2">
        <Label>Citation Style Policy</Label>
        <Select
          value={effective.citation.stylePolicy}
          onValueChange={(value: string) =>
            void setCitationStylePolicy(
              value as "auto" | "cite" | "citep" | "autocite",
              null,
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
  );
}
