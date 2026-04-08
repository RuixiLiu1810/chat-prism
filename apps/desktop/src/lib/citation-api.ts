import { invoke } from "@tauri-apps/api/core";

export interface CitationCandidate {
  paper_id: string;
  title: string;
  year?: number;
  venue?: string;
  abstract_text?: string;
  doi?: string;
  url?: string;
  authors: string[];
  citation_count?: number;
  score: number;
  evidence_sentences?: string[];
  score_explain?: CitationScoreExplain;
}

export interface CitationScoreExplain {
  sem_title: number;
  sem_abstract: number;
  phrase: number;
  recency: number;
  strength: number;
  contradiction_penalty: number;
  formula_penalty?: number;
  context_factor: number;
  final_score: number;
}

export interface CitationSearchAttemptDebug {
  query: string;
  provider: string;
  ok: boolean;
  error?: string;
  result_count: number;
  candidates: CitationCandidate[];
}

export interface CitationQueryPlanItem {
  query: string;
  strategy: string;
  source: string;
  weight: number;
  quality: CitationQueryQualityDebug;
}

export interface CitationQueryQualityDebug {
  total: number;
  semantic_sim: number;
  anchor_coverage: number;
  specificity: number;
  noise_penalty: number;
  length_penalty: number;
}

export interface CitationProviderBudgetDebug {
  provider: string;
  initial: number;
  used: number;
  skipped_due_to_budget: number;
  skipped_due_to_rate_limit: number;
}

export interface CitationQueryExecutionDebug {
  query: string;
  source: string;
  strategy: string;
  weight: number;
  quality_score: number;
  s2_status: string;
  openalex_status: string;
  crossref_status: string;
  pubmed_status: string;
}

export interface CitationSearchDebug {
  selected_text: string;
  preprocessed_text: string;
  need_decision: CitationNeedDecisionDebug;
  latency_ms: number;
  query_plan: CitationQueryPlanItem[];
  queries: string[];
  llm_query_enabled: boolean;
  llm_query_attempted: boolean;
  llm_query_error?: string;
  query_embedding_provider: string;
  query_embedding_timeout_ms: number;
  query_embedding_fallback_count: number;
  query_embedding_error?: string;
  query_execution_top_n: number;
  query_execution_mmr_lambda: number;
  query_execution_min_quality: number;
  query_execution_min_hit_ratio: number;
  query_execution_hit_score_threshold: number;
  query_execution_selected_count: number;
  stop_reason?: string;
  stop_stage?: string;
  stop_hit_ratio?: number;
  stop_quality_hits: number;
  stop_attempted_queries: number;
  stop_merged_count: number;
  per_query_limit: number;
  has_s2_api_key: boolean;
  s2_rate_limited: boolean;
  provider_budgets: CitationProviderBudgetDebug[];
  query_execution: CitationQueryExecutionDebug[];
  attempts: CitationSearchAttemptDebug[];
  merged_results: CitationCandidate[];
  final_error?: string;
}

export interface CitationNeedDecisionDebug {
  needs_citation: boolean;
  level: "must" | "suggest" | "no" | string;
  claim_type: string;
  recommended_refs: number;
  score: number;
  reasons: string[];
}

export interface CitationSearchResponse {
  results: CitationCandidate[];
  need_decision: CitationNeedDecisionDebug;
}

export async function searchCitations(
  selectedText: string,
  limit = 8,
  projectRoot?: string | null,
): Promise<CitationSearchResponse> {
  return invoke<CitationSearchResponse>("citation_search", {
    selectedText,
    limit,
    projectRoot: projectRoot ?? null,
  });
}

export async function searchCitationsDebug(
  selectedText: string,
  limit = 8,
  projectRoot?: string | null,
): Promise<CitationSearchDebug> {
  return invoke<CitationSearchDebug>("citation_search_debug", {
    selectedText,
    limit,
    projectRoot: projectRoot ?? null,
  });
}
