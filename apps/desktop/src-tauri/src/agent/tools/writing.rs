use std::collections::{BTreeSet, HashMap};
use std::sync::OnceLock;

use serde::Deserialize;
use serde_json::{Value, json};
use tokio::sync::watch;

use super::{
    AgentToolResult, cancelled_result, error_result, is_cancelled, ok_result, resolve_project_path,
    tool_arg_optional_string, tool_arg_optional_usize, tool_arg_string,
};

pub(crate) async fn execute_draft_section(
    call_id: &str,
    args: Value,
    cancel_rx: Option<watch::Receiver<bool>>,
) -> AgentToolResult {
    if is_cancelled(cancel_rx.as_ref()) {
        return cancelled_result("draft_section", call_id);
    }

    let section_type = match tool_arg_string(&args, "section_type") {
        Ok(value) => value,
        Err(message) => return error_result("draft_section", call_id, message),
    };
    let key_points = parse_string_list_arg(&args, "key_points");
    if key_points.is_empty() {
        return error_result(
            "draft_section",
            call_id,
            "key_points must contain at least one non-empty item".to_string(),
        );
    }

    let tone = tool_arg_optional_string(&args, "tone").unwrap_or_else(|| "formal".to_string());
    let target_words = tool_arg_optional_usize(&args, "target_words")
        .unwrap_or(320)
        .clamp(120, 1400);
    let citation_keys = parse_string_list_arg(&args, "citation_keys");
    let output_format =
        normalize_output_format(tool_arg_optional_string(&args, "output_format").as_deref());
    let default_citation_style = citation_style_for_output_format(output_format);

    let mut paragraphs = Vec::new();
    for (idx, point) in key_points.iter().enumerate() {
        let lead = match idx {
            0 => "First",
            1 => "Additionally",
            2 => "Furthermore",
            _ => "Moreover",
        };
        let mut sentence = ensure_sentence(point);
        sentence = sentence.replace("  ", " ");
        let citation_tail = citation_keys
            .get(idx)
            .or_else(|| citation_keys.first())
            .map(|key| citation_marker(default_citation_style, key))
            .unwrap_or_default();

        let paragraph = if citation_tail.is_empty() {
            format!("{}{},", lead, if idx == 0 { "" } else { " " }) + &sentence.to_lowercase()
        } else {
            format!(
                "{}{}, {} {}",
                lead,
                if idx == 0 { "" } else { " " },
                sentence.to_lowercase(),
                citation_tail
            )
        };
        paragraphs.push(capitalize_first(paragraph.trim()));
    }

    paragraphs.push(match section_type.to_ascii_lowercase().as_str() {
        "introduction" => "Together, these points establish the scientific context and motivate the central objective of the study.".to_string(),
        "methods" => "This description is intended to be reproducible and should be refined with exact protocol parameters before submission.".to_string(),
        "results" => "The results are presented descriptively; causal interpretation should be deferred to the Discussion section.".to_string(),
        "discussion" => "These observations should be interpreted in light of prior evidence, limitations, and translational relevance.".to_string(),
        _ => "This draft should be refined with precise data points and citations from the project evidence base.".to_string(),
    });

    let heading = section_heading(&section_type, output_format);
    let draft_body = paragraphs.join("\n\n");
    let combined = if heading.is_empty() {
        draft_body
    } else {
        format!("{}\n\n{}", heading, draft_body)
    };
    let (draft, truncated) = trim_to_word_limit(&combined, target_words);

    let estimated_words = count_words(&draft);
    let mut quality_notes = vec![
        format!("Tone target applied: {}.", tone),
        "Uses cautious scientific language and avoids causal over-claiming by default.".to_string(),
    ];
    if citation_keys.is_empty() {
        quality_notes.push(
            "No citation keys were provided; add evidence-backed citations before finalizing."
                .to_string(),
        );
    }
    if truncated {
        quality_notes.push(format!(
            "Draft was trimmed to respect target_words={}.",
            target_words
        ));
    }

    ok_result(
        "draft_section",
        call_id,
        json!({
            "sectionType": section_type,
            "tone": tone,
            "targetWords": target_words,
            "estimatedWords": estimated_words,
            "outputFormat": output_format_label(output_format),
            "citationKeysUsed": citation_keys,
            "draft": draft,
            "qualityNotes": quality_notes,
        }),
        format!(
            "Drafted {} section (~{} words).",
            section_type, estimated_words
        ),
    )
}

