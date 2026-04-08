mod providers;
mod query;
mod scoring;
mod types;

// Re-export public types used externally (Tauri commands return these).
#[allow(unused_imports)]
pub use types::{
    CitationCandidate, CitationNeedDecisionDebug, CitationProviderBudgetDebug,
    CitationQueryExecutionDebug, CitationQueryPlanItem, CitationQueryQualityDebug,
    CitationScoreExplain, CitationSearchAttemptDebug, CitationSearchDebug, CitationSearchResponse,
};

use std::collections::HashSet;
use std::time::{Duration, Instant};

use crate::settings;
use serde::Serialize;

use providers::*;
use query::*;
use scoring::*;
use types::*;

fn finalize_citation_run(
    mut debug: CitationSearchDebug,
    merged_results: Vec<CitationCandidate>,
    error: Option<String>,
    started_at: Instant,
) -> CitationSearchRun {
    let elapsed = started_at.elapsed().as_millis();
    debug.latency_ms = elapsed.min(u128::from(u64::MAX)) as u64;
    CitationSearchRun {
        merged_results,
        debug,
        error,
    }
}

#[derive(Debug, Clone, Copy)]
pub struct AgentLiteratureSearchOptions {
    pub limit: u32,
    pub enable_mesh_expansion: bool,
    pub min_year: Option<u16>,
    pub max_year: Option<u16>,
}

