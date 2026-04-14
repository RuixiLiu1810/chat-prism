use serde_json::{Value, json};
use tokio::sync::watch;

use super::{
    AgentToolResult, cancelled_result, error_result, is_cancelled, is_document_resource_path,
    load_document_runtime_content, ok_result, resolve_project_path, tool_arg_optional_string,
    tool_arg_string,
};

#[derive(Debug, Clone)]
struct ReviewFinding {
    severity: &'static str,
    dimension: &'static str,
    message: String,
    evidence: Option<String>,
    suggestion: String,
}

pub(crate) async fn execute_review_manuscript(
    project_root: &str,
    call_id: &str,
    args: Value,
    cancel_rx: Option<watch::Receiver<bool>>,
) -> AgentToolResult {
    if is_cancelled(cancel_rx.as_ref()) {
        return cancelled_result("review_manuscript", call_id);
    }

    let focus = tool_arg_optional_string(&args, "focus");
    let checklist = parse_string_list_arg(&args, "checklist");
    let (text, source_label) =
        match read_text_argument_or_file(project_root, &args, cancel_rx).await {
            Ok(value) => value,
            Err(message) => return error_result("review_manuscript", call_id, message),
        };

    let findings = collect_manuscript_review_findings(&text, focus.as_deref(), &checklist);
    let strengths = collect_positive_signals(&text);
    let critical = findings
        .iter()
        .filter(|item| item.severity == "critical")
        .count();
    let major = findings
        .iter()
        .filter(|item| item.severity == "major")
        .count();
    let minor = findings
        .iter()
        .filter(|item| item.severity == "minor")
        .count();

    let summary = if findings.is_empty() {
        "No major structural issues were detected from the available manuscript text; keep validating domain-specific rigor manually.".to_string()
    } else {
        format!(
            "Generated {} review findings (critical: {}, major: {}, minor: {}).",
            findings.len(),
            critical,
            major,
            minor
        )
    };

    ok_result(
        "review_manuscript",
        call_id,
        json!({
            "source": source_label,
            "focus": focus,
            "checklist": checklist,
            "summary": summary,
            "severityCounts": {
                "critical": critical,
                "major": major,
                "minor": minor,
            },
            "findingCount": findings.len(),
            "findings": findings.iter().map(review_finding_value).collect::<Vec<_>>(),
            "strengths": strengths,
        }),
        summary,
    )
}

pub(crate) async fn execute_check_statistics(
    project_root: &str,
    call_id: &str,
    args: Value,
    cancel_rx: Option<watch::Receiver<bool>>,
) -> AgentToolResult {
    if is_cancelled(cancel_rx.as_ref()) {
        return cancelled_result("check_statistics", call_id);
    }

    let (text, source_label) =
        match read_text_argument_or_file(project_root, &args, cancel_rx).await {
            Ok(value) => value,
            Err(message) => return error_result("check_statistics", call_id, message),
        };

    let sentences = split_sentences(&text);
    let mut findings = Vec::<ReviewFinding>::new();

    let stat_sentences = sentences
        .iter()
        .filter(|line| {
            contains_any(
                line,
                &[
                    "p ",
                    "p<",
                    "p =",
                    "ci",
                    "confidence interval",
                    "anova",
                    "t-test",
                    "regression",
                    "odds ratio",
                    "hazard ratio",
                ],
            )
        })
        .cloned()
        .collect::<Vec<_>>();
    if stat_sentences.is_empty() {
        findings.push(ReviewFinding {
            severity: "major",
            dimension: "statistics",
            message: "No explicit statistical reporting cues were detected (e.g., p-values, confidence intervals, model names).".to_string(),
            evidence: None,
            suggestion: "Report statistical tests, uncertainty intervals, and key quantitative effect estimates in the Results.".to_string(),
        });
    }

    let significance_claims_without_stats = sentences
        .iter()
        .filter(|line| {
            contains_any(
                line,
                &[
                    "significant",
                    "significantly",
                    "improved",
                    "increase",
                    "decrease",
                ],
            ) && !contains_any(line, &["p ", "p<", "p =", "ci", "confidence interval"])
        })
        .take(4)
        .cloned()
        .collect::<Vec<_>>();
    for claim in significance_claims_without_stats {
        findings.push(ReviewFinding {
            severity: "major",
            dimension: "statistics",
            message: "Potential significance claim without accompanying statistical evidence."
                .to_string(),
            evidence: Some(claim),
            suggestion:
                "Attach p-values and confidence intervals for this claim, or soften the language."
                    .to_string(),
        });
    }

    let sample_sentences = sentences
        .iter()
        .filter(|line| {
            contains_any(
                line,
                &["participants", "patients", "subjects", "samples", "cohort"],
            )
        })
        .take(4)
        .cloned()
        .collect::<Vec<_>>();
    let has_explicit_n = text.to_ascii_lowercase().contains("n=") || text.contains("n =");
    if !sample_sentences.is_empty() && !has_explicit_n {
        findings.push(ReviewFinding {
            severity: "minor",
            dimension: "statistics",
            message: "Sample descriptors are present, but explicit sample size notation (n=...) is missing.".to_string(),
            evidence: sample_sentences.first().cloned(),
            suggestion: "Add explicit sample size values near first mention of each cohort/group.".to_string(),
        });
    }

    let summary = if findings.is_empty() {
        "Statistics check passed with no obvious reporting gaps from heuristic scan.".to_string()
    } else {
        format!(
            "Statistics check found {} potential reporting issues.",
            findings.len()
        )
    };

    ok_result(
        "check_statistics",
        call_id,
        json!({
            "source": source_label,
            "summary": summary,
            "statSentenceCount": stat_sentences.len(),
            "findings": findings.iter().map(review_finding_value).collect::<Vec<_>>(),
        }),
        summary,
    )
}

