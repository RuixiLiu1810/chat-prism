use std::collections::{BTreeMap, BTreeSet, HashSet};

use serde_json::{json, Value};
use tokio::sync::watch;

use super::{
    cancelled_result, error_result, is_cancelled, is_document_resource_path,
    load_document_runtime_content, ok_result, tool_arg_optional_string, tool_arg_optional_usize,
    tool_arg_string, AgentToolResult, DocumentRuntimeContent,
};
use crate::citation::{
    search_literature_for_agent, AgentLiteratureSearchOptions, CitationCandidate,
};

#[derive(Debug, Clone)]
struct PaperAnalysis {
    path: String,
    objective: String,
    methods: Vec<String>,
    findings: Vec<String>,
    limitations: Vec<String>,
    relevance_score: f32,
}

pub(crate) async fn execute_search_literature(
    call_id: &str,
    args: Value,
    cancel_rx: Option<watch::Receiver<bool>>,
) -> AgentToolResult {
    if is_cancelled(cancel_rx.as_ref()) {
        return cancelled_result("search_literature", call_id);
    }

    let query = match tool_arg_string(&args, "query") {
        Ok(value) => value,
        Err(message) => return error_result("search_literature", call_id, message),
    };
    let limit = tool_arg_optional_usize(&args, "limit")
        .unwrap_or(10)
        .clamp(1, 20) as u32;
    let min_year = args
        .get("min_year")
        .and_then(Value::as_u64)
        .and_then(|value| u16::try_from(value).ok());
    let max_year = args
        .get("max_year")
        .and_then(Value::as_u64)
        .and_then(|value| u16::try_from(value).ok());
    let enable_mesh_expansion = args
        .get("mesh_expansion")
        .and_then(Value::as_bool)
        .unwrap_or(true);

    match search_literature_for_agent(
        &query,
        AgentLiteratureSearchOptions {
            limit,
            enable_mesh_expansion,
            min_year,
            max_year,
        },
    )
    .await
    {
        Ok(output) => {
            let results = output
                .results
                .iter()
                .map(literature_result_value)
                .collect::<Vec<_>>();
            ok_result(
                "search_literature",
                call_id,
                json!({
                    "query": output.query,
                    "limit": limit,
                    "meshExpansion": enable_mesh_expansion,
                    "minYear": min_year,
                    "maxYear": max_year,
                    "queryPlan": output.query_plan,
                    "executedQueries": output.executed_queries,
                    "providerErrors": output.provider_errors,
                    "results": results,
                    "resultCount": results.len(),
                }),
                format!("Found {} literature candidates.", results.len()),
            )
        }
        Err(message) => error_result("search_literature", call_id, message),
    }
}

pub(crate) async fn execute_analyze_paper(
    project_root: &str,
    call_id: &str,
    args: Value,
    cancel_rx: Option<watch::Receiver<bool>>,
) -> AgentToolResult {
    if is_cancelled(cancel_rx.as_ref()) {
        return cancelled_result("analyze_paper", call_id);
    }

    let path = match tool_arg_string(&args, "path") {
        Ok(value) => value,
        Err(message) => return error_result("analyze_paper", call_id, message),
    };
    let focus = tool_arg_optional_string(&args, "focus");
    let max_items = tool_arg_optional_usize(&args, "max_items")
        .unwrap_or(4)
        .clamp(2, 8);

    let analysis =
        match analyze_single_paper(project_root, &path, focus.as_deref(), max_items, cancel_rx)
            .await
        {
            Ok(value) => value,
            Err(message) => return error_result("analyze_paper", call_id, message),
        };
    ok_result(
        "analyze_paper",
        call_id,
        json!({
            "path": analysis.path,
            "objective": analysis.objective,
            "methods": analysis.methods,
            "findings": analysis.findings,
            "limitations": analysis.limitations,
            "relevance": analysis.relevance_score,
        }),
        format!("Analyzed paper {}.", path),
    )
}