impl Default for AgentLiteratureSearchOptions {
    fn default() -> Self {
        Self {
            limit: 8,
            enable_mesh_expansion: false,
            min_year: None,
            max_year: None,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct AgentLiteratureSearchOutput {
    pub query: String,
    pub query_plan: Vec<CitationQueryPlanItem>,
    pub executed_queries: Vec<CitationQueryExecutionDebug>,
    pub provider_errors: Vec<String>,
    pub results: Vec<CitationCandidate>,
}

async fn run_citation_search(
    app: &tauri::AppHandle,
    selected_text: String,
    limit: Option<u32>,
    project_root: Option<String>,
) -> CitationSearchRun {
    let started_at = Instant::now();
    let raw_selected = selected_text.trim();
    let limit = limit.unwrap_or(8).clamp(1, 20);
    let rule_query_plan = if raw_selected.is_empty() {
        Vec::new()
    } else {
        build_search_query_plan(raw_selected)
    };
    let preprocessed = if raw_selected.is_empty() {
        String::new()
    } else {
        preprocess_selected_text(raw_selected)
    };
    let score_basis = if preprocessed.is_empty() {
        raw_selected
    } else {
        preprocessed.as_str()
    };

    let llm_runtime = settings::load_citation_llm_runtime(app, project_root.as_deref()).ok();
    let llm_query_enabled = llm_runtime.as_ref().map(|cfg| cfg.enabled).unwrap_or(false);
    let provider_runtime =
        settings::load_citation_provider_runtime(app, project_root.as_deref()).ok();
    let s2_enabled = provider_runtime
        .as_ref()
        .map(|cfg| cfg.semantic_scholar_enabled)
        .unwrap_or(true);
    let s2_api_key_from_settings = provider_runtime
        .as_ref()
        .and_then(|cfg| cfg.semantic_scholar_api_key.clone())
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty());
    let s2_api_key_from_env = std::env::var("S2_API_KEY")
        .ok()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty());
    // Env key has highest priority for local debugging / overrides.
    let s2_api_key = s2_api_key_from_env.or(s2_api_key_from_settings);
    let mut llm_query_attempted = false;
    let mut llm_query_error: Option<String> = None;
    let embedding_runtime =
        settings::load_citation_query_embedding_runtime(app, project_root.as_deref()).ok();
    let query_embedding_provider = embedding_runtime
        .as_ref()
        .map(|cfg| QueryEmbeddingProvider::from_raw(&cfg.provider))
        .unwrap_or(QueryEmbeddingProvider::None);
    let query_embedding_timeout_ms = embedding_runtime
        .as_ref()
        .map(|cfg| cfg.timeout_ms)
        .unwrap_or(1200)
        .clamp(100, 10000);
    let mut query_embedding_fallback_count = 0u32;
    let mut query_embedding_error: Option<String> = None;
    let execution_runtime =
        settings::load_citation_query_execution_runtime(app, project_root.as_deref()).ok();
    let query_execution_top_n = execution_runtime
        .as_ref()
        .map(|cfg| cfg.top_n)
        .unwrap_or(QUERY_EXECUTION_DEFAULT_TOP_N)
        .max(1);
    let query_execution_mmr_lambda = execution_runtime
        .as_ref()
        .map(|cfg| cfg.mmr_lambda)
        .unwrap_or(QUERY_EXECUTION_DEFAULT_MMR_LAMBDA)
        .clamp(0.0, 1.0);
    let query_execution_min_quality = execution_runtime
        .as_ref()
        .map(|cfg| cfg.min_quality)
        .unwrap_or(QUERY_EXECUTION_DEFAULT_MIN_QUALITY)
        .clamp(0.0, 1.0);
    let query_execution_min_hit_ratio = execution_runtime
        .as_ref()
        .map(|cfg| cfg.min_hit_ratio)
        .unwrap_or(QUERY_EXECUTION_DEFAULT_MIN_HIT_RATIO)
        .clamp(0.0, 1.0);
    let query_execution_hit_score_threshold = execution_runtime
        .as_ref()
        .map(|cfg| cfg.hit_score_threshold)
        .unwrap_or(QUERY_EXECUTION_DEFAULT_HIT_SCORE_THRESHOLD)
        .clamp(0.0, 1.0);
    let need_decision = classify_citation_need(score_basis);

    let mut query_plan = rule_query_plan;
    if let Some(cfg) = llm_runtime.as_ref().filter(|cfg| cfg.enabled) {
        llm_query_attempted = true;
        match generate_llm_query_plan(score_basis, cfg).await {
            Ok(llm_plan) => {
                let mut seen = query_plan
                    .iter()
                    .map(|q| q.query.to_lowercase())
                    .collect::<HashSet<_>>();
                for item in llm_plan {
                    if seen.insert(item.query.to_lowercase()) {
                        query_plan.push(item);
                    }
                }
            }
            Err(err) => {
                llm_query_error = Some(err);
            }
        }
    }

    for item in &mut query_plan {
        let lexical_quality = score_query_quality(&item.query, score_basis);
        if query_embedding_provider == QueryEmbeddingProvider::None {
            item.quality = lexical_quality;
            continue;
        }

        let started = Instant::now();
        match score_query_quality_with_embedding_provider(
            &item.query,
            score_basis,
            query_embedding_provider,
        ) {
            Ok(quality) => {
                if started.elapsed() > Duration::from_millis(query_embedding_timeout_ms) {
                    query_embedding_fallback_count =
                        query_embedding_fallback_count.saturating_add(1);
                    if query_embedding_error.is_none() {
                        query_embedding_error = Some(format!(
                            "Local embedding scoring exceeded timeout ({}ms), fallback to lexical score.",
                            query_embedding_timeout_ms
                        ));
                    }
                    item.quality = lexical_quality;
                } else {
                    item.quality = quality;
                }
            }
            Err(err) => {
                query_embedding_fallback_count = query_embedding_fallback_count.saturating_add(1);
                if query_embedding_error.is_none() {
                    query_embedding_error = Some(err);
                }
                item.quality = lexical_quality;
            }
        }
    }

    query_plan.sort_by(|a, b| {
        let source_rank = |source: &str| if source == "rule" { 0usize } else { 1usize };
        b.quality
            .total
            .total_cmp(&a.quality.total)
            .then_with(|| b.weight.total_cmp(&a.weight))
            .then_with(|| source_rank(&a.source).cmp(&source_rank(&b.source)))
            .then_with(|| {
                b.quality
                    .anchor_coverage
                    .total_cmp(&a.quality.anchor_coverage)
            })
            .then_with(|| b.quality.semantic_sim.total_cmp(&a.quality.semantic_sim))
            .then_with(|| b.quality.specificity.total_cmp(&a.quality.specificity))
            .then_with(|| a.quality.noise_penalty.total_cmp(&b.quality.noise_penalty))
            .then_with(|| {
                a.quality
                    .length_penalty
                    .total_cmp(&b.quality.length_penalty)
            })
            .then_with(|| a.strategy.cmp(&b.strategy))
    });

    let queries = query_plan
        .iter()
        .map(|q| q.query.clone())
        .collect::<Vec<_>>();
    let execution_plan = select_execution_query_plan(
        &query_plan,
        query_execution_top_n,
        query_execution_min_quality,
        query_execution_mmr_lambda,
    );
    let per_query_limit = (limit * 2).clamp(6, 20);
    let has_s2_api_key = s2_api_key.is_some();

    let mut debug = CitationSearchDebug {
        selected_text: raw_selected.to_string(),
        preprocessed_text: preprocessed.clone(),
        need_decision,
        latency_ms: 0,
        query_plan: query_plan.clone(),
        queries: queries.clone(),
        llm_query_enabled,
        llm_query_attempted,
        llm_query_error,
        query_embedding_provider: query_embedding_provider.as_str().to_string(),
        query_embedding_timeout_ms,
        query_embedding_fallback_count,
        query_embedding_error,
        query_execution_top_n,
        query_execution_mmr_lambda,
        query_execution_min_quality,
        query_execution_min_hit_ratio,
        query_execution_hit_score_threshold,
        query_execution_selected_count: execution_plan.len(),
        stop_reason: None,
        stop_stage: None,
        stop_hit_ratio: None,
        stop_quality_hits: 0,
        stop_attempted_queries: 0,
        stop_merged_count: 0,
        per_query_limit,
        has_s2_api_key,
        s2_rate_limited: false,
        provider_budgets: Vec::new(),
        query_execution: Vec::new(),
        attempts: Vec::new(),
        merged_results: Vec::new(),
        final_error: None,
    };

    if raw_selected.is_empty() || execution_plan.is_empty() {
        debug.stop_reason = Some(if raw_selected.is_empty() {
            "empty_selection".to_string()
        } else {
            "no_executable_query".to_string()
        });
        return finalize_citation_run(debug, Vec::new(), None, started_at);
    }

    let mut merged: Vec<CitationCandidate> = Vec::new();
    let mut errors: Vec<String> = Vec::new();
    let mut openalex_responded = false;
    let mut crossref_responded = false;
    let mut pubmed_responded = false;
    let mut openalex_rate_limited = false;
    let mut crossref_rate_limited = false;
    let mut pubmed_rate_limited = false;
    let mut s2_rate_limited = false;
    let rule_query_count = execution_plan.iter().filter(|q| q.source == "rule").count();
    let llm_query_count = execution_plan.len().saturating_sub(rule_query_count);
    let mut query_execution = Vec::<CitationQueryExecutionDebug>::new();
    let limit_usize = limit as usize;
    let mut quality_hit_queries = 0usize;
    let mut attempted_queries = 0usize;
    let mut stop_reason: Option<String> = None;
    let mut stop_stage: Option<String> = None;
    let mut stop_hit_ratio: Option<f32> = None;
    // Without API key, keep S2 attempts conservative to avoid hard rate limiting.
    let mut s2_budget: usize = if !s2_enabled {
        0
    } else if has_s2_api_key {
        execution_plan.len()
    } else {
        1
    };
    let s2_initial_budget = s2_budget;
    let mut s2_used = 0usize;
    let mut s2_skipped_budget = 0usize;
    let mut s2_skipped_rate_limit = 0usize;

    let mut openalex_llm_budget = llm_query_count.min(2);
    let openalex_initial_budget = rule_query_count + openalex_llm_budget;
    let mut openalex_used = 0usize;
    let mut openalex_skipped_budget = 0usize;
    let mut openalex_skipped_rate_limit = 0usize;

    let mut crossref_llm_budget = llm_query_count.min(1);
    let crossref_initial_budget = rule_query_count + crossref_llm_budget;
    let mut crossref_used = 0usize;
    let mut crossref_skipped_budget = 0usize;
    let mut crossref_skipped_rate_limit = 0usize;

    let mut pubmed_llm_budget = llm_query_count.min(2);
    let pubmed_initial_budget = rule_query_count + pubmed_llm_budget;
    let mut pubmed_used = 0usize;
    let mut pubmed_skipped_budget = 0usize;
    let mut pubmed_skipped_rate_limit = 0usize;

    'query_loop: for item in &execution_plan {
        let q = item.query.clone();
        let mut query_has_quality_hit = false;
        let mut query_attempted_provider = false;
        let mut exec = CitationQueryExecutionDebug {
            query: q.clone(),
            source: item.source.clone(),
            strategy: item.strategy.clone(),
            weight: item.weight,
            quality_score: item.quality.total,
            s2_status: "pending".to_string(),
            openalex_status: "pending".to_string(),
            crossref_status: "pending".to_string(),
            pubmed_status: "pending".to_string(),
        };

        let s2_allowed = item.source == "rule" || item.weight >= 0.80;
        if !s2_enabled {
            exec.s2_status = "skipped_disabled".to_string();
        } else if !s2_allowed {
            exec.s2_status = "skipped_low_weight".to_string();
        } else if s2_rate_limited {
            s2_skipped_rate_limit += 1;
            exec.s2_status = "skipped_rate_limited".to_string();
        } else if s2_budget == 0 {
            s2_skipped_budget += 1;
            exec.s2_status = "skipped_budget".to_string();
        } else {
            s2_budget -= 1;
            s2_used += 1;
            let s2_query = build_s2_compact_query(&q);
            if s2_query.trim().is_empty() {
                exec.s2_status = "skipped_empty_query".to_string();
                debug.attempts.push(CitationSearchAttemptDebug {
                    query: q.clone(),
                    provider: "semantic_scholar".to_string(),
                    ok: false,
                    error: Some("Skipped empty compact query".to_string()),
                    result_count: 0,
                    candidates: Vec::new(),
                });
            } else {
                query_attempted_provider = true;
                match search_semantic_scholar(
                    &s2_query,
                    score_basis,
                    per_query_limit,
                    s2_api_key.as_deref(),
                )
                .await
                {
                    Ok(candidates) => {
                        let candidates = apply_query_context_scores(candidates, item, PROVIDER_S2);
                        exec.s2_status = format!("ok({})", candidates.len());
                        debug.attempts.push(CitationSearchAttemptDebug {
                            query: s2_query.clone(),
                            provider: "semantic_scholar".to_string(),
                            ok: true,
                            error: None,
                            result_count: candidates.len(),
                            candidates: candidates.clone(),
                        });
                        if !candidates.is_empty() {
                            if has_quality_hit(&candidates, query_execution_hit_score_threshold) {
                                query_has_quality_hit = true;
                            }
                            merged = merge_candidates(merged, candidates);
                        }
                    }
                    Err(err) => {
                        if err.contains("status 429")
                            || err.contains("Too Many Requests")
                            || err.contains("circuit cooldown")
                        {
                            s2_rate_limited = true;
                            exec.s2_status = "error_rate_limited".to_string();
                        } else {
                            errors.push(format!("S2 [{}]: {}", short_query(&s2_query), err));
                            exec.s2_status = "error".to_string();
                        }
                        debug.attempts.push(CitationSearchAttemptDebug {
                            query: s2_query,
                            provider: "semantic_scholar".to_string(),
                            ok: false,
                            error: Some(err),
                            result_count: 0,
                            candidates: Vec::new(),
                        });
                    }
                }
            }
        }

        let attempted_with_current = attempted_queries + usize::from(query_attempted_provider);
        let hit_with_current = quality_hit_queries + usize::from(query_has_quality_hit);
        if let Some(hit_ratio) = should_stop_early(
            &merged,
            limit_usize,
            hit_with_current,
            attempted_with_current,
            query_execution_min_hit_ratio,
        ) {
            attempted_queries = attempted_with_current;
            quality_hit_queries = hit_with_current;
            stop_reason = Some("enough_results_hit_ratio".to_string());
            stop_stage = Some("after_semantic_scholar".to_string());
            stop_hit_ratio = Some(hit_ratio);
            exec.openalex_status = "skipped_enough_results".to_string();
            exec.crossref_status = "skipped_enough_results".to_string();
            exec.pubmed_status = "skipped_enough_results".to_string();
            query_execution.push(exec);
            break 'query_loop;
        }

        let openalex_allowed = if item.source == "llm" {
            if openalex_llm_budget == 0 {
                false
            } else {
                openalex_llm_budget -= 1;
                true
            }
        } else {
            true
        };

        if !openalex_allowed {
            openalex_skipped_budget += 1;
            exec.openalex_status = "skipped_llm_budget".to_string();
        } else if openalex_rate_limited {
            openalex_skipped_rate_limit += 1;
            exec.openalex_status = "skipped_rate_limited".to_string();
        } else {
            let openalex_query = build_openalex_compact_query(&q);
            if openalex_query.trim().is_empty() {
                exec.openalex_status = "skipped_empty_query".to_string();
                debug.attempts.push(CitationSearchAttemptDebug {
                    query: q.clone(),
                    provider: "openalex".to_string(),
                    ok: false,
                    error: Some("Skipped empty compact query".to_string()),
                    result_count: 0,
                    candidates: Vec::new(),
                });
            } else {
                openalex_used += 1;
                query_attempted_provider = true;
                match search_openalex(&openalex_query, score_basis, per_query_limit).await {
                    Ok(candidates) => {
                        let candidates =
                            apply_query_context_scores(candidates, item, PROVIDER_OPENALEX);
                        openalex_responded = true;
                        exec.openalex_status = format!("ok({})", candidates.len());
                        debug.attempts.push(CitationSearchAttemptDebug {
                            query: openalex_query.clone(),
                            provider: "openalex".to_string(),
                            ok: true,
                            error: None,
                            result_count: candidates.len(),
                            candidates: candidates.clone(),
                        });
                        if !candidates.is_empty() {
                            if has_quality_hit(&candidates, query_execution_hit_score_threshold) {
                                query_has_quality_hit = true;
                            }
                            merged = merge_candidates(merged, candidates);
                        }
                    }
                    Err(err) => {
                        if err.contains("status 429")
                            || err.contains("Too Many Requests")
                            || err.contains("circuit cooldown")
                        {
                            openalex_rate_limited = true;
                            exec.openalex_status = "error_rate_limited".to_string();
                        } else {
                            exec.openalex_status = "error".to_string();
                        }
                        errors.push(format!(
                            "OpenAlex [{}]: {}",
                            short_query(&openalex_query),
                            err
                        ));
                        debug.attempts.push(CitationSearchAttemptDebug {
                            query: openalex_query.clone(),
                            provider: "openalex".to_string(),
                            ok: false,
                            error: Some(err),
                            result_count: 0,
                            candidates: Vec::new(),
                        });
                    }
                }
            }
        }

        let attempted_with_current = attempted_queries + usize::from(query_attempted_provider);
        let hit_with_current = quality_hit_queries + usize::from(query_has_quality_hit);
        if let Some(hit_ratio) = should_stop_early(
            &merged,
            limit_usize,
            hit_with_current,
            attempted_with_current,
            query_execution_min_hit_ratio,
        ) {
            attempted_queries = attempted_with_current;
            quality_hit_queries = hit_with_current;
            stop_reason = Some("enough_results_hit_ratio".to_string());
            stop_stage = Some("after_openalex".to_string());
            stop_hit_ratio = Some(hit_ratio);
            exec.crossref_status = "skipped_enough_results".to_string();
            exec.pubmed_status = "skipped_enough_results".to_string();
            query_execution.push(exec);
            break 'query_loop;
        }

        let crossref_allowed_by_weight = item.source == "rule" || item.weight >= 0.90;
        if !crossref_allowed_by_weight {
            exec.crossref_status = "skipped_low_weight".to_string();
        } else {
            let crossref_allowed_by_budget = if item.source == "llm" {
                if crossref_llm_budget == 0 {
                    false
                } else {
                    crossref_llm_budget -= 1;
                    true
                }
            } else {
                true
            };

            if !crossref_allowed_by_budget {
                crossref_skipped_budget += 1;
                exec.crossref_status = "skipped_llm_budget".to_string();
            } else if crossref_rate_limited {
                crossref_skipped_rate_limit += 1;
                exec.crossref_status = "skipped_rate_limited".to_string();
            } else {
                let crossref_query = build_crossref_compact_query(&q);
                if crossref_query.trim().is_empty() {
                    exec.crossref_status = "skipped_empty_query".to_string();
                    debug.attempts.push(CitationSearchAttemptDebug {
                        query: q.clone(),
                        provider: "crossref".to_string(),
                        ok: false,
                        error: Some("Skipped empty compact query".to_string()),
                        result_count: 0,
                        candidates: Vec::new(),
                    });
                } else {
                    crossref_used += 1;
                    query_attempted_provider = true;
                    match search_crossref(&crossref_query, score_basis, per_query_limit).await {
                        Ok(candidates) => {
                            let candidates =
                                apply_query_context_scores(candidates, item, PROVIDER_CROSSREF);
                            crossref_responded = true;
                            exec.crossref_status = format!("ok({})", candidates.len());
                            debug.attempts.push(CitationSearchAttemptDebug {
                                query: crossref_query.clone(),
                                provider: "crossref".to_string(),
                                ok: true,
                                error: None,
                                result_count: candidates.len(),
                                candidates: candidates.clone(),
                            });
                            if !candidates.is_empty() {
                                if has_quality_hit(&candidates, query_execution_hit_score_threshold)
                                {
                                    query_has_quality_hit = true;
                                }
                                merged = merge_candidates(merged, candidates);
                            }
                        }
                        Err(err) => {
                            if err.contains("status 429")
                                || err.contains("Too Many Requests")
                                || err.contains("circuit cooldown")
                            {
                                crossref_rate_limited = true;
                                exec.crossref_status = "error_rate_limited".to_string();
                            } else {
                                exec.crossref_status = "error".to_string();
                            }
                            errors.push(format!(
                                "Crossref [{}]: {}",
                                short_query(&crossref_query),
                                err
                            ));
                            debug.attempts.push(CitationSearchAttemptDebug {
                                query: crossref_query.clone(),
                                provider: "crossref".to_string(),
                                ok: false,
                                error: Some(err),
                                result_count: 0,
                                candidates: Vec::new(),
                            });
                        }
                    };
                }
            }
        }

        let pubmed_allowed_by_weight = item.source == "rule" || item.weight >= 0.78;
        if !pubmed_allowed_by_weight {
            exec.pubmed_status = "skipped_low_weight".to_string();
        } else {
            let pubmed_allowed_by_budget = if item.source == "llm" {
                if pubmed_llm_budget == 0 {
                    false
                } else {
                    pubmed_llm_budget -= 1;
                    true
                }
            } else {
                true
            };

            if !pubmed_allowed_by_budget {
                pubmed_skipped_budget += 1;
                exec.pubmed_status = "skipped_llm_budget".to_string();
            } else if pubmed_rate_limited {
                pubmed_skipped_rate_limit += 1;
                exec.pubmed_status = "skipped_rate_limited".to_string();
            } else {
                let pubmed_query = build_pubmed_compact_query(&q);
                if pubmed_query.trim().is_empty() {
                    exec.pubmed_status = "skipped_empty_query".to_string();
                    debug.attempts.push(CitationSearchAttemptDebug {
                        query: q.clone(),
                        provider: "pubmed".to_string(),
                        ok: false,
                        error: Some("Skipped empty compact query".to_string()),
                        result_count: 0,
                        candidates: Vec::new(),
                    });
                } else {
                    pubmed_used += 1;
                    query_attempted_provider = true;
                    match search_pubmed(&pubmed_query, score_basis, per_query_limit, None, None)
                        .await
                    {
                        Ok(candidates) => {
                            let candidates =
                                apply_query_context_scores(candidates, item, PROVIDER_PUBMED);
                            pubmed_responded = true;
                            exec.pubmed_status = format!("ok({})", candidates.len());
                            debug.attempts.push(CitationSearchAttemptDebug {
                                query: pubmed_query.clone(),
                                provider: "pubmed".to_string(),
                                ok: true,
                                error: None,
                                result_count: candidates.len(),
                                candidates: candidates.clone(),
                            });
                            if !candidates.is_empty() {
                                if has_quality_hit(&candidates, query_execution_hit_score_threshold)
                                {
                                    query_has_quality_hit = true;
                                }
                                merged = merge_candidates(merged, candidates);
                            }
                        }
                        Err(err) => {
                            if err.contains("status 429")
                                || err.contains("Too Many Requests")
                                || err.contains("circuit cooldown")
                            {
                                pubmed_rate_limited = true;
                                exec.pubmed_status = "error_rate_limited".to_string();
                            } else {
                                exec.pubmed_status = "error".to_string();
                            }
                            errors.push(format!(
                                "PubMed [{}]: {}",
                                short_query(&pubmed_query),
                                err
                            ));
                            debug.attempts.push(CitationSearchAttemptDebug {
                                query: pubmed_query.clone(),
                                provider: "pubmed".to_string(),
                                ok: false,
                                error: Some(err),
                                result_count: 0,
                                candidates: Vec::new(),
                            });
                        }
                    };
                }
            }
        }

        attempted_queries += usize::from(query_attempted_provider);
        quality_hit_queries += usize::from(query_has_quality_hit);
        query_execution.push(exec);

        if let Some(hit_ratio) = should_stop_early(
            &merged,
            limit_usize,
            quality_hit_queries,
            attempted_queries,
            query_execution_min_hit_ratio,
        ) {
            stop_reason = Some("enough_results_hit_ratio".to_string());
            stop_stage = Some("after_pubmed".to_string());
            stop_hit_ratio = Some(hit_ratio);
            break 'query_loop;
        }
    }