pub(crate) async fn execute_verify_references(
    project_root: &str,
    call_id: &str,
    args: Value,
    cancel_rx: Option<watch::Receiver<bool>>,
) -> AgentToolResult {
    if is_cancelled(cancel_rx.as_ref()) {
        return cancelled_result("verify_references", call_id);
    }

    let (text, source_label) =
        match read_text_argument_or_file(project_root, &args, cancel_rx).await {
            Ok(value) => value,
            Err(message) => return error_result("verify_references", call_id, message),
        };

    let author_year_count = count_author_year_markers(&text);
    let bracket_count = count_bracket_markers(&text);
    let latex_count = count_substring(&text, "\\cite{");
    let styles_in_use = [author_year_count > 0, bracket_count > 0, latex_count > 0]
        .iter()
        .filter(|present| **present)
        .count();

    let mut findings = Vec::<ReviewFinding>::new();
    if styles_in_use > 1 {
        findings.push(ReviewFinding {
            severity: "major",
            dimension: "references",
            message: "Multiple citation marker styles detected in the same manuscript.".to_string(),
            evidence: Some(format!(
                "author-year={}, bracket={}, latex={}",
                author_year_count, bracket_count, latex_count
            )),
            suggestion:
                "Normalize all citation markers to a single target style before submission."
                    .to_string(),
        });
    }

    let unsupported_claims = split_sentences(&text)
        .into_iter()
        .filter(|line| {
            contains_any(
                line,
                &["previous studies", "the literature", "reported", "as shown"],
            ) && !looks_like_cited_sentence(line)
        })
        .take(4)
        .collect::<Vec<_>>();
    for claim in unsupported_claims {
        findings.push(ReviewFinding {
            severity: "minor",
            dimension: "references",
            message: "Narrative claim appears without nearby citation marker.".to_string(),
            evidence: Some(claim),
            suggestion: "Attach a supporting citation to this statement or rewrite as an uncited hypothesis.".to_string(),
        });
    }

    let summary = if findings.is_empty() {
        "Reference style appears internally consistent from heuristic scan.".to_string()
    } else {
        format!(
            "Reference verification surfaced {} potential citation issues.",
            findings.len()
        )
    };

    ok_result(
        "verify_references",
        call_id,
        json!({
            "source": source_label,
            "summary": summary,
            "citationMarkers": {
                "authorYear": author_year_count,
                "bracketNumeric": bracket_count,
                "latexCite": latex_count,
            },
            "findings": findings.iter().map(review_finding_value).collect::<Vec<_>>(),
        }),
        summary,
    )
}