pub(crate) async fn execute_compare_papers(
    project_root: &str,
    call_id: &str,
    args: Value,
    cancel_rx: Option<watch::Receiver<bool>>,
) -> AgentToolResult {
    if is_cancelled(cancel_rx.as_ref()) {
        return cancelled_result("compare_papers", call_id);
    }
    let focus = tool_arg_optional_string(&args, "focus");
    let papers = parse_paper_paths(&args);
    if papers.len() < 2 {
        return error_result(
            "compare_papers",
            call_id,
            "compare_papers requires at least two paper paths (papers array).".to_string(),
        );
    }

    let mut analyses = Vec::<PaperAnalysis>::new();
    for path in papers {
        if is_cancelled(cancel_rx.as_ref()) {
            return cancelled_result("compare_papers", call_id);
        }
        match analyze_single_paper(project_root, &path, focus.as_deref(), 4, cancel_rx.clone())
            .await
        {
            Ok(analysis) => analyses.push(analysis),
            Err(message) => return error_result("compare_papers", call_id, message),
        }
    }

    let shared = shared_finding_signals(&analyses);
    let conflicts = conflicting_signals(&analyses);
    let method_diff = analyses
        .iter()
        .map(|analysis| {
            json!({
                "path": analysis.path,
                "methods": analysis.methods,
            })
        })
        .collect::<Vec<_>>();

    ok_result(
        "compare_papers",
        call_id,
        json!({
            "papers": analyses.iter().map(|a| json!({
                "path": a.path,
                "objective": a.objective,
                "methods": a.methods,
                "findings": a.findings,
                "limitations": a.limitations,
                "relevance": a.relevance_score,
            })).collect::<Vec<_>>(),
            "sharedFindings": shared,
            "conflictingFindings": conflicts,
            "methodologyDifferences": method_diff,
        }),
        format!("Compared {} papers.", analyses.len()),
    )
}

pub(crate) async fn execute_synthesize_evidence(
    project_root: &str,
    call_id: &str,
    args: Value,
    cancel_rx: Option<watch::Receiver<bool>>,
) -> AgentToolResult {
    if is_cancelled(cancel_rx.as_ref()) {
        return cancelled_result("synthesize_evidence", call_id);
    }
    let focus = tool_arg_optional_string(&args, "focus");
    let papers = parse_paper_paths(&args);
    if papers.is_empty() {
        return error_result(
            "synthesize_evidence",
            call_id,
            "synthesize_evidence requires papers array.".to_string(),
        );
    }

    let mut analyses = Vec::<PaperAnalysis>::new();
    for path in papers {
        if is_cancelled(cancel_rx.as_ref()) {
            return cancelled_result("synthesize_evidence", call_id);
        }
        match analyze_single_paper(project_root, &path, focus.as_deref(), 4, cancel_rx.clone())
            .await
        {
            Ok(analysis) => analyses.push(analysis),
            Err(message) => return error_result("synthesize_evidence", call_id, message),
        }
    }

    let evidence_blocks = build_evidence_blocks(&analyses);
    ok_result(
        "synthesize_evidence",
        call_id,
        json!({
            "focus": focus,
            "paperCount": analyses.len(),
            "evidenceBlocks": evidence_blocks,
        }),
        format!(
            "Synthesized evidence across {} papers into {} themes.",
            analyses.len(),
            evidence_blocks.len()
        ),
    )
}

pub(crate) async fn execute_extract_methodology(
    project_root: &str,
    call_id: &str,
    args: Value,
    cancel_rx: Option<watch::Receiver<bool>>,
) -> AgentToolResult {
    if is_cancelled(cancel_rx.as_ref()) {
        return cancelled_result("extract_methodology", call_id);
    }
    let path = match tool_arg_string(&args, "path") {
        Ok(value) => value,
        Err(message) => return error_result("extract_methodology", call_id, message),
    };
    let runtime = match load_document_runtime_content(project_root, &path, cancel_rx.clone()).await
    {
        Ok(value) => value,
        Err(message) => return error_result("extract_methodology", call_id, message),
    };
    let text = document_text(&runtime);
    let sentences = split_sentences(&text);
    let study_design = find_best_sentence(
        &sentences,
        &[
            "randomized",
            "cohort",
            "in vitro",
            "in vivo",
            "retrospective",
            "prospective",
            "design",
        ],
        1,
    )
    .first()
    .cloned()
    .unwrap_or_else(|| "Not explicitly identified in extracted text.".to_string());
    let sample = find_best_sentence(
        &sentences,
        &[
            "sample",
            "participant",
            "patients",
            "specimen",
            "n=",
            "subjects",
        ],
        2,
    );
    let intervention = find_best_sentence(
        &sentences,
        &[
            "treated",
            "intervention",
            "exposure",
            "dosed",
            "coating",
            "functionalized",
        ],
        2,
    );
    let endpoints = find_best_sentence(
        &sentences,
        &[
            "endpoint",
            "outcome",
            "measured",
            "evaluated",
            "assessed",
            "contact angle",
        ],
        3,
    );
    let statistics = find_best_sentence(
        &sentences,
        &[
            "p <",
            "p=",
            "anova",
            "t-test",
            "confidence interval",
            "standard deviation",
        ],
        3,
    );

    ok_result(
        "extract_methodology",
        call_id,
        json!({
            "path": path,
            "studyDesign": study_design,
            "sample": sample,
            "intervention": intervention,
            "endpoints": endpoints,
            "statistics": statistics,
        }),
        format!("Extracted methodology structure from {}.", path),
    )
}