    if stop_reason.is_none() {
        stop_reason = Some("execution_plan_exhausted".to_string());
    }
    if stop_hit_ratio.is_none() && attempted_queries > 0 {
        stop_hit_ratio = Some(quality_hit_queries as f32 / attempted_queries as f32);
    }

    debug.s2_rate_limited = s2_rate_limited;
    debug.stop_reason = stop_reason;
    debug.stop_stage = stop_stage;
    debug.stop_hit_ratio = stop_hit_ratio;
    debug.stop_quality_hits = quality_hit_queries;
    debug.stop_attempted_queries = attempted_queries;
    debug.stop_merged_count = merged.len();
    debug.query_execution = query_execution;
    debug.provider_budgets = vec![
        CitationProviderBudgetDebug {
            provider: "semantic_scholar".to_string(),
            initial: s2_initial_budget,
            used: s2_used,
            skipped_due_to_budget: s2_skipped_budget,
            skipped_due_to_rate_limit: s2_skipped_rate_limit,
        },
        CitationProviderBudgetDebug {
            provider: "openalex".to_string(),
            initial: openalex_initial_budget,
            used: openalex_used,
            skipped_due_to_budget: openalex_skipped_budget,
            skipped_due_to_rate_limit: openalex_skipped_rate_limit,
        },
        CitationProviderBudgetDebug {
            provider: "crossref".to_string(),
            initial: crossref_initial_budget,
            used: crossref_used,
            skipped_due_to_budget: crossref_skipped_budget,
            skipped_due_to_rate_limit: crossref_skipped_rate_limit,
        },
        CitationProviderBudgetDebug {
            provider: "pubmed".to_string(),
            initial: pubmed_initial_budget,
            used: pubmed_used,
            skipped_due_to_budget: pubmed_skipped_budget,
            skipped_due_to_rate_limit: pubmed_skipped_rate_limit,
        },
    ];