pub(crate) async fn execute_generate_response_letter(
    call_id: &str,
    args: Value,
    cancel_rx: Option<watch::Receiver<bool>>,
) -> AgentToolResult {
    if is_cancelled(cancel_rx.as_ref()) {
        return cancelled_result("generate_response_letter", call_id);
    }

    let reviewer_comments = parse_string_list_arg(&args, "reviewer_comments");
    if reviewer_comments.is_empty() {
        return error_result(
            "generate_response_letter",
            call_id,
            "reviewer_comments must include at least one comment.".to_string(),
        );
    }

    let revision_plan = tool_arg_optional_string(&args, "revision_plan");
    let tone =
        tool_arg_optional_string(&args, "tone").unwrap_or_else(|| "professional".to_string());
    let mut response_points = Vec::<Value>::new();

    for (idx, comment) in reviewer_comments.iter().enumerate() {
        let item = idx + 1;
        response_points.push(json!({
            "id": format!("R{}", item),
            "reviewerComment": comment,
            "response": format!(
                "We thank the reviewer for this comment and have revised the manuscript to address point {} with explicit changes and clarified rationale.",
                item
            ),
            "action": format!(
                "Update the relevant section and add an explicit tracked-change note for comment {}.",
                item
            ),
        }));
    }

    let mut letter = String::new();
    letter.push_str("Dear Editor and Reviewers,\n\n");
    letter.push_str(
        "We appreciate the thoughtful and constructive feedback. Below we provide a point-by-point response and summarize the corresponding revisions.\n\n",
    );
    if let Some(plan) = revision_plan.as_deref() {
        letter.push_str("Revision overview:\n");
        letter.push_str(plan);
        if !plan.ends_with('\n') {
            letter.push('\n');
        }
        letter.push('\n');
    }
    for point in &response_points {
        let id = point.get("id").and_then(Value::as_str).unwrap_or("R?");
        let comment = point
            .get("reviewerComment")
            .and_then(Value::as_str)
            .unwrap_or("");
        let response = point.get("response").and_then(Value::as_str).unwrap_or("");
        letter.push_str(&format!(
            "{}\nReviewer comment: {}\nResponse: {}\n\n",
            id, comment, response
        ));
    }
    letter.push_str("Sincerely,\nThe Authors");

    let summary = format!(
        "Generated response letter draft for {} reviewer comments.",
        reviewer_comments.len()
    );

    ok_result(
        "generate_response_letter",
        call_id,
        json!({
            "tone": tone,
            "commentCount": reviewer_comments.len(),
            "responsePoints": response_points,
            "letter": letter,
            "summary": summary,
        }),
        summary,
    )
}

pub(crate) async fn execute_track_revisions(
    project_root: &str,
    call_id: &str,
    args: Value,
    cancel_rx: Option<watch::Receiver<bool>>,
) -> AgentToolResult {
    if is_cancelled(cancel_rx.as_ref()) {
        return cancelled_result("track_revisions", call_id);
    }

    let old_text =
        match read_text_from_text_or_path(project_root, &args, "old_text", "old_path").await {
            Ok(value) => value,
            Err(message) => return error_result("track_revisions", call_id, message),
        };
    let new_text =
        match read_text_from_text_or_path(project_root, &args, "new_text", "new_path").await {
            Ok(value) => value,
            Err(message) => return error_result("track_revisions", call_id, message),
        };

    let old_lines = split_lines(&old_text);
    let new_lines = split_lines(&new_text);
    let shared = old_lines
        .iter()
        .zip(new_lines.iter())
        .take_while(|(old, new)| old == new)
        .count();
    let changed_line_window = shared + 1;
    let changed_lines = naive_changed_line_count(&old_lines, &new_lines);
    let old_words = count_words(&old_text);
    let new_words = count_words(&new_text);
    let delta_words = new_words as isize - old_words as isize;

    let summary = format!(
        "Tracked revisions: {} changed lines, word delta {} ({} -> {}).",
        changed_lines, delta_words, old_words, new_words
    );

    ok_result(
        "track_revisions",
        call_id,
        json!({
            "summary": summary,
            "changedLineCount": changed_lines,
            "firstChangedLine": changed_line_window,
            "oldWordCount": old_words,
            "newWordCount": new_words,
            "deltaWordCount": delta_words,
        }),
        summary,
    )
}