pub(crate) async fn execute_restructure_outline(
    call_id: &str,
    args: Value,
    cancel_rx: Option<watch::Receiver<bool>>,
) -> AgentToolResult {
    if is_cancelled(cancel_rx.as_ref()) {
        return cancelled_result("restructure_outline", call_id);
    }

    let mut input_sections = parse_string_list_arg(&args, "sections");
    if input_sections.is_empty() {
        let outline = tool_arg_optional_string(&args, "outline").unwrap_or_default();
        input_sections = parse_outline_lines(&outline);
    }
    if input_sections.is_empty() {
        return error_result(
            "restructure_outline",
            call_id,
            "Provide at least one section via sections[] or outline.".to_string(),
        );
    }

    let manuscript_type = tool_arg_optional_string(&args, "manuscript_type")
        .unwrap_or_else(|| "imrad".to_string())
        .to_ascii_lowercase();

    let selected_template = select_outline_template(&manuscript_type);
    let template_sections = selected_template
        .map(template_section_labels)
        .unwrap_or_else(|| fallback_template_sections(&manuscript_type));
    let mut used = BTreeSet::<usize>::new();
    let mut revised = Vec::<Value>::new();

    for canonical in &template_sections {
        if let Some((idx, original)) = input_sections
            .iter()
            .enumerate()
            .find(|(_, section)| canonical_key(section) == canonical_key(canonical))
        {
            used.insert(idx);
            revised.push(json!({
                "section": original,
                "source": "existing",
                "rationale": section_rationale(canonical),
                "templateGuidance": template_guidance(selected_template, canonical),
                "wordTarget": template_word_target(selected_template, canonical),
            }));
        } else {
            revised.push(json!({
                "section": canonical,
                "source": "added",
                "rationale": section_rationale(canonical),
                "templateGuidance": template_guidance(selected_template, canonical),
                "wordTarget": template_word_target(selected_template, canonical),
            }));
        }
    }

    for (idx, section) in input_sections.iter().enumerate() {
        if used.contains(&idx) {
            continue;
        }
        revised.push(json!({
            "section": section,
            "source": "carried_over",
            "rationale": "Retained from the original outline as a domain-specific or supplementary section."
        }));
    }

    let section_count = revised.len();
    let added_count = revised
        .iter()
        .filter(|item| item.get("source") == Some(&Value::String("added".to_string())))
        .count();

    ok_result(
        "restructure_outline",
        call_id,
        json!({
            "manuscriptType": manuscript_type,
            "templateId": selected_template.map(|template| template.id.clone()),
            "templateName": selected_template.map(|template| template.name.clone()),
            "originalSections": input_sections,
            "revisedOutline": revised,
            "addedSectionCount": added_count,
        }),
        format!(
            "Restructured outline into {} sections ({} added).",
            section_count, added_count
        ),
    )
}

pub(crate) async fn execute_check_consistency(
    project_root: &str,
    call_id: &str,
    args: Value,
    cancel_rx: Option<watch::Receiver<bool>>,
) -> AgentToolResult {
    if is_cancelled(cancel_rx.as_ref()) {
        return cancelled_result("check_consistency", call_id);
    }

    let (text, source_label) = match read_text_argument_or_file(project_root, &args).await {
        Ok(value) => value,
        Err(message) => return error_result("check_consistency", call_id, message),
    };

    let mut findings = Vec::<Value>::new();
    findings.extend(check_todo_placeholders(&text));
    findings.extend(check_abbreviation_definitions(&text));
    findings.extend(check_terminology_variants(&text));
    findings.extend(check_citation_marker_mixing(&text));
    findings.extend(check_numbering_sequence(&text, "Figure"));
    findings.extend(check_numbering_sequence(&text, "Table"));

    let major = findings
        .iter()
        .filter(|f| f.get("severity") == Some(&Value::String("major".to_string())))
        .count();
    let minor = findings
        .iter()
        .filter(|f| f.get("severity") == Some(&Value::String("minor".to_string())))
        .count();

    let summary = if findings.is_empty() {
        "No obvious consistency issues were detected in the provided text.".to_string()
    } else {
        format!(
            "Found {} consistency issues (major: {}, minor: {}).",
            findings.len(),
            major,
            minor
        )
    };

    ok_result(
        "check_consistency",
        call_id,
        json!({
            "source": source_label,
            "summary": summary,
            "findingCount": findings.len(),
            "findings": findings,
        }),
        summary,
    )
}

