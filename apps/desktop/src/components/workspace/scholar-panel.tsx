import { useEffect, useMemo, useState } from "react";
import { LoaderIcon } from "lucide-react";
import { toast } from "sonner";
import { useDocumentStore } from "@/stores/document-store";
import { useCitationStore } from "@/stores/citation-store";
import { useSettingsStore } from "@/stores/settings-store";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import type { CitationSearchDebug } from "@/lib/citation-api";

function normalizeDoi(doi: string) {
  return doi
    .trim()
    .toLowerCase()
    .replace(/^https?:\/\/doi\.org\//, "")
    .replace(/^doi:/, "");
}

function uniqueKeepOrder(items: string[]) {
  const seen = new Set<string>();
  const out: string[] = [];
  for (const item of items) {
    const trimmed = item.trim();
    if (!trimmed || seen.has(trimmed)) continue;
    seen.add(trimmed);
    out.push(trimmed);
  }
  return out;
}

function buildCandidateKey(
  candidate: CitationSearchDebug["merged_results"][number],
  index: number,
) {
  const paperId =
    typeof candidate.paper_id === "string" ? candidate.paper_id.trim() : "";
  if (paperId) return paperId;
  const doi = typeof candidate.doi === "string" ? normalizeDoi(candidate.doi) : "";
  if (doi) return `doi:${doi}`;
  const title = typeof candidate.title === "string" ? candidate.title.trim() : "";
  return `idx:${index}:${title}`;
}

function buildEvalSample(
  debug: CitationSearchDebug,
  expected: { dois: string[]; titles: string[]; no_match?: boolean },
) {
  return {
    id: `sample_${new Date().toISOString().replace(/[:.]/g, "-")}`,
    selected_text: debug.selected_text,
    latency_ms: debug.latency_ms,
    expected,
    merged_results: debug.merged_results.map((c) => ({
      paper_id: c.paper_id,
      title: c.title,
      year: c.year ?? null,
      venue: c.venue ?? null,
      doi: c.doi ?? null,
      url: c.url ?? null,
      score: c.score,
      score_explain: c.score_explain ?? null,
      evidence_sentences: c.evidence_sentences ?? [],
    })),
    debug: {
      preprocessed_text: debug.preprocessed_text,
      query_plan: debug.query_plan,
      provider_budgets: debug.provider_budgets,
      query_execution: debug.query_execution,
      stop_reason: debug.stop_reason,
      stop_stage: debug.stop_stage,
      stop_hit_ratio: debug.stop_hit_ratio,
      stop_quality_hits: debug.stop_quality_hits,
      stop_attempted_queries: debug.stop_attempted_queries,
      stop_merged_count: debug.stop_merged_count,
    },
  };
}

function buildQueryExecutionKey(source: string, strategy: string, query: string) {
  return `${source}::${strategy}::${query}`;
}

function formatStopReason(reason?: string) {
  if (!reason) return "n/a";
  if (reason === "enough_results_hit_ratio") return "early stop: enough results + hit ratio reached";
  if (reason === "execution_plan_exhausted") return "finished: execution plan exhausted";
  if (reason === "empty_selection") return "stopped: empty selection";
  if (reason === "no_executable_query") return "stopped: no executable query";
  return reason;
}

function formatStopStage(stage?: string) {
  if (!stage) return "n/a";
  if (stage === "after_semantic_scholar") return "after Semantic Scholar";
  if (stage === "after_openalex") return "after OpenAlex";
  if (stage === "after_crossref") return "after Crossref";
  return stage;
}

export function ScholarPanel() {
  const [debugOpen, setDebugOpen] = useState(false);
  const [selectedLabelKeys, setSelectedLabelKeys] = useState<string[]>([]);
  const [labelNoMatch, setLabelNoMatch] = useState(false);
  const activeFileId = useDocumentStore((s) => s.activeFileId);
  const files = useDocumentStore((s) => s.files);
  const isSearching = useCitationStore((s) => s.isSearching);
  const isDebugSearching = useCitationStore((s) => s.isDebugSearching);
  const isApplying = useCitationStore((s) => s.isApplying);
  const results = useCitationStore((s) => s.results);
  const autoCandidates = useCitationStore((s) => s.autoCandidates);
  const reviewCandidates = useCitationStore((s) => s.reviewCandidates);
  const lastAutoAppliedTitle = useCitationStore((s) => s.lastAutoAppliedTitle);
  const error = useCitationStore((s) => s.error);
  const lastInsertedCitekey = useCitationStore((s) => s.lastInsertedCitekey);
  const decisionHint = useCitationStore((s) => s.decisionHint);
  const debugInfo = useCitationStore((s) => s.debugInfo);
  const citationStylePolicy = useSettingsStore(
    (s) => s.effective.citation.stylePolicy,
  );
  const searchFromSelection = useCitationStore((s) => s.searchFromSelection);
  const runDebugFromSelection = useCitationStore((s) => s.runDebugFromSelection);
  const clearDebugInfo = useCitationStore((s) => s.clearDebugInfo);
  const applyCandidate = useCitationStore((s) => s.applyCandidate);
  const activeFile = files.find((f) => f.id === activeFileId);
  const canSearch = activeFile?.type === "tex";
  const shownCandidates =
    reviewCandidates.length > 0 ? reviewCandidates : results.slice(0, 4);

  useEffect(() => {
    if (!debugOpen || !debugInfo) {
      setSelectedLabelKeys([]);
      setLabelNoMatch(false);
    }
  }, [debugInfo, debugOpen]);

  const labeledExpected = useMemo(() => {
    if (!debugInfo) {
      return { dois: [], titles: [], no_match: false };
    }
    const selected = debugInfo.merged_results.filter((candidate, index) =>
      selectedLabelKeys.includes(buildCandidateKey(candidate, index)),
    );
    const dois = uniqueKeepOrder(
      selected
        .map((candidate) =>
          typeof candidate.doi === "string" ? normalizeDoi(candidate.doi) : "",
        )
        .filter((doi) => doi.length > 0),
    );
    const titles = uniqueKeepOrder(
      selected
        .map((candidate) =>
          typeof candidate.title === "string" ? candidate.title.trim() : "",
        )
        .filter((title) => title.length > 0),
    );
    return { dois, titles, no_match: labelNoMatch };
  }, [debugInfo, labelNoMatch, selectedLabelKeys]);

  const executedQueryKeys = useMemo(() => {
    const set = new Set<string>();
    if (!debugInfo) return set;
    for (const step of debugInfo.query_execution) {
      set.add(buildQueryExecutionKey(step.source, step.strategy, step.query));
    }
    return set;
  }, [debugInfo]);

  const toggleCandidateForLabel = (
    candidate: CitationSearchDebug["merged_results"][number],
    index: number,
  ) => {
    const key = buildCandidateKey(candidate, index);
    setLabelNoMatch(false);
    setSelectedLabelKeys((prev) =>
      prev.includes(key) ? prev.filter((item) => item !== key) : [...prev, key],
    );
  };

  const selectTop1ForLabel = () => {
    if (!debugInfo || debugInfo.merged_results.length === 0) return;
    const key = buildCandidateKey(debugInfo.merged_results[0], 0);
    setLabelNoMatch(false);
    setSelectedLabelKeys([key]);
  };

  const clearLabelSelection = () => {
    setSelectedLabelKeys([]);
    setLabelNoMatch(false);
  };

  const markNoMatchForLabel = () => {
    setSelectedLabelKeys([]);
    setLabelNoMatch(true);
  };

  const copyEvalSample = async () => {
    if (!debugInfo) return;
    const sample = buildEvalSample(debugInfo, {
      dois: [],
      titles: [],
    });
    const text = JSON.stringify(sample, null, 2);
    try {
      await navigator.clipboard.writeText(text);
      toast.success("Eval sample copied to clipboard.");
    } catch {
      toast.error("Failed to copy eval sample.");
    }
  };

  const copyLabeledEvalSample = async () => {
    if (!debugInfo) return;
    if (
      labeledExpected.dois.length === 0 &&
      labeledExpected.titles.length === 0 &&
      !labeledExpected.no_match
    ) {
      toast.error("Select at least one candidate before copying labeled sample.");
      return;
    }
    const sample = buildEvalSample(debugInfo, labeledExpected);
    const text = JSON.stringify(sample, null, 2);
    try {
      await navigator.clipboard.writeText(text);
      if (labeledExpected.no_match) {
        toast.success("Labeled sample copied (no_match).");
      } else {
        toast.success(
          `Labeled sample copied (${labeledExpected.dois.length} DOI, ${labeledExpected.titles.length} title).`,
        );
      }
    } catch {
      toast.error("Failed to copy labeled sample.");
    }
  };

  return (
    <div className="flex h-full flex-col">
      <div className="min-h-0 flex-1 overflow-y-auto py-0.5">
        <div className="mx-2 mb-1 rounded border border-sidebar-border/70 bg-sidebar-accent/20 p-2">
          <div className="mb-1 flex items-center justify-between">
            <div>
              <span className="font-medium text-[11px]">Selection Citation</span>
              <p className="text-[10px] text-muted-foreground">
                Style: <code>{citationStylePolicy}</code> (change in Settings)
              </p>
            </div>
            <div className="flex items-center gap-1">
              <Button
                variant="outline"
                size="sm"
                className="h-6 px-2 text-[10px]"
                disabled={!canSearch || isSearching || isApplying}
                onClick={searchFromSelection}
              >
                {isSearching ? (
                  <>
                    <LoaderIcon className="mr-1 size-3 animate-spin" />
                    Searching
                  </>
                ) : (
                  "Search"
                )}
              </Button>
              <Button
                variant="outline"
                size="sm"
                className="h-6 px-2 text-[10px]"
                disabled={!canSearch || isSearching || isApplying || isDebugSearching}
                onClick={() => {
                  setDebugOpen(true);
                  void runDebugFromSelection();
                }}
              >
                {isDebugSearching ? (
                  <>
                    <LoaderIcon className="mr-1 size-3 animate-spin" />
                    Debugging
                  </>
                ) : (
                  "Debug"
                )}
              </Button>
            </div>
          </div>
          {!canSearch && (
            <p className="text-[10px] text-muted-foreground">
              Open a <code>.tex</code> file first, then select sentence text.
            </p>
          )}
          {error && <p className="mt-1 text-[10px] text-destructive">{error}</p>}
          {lastAutoAppliedTitle && (
            <p className="mt-1 text-[10px] text-muted-foreground">
              Auto-cited (high confidence): {lastAutoAppliedTitle}
            </p>
          )}
          {lastInsertedCitekey && (
            <p className="mt-1 text-[10px] text-muted-foreground">
              Inserted: <code>{lastInsertedCitekey}</code>
            </p>
          )}
          {results.length > 0 && (
            <p className="mt-1 text-[10px] text-muted-foreground">
              {autoCandidates.length} auto · {reviewCandidates.length} review
            </p>
          )}
          {decisionHint && (
            <p className="mt-1 text-[10px] text-muted-foreground">{decisionHint}</p>
          )}
          {shownCandidates.length > 0 && (
            <div className="mt-2 space-y-1">
              {shownCandidates.map((result) => (
                <div
                  key={result.paper_id || `${result.title}-${result.year ?? ""}`}
                  className="rounded border border-sidebar-border/60 bg-sidebar px-1.5 py-1"
                >
                  <div className="line-clamp-2 text-[10px] leading-tight">
                    {result.title}
                  </div>
                  <div className="mt-0.5 flex items-center justify-between text-[10px] text-muted-foreground">
                    <span>
                      {result.year ?? "n.d."} · {Math.round(result.score * 100)}%
                    </span>
                    <button
                      className="rounded px-1.5 py-0.5 text-[10px] text-foreground underline"
                      onClick={() => applyCandidate(result)}
                      disabled={isApplying}
                    >
                      Cite
                    </button>
                  </div>
                </div>
              ))}
            </div>
          )}
        </div>
      </div>

      <Dialog
        open={debugOpen}
        onOpenChange={(open) => {
          setDebugOpen(open);
          if (!open) clearDebugInfo();
        }}
      >
        <DialogContent className="max-h-[80vh] overflow-y-auto sm:max-w-3xl">
          <DialogHeader>
            <div className="flex items-center justify-between gap-2">
              <DialogTitle>Citation Search Debug</DialogTitle>
              <div className="flex items-center gap-1">
                <Button
                  variant="outline"
                  size="sm"
                  className="h-7 text-xs"
                  onClick={() => void copyEvalSample()}
                  disabled={!debugInfo || isDebugSearching}
                >
                  Copy Raw
                </Button>
                <Button
                  variant="outline"
                  size="sm"
                  className="h-7 text-xs"
                  onClick={() => void copyLabeledEvalSample()}
                  disabled={!debugInfo || isDebugSearching}
                >
                  Copy Labeled
                </Button>
              </div>
            </div>
          </DialogHeader>
          {isDebugSearching && (
            <div className="flex items-center gap-2 text-sm text-muted-foreground">
              <LoaderIcon className="size-4 animate-spin" />
              Running debug search...
            </div>
          )}
          {!isDebugSearching && !debugInfo && (
            <p className="text-sm text-muted-foreground">No debug data yet.</p>
          )}
          {!isDebugSearching && debugInfo && (
            <div className="space-y-3 text-xs">
              <div>
                <div className="font-medium">Query Plan</div>
                <div className="mt-1 rounded border border-sidebar-border/60 bg-sidebar px-2 py-1">
                  <div className="mb-1 text-muted-foreground">
                    LLM rewrite:{" "}
                    {debugInfo.llm_query_enabled
                      ? debugInfo.llm_query_attempted
                        ? "enabled"
                        : "enabled (not attempted)"
                      : "disabled"}
                    {` · embedding: ${debugInfo.query_embedding_provider}`}
                    {` (${debugInfo.query_embedding_timeout_ms}ms)`}
                    {debugInfo.query_embedding_fallback_count > 0
                      ? ` · fallback=${debugInfo.query_embedding_fallback_count}`
                      : ""}
                    {` · latency: ${debugInfo.latency_ms}ms`}
                    {debugInfo.llm_query_error
                      ? ` · error: ${debugInfo.llm_query_error}`
                      : ""}
                    {debugInfo.query_embedding_error
                      ? ` · embed_error: ${debugInfo.query_embedding_error}`
                      : ""}
                    {` · exec(topN=${debugInfo.query_execution_top_n}, selected=${debugInfo.query_execution_selected_count}, λ=${debugInfo.query_execution_mmr_lambda.toFixed(2)}, minQ=${debugInfo.query_execution_min_quality.toFixed(2)}, hitRatio=${debugInfo.query_execution_min_hit_ratio.toFixed(2)}, hitScore=${debugInfo.query_execution_hit_score_threshold.toFixed(2)})`}
                  </div>
                  <div className="mb-1 text-muted-foreground">
                    {`Stop: ${formatStopReason(debugInfo.stop_reason)} · stage: ${formatStopStage(debugInfo.stop_stage)}`}
                    {` · hitRatio=${(debugInfo.stop_hit_ratio ?? 0).toFixed(2)} (${debugInfo.stop_quality_hits}/${debugInfo.stop_attempted_queries})`}
                    {` · merged=${debugInfo.stop_merged_count}`}
                  </div>
                  {debugInfo.query_plan.length > 0 ? (
                    debugInfo.query_plan.map((item, idx) => (
                      <div key={`${idx}-${item.query}`} className="mb-1 last:mb-0">
                        <div>
                          {idx + 1}. [{item.source}/{item.strategy}] w=
                          {item.weight.toFixed(2)} · q=
                          {item.quality.total.toFixed(3)} ·{" "}
                          {executedQueryKeys.has(
                            buildQueryExecutionKey(
                              item.source,
                              item.strategy,
                              item.query,
                            ),
                          )
                            ? "executed"
                            : "not executed"}
                          {" · "}
                          {item.query}
                        </div>
                        <div className="text-muted-foreground">
                          sem={item.quality.semantic_sim.toFixed(3)} · anchor=
                          {item.quality.anchor_coverage.toFixed(3)} · spec=
                          {item.quality.specificity.toFixed(3)} · noise=
                          {item.quality.noise_penalty.toFixed(3)} · len_pen=
                          {item.quality.length_penalty.toFixed(3)}
                        </div>
                      </div>
                    ))
                  ) : (
                    <div className="text-muted-foreground">No query plan generated.</div>
                  )}
                </div>
              </div>

              <div>
                <div className="font-medium">Queries</div>
                <div className="mt-1 rounded border border-sidebar-border/60 bg-sidebar px-2 py-1">
                  {debugInfo.queries.length > 0 ? (
                    debugInfo.queries.map((q, idx) => (
                      <div key={`${idx}-${q}`}>
                        {idx + 1}. {q}
                      </div>
                    ))
                  ) : (
                    <div className="text-muted-foreground">No query generated.</div>
                  )}
                </div>
              </div>

              <div>
                <div className="font-medium">Provider Budgets</div>
                <div className="mt-1 rounded border border-sidebar-border/60 bg-sidebar px-2 py-1">
                  {debugInfo.provider_budgets.length > 0 ? (
                    debugInfo.provider_budgets.map((b, idx) => (
                      <div key={`${idx}-${b.provider}`}>
                        [{b.provider}] initial={b.initial} used={b.used} · skip(budget)=
                        {b.skipped_due_to_budget} · skip(rate_limit)=
                        {b.skipped_due_to_rate_limit}
                      </div>
                    ))
                  ) : (
                    <div className="text-muted-foreground">No provider budget info.</div>
                  )}
                </div>
              </div>

              <div>
                <div className="font-medium">Execution Order</div>
                <div className="mt-1 rounded border border-sidebar-border/60 bg-sidebar px-2 py-1">
                  {debugInfo.query_execution.length > 0 ? (
                    debugInfo.query_execution.map((step, idx) => (
                      <div key={`${idx}-${step.query}`} className="mb-1 last:mb-0">
                        <div>
                          {idx + 1}. [{step.source}/{step.strategy}] w=
                          {step.weight.toFixed(2)} · q=
                          {step.quality_score.toFixed(3)} · {step.query}
                        </div>
                        <div className="text-muted-foreground">
                          S2: {step.s2_status} · OA: {step.openalex_status} · CR:{" "}
                          {step.crossref_status}
                        </div>
                      </div>
                    ))
                  ) : (
                    <div className="text-muted-foreground">No execution trace.</div>
                  )}
                </div>
              </div>

              <div>
                <div className="font-medium">Preprocessed Selection</div>
                <div className="mt-1 rounded border border-sidebar-border/60 bg-sidebar px-2 py-1 whitespace-pre-wrap break-words">
                  {debugInfo.preprocessed_text || "(empty)"}
                </div>
              </div>

              <div>
                <div className="font-medium">Provider Attempts</div>
                <div className="mt-1 space-y-2">
                  {debugInfo.attempts.map((a, idx) => (
                    <details
                      key={`${a.provider}-${a.query}-${idx}`}
                      className="rounded border border-sidebar-border/60 bg-sidebar px-2 py-1"
                    >
                      <summary className="cursor-pointer">
                        [{a.provider}] {a.query} ·{" "}
                        {a.ok
                          ? `${a.result_count} results`
                          : `failed${a.error ? `: ${a.error.slice(0, 96)}` : ""}`}
                      </summary>
                      {a.error && (
                        <div className="mt-1 text-destructive">Error: {a.error}</div>
                      )}
                      {a.candidates.length > 0 ? (
                        <div className="mt-1 space-y-1">
                          {a.candidates.map((c, j) => (
                            <div
                              key={`${idx}-${j}-${c.paper_id || c.title}`}
                              className="rounded border border-sidebar-border/50 px-2 py-1"
                            >
                              <div className="font-medium">{c.title}</div>
                              <div className="text-muted-foreground">
                                {c.year ?? "n.d."} · score {Math.round(c.score * 100)}%
                              </div>
                              {c.score_explain && (
                                <div className="text-muted-foreground">
                                  explain: title=
                                  {Math.round(c.score_explain.sem_title * 100)}% · abs=
                                  {Math.round(c.score_explain.sem_abstract * 100)}% ·
                                  phrase={Math.round(c.score_explain.phrase * 100)}% ·
                                  recency={Math.round(c.score_explain.recency * 100)}% ·
                                  strength={Math.round(c.score_explain.strength * 100)}%
                                  · contra_penalty=
                                  {Math.round(
                                    c.score_explain.contradiction_penalty * 100,
                                  )}
                                  %
                                  {typeof c.score_explain.formula_penalty === "number" && (
                                    <>
                                      {" "}
                                      · formula_penalty=
                                      {Math.round(c.score_explain.formula_penalty * 100)}%
                                    </>
                                  )}
                                </div>
                              )}
                              {c.doi && <div>DOI: {c.doi}</div>}
                              {c.url && <div className="break-all">URL: {c.url}</div>}
                              {c.evidence_sentences &&
                                c.evidence_sentences.length > 0 && (
                                  <div className="mt-1">
                                    <div className="text-muted-foreground">Evidence:</div>
                                    {c.evidence_sentences.map((sentence, k) => (
                                      <div
                                        key={`${idx}-${j}-ev-${k}`}
                                        className="line-clamp-2 text-muted-foreground"
                                      >
                                        - {sentence}
                                      </div>
                                    ))}
                                  </div>
                                )}
                              {c.abstract_text && (
                                <div className="line-clamp-3 text-muted-foreground">
                                  {c.abstract_text}
                                </div>
                              )}
                            </div>
                          ))}
                        </div>
                      ) : (
                        <div className="mt-1 text-muted-foreground">No results.</div>
                      )}
                    </details>
                  ))}
                </div>
              </div>

              <div>
                <div className="font-medium">Merged Final Results</div>
                <div className="mt-1 rounded border border-sidebar-border/60 bg-sidebar px-2 py-1">
                  {debugInfo.merged_results.length > 0 ? (
                    <div className="space-y-1">
                      {debugInfo.merged_results.map((c, idx) => {
                        const key = buildCandidateKey(c, idx);
                        const selected = selectedLabelKeys.includes(key);
                        return (
                          <div
                            key={`${idx}-${key}`}
                            className="rounded border border-sidebar-border/50 px-2 py-1"
                          >
                            <div className="flex items-start justify-between gap-2">
                              <div>
                                <div>
                                  {idx + 1}. {c.title} ({c.year ?? "n.d."}) ·{" "}
                                  {Math.round(c.score * 100)}%
                                </div>
                                {c.doi && (
                                  <div className="break-all text-muted-foreground">
                                    DOI: {c.doi}
                                  </div>
                                )}
                              </div>
                              <button
                                type="button"
                                className="rounded border border-sidebar-border px-2 py-0.5 text-[11px] text-muted-foreground transition-colors hover:bg-sidebar-accent/40 hover:text-foreground"
                                onClick={() => toggleCandidateForLabel(c, idx)}
                              >
                                {selected ? "Selected" : "Select"}
                              </button>
                            </div>
                          </div>
                        );
                      })}
                    </div>
                  ) : (
                    <div className="text-muted-foreground">No merged result.</div>
                  )}
                </div>
              </div>

              <div>
                <div className="font-medium">Labeling</div>
                <div className="mt-1 rounded border border-sidebar-border/60 bg-sidebar px-2 py-1">
                  <div className="mb-2 flex items-center gap-1">
                    <Button
                      variant="outline"
                      size="sm"
                      className="h-6 px-2 text-[11px]"
                      onClick={selectTop1ForLabel}
                      disabled={debugInfo.merged_results.length === 0}
                    >
                      Select Top1
                    </Button>
                    <Button
                      variant="outline"
                      size="sm"
                      className="h-6 px-2 text-[11px]"
                      onClick={markNoMatchForLabel}
                      disabled={labelNoMatch}
                    >
                      Mark No Match
                    </Button>
                    <Button
                      variant="outline"
                      size="sm"
                      className="h-6 px-2 text-[11px]"
                      onClick={clearLabelSelection}
                      disabled={selectedLabelKeys.length === 0 && !labelNoMatch}
                    >
                      Clear
                    </Button>
                    <Button
                      variant="outline"
                      size="sm"
                      className="h-6 px-2 text-[11px]"
                      onClick={() => void copyLabeledEvalSample()}
                      disabled={
                        labeledExpected.dois.length === 0 &&
                        labeledExpected.titles.length === 0 &&
                        !labeledExpected.no_match
                      }
                    >
                      Copy Labeled Sample
                    </Button>
                  </div>
                  <div className="text-muted-foreground">
                    Selected candidates: {selectedLabelKeys.length}
                  </div>
                  <div className="mt-1 text-muted-foreground">
                    expected.no_match: {labeledExpected.no_match ? "true" : "false"}
                  </div>
                  <div className="mt-1 text-muted-foreground">
                    expected.dois:{" "}
                    {labeledExpected.dois.length > 0
                      ? labeledExpected.dois.join(", ")
                      : "(empty)"}
                  </div>
                  <div className="mt-1 text-muted-foreground">
                    expected.titles:{" "}
                    {labeledExpected.titles.length > 0
                      ? labeledExpected.titles.join(" | ")
                      : "(empty)"}
                  </div>
                </div>
              </div>
            </div>
          )}
        </DialogContent>
      </Dialog>
    </div>
  );
}