fn review_finding_value(item: &ReviewFinding) -> Value {
    json!({
        "severity": item.severity,
        "dimension": item.dimension,
        "message": item.message,
        "evidence": item.evidence,
        "suggestion": item.suggestion,
    })
}

fn collect_manuscript_review_findings(
    text: &str,
    focus: Option<&str>,
    checklist: &[String],
) -> Vec<ReviewFinding> {
    let mut findings = Vec::<ReviewFinding>::new();
    let lower = text.to_ascii_lowercase();
    let sentences = split_sentences(text);

    if !contains_any(&lower, &["objective", "aim", "purpose", "hypothesis"]) {
        findings.push(ReviewFinding {
            severity: "major",
            dimension: "scientific_rigor",
            message:
                "The manuscript does not clearly state an objective/hypothesis in extracted text."
                    .to_string(),
            evidence: None,
            suggestion: "Add a clear objective statement near the end of the Introduction."
                .to_string(),
        });
    }

    if !contains_any(
        &lower,
        &["method", "protocol", "randomized", "study design", "cohort"],
    ) {
        findings.push(ReviewFinding {
            severity: "critical",
            dimension: "completeness",
            message: "Methods description appears incomplete or absent in extracted text.".to_string(),
            evidence: None,
            suggestion: "Expand Methods with design, participants/samples, interventions, and analysis details.".to_string(),
        });
    }

    if !contains_any(&lower, &["limitation", "limitations", "weakness", "bias"]) {
        findings.push(ReviewFinding {
            severity: "minor",
            dimension: "clarity",
            message: "No explicit limitations language was detected.".to_string(),
            evidence: None,
            suggestion: "Add a dedicated limitations paragraph to contextualize interpretation."
                .to_string(),
        });
    }

    if contains_any(&lower, &["significant", "improved", "better", "effective"])
        && !contains_any(&lower, &["p ", "p<", "p =", "confidence interval", "ci"])
    {
        findings.push(ReviewFinding {
            severity: "major",
            dimension: "scientific_rigor",
            message:
                "Strong claims are present without obvious quantitative uncertainty reporting."
                    .to_string(),
            evidence: sentences
                .iter()
                .find(|line| contains_any(line, &["significant", "improved", "effective"]))
                .cloned(),
            suggestion: "Report quantitative effect sizes with uncertainty and statistical tests."
                .to_string(),
        });
    }

    if let Some(request_focus) = focus {
        findings.push(ReviewFinding {
            severity: "minor",
            dimension: "scope",
            message: "Focus-aware review note generated for the requested emphasis.".to_string(),
            evidence: Some(request_focus.to_string()),
            suggestion:
                "Prioritize revision items that directly impact the requested focus question."
                    .to_string(),
        });
    }

    if checklist
        .iter()
        .any(|item| item.eq_ignore_ascii_case("consort"))
        && !contains_any(&lower, &["randomized", "allocation", "blinded"])
    {
        findings.push(ReviewFinding {
            severity: "major",
            dimension: "ethics_reporting",
            message: "CONSORT-oriented checklist requested, but trial allocation/blinding cues are unclear.".to_string(),
            evidence: None,
            suggestion: "Add CONSORT-aligned reporting details (allocation, masking, participant flow).".to_string(),
        });
    }

    findings
}

fn collect_positive_signals(text: &str) -> Vec<String> {
    let mut strengths = Vec::<String>::new();
    if contains_any(text, &["objective", "aim", "purpose"]) {
        strengths.push("Objective framing appears present.".to_string());
    }
    if contains_any(text, &["method", "protocol", "sample", "cohort"]) {
        strengths.push("Methods-related descriptors are present.".to_string());
    }
    if contains_any(text, &["limitation", "limitations"]) {
        strengths.push("The manuscript acknowledges limitations.".to_string());
    }
    if strengths.is_empty() {
        strengths.push("No strong structure cues detected from heuristic scan.".to_string());
    }
    strengths
}