pub(crate) async fn execute_generate_abstract(
    project_root: &str,
    call_id: &str,
    args: Value,
    cancel_rx: Option<watch::Receiver<bool>>,
) -> AgentToolResult {
    if is_cancelled(cancel_rx.as_ref()) {
        return cancelled_result("generate_abstract", call_id);
    }

    let (text, source_label) = match read_text_argument_or_file(project_root, &args).await {
        Ok(value) => value,
        Err(message) => return error_result("generate_abstract", call_id, message),
    };

    let structured = args
        .get("structured")
        .and_then(Value::as_bool)
        .unwrap_or(true);
    let word_limit = tool_arg_optional_usize(&args, "word_limit")
        .unwrap_or(250)
        .clamp(40, 450);

    let sentences = split_sentences(&text);
    if sentences.is_empty() {
        return error_result(
            "generate_abstract",
            call_id,
            "No usable text was found for abstract generation.".to_string(),
        );
    }

    let raw = if structured {
        build_structured_abstract(&sentences)
    } else {
        sentences
            .iter()
            .take(6)
            .cloned()
            .collect::<Vec<_>>()
            .join(" ")
    };

    let (abstract_text, truncated) = trim_to_word_limit(&raw, word_limit);
    let words = count_words(&abstract_text);

    ok_result(
        "generate_abstract",
        call_id,
        json!({
            "source": source_label,
            "structured": structured,
            "wordLimit": word_limit,
            "wordCount": words,
            "truncated": truncated,
            "abstract": abstract_text,
        }),
        format!("Generated abstract ({} words).", words),
    )
}

pub(crate) async fn execute_insert_citation(
    call_id: &str,
    args: Value,
    cancel_rx: Option<watch::Receiver<bool>>,
) -> AgentToolResult {
    if is_cancelled(cancel_rx.as_ref()) {
        return cancelled_result("insert_citation", call_id);
    }

    let text = match tool_arg_string(&args, "text") {
        Ok(value) => value,
        Err(message) => return error_result("insert_citation", call_id, message),
    };

    let citation_key = tool_arg_optional_string(&args, "citation_key").or_else(|| {
        parse_string_list_arg(&args, "citation_keys")
            .first()
            .cloned()
    });
    let Some(citation_key) = citation_key else {
        return error_result(
            "insert_citation",
            call_id,
            "Provide citation_key or citation_keys with at least one value.".to_string(),
        );
    };

    let style = normalize_citation_style(tool_arg_optional_string(&args, "style").as_deref());
    let placement = tool_arg_optional_string(&args, "placement")
        .unwrap_or_else(|| "sentence_end".to_string())
        .to_ascii_lowercase();
    let dedupe = args.get("dedupe").and_then(Value::as_bool).unwrap_or(true);
    let marker = citation_marker(style, &citation_key);

    let already_present = text.contains(&marker);
    let updated_text = if dedupe && already_present {
        text.clone()
    } else if placement == "append" {
        format!("{} {}", text.trim_end(), marker)
    } else {
        insert_marker_before_terminal_punctuation(&text, &marker)
    };

    let inserted = !(dedupe && already_present);
    let summary = if inserted {
        format!("Inserted citation marker {}.", marker)
    } else {
        format!(
            "Citation marker {} already present; skipped duplicate insert.",
            marker
        )
    };

    ok_result(
        "insert_citation",
        call_id,
        json!({
            "citationKey": citation_key,
            "style": citation_style_label(style),
            "marker": marker,
            "inserted": inserted,
            "text": updated_text,
            "summary": summary,
        }),
        summary,
    )
}

#[derive(Clone, Copy)]
enum WritingOutputFormat {
    Markdown,
    Latex,
    Plain,
}

#[derive(Clone, Copy)]
enum CitationStyle {
    Latex,
    Markdown,
    Vancouver,
}

#[derive(Debug, Clone, Deserialize)]
struct WritingTemplateFile {
    id: String,
    name: String,
    #[serde(default)]
    aliases: Vec<String>,
    #[serde(default)]
    sections: Vec<WritingTemplateSection>,
}

#[derive(Debug, Clone, Deserialize)]
struct WritingTemplateSection {
    id: String,
    label: String,
    guidance: Option<String>,
    word_target: Option<u16>,
}