async fn analyze_single_paper(
    project_root: &str,
    path: &str,
    focus: Option<&str>,
    max_items: usize,
    cancel_rx: Option<watch::Receiver<bool>>,
) -> Result<PaperAnalysis, String> {
    if !is_document_resource_path(path) {
        return Err(format!(
            "{} is not a PDF/DOCX path. analyze_paper currently supports document resources only.",
            path
        ));
    }

    let runtime_content = load_document_runtime_content(project_root, path, cancel_rx).await?;
    let text = document_text(&runtime_content);
    let sentences = split_sentences(&text);
    if sentences.is_empty() {
        return Err(format!("No readable text found in {}.", path));
    }

    let objective = find_best_sentence(
        &sentences,
        &[
            "objective",
            "aim",
            "purpose",
            "investigate",
            "we report",
            "this study",
        ],
        1,
    )
    .first()
    .cloned()
    .unwrap_or_else(|| fallback_sentence(&sentences));
    let methods = find_best_sentence(
        &sentences,
        &[
            "method",
            "materials",
            "protocol",
            "synthesized",
            "prepared",
            "measured",
            "characterized",
        ],
        max_items,
    );
    let findings = find_best_sentence(
        &sentences,
        &[
            "result",
            "observed",
            "showed",
            "improved",
            "increased",
            "decreased",
            "significant",
        ],
        max_items,
    );
    let limitations = find_best_sentence(
        &sentences,
        &[
            "limitation",
            "however",
            "future work",
            "further study",
            "constraint",
            "uncertain",
        ],
        max_items,
    );
    let relevance = focus
        .map(|target| relevance_score(&sentences, target))
        .unwrap_or(0.6);

    Ok(PaperAnalysis {
        path: path.to_string(),
        objective,
        methods,
        findings,
        limitations: if limitations.is_empty() {
            vec!["No explicit limitations sentence found in extracted text.".to_string()]
        } else {
            limitations
        },
        relevance_score: relevance,
    })
}

fn document_text(runtime_content: &DocumentRuntimeContent) -> String {
    let primary = if runtime_content.searchable_text.trim().is_empty() {
        runtime_content.excerpt.as_str()
    } else {
        runtime_content.searchable_text.as_str()
    };
    primary.replace("\r\n", "\n")
}

fn split_sentences(text: &str) -> Vec<String> {
    let normalized = text.replace('\n', " ");
    normalized
        .split(|ch: char| matches!(ch, '.' | '?' | '!' | ';'))
        .map(str::trim)
        .filter(|sentence| sentence.len() >= 16)
        .map(collapse_whitespace)
        .filter(|sentence| !sentence.is_empty())
        .collect()
}

fn find_best_sentence(sentences: &[String], keywords: &[&str], max_items: usize) -> Vec<String> {
    if max_items == 0 {
        return Vec::new();
    }
    let keyword_set = keywords
        .iter()
        .map(|keyword| keyword.to_ascii_lowercase())
        .collect::<Vec<_>>();
    let mut scored = sentences
        .iter()
        .map(|sentence| {
            let lowered = sentence.to_ascii_lowercase();
            let score = keyword_set
                .iter()
                .filter(|keyword| lowered.contains(keyword.as_str()))
                .count();
            (score, sentence.clone())
        })
        .filter(|(score, _)| *score > 0)
        .collect::<Vec<_>>();
    scored.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.len().cmp(&b.1.len())));
    scored
        .into_iter()
        .map(|(_, sentence)| sentence)
        .take(max_items)
        .collect()
}

fn fallback_sentence(sentences: &[String]) -> String {
    sentences
        .iter()
        .find(|sentence| sentence.len() >= 24)
        .cloned()
        .unwrap_or_else(|| {
            "Objective could not be reliably extracted from current text.".to_string()
        })
}

fn relevance_score(sentences: &[String], focus: &str) -> f32 {
    let focus_tokens = tokenize(focus);
    if focus_tokens.is_empty() {
        return 0.6;
    }
    let mut best = 0.0f32;
    for sentence in sentences.iter().take(80) {
        let tokens = tokenize(sentence);
        if tokens.is_empty() {
            continue;
        }
        let overlap = overlap_ratio(&tokens, &focus_tokens);
        if overlap > best {
            best = overlap;
        }
    }
    best.clamp(0.0, 1.0)
}

fn tokenize(input: &str) -> Vec<String> {
    input
        .to_ascii_lowercase()
        .split(|ch: char| !ch.is_ascii_alphanumeric())
        .filter(|token| token.len() >= 3)
        .map(str::to_string)
        .collect()
}

fn overlap_ratio(a: &[String], b: &[String]) -> f32 {
    if a.is_empty() || b.is_empty() {
        return 0.0;
    }
    let set_a = a.iter().collect::<HashSet<_>>();
    let set_b = b.iter().collect::<HashSet<_>>();
    let overlap = set_a.intersection(&set_b).count() as f32;
    overlap / (set_b.len() as f32)
}