fn parse_string_list_arg(args: &Value, key: &str) -> Vec<String> {
    if let Some(items) = args.get(key).and_then(Value::as_array) {
        return items
            .iter()
            .filter_map(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
            .collect();
    }

    if let Some(raw) = args.get(key).and_then(Value::as_str) {
        return raw
            .lines()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
            .collect();
    }

    Vec::new()
}

async fn read_text_argument_or_file(
    project_root: &str,
    args: &Value,
    cancel_rx: Option<watch::Receiver<bool>>,
) -> Result<(String, String), String> {
    if let Some(text) = tool_arg_optional_string(args, "text") {
        return Ok((text, "inline_text".to_string()));
    }

    let path = tool_arg_string(args, "path")?;
    if is_document_resource_path(&path) {
        let runtime = load_document_runtime_content(project_root, &path, cancel_rx).await?;
        let content = if runtime.searchable_text.trim().is_empty() {
            runtime.excerpt
        } else {
            runtime.searchable_text
        };
        return Ok((content, format!("document:{}", path)));
    }

    let full_path = resolve_project_path(project_root, &path)?;
    let text = tokio::fs::read_to_string(&full_path)
        .await
        .map_err(|err| format!("Unable to read {}: {}", path, err))?;
    Ok((text, format!("file:{}", path)))
}

async fn read_text_from_text_or_path(
    project_root: &str,
    args: &Value,
    text_key: &str,
    path_key: &str,
) -> Result<String, String> {
    if let Some(text) = tool_arg_optional_string(args, text_key) {
        return Ok(text);
    }
    let path = tool_arg_string(args, path_key)?;
    let full_path = resolve_project_path(project_root, &path)?;
    tokio::fs::read_to_string(&full_path)
        .await
        .map_err(|err| format!("Unable to read {}: {}", path, err))
}

fn split_sentences(text: &str) -> Vec<String> {
    text.replace('\n', " ")
        .split(['.', '!', '?'])
        .map(str::trim)
        .filter(|line| line.len() >= 20)
        .map(str::to_string)
        .collect()
}

fn split_lines(text: &str) -> Vec<String> {
    text.lines().map(str::to_string).collect()
}

fn contains_any(text: &str, patterns: &[&str]) -> bool {
    let lower = text.to_ascii_lowercase();
    patterns
        .iter()
        .any(|pattern| lower.contains(&pattern.to_ascii_lowercase()))
}

fn looks_like_cited_sentence(line: &str) -> bool {
    line.contains('(') && line.contains(')')
        || line.contains('[') && line.contains(']')
        || line.contains("\\cite{")
}

fn count_substring(text: &str, needle: &str) -> usize {
    text.match_indices(needle).count()
}

fn count_author_year_markers(text: &str) -> usize {
    text.lines()
        .filter(|line| {
            let l = line.trim();
            l.contains('(')
                && l.contains(')')
                && l.chars().any(|ch| ch.is_ascii_digit())
                && l.contains(',')
        })
        .count()
}

fn count_bracket_markers(text: &str) -> usize {
    text.lines()
        .map(|line| {
            let bytes = line.as_bytes();
            let mut count = 0usize;
            let mut idx = 0usize;
            while idx < bytes.len() {
                if bytes[idx] == b'[' {
                    let mut j = idx + 1;
                    let mut digit_count = 0usize;
                    while j < bytes.len() && bytes[j].is_ascii_digit() {
                        digit_count += 1;
                        j += 1;
                    }
                    if digit_count > 0 && j < bytes.len() && bytes[j] == b']' {
                        count += 1;
                        idx = j;
                    }
                }
                idx += 1;
            }
            count
        })
        .sum()
}

fn count_words(text: &str) -> usize {
    text.split_whitespace()
        .filter(|token| !token.is_empty())
        .count()
}

fn naive_changed_line_count(old_lines: &[String], new_lines: &[String]) -> usize {
    let max_len = old_lines.len().max(new_lines.len());
    let mut changed = 0usize;
    for idx in 0..max_len {
        if old_lines.get(idx) != new_lines.get(idx) {
            changed += 1;
        }
    }
    changed
}