const IMRAD_TEMPLATE_JSON: &str = include_str!("../templates/imrad_standard.json");
const REVIEW_TEMPLATE_JSON: &str = include_str!("../templates/review_article.json");
const CASE_REPORT_TEMPLATE_JSON: &str = include_str!("../templates/case_report.json");
const METHODS_TEMPLATE_JSON: &str = include_str!("../templates/methods_paper.json");

static WRITING_TEMPLATES: OnceLock<Vec<WritingTemplateFile>> = OnceLock::new();

fn normalize_output_format(raw: Option<&str>) -> WritingOutputFormat {
    match raw.unwrap_or_default().trim().to_ascii_lowercase().as_str() {
        "latex" => WritingOutputFormat::Latex,
        "plain" | "text" => WritingOutputFormat::Plain,
        _ => WritingOutputFormat::Markdown,
    }
}

fn normalize_citation_style(raw: Option<&str>) -> CitationStyle {
    match raw.unwrap_or_default().trim().to_ascii_lowercase().as_str() {
        "markdown" => CitationStyle::Markdown,
        "vancouver" | "numbered" => CitationStyle::Vancouver,
        _ => CitationStyle::Latex,
    }
}

fn citation_style_for_output_format(format: WritingOutputFormat) -> CitationStyle {
    match format {
        WritingOutputFormat::Latex => CitationStyle::Latex,
        WritingOutputFormat::Markdown => CitationStyle::Markdown,
        WritingOutputFormat::Plain => CitationStyle::Latex,
    }
}

fn output_format_label(format: WritingOutputFormat) -> &'static str {
    match format {
        WritingOutputFormat::Markdown => "markdown",
        WritingOutputFormat::Latex => "latex",
        WritingOutputFormat::Plain => "plain",
    }
}

fn citation_style_label(style: CitationStyle) -> &'static str {
    match style {
        CitationStyle::Latex => "latex",
        CitationStyle::Markdown => "markdown",
        CitationStyle::Vancouver => "vancouver",
    }
}

fn citation_marker(style: CitationStyle, key: &str) -> String {
    match style {
        CitationStyle::Latex => format!("\\cite{{{}}}", key),
        CitationStyle::Markdown => format!("[@{}]", key),
        CitationStyle::Vancouver => {
            if key.chars().all(|c| c.is_ascii_digit()) {
                format!("[{}]", key)
            } else {
                format!("[ref:{}]", key)
            }
        }
    }
}

fn section_heading(section_type: &str, format: WritingOutputFormat) -> String {
    let normalized = capitalize_first(section_type.trim());
    match format {
        WritingOutputFormat::Markdown => format!("## {}", normalized),
        WritingOutputFormat::Latex => format!("\\subsection*{{{}}}", normalized),
        WritingOutputFormat::Plain => String::new(),
    }
}