fn parse_paper_paths(args: &Value) -> Vec<String> {
    let mut out = Vec::<String>::new();
    if let Some(path) = tool_arg_optional_string(args, "path") {
        if !path.trim().is_empty() {
            out.push(path.trim().to_string());
        }
    }
    if let Some(paths) = args.get("papers").and_then(Value::as_array) {
        for item in paths {
            let path = if let Some(value) = item.as_str() {
                value.to_string()
            } else {
                item.get("path")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string()
            };
            let normalized = path.trim().to_string();
            if !normalized.is_empty() {
                out.push(normalized);
            }
        }
    }
    out.into_iter()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>()
}

fn shared_finding_signals(analyses: &[PaperAnalysis]) -> Vec<Value> {
    let mut counts = BTreeMap::<String, usize>::new();
    for analysis in analyses {
        let mut local = HashSet::<String>::new();
        for finding in &analysis.findings {
            for token in tokenize(finding) {
                if token.len() >= 5 {
                    local.insert(token);
                }
            }
        }
        for token in local {
            *counts.entry(token).or_insert(0) += 1;
        }
    }

    counts
        .into_iter()
        .filter(|(_, count)| *count >= 2)
        .take(8)
        .map(|(token, count)| {
            json!({
                "signal": token,
                "paperCount": count,
            })
        })
        .collect()
}

fn conflicting_signals(analyses: &[PaperAnalysis]) -> Vec<Value> {
    let positive = ["increase", "improve", "enhance", "higher", "promote"];
    let negative = ["decrease", "reduce", "lower", "inhibit", "worse"];

    let mut signals = Vec::<Value>::new();
    for analysis in analyses {
        let findings_text = analysis.findings.join(" ").to_ascii_lowercase();
        let has_positive = positive.iter().any(|token| findings_text.contains(token));
        let has_negative = negative.iter().any(|token| findings_text.contains(token));
        if has_positive && has_negative {
            signals.push(json!({
                "path": analysis.path,
                "message": "Contains both positive and negative directional findings; review context for conditional effects.",
            }));
        }
    }
    signals
}

fn build_evidence_blocks(analyses: &[PaperAnalysis]) -> Vec<Value> {
    let mut groups = BTreeMap::<String, Vec<(String, String)>>::new();
    for analysis in analyses {
        for finding in &analysis.findings {
            let theme = detect_theme(finding);
            groups
                .entry(theme)
                .or_default()
                .push((analysis.path.clone(), finding.clone()));
        }
    }

    groups
        .into_iter()
        .map(|(theme, entries)| {
            let mut sources = entries
                .iter()
                .map(|(path, _)| path.clone())
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect::<Vec<_>>();
            if sources.is_empty() {
                sources.push("unknown".to_string());
            }
            let supporting = entries
                .iter()
                .take(6)
                .map(|(path, sentence)| {
                    json!({
                        "source": path,
                        "evidence": sentence,
                    })
                })
                .collect::<Vec<_>>();
            let synthesis = entries
                .iter()
                .take(3)
                .map(|(_, sentence)| sentence.as_str())
                .collect::<Vec<_>>()
                .join(" ");
            json!({
                "theme": theme,
                "synthesis": synthesis,
                "sources": sources,
                "supportingEvidence": supporting,
            })
        })
        .collect::<Vec<_>>()
}

fn detect_theme(text: &str) -> String {
    let lowered = text.to_ascii_lowercase();
    if lowered.contains("mechanism") || lowered.contains("pathway") {
        return "Mechanism".to_string();
    }
    if lowered.contains("safety")
        || lowered.contains("toxicity")
        || lowered.contains("adverse")
        || lowered.contains("risk")
    {
        return "Safety".to_string();
    }
    if lowered.contains("method")
        || lowered.contains("protocol")
        || lowered.contains("prepared")
        || lowered.contains("synthesized")
    {
        return "Methodology".to_string();
    }
    "Outcome".to_string()
}

fn literature_result_value(candidate: &CitationCandidate) -> Value {
    let pmid = candidate
        .paper_id
        .strip_prefix("pubmed:")
        .map(str::to_string)
        .or_else(|| {
            candidate.url.as_ref().and_then(|url| {
                let trimmed = url.trim_end_matches('/');
                trimmed.rsplit('/').next().map(str::to_string)
            })
        });
    json!({
        "paperId": candidate.paper_id,
        "title": candidate.title,
        "year": candidate.year,
        "journal": candidate.venue,
        "abstract": candidate.abstract_text,
        "doi": candidate.doi,
        "pmid": pmid,
        "url": candidate.url,
        "authors": candidate.authors,
        "score": candidate.score,
    })
}

fn collapse_whitespace(input: &str) -> String {
    input.split_whitespace().collect::<Vec<_>>().join(" ")
}