    if merged.is_empty() {
        // Semantic Scholar can be rate-limited (429). If OpenAlex/Crossref responded but found no hit,
        // treat as normal "no match" rather than hard failure.
        if openalex_responded || crossref_responded || pubmed_responded {
            debug.merged_results = Vec::new();
            return finalize_citation_run(debug, Vec::new(), None, started_at);
        }
        if errors.is_empty() {
            if s2_rate_limited {
                let err = "Citation search is temporarily rate-limited by Semantic Scholar and fallback providers did not respond."
                    .to_string();
                debug.final_error = Some(err.clone());
                return finalize_citation_run(debug, Vec::new(), Some(err), started_at);
            }
            debug.merged_results = Vec::new();
            return finalize_citation_run(debug, Vec::new(), None, started_at);
        }
        let details = errors.join(" | ");
        let details = truncate_chars(&details, 520);
        let err = format!("Citation search failed. {}", details);
        debug.final_error = Some(err.clone());
        return finalize_citation_run(debug, Vec::new(), Some(err), started_at);
    }

    merged.sort_by(|a, b| b.score.total_cmp(&a.score));
    merged.truncate(limit as usize);
    debug.merged_results = merged.clone();

    finalize_citation_run(debug, merged, None, started_at)
}

pub async fn search_literature_for_agent(
    query: &str,
    options: AgentLiteratureSearchOptions,
) -> Result<AgentLiteratureSearchOutput, String> {
    let cleaned = preprocess_selected_text(query);
    if cleaned.is_empty() {
        return Err("search_literature query must not be empty.".to_string());
    }

    let limit = options.limit.clamp(1, 20);
    let mut query_plan = build_search_query_plan_with_options(
        &cleaned,
        QueryPlanBuildOptions {
            enable_mesh_expansion: options.enable_mesh_expansion,
            min_year: options.min_year,
            max_year: options.max_year,
        },
    );
    for item in &mut query_plan {
        item.quality = score_query_quality(&item.query, &cleaned);
    }
    query_plan.sort_by(|a, b| {
        b.quality
            .total
            .total_cmp(&a.quality.total)
            .then_with(|| b.weight.total_cmp(&a.weight))
    });

    let executed_plan = select_execution_query_plan(&query_plan, 4, 0.12, 0.75);
    let per_query_limit = (limit.saturating_mul(2)).clamp(6, 20);
    let s2_api_key = std::env::var("S2_API_KEY")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());

    let mut merged = Vec::<CitationCandidate>::new();
    let mut provider_errors = Vec::<String>::new();
    let mut executed_queries = Vec::<CitationQueryExecutionDebug>::new();

    for item in &executed_plan {
        let mut exec = CitationQueryExecutionDebug {
            query: item.query.clone(),
            source: item.source.clone(),
            strategy: item.strategy.clone(),
            weight: item.weight,
            quality_score: item.quality.total,
            s2_status: "pending".to_string(),
            openalex_status: "pending".to_string(),
            crossref_status: "pending".to_string(),
            pubmed_status: "pending".to_string(),
        };

        let s2_query = build_s2_compact_query(&item.query);
        match search_semantic_scholar(&s2_query, &cleaned, per_query_limit, s2_api_key.as_deref())
            .await
        {
            Ok(candidates) => {
                let candidates = apply_query_context_scores(candidates, item, PROVIDER_S2);
                exec.s2_status = format!("ok({})", candidates.len());
                if !candidates.is_empty() {
                    merged = merge_candidates(merged, candidates);
                }
            }
            Err(err) => {
                exec.s2_status = "error".to_string();
                provider_errors.push(format!("S2 [{}]: {}", short_query(&s2_query), err));
            }
        }

        let openalex_query = build_openalex_compact_query(&item.query);
        match search_openalex(&openalex_query, &cleaned, per_query_limit).await {
            Ok(candidates) => {
                let candidates = apply_query_context_scores(candidates, item, PROVIDER_OPENALEX);
                exec.openalex_status = format!("ok({})", candidates.len());
                if !candidates.is_empty() {
                    merged = merge_candidates(merged, candidates);
                }
            }
            Err(err) => {
                exec.openalex_status = "error".to_string();
                provider_errors.push(format!(
                    "OpenAlex [{}]: {}",
                    short_query(&openalex_query),
                    err
                ));
            }
        }

        let crossref_query = build_crossref_compact_query(&item.query);
        match search_crossref(&crossref_query, &cleaned, per_query_limit).await {
            Ok(candidates) => {
                let candidates = apply_query_context_scores(candidates, item, PROVIDER_CROSSREF);
                exec.crossref_status = format!("ok({})", candidates.len());
                if !candidates.is_empty() {
                    merged = merge_candidates(merged, candidates);
                }
            }
            Err(err) => {
                exec.crossref_status = "error".to_string();
                provider_errors.push(format!(
                    "Crossref [{}]: {}",
                    short_query(&crossref_query),
                    err
                ));
            }
        }

        let pubmed_query = build_pubmed_compact_query(&item.query);
        match search_pubmed(
            &pubmed_query,
            &cleaned,
            per_query_limit,
            options.min_year,
            options.max_year,
        )
        .await
        {
            Ok(candidates) => {
                let candidates = apply_query_context_scores(candidates, item, PROVIDER_PUBMED);
                exec.pubmed_status = format!("ok({})", candidates.len());
                if !candidates.is_empty() {
                    merged = merge_candidates(merged, candidates);
                }
            }
            Err(err) => {
                exec.pubmed_status = "error".to_string();
                provider_errors.push(format!("PubMed [{}]: {}", short_query(&pubmed_query), err));
            }
        }

        executed_queries.push(exec);
    }

    merged.sort_by(|a, b| b.score.total_cmp(&a.score));
    merged.truncate(limit as usize);

    if merged.is_empty() && !provider_errors.is_empty() {
        return Err(format!(
            "Literature search failed. {}",
            truncate_chars(&provider_errors.join(" | "), 520)
        ));
    }

    Ok(AgentLiteratureSearchOutput {
        query: cleaned,
        query_plan,
        executed_queries,
        provider_errors,
        results: merged,
    })
}