fn parse_string_list_arg(args: &Value, key: &str) -> Vec<String> {
    match args.get(key) {
        Some(Value::Array(values)) => values
            .iter()
            .filter_map(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
            .collect::<Vec<_>>(),
        Some(Value::String(text)) => text
            .split('\n')
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .map(|line| line.trim_start_matches('-').trim().to_string())
            .collect::<Vec<_>>(),
        _ => Vec::new(),
    }
}

fn parse_outline_lines(outline: &str) -> Vec<String> {
    outline
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(|line| {
            line.trim_start_matches('#')
                .trim_start_matches('-')
                .trim_start_matches('*')
                .trim()
                .to_string()
        })
        .filter(|line| !line.is_empty())
        .collect()
}

fn ensure_sentence(value: &str) -> String {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    if trimmed.ends_with('.') || trimmed.ends_with('!') || trimmed.ends_with('?') {
        trimmed.to_string()
    } else {
        format!("{}.", trimmed)
    }
}

fn capitalize_first(value: &str) -> String {
    let mut chars = value.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

fn count_words(text: &str) -> usize {
    text.split_whitespace().count()
}

fn trim_to_word_limit(text: &str, limit: usize) -> (String, bool) {
    let words = text.split_whitespace().collect::<Vec<_>>();
    if words.len() <= limit {
        return (text.trim().to_string(), false);
    }

    let trimmed = words[..limit].join(" ");
    let final_text = if trimmed.ends_with('.') {
        trimmed
    } else {
        format!("{}...", trimmed)
    };
    (final_text, true)
}

fn fallback_template_sections(manuscript_type: &str) -> Vec<String> {
    match manuscript_type {
        "review" => vec![
            "Title",
            "Abstract",
            "Introduction",
            "Thematic Evidence Synthesis",
            "Research Gaps",
            "Conclusion",
            "References",
        ]
        .into_iter()
        .map(str::to_string)
        .collect::<Vec<_>>(),
        "case_report" => vec![
            "Title",
            "Abstract",
            "Case Presentation",
            "Diagnostic Assessment",
            "Intervention and Outcome",
            "Discussion",
            "References",
        ]
        .into_iter()
        .map(str::to_string)
        .collect::<Vec<_>>(),
        "methods" => vec![
            "Title",
            "Abstract",
            "Introduction",
            "Methods",
            "Validation",
            "Limitations",
            "Conclusion",
            "References",
        ]
        .into_iter()
        .map(str::to_string)
        .collect::<Vec<_>>(),
        _ => vec![
            "Title",
            "Abstract",
            "Introduction",
            "Methods",
            "Results",
            "Discussion",
            "Conclusion",
            "References",
        ]
        .into_iter()
        .map(str::to_string)
        .collect::<Vec<_>>(),
    }
}

fn writing_templates() -> &'static [WritingTemplateFile] {
    WRITING_TEMPLATES.get_or_init(|| {
        [
            IMRAD_TEMPLATE_JSON,
            REVIEW_TEMPLATE_JSON,
            CASE_REPORT_TEMPLATE_JSON,
            METHODS_TEMPLATE_JSON,
        ]
        .iter()
        .filter_map(|raw| serde_json::from_str::<WritingTemplateFile>(raw).ok())
        .collect::<Vec<_>>()
    })
}

fn select_outline_template(manuscript_type: &str) -> Option<&'static WritingTemplateFile> {
    let normalized = manuscript_type.trim().to_ascii_lowercase();
    writing_templates().iter().find(|template| {
        template.id.eq_ignore_ascii_case(&normalized)
            || template
                .aliases
                .iter()
                .any(|alias| alias.eq_ignore_ascii_case(&normalized))
    })
}

fn template_section_labels(template: &WritingTemplateFile) -> Vec<String> {
    template
        .sections
        .iter()
        .map(|section| section.label.trim())
        .filter(|label| !label.is_empty())
        .map(str::to_string)
        .collect::<Vec<_>>()
}

fn template_section_for<'a>(
    template: &'a WritingTemplateFile,
    section_label: &str,
) -> Option<&'a WritingTemplateSection> {
    let key = canonical_key(section_label);
    template.sections.iter().find(|section| {
        canonical_key(&section.label) == key
            || canonical_key(&section.id) == key
            || section.id.eq_ignore_ascii_case(section_label)
    })
}

fn template_guidance(
    template: Option<&WritingTemplateFile>,
    section_label: &str,
) -> Option<String> {
    template.and_then(|template| {
        template_section_for(template, section_label).and_then(|section| {
            section
                .guidance
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string)
        })
    })
}

fn template_word_target(
    template: Option<&WritingTemplateFile>,
    section_label: &str,
) -> Option<u16> {
    template
        .and_then(|template| template_section_for(template, section_label))
        .and_then(|section| section.word_target)
}

fn canonical_key(value: &str) -> String {
    let lower = value.to_ascii_lowercase();
    if lower.contains("intro") {
        "introduction".to_string()
    } else if lower.contains("method") {
        "methods".to_string()
    } else if lower.contains("result") {
        "results".to_string()
    } else if lower.contains("discuss") {
        "discussion".to_string()
    } else if lower.contains("conclu") {
        "conclusion".to_string()
    } else if lower.contains("abstract") {
        "abstract".to_string()
    } else if lower.contains("reference") || lower.contains("bibliograph") {
        "references".to_string()
    } else if lower.contains("title") {
        "title".to_string()
    } else {
        lower
            .split_whitespace()
            .take(3)
            .collect::<Vec<_>>()
            .join(" ")
    }
}

fn section_rationale(section: &str) -> &'static str {
    match canonical_key(section).as_str() {
        "title" => "Sets scope and primary contribution clearly.",
        "abstract" => "Provides concise background, approach, findings, and implication.",
        "introduction" => "Frames background, knowledge gap, and objective.",
        "methods" => "Ensures reproducibility and transparent procedural detail.",
        "results" => "Reports observed outcomes without over-interpretation.",
        "discussion" => "Interprets findings, limitations, and implications.",
        "conclusion" => "Closes with take-home contribution and future direction.",
        "references" => "Documents evidence sources and traceability.",
        _ => "Maintains domain-specific context from the original outline.",
    }
}

async fn read_text_argument_or_file(
    project_root: &str,
    args: &Value,
) -> Result<(String, String), String> {
    if let Some(text) = tool_arg_optional_string(args, "text") {
        return Ok((text, "inline_text".to_string()));
    }

    let path = tool_arg_optional_string(args, "path")
        .ok_or_else(|| "Provide text or path for this tool.".to_string())?;
    let full_path = resolve_project_path(project_root, &path)?;
    let content = tokio::fs::read_to_string(full_path)
        .await
        .map_err(|err| format!("Failed to read {}: {}", path, err))?;
    Ok((content, path))
}

fn check_todo_placeholders(text: &str) -> Vec<Value> {
    let markers = ["TODO", "TBD", "XXX", "??"];
    markers
        .iter()
        .filter_map(|marker| {
            line_number_hint(text, marker).map(|line| {
                json!({
                    "severity": "major",
                    "category": "placeholder",
                    "message": format!("Found placeholder marker '{}'.", marker),
                    "locationHint": format!("line {}", line)
                })
            })
        })
        .collect()
}

fn check_abbreviation_definitions(text: &str) -> Vec<Value> {
    let mut counts = HashMap::<String, usize>::new();
    for token in text.split(|ch: char| !ch.is_ascii_alphanumeric()) {
        if is_abbreviation_token(token) {
            *counts.entry(token.to_string()).or_insert(0) += 1;
        }
    }

    counts
        .into_iter()
        .filter(|(_, count)| *count >= 2)
        .filter(|(abbr, _)| {
            let plain_define = format!("{} (", abbr);
            let reverse_define = format!("({})", abbr);
            !(text.contains(&plain_define) || text.contains(&reverse_define))
        })
        .map(|(abbr, _)| {
            json!({
                "severity": "major",
                "category": "abbreviation",
                "message": format!("Abbreviation '{}' appears multiple times without an explicit first-definition.", abbr),
                "locationHint": line_number_hint(text, &abbr).map(|line| format!("line {}", line)).unwrap_or_else(|| "unknown".to_string())
            })
        })
        .collect()
}

fn is_abbreviation_token(token: &str) -> bool {
    let chars = token.chars().collect::<Vec<_>>();
    if chars.len() < 2 || chars.len() > 8 {
        return false;
    }
    let mut alpha_count = 0usize;
    for ch in chars {
        if ch.is_ascii_uppercase() {
            alpha_count += 1;
            continue;
        }
        if ch.is_ascii_digit() {
            continue;
        }
        return false;
    }
    alpha_count >= 2
}

fn check_terminology_variants(text: &str) -> Vec<Value> {
    let pairs = [
        ("in vitro", "in-vitro"),
        ("in vivo", "in-vivo"),
        ("data set", "dataset"),
        ("follow up", "follow-up"),
    ];

    pairs
        .iter()
        .filter(|(a, b)| {
            let lower = text.to_ascii_lowercase();
            lower.contains(a) && lower.contains(b)
        })
        .map(|(a, b)| {
            json!({
                "severity": "minor",
                "category": "terminology",
                "message": format!("Mixed terminology variants detected: '{}' and '{}'.", a, b),
                "locationHint": "global"
            })
        })
        .collect()
}

fn check_citation_marker_mixing(text: &str) -> Vec<Value> {
    let has_latex = text.contains("\\cite{");
    let has_numeric = contains_numeric_bracket_citation(text);
    if has_latex && has_numeric {
        return vec![json!({
            "severity": "minor",
            "category": "citation_style",
            "message": "Mixed citation marker styles detected (LaTeX cite commands and numeric bracket citations).",
            "locationHint": "global"
        })];
    }
    Vec::new()
}

fn contains_numeric_bracket_citation(text: &str) -> bool {
    let bytes = text.as_bytes();
    let mut i = 0usize;
    while i + 2 < bytes.len() {
        if bytes[i] == b'[' {
            let mut j = i + 1;
            while j < bytes.len() && bytes[j].is_ascii_digit() {
                j += 1;
            }
            if j > i + 1 && j < bytes.len() && bytes[j] == b']' {
                return true;
            }
        }
        i += 1;
    }
    false
}