#[tauri::command]
pub async fn citation_search(
    app: tauri::AppHandle,
    selected_text: String,
    limit: Option<u32>,
    project_root: Option<String>,
) -> Result<CitationSearchResponse, String> {
    let run = run_citation_search(&app, selected_text, limit, project_root).await;
    if let Some(err) = run.error {
        return Err(err);
    }
    Ok(CitationSearchResponse {
        results: run.merged_results,
        need_decision: run.debug.need_decision,
    })
}

#[tauri::command]
pub async fn citation_search_debug(
    app: tauri::AppHandle,
    selected_text: String,
    limit: Option<u32>,
    project_root: Option<String>,
) -> Result<CitationSearchDebug, String> {
    let run = run_citation_search(&app, selected_text, limit, project_root).await;
    Ok(run.debug)
}

#[cfg(test)]
mod tests {
    use super::scoring::compute_score_explain;

    #[test]
    fn hardcase_material_match_should_beat_generic_hydrothermal_title() {
        let claim =
            "Na2TiO3 nanotubes were synthesized on Ti substrates via a hydrothermal method.";
        let expected_title =
            "Investigation of photocatalytic activity of TiO2 nanotubes synthesized by hydrothermal method";
        let generic_title =
            "Anti-radar application of multiwalled carbon nanotubes and zinc oxide synthesized using a hydrothermal method";

        let expected = compute_score_explain(claim, expected_title, "", Some(2022), Some(10));
        let generic = compute_score_explain(claim, generic_title, "", Some(2020), Some(10));

        assert!(
            expected.final_score > generic.final_score,
            "expected score {} should be greater than generic score {}",
            expected.final_score,
            generic.final_score
        );
        assert!(
            expected.formula_penalty <= generic.formula_penalty,
            "expected formula penalty {} should be <= generic formula penalty {}",
            expected.formula_penalty,
            generic.formula_penalty
        );
    }
}