fn check_numbering_sequence(text: &str, label: &str) -> Vec<Value> {
    let mut numbers = Vec::<usize>::new();
    let tokens = text
        .split_whitespace()
        .map(|token| token.trim_matches(|ch: char| !ch.is_ascii_alphanumeric()))
        .collect::<Vec<_>>();

    for idx in 0..tokens.len() {
        let token = tokens[idx].to_ascii_lowercase();
        let is_label = match label {
            "Figure" => token == "figure" || token == "fig" || token == "fig.",
            "Table" => token == "table",
            _ => false,
        };
        if !is_label || idx + 1 >= tokens.len() {
            continue;
        }
        let next = tokens[idx + 1]
            .trim_matches(|ch: char| !ch.is_ascii_alphanumeric())
            .to_string();
        let digits = next
            .chars()
            .take_while(|ch| ch.is_ascii_digit())
            .collect::<String>();
        if let Ok(value) = digits.parse::<usize>() {
            numbers.push(value);
        }
    }

    if numbers.len() < 2 {
        return Vec::new();
    }

    numbers.sort_unstable();
    numbers.dedup();

    let mut findings = Vec::<Value>::new();
    for pair in numbers.windows(2) {
        if let [left, right] = pair {
            if *right > *left + 1 {
                findings.push(json!({
                    "severity": "minor",
                    "category": "numbering",
                    "message": format!("{} numbering gap detected between {} and {}.", label, left, right),
                    "locationHint": "global"
                }));
            }
        }
    }
    findings
}

fn line_number_hint(text: &str, needle: &str) -> Option<usize> {
    text.lines()
        .enumerate()
        .find(|(_, line)| line.contains(needle))
        .map(|(idx, _)| idx + 1)
}

fn split_sentences(text: &str) -> Vec<String> {
    let normalized = text.replace('\n', " ");
    let mut sentences = Vec::<String>::new();
    let mut current = String::new();

    for ch in normalized.chars() {
        current.push(ch);
        if matches!(ch, '.' | '!' | '?') {
            let candidate = current.trim();
            if candidate.len() >= 20 {
                sentences.push(candidate.to_string());
            }
            current.clear();
        }
    }

    if !current.trim().is_empty() {
        let candidate = current.trim();
        if candidate.len() >= 20 {
            sentences.push(candidate.to_string());
        }
    }

    if sentences.is_empty() {
        normalized
            .split('.')
            .map(str::trim)
            .filter(|line| line.len() >= 12)
            .take(6)
            .map(ensure_sentence)
            .collect()
    } else {
        sentences
    }
}

fn build_structured_abstract(sentences: &[String]) -> String {
    let fallback = |index: usize| sentences.get(index).cloned().unwrap_or_default();
    let pick = |keywords: &[&str], fallback_index: usize| -> String {
        for sentence in sentences {
            let lower = sentence.to_ascii_lowercase();
            if keywords.iter().any(|keyword| lower.contains(keyword)) {
                return sentence.clone();
            }
        }
        fallback(fallback_index)
    };

    let background = pick(&["background", "introduction", "objective", "aim"], 0);
    let methods = pick(
        &["method", "protocol", "randomized", "assessed", "measured"],
        1,
    );
    let results = pick(
        &["result", "improved", "increase", "decrease", "significant"],
        2,
    );
    let conclusion = pick(&["conclusion", "suggest", "indicate", "implication"], 3);

    format!(
        "Background: {}\nMethods: {}\nResults: {}\nConclusions: {}",
        ensure_sentence(&background),
        ensure_sentence(&methods),
        ensure_sentence(&results),
        ensure_sentence(&conclusion)
    )
}

fn insert_marker_before_terminal_punctuation(text: &str, marker: &str) -> String {
    let trimmed = text.trim_end();
    let trailing_ws = &text[trimmed.len()..];
    let terminal = trimmed.chars().last();
    let punctuation = terminal
        .map(|ch| matches!(ch, '.' | '!' | '?' | ';' | ':'))
        .unwrap_or(false);

    if punctuation {
        let split_at = trimmed
            .char_indices()
            .last()
            .map(|(idx, _)| idx)
            .unwrap_or(0);
        let head = &trimmed[..split_at];
        let tail = &trimmed[split_at..];
        format!("{} {}{}{}", head, marker, tail, trailing_ws)
    } else {
        format!("{} {}{}", trimmed, marker, trailing_ws)
    }
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};
    use tempfile::tempdir;

    use super::{
        execute_check_consistency, execute_draft_section, execute_generate_abstract,
        execute_insert_citation, execute_restructure_outline, select_outline_template,
    };

    #[tokio::test]
    async fn draft_section_produces_text_and_metadata() {
        let result = execute_draft_section(
            "call-1",
            json!({
                "section_type": "Introduction",
                "key_points": [
                    "Glioma has poor prognosis under current standard of care",
                    "Recent work suggests immune modulation can improve response"
                ],
                "citation_keys": ["Smith2024"],
                "target_words": 120
            }),
            None,
        )
        .await;

        assert!(!result.is_error, "{:?}", result.content);
        assert_eq!(result.content["sectionType"], json!("Introduction"));
        assert!(
            result
                .content
                .get("draft")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .contains("Introduction")
        );
    }

    #[tokio::test]
    async fn insert_citation_avoids_duplicate_when_dedupe_enabled() {
        let result = execute_insert_citation(
            "call-2",
            json!({
                "text": "This effect is reproducible \\cite{Lee2023}.",
                "citation_key": "Lee2023",
                "style": "latex",
                "dedupe": true
            }),
            None,
        )
        .await;

        assert!(!result.is_error, "{:?}", result.content);
        assert_eq!(result.content["inserted"], json!(false));
    }

    #[tokio::test]
    async fn check_consistency_flags_major_placeholders() {
        let result = execute_check_consistency(
            ".",
            "call-3",
            json!({
                "text": "TODO: revise this section. MRI was used. MRI showed signal changes."
            }),
            None,
        )
        .await;

        assert!(!result.is_error, "{:?}", result.content);
        let findings = result
            .content
            .get("findings")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        assert!(!findings.is_empty());
        assert!(
            findings.iter().any(|entry| {
                entry.get("severity") == Some(&Value::String("major".to_string()))
            })
        );
    }

    #[tokio::test]
    async fn generate_abstract_respects_word_limit() {
        let text = [
            "Background: This study evaluates a biomarker-guided strategy for treatment response.",
            "Methods: We conducted a retrospective cohort analysis and measured progression-free survival.",
            "Results: The intervention group showed improved response rates with statistically significant separation.",
            "Conclusions: The findings support prospective validation in larger cohorts.",
        ]
        .join(" ");

        let result = execute_generate_abstract(
            ".",
            "call-4",
            json!({ "text": text, "structured": true, "word_limit": 40 }),
            None,
        )
        .await;

        assert!(!result.is_error, "{:?}", result.content);
        let count = result
            .content
            .get("wordCount")
            .and_then(Value::as_u64)
            .unwrap_or_default();
        assert!(count <= 40);
    }

    #[tokio::test]
    async fn consistency_can_read_from_path() {
        let temp = tempdir().unwrap();
        let project_root = temp.path().to_string_lossy().to_string();
        let file = temp.path().join("draft.txt");
        tokio::fs::write(&file, "Table 1 is reported, then Table 3 appears.")
            .await
            .unwrap();

        let result = execute_check_consistency(
            &project_root,
            "call-5",
            json!({ "path": "draft.txt" }),
            None,
        )
        .await;

        assert!(!result.is_error, "{:?}", result.content);
        assert_eq!(result.content["source"], json!("draft.txt"));
    }

    #[test]
    fn template_registry_contains_expected_manuscript_types() {
        assert!(select_outline_template("imrad").is_some());
        assert!(select_outline_template("review").is_some());
        assert!(select_outline_template("case_report").is_some());
        assert!(select_outline_template("methods_paper").is_some());
    }

    #[tokio::test]
    async fn restructure_outline_returns_template_metadata() {
        let result = execute_restructure_outline(
            "call-outline",
            json!({
                "manuscript_type": "review",
                "sections": ["Introduction", "Conclusion"]
            }),
            None,
        )
        .await;
        assert!(!result.is_error, "{:?}", result.content);
        assert_eq!(result.content["templateId"], json!("review_article"));
        assert!(
            result
                .content
                .get("revisedOutline")
                .and_then(Value::as_array)
                .map(|items| items
                    .iter()
                    .any(|entry| entry.get("templateGuidance").is_some()))
                .unwrap_or(false)
        );
    }
}
