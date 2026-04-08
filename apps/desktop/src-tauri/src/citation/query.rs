use std::collections::{HashMap, HashSet};
use std::time::Duration;

use reqwest::Url;

use crate::settings;

use super::types::*;

// --- Tokenization ---

pub(crate) fn tokenize_lower(s: &str) -> Vec<String> {
    s.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|t| !t.is_empty())
        .map(ToString::to_string)
        .collect()
}

pub(crate) fn is_stop_token(token: &str) -> bool {
    EN_STOP_WORDS.contains(&token)
}

pub(crate) fn is_unit_token(token: &str) -> bool {
    UNIT_TOKENS.contains(&token)
}

pub(crate) fn is_method_hint_token(token: &str) -> bool {
    METHOD_HINT_TOKENS.contains(&token) || token.ends_with("thermal")
}

pub(crate) fn is_morphology_hint_token(token: &str) -> bool {
    MORPHOLOGY_HINT_TOKENS.contains(&token)
}

pub(crate) fn is_process_noise_token(token: &str) -> bool {
    PROCESS_NOISE_TOKENS.contains(&token)
}

pub(crate) fn is_formula_like_token(token: &str) -> bool {
    has_digit(token)
        && token.len() >= 4
        && token.chars().any(|c| c.is_ascii_alphabetic())
        && !token.chars().all(|c| c.is_ascii_digit())
}

pub(crate) fn is_anchor_token(token: &str) -> bool {
    is_formula_like_token(token)
        || is_method_hint_token(token)
        || is_morphology_hint_token(token)
        || CHEMICAL_SHORT_TOKENS.contains(&token)
}

pub(crate) fn has_digit(token: &str) -> bool {
    token.chars().any(|c| c.is_ascii_digit())
}

pub(crate) fn is_numeric_token(token: &str) -> bool {
    !token.is_empty() && token.chars().all(|c| c.is_ascii_digit())
}

pub(crate) fn content_tokens(s: &str) -> Vec<String> {
    tokenize_lower(s)
        .into_iter()
        .filter(|t| t.len() >= 3 && !is_stop_token(t))
        .collect()
}

fn tokenize_alnum_preserve_case(s: &str) -> Vec<String> {
    s.split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|t| !t.is_empty())
        .map(ToString::to_string)
        .collect()
}

fn is_known_element_symbol(symbol_lower: &str) -> bool {
    matches!(
        symbol_lower,
        "h" | "he"
            | "li"
            | "be"
            | "b"
            | "c"
            | "n"
            | "o"
            | "f"
            | "ne"
            | "na"
            | "mg"
            | "al"
            | "si"
            | "p"
            | "s"
            | "cl"
            | "ar"
            | "k"
            | "ca"
            | "sc"
            | "ti"
            | "v"
            | "cr"
            | "mn"
            | "fe"
            | "co"
            | "ni"
            | "cu"
            | "zn"
            | "ga"
            | "ge"
            | "as"
            | "se"
            | "br"
            | "kr"
            | "rb"
            | "sr"
            | "y"
            | "zr"
            | "nb"
            | "mo"
            | "tc"
            | "ru"
            | "rh"
            | "pd"
            | "ag"
            | "cd"
            | "in"
            | "sn"
            | "sb"
            | "te"
            | "i"
            | "xe"
            | "cs"
            | "ba"
            | "la"
            | "ce"
            | "pr"
            | "nd"
            | "sm"
            | "eu"
            | "gd"
            | "tb"
            | "dy"
            | "ho"
            | "er"
            | "tm"
            | "yb"
            | "lu"
            | "hf"
            | "ta"
            | "w"
            | "re"
            | "os"
            | "ir"
            | "pt"
            | "au"
            | "hg"
            | "tl"
            | "pb"
            | "bi"
            | "po"
            | "at"
            | "rn"
            | "fr"
            | "ra"
            | "ac"
            | "th"
            | "pa"
            | "u"
    )
}

// --- Formula parsing ---

pub(crate) fn parse_formula_elements(token: &str) -> Option<Vec<String>> {
    let chars = token.chars().collect::<Vec<_>>();
    if chars.len() < 2 || chars.len() > 20 {
        return None;
    }
    if !chars.iter().any(|c| c.is_ascii_alphabetic()) {
        return None;
    }
    let has_digit = chars.iter().any(|c| c.is_ascii_digit());
    let uppercase_count = chars.iter().filter(|c| c.is_ascii_uppercase()).count();
    if uppercase_count == 0 {
        return None;
    }
    // For non-digit tokens, require at least 2 uppercase markers to avoid
    // accidentally treating normal words as formulas.
    if !has_digit && uppercase_count < 2 {
        return None;
    }

    let mut elements = Vec::<String>::new();
    let mut i = 0usize;
    while i < chars.len() {
        let ch = chars[i];
        if !ch.is_ascii_uppercase() {
            return None;
        }
        let mut symbol = String::new();
        symbol.push(ch.to_ascii_lowercase());
        i += 1;
        if i < chars.len() && chars[i].is_ascii_lowercase() {
            symbol.push(chars[i]);
            i += 1;
        }
        if !is_known_element_symbol(&symbol) {
            return None;
        }
        elements.push(symbol);
        while i < chars.len() && chars[i].is_ascii_digit() {
            i += 1;
        }
    }

    if elements.is_empty() {
        return None;
    }
    if !has_digit && elements.len() < 2 {
        return None;
    }
    elements.sort();
    elements.dedup();
    Some(elements)
}

pub(crate) fn extract_formula_elements(text: &str, max_elements: usize) -> HashSet<String> {
    let mut out = HashSet::<String>::new();
    for token in tokenize_alnum_preserve_case(text) {
        if let Some(elements) = parse_formula_elements(&token) {
            for element in elements {
                out.insert(element);
                if out.len() >= max_elements {
                    return out;
                }
            }
        }
    }
    out
}

pub(crate) fn query_tokens(s: &str) -> Vec<String> {
    tokenize_lower(s)
        .into_iter()
        .filter(|t| {
            if is_stop_token(t) {
                return false;
            }
            if is_process_noise_token(t) {
                return false;
            }
            if is_unit_token(t) {
                return false;
            }
            if is_numeric_token(t) {
                return false;
            }
            if is_anchor_token(t) {
                return true;
            }
            if has_digit(t) {
                return true;
            }
            t.len() >= 3
        })
        .collect()
}

// --- Text preprocessing ---

pub(crate) fn collapse_whitespace(input: &str) -> String {
    input.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn find_matching_brace(s: &str, open_brace_idx: usize) -> Option<usize> {
    let bytes = s.as_bytes();
    let mut depth: i32 = 0;
    for (i, b) in bytes.iter().enumerate().skip(open_brace_idx) {
        match *b as char {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(i);
                }
            }
            _ => {}
        }
    }
    None
}

fn strip_braced_command(mut text: String, command_with_brace: &str) -> String {
    while let Some(start) = text.find(command_with_brace) {
        let open = start + command_with_brace.len() - 1;
        if let Some(end) = find_matching_brace(&text, open) {
            text.replace_range(start..=end, " ");
        } else {
            text.replace_range(start..start + command_with_brace.len(), " ");
        }
    }
    text
}

fn strip_inline_math(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut in_math = false;
    for ch in input.chars() {
        if ch == '$' {
            in_math = !in_math;
            out.push(' ');
            continue;
        }
        if !in_math {
            out.push(ch);
        }
    }
    out
}

pub(crate) fn preprocess_selected_text(input: &str) -> String {
    let mut cleaned = input.replace('\n', " ");
    for cmd in [
        "\\cite{",
        "\\citep{",
        "\\citet{",
        "\\autocite{",
        "\\parencite{",
        "\\textcite{",
        "\\ref{",
        "\\eqref{",
        "\\label{",
    ] {
        cleaned = strip_braced_command(cleaned, cmd);
    }
    cleaned = strip_inline_math(&cleaned);

    // Remove remaining LaTeX command markers while keeping surrounding words.
    let mut no_cmd = String::with_capacity(cleaned.len());
    let mut chars = cleaned.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\\' {
            while let Some(next) = chars.peek() {
                if next.is_alphabetic() || *next == '*' {
                    chars.next();
                } else {
                    break;
                }
            }
            no_cmd.push(' ');
            continue;
        }
        if ch == '{' || ch == '}' || ch == '[' || ch == ']' {
            no_cmd.push(' ');
        } else {
            no_cmd.push(ch);
        }
    }
    collapse_whitespace(&no_cmd)
}

pub(crate) fn split_sentences(text: &str) -> Vec<String> {
    text.split(|c: char| ".!?;\n。！？；".contains(c))
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(ToString::to_string)
        .collect()
}

// --- Citation need detection ---

fn contains_any_phrase(haystack: &str, phrases: &[&str]) -> bool {
    phrases.iter().any(|phrase| haystack.contains(phrase))
}

fn contains_any_token(tokens: &[String], hints: &[&str]) -> bool {
    tokens.iter().any(|t| hints.contains(&t.as_str()))
}

pub(crate) fn classify_claim_type(lower_text: &str, tokens: &[String]) -> String {
    if contains_any_token(tokens, CLAIM_TYPE_STAT_HINTS) {
        return "statistic".to_string();
    }
    if contains_any_token(tokens, CLAIM_TYPE_METHOD_HINTS)
        || tokens.iter().any(|t| is_method_hint_token(t))
    {
        return "method".to_string();
    }
    if contains_any_token(tokens, CLAIM_TYPE_MECHANISM_HINTS) {
        return "mechanism".to_string();
    }
    if contains_any_phrase(lower_text, CLAIM_TYPE_DEFINITION_HINTS) {
        return "definition".to_string();
    }
    "background".to_string()
}

pub(crate) fn classify_citation_need(text: &str) -> CitationNeedDecisionDebug {
    let cleaned = collapse_whitespace(text);
    if cleaned.trim().is_empty() {
        return CitationNeedDecisionDebug {
            needs_citation: false,
            level: "no".to_string(),
            claim_type: "background".to_string(),
            recommended_refs: 0,
            score: 0.0,
            reasons: vec!["empty selection".to_string()],
        };
    }

    let lower_text = cleaned.to_lowercase();
    let tokens = tokenize_lower(&lower_text);
    let token_count = tokens.len();
    let mut score = 0.0f32;
    let mut reasons = Vec::<String>::new();

    if contains_any_phrase(&lower_text, CITATION_TRIGGER_PHRASES) {
        score += 0.40;
        reasons.push("contains prior-work cue phrase".to_string());
    }
    let has_numeric = cleaned.chars().any(|c| c.is_ascii_digit());
    if has_numeric {
        score += 0.17;
        reasons.push("contains numeric statement".to_string());
    }
    if cleaned.contains('%') || lower_text.contains("percent") || lower_text.contains("percentage")
    {
        score += 0.14;
        reasons.push("contains percentage/statistical claim".to_string());
    }
    if tokens
        .iter()
        .any(|t| is_method_hint_token(t) || is_formula_like_token(t))
    {
        score += 0.14;
        reasons.push("contains method/material anchor terms".to_string());
    }
    if contains_any_token(&tokens, CLAIM_TYPE_MECHANISM_HINTS) {
        score += 0.14;
        reasons.push("contains mechanism/causal wording".to_string());
    }
    if contains_any_token(&tokens, CLAIM_TYPE_STAT_HINTS) {
        score += 0.12;
        reasons.push("contains epidemiology/statistical wording".to_string());
    }
    if contains_any_phrase(&lower_text, SELF_AUTHORED_PHRASES) {
        score -= 0.34;
        reasons.push("contains self-authored narrative cue".to_string());
    }
    if token_count <= 7
        && !contains_any_phrase(&lower_text, CITATION_TRIGGER_PHRASES)
        && !has_numeric
    {
        score -= 0.10;
        reasons.push("short generic sentence with weak evidence cues".to_string());
    }

    score = score.clamp(0.0, 1.0);
    let claim_type = classify_claim_type(&lower_text, &tokens);
    let mut level = if score >= 0.56 {
        "must"
    } else if score >= 0.32 {
        "suggest"
    } else {
        "no"
    };
    let has_anchor_cue = contains_any_phrase(&lower_text, CITATION_TRIGGER_PHRASES)
        || has_numeric
        || tokens
            .iter()
            .any(|t| is_method_hint_token(t) || is_formula_like_token(t))
        || contains_any_token(&tokens, CLAIM_TYPE_MECHANISM_HINTS)
        || contains_any_token(&tokens, CLAIM_TYPE_STAT_HINTS);
    if level == "no" && has_anchor_cue && score >= 0.22 {
        level = "suggest";
        reasons.push("conservative uplift to avoid false-negative citation miss".to_string());
    }
    let needs_citation = level != "no";
    let recommended_refs = if !needs_citation {
        0
    } else if contains_any_phrase(&lower_text, CONTROVERSY_HINTS) {
        3
    } else if claim_type == "mechanism" || claim_type == "statistic" {
        2
    } else {
        1
    };

    if reasons.is_empty() {
        reasons.push("no strong citation cues detected".to_string());
    }

    CitationNeedDecisionDebug {
        needs_citation,
        level: level.to_string(),
        claim_type,
        recommended_refs,
        score,
        reasons,
    }
}

pub(crate) fn build_keyword_query(text: &str) -> Option<String> {
    let mut counts: HashMap<String, u32> = HashMap::new();
    for token in query_tokens(text) {
        if token.len() < 3 && !has_digit(&token) {
            continue;
        }
        *counts.entry(token).or_insert(0) += 1;
    }
    if counts.is_empty() {
        return None;
    }
    let mut items = counts.into_iter().collect::<Vec<_>>();
    items.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    let query = items
        .into_iter()
        .take(12)
        .map(|(k, _)| k)
        .collect::<Vec<_>>()
        .join(" ");
    if query.is_empty() {
        None
    } else {
        Some(query)
    }
}

// --- Query building ---

pub(crate) fn truncate_chars(s: &str, max_chars: usize) -> String {
    s.chars().take(max_chars).collect::<String>()
}

fn sentence_salience(sentence: &str) -> usize {
    let tokens = query_tokens(sentence);
    if tokens.is_empty() {
        return 0;
    }
    let anchor_hits = tokens.iter().filter(|t| is_anchor_token(t)).count();
    let domain_hits = tokens
        .iter()
        .filter(|t| DOMAIN_HINT_TOKENS.contains(&t.as_str()))
        .count();
    let noise_hits = tokenize_lower(sentence)
        .into_iter()
        .filter(|t| is_process_noise_token(t))
        .count();
    tokens
        .len()
        .min(20)
        .saturating_add(anchor_hits * 8)
        .saturating_add(domain_hits * 3)
        .saturating_sub(noise_hits * 2)
}

pub(crate) fn top_salient_sentences(text: &str, max_n: usize) -> Vec<String> {
    let mut ranked = split_sentences(text)
        .into_iter()
        .map(|s| {
            let salience = sentence_salience(&s);
            (salience, s)
        })
        .filter(|(salience, s)| *salience >= 4 && s.len() >= 24)
        .collect::<Vec<_>>();
    ranked.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| b.1.len().cmp(&a.1.len())));
    ranked.into_iter().take(max_n).map(|(_, s)| s).collect()
}

pub(crate) fn semantic_token_weight(token: &str, position: usize) -> f32 {
    let mut weight = 1.0f32;
    if is_formula_like_token(token) {
        weight += 2.4;
    }
    if is_method_hint_token(token) {
        weight += 1.5;
    }
    if is_morphology_hint_token(token) {
        weight += 1.25;
    }
    if CHEMICAL_SHORT_TOKENS.contains(&token) {
        weight += 1.15;
    }
    if DOMAIN_HINT_TOKENS.contains(&token) {
        weight += 0.65;
    }
    if has_digit(token) {
        weight += 0.55;
    }
    if token.len() >= 9 {
        weight += 0.22;
    }
    if is_process_noise_token(token) || is_unit_token(token) || is_numeric_token(token) {
        weight *= 0.2;
    }
    let positional_boost = if position < 40 {
        1.15
    } else if position < 90 {
        1.07
    } else {
        1.0
    };
    weight * positional_boost
}

pub(crate) fn build_anchor_compact_query(text: &str, max_tokens: usize) -> Option<String> {
    let tokens = query_tokens(text);
    if tokens.is_empty() || max_tokens == 0 {
        return None;
    }

    let mut scored = HashMap::<String, f32>::new();
    for (idx, token) in tokens.into_iter().enumerate() {
        *scored.entry(token.clone()).or_insert(0.0) += semantic_token_weight(&token, idx);
    }

    let mut formulas = Vec::<(f32, String)>::new();
    let mut methods = Vec::<(f32, String)>::new();
    let mut morphs = Vec::<(f32, String)>::new();
    let mut others = Vec::<(f32, String)>::new();
    for (token, score) in scored {
        if is_formula_like_token(&token) || CHEMICAL_SHORT_TOKENS.contains(&token.as_str()) {
            formulas.push((score, token));
        } else if is_method_hint_token(&token) {
            methods.push((score, token));
        } else if is_morphology_hint_token(&token) {
            morphs.push((score, token));
        } else {
            others.push((score, token));
        }
    }
    let sort_desc = |items: &mut Vec<(f32, String)>| {
        items.sort_by(|a, b| b.0.total_cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
    };
    sort_desc(&mut formulas);
    sort_desc(&mut methods);
    sort_desc(&mut morphs);
    sort_desc(&mut others);

    let mut out = Vec::<String>::new();
    for (_, token) in formulas.into_iter().take(3) {
        out.push(token);
    }
    for (_, token) in methods.into_iter().take(3) {
        if !out.contains(&token) {
            out.push(token);
        }
    }
    for (_, token) in morphs.into_iter().take(2) {
        if !out.contains(&token) {
            out.push(token);
        }
    }
    for (_, token) in others {
        if out.len() >= max_tokens {
            break;
        }
        if !out.contains(&token) {
            out.push(token);
        }
    }
    out.truncate(max_tokens);
    if out.len() < 2 {
        None
    } else {
        Some(out.join(" "))
    }
}

pub(crate) fn build_semantic_focus_query(text: &str, max_tokens: usize) -> Option<String> {
    let tokens = query_tokens(text);
    if tokens.is_empty() {
        return None;
    }
    let mut scored = HashMap::<String, f32>::new();
    for (idx, token) in tokens.into_iter().enumerate() {
        let w = semantic_token_weight(&token, idx);
        *scored.entry(token).or_insert(0.0) += w;
    }
    let mut items = scored.into_iter().collect::<Vec<_>>();
    items.sort_by(|a, b| b.1.total_cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    let query = items
        .into_iter()
        .take(max_tokens)
        .map(|(token, _)| token)
        .collect::<Vec<_>>()
        .join(" ");
    if query.trim().is_empty() {
        None
    } else {
        Some(query)
    }
}

fn build_semantic_phrase_queries(text: &str, max_n: usize) -> Vec<String> {
    let mut phrases = Vec::<(f32, String)>::new();
    let mut seen = HashSet::<String>::new();
    for sentence in top_salient_sentences(text, 3) {
        let tokens = query_tokens(&sentence);
        if tokens.len() < 2 {
            continue;
        }
        for win in 2..=3 {
            if tokens.len() < win {
                continue;
            }
            for i in 0..=(tokens.len() - win) {
                let slice = &tokens[i..(i + win)];
                let anchor_hits = slice.iter().filter(|t| is_anchor_token(t)).count();
                let has_strong_anchor = slice
                    .iter()
                    .any(|t| is_formula_like_token(t) || is_method_hint_token(t));
                if anchor_hits == 0 || !has_strong_anchor {
                    continue;
                }
                let phrase = slice.join(" ");
                if !seen.insert(phrase.clone()) {
                    continue;
                }
                let score = slice
                    .iter()
                    .enumerate()
                    .map(|(j, t)| semantic_token_weight(t, i + j))
                    .sum::<f32>();
                phrases.push((score + anchor_hits as f32 * 0.3, phrase));
            }
        }
    }
    phrases.sort_by(|a, b| b.0.total_cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
    phrases.into_iter().take(max_n).map(|(_, p)| p).collect()
}

fn build_numeric_context_queries(text: &str, max_n: usize) -> Vec<String> {
    let mut out = Vec::<String>::new();
    let mut seen = HashSet::<String>::new();
    for sentence in split_sentences(text) {
        let raw_tokens = tokenize_lower(&sentence);
        if raw_tokens.is_empty() {
            continue;
        }
        for idx in 0..raw_tokens.len() {
            if !is_numeric_token(&raw_tokens[idx]) {
                continue;
            }
            let start = idx.saturating_sub(2);
            let end = (idx + 3).min(raw_tokens.len());
            let mut phrase_tokens = Vec::<String>::new();
            for t in &raw_tokens[start..end] {
                if is_stop_token(t) {
                    continue;
                }
                if is_process_noise_token(t) {
                    continue;
                }
                if t.len() >= 3 || is_numeric_token(t) || is_unit_token(t) || has_digit(t) {
                    phrase_tokens.push(t.clone());
                }
            }
            let anchor_hits = phrase_tokens.iter().filter(|t| is_anchor_token(t)).count();
            if phrase_tokens.len() < 2 || anchor_hits == 0 {
                continue;
            }
            let phrase = phrase_tokens.join(" ");
            if seen.insert(phrase.clone()) {
                out.push(phrase);
            }
            if out.len() >= max_n {
                return out;
            }
        }
    }
    out
}

#[derive(Debug, Clone, Copy, Default)]
pub struct QueryPlanBuildOptions {
    pub enable_mesh_expansion: bool,
    pub min_year: Option<u16>,
    pub max_year: Option<u16>,
}

fn mesh_expansion_terms(cleaned_text: &str, max_n: usize) -> Vec<String> {
    let lowered = cleaned_text.to_ascii_lowercase();
    let mut terms = Vec::<String>::new();
    let mut push = |term: &str| {
        if terms.iter().any(|existing| existing == term) {
            return;
        }
        if terms.len() < max_n {
            terms.push(term.to_string());
        }
    };

    if lowered.contains("cancer")
        || lowered.contains("tumor")
        || lowered.contains("leukemia")
        || lowered.contains("carcinoma")
    {
        push("Neoplasms[MeSH Terms]");
    }
    if lowered.contains("crispr") || lowered.contains("gene editing") || lowered.contains("genome")
    {
        push("CRISPR-Cas Systems[MeSH Terms]");
        push("Gene Editing[MeSH Terms]");
    }
    if lowered.contains("photocatal")
        || lowered.contains("tio2")
        || lowered.contains("nanotube")
        || lowered.contains("catalyst")
    {
        push("Photocatalysis[MeSH Terms]");
        push("Nanostructures[MeSH Terms]");
    }
    if lowered.contains("hydrophobic")
        || lowered.contains("wettability")
        || lowered.contains("contact angle")
    {
        push("Hydrophobic and Hydrophilic Interactions[MeSH Terms]");
    }
    if lowered.contains("randomized")
        || lowered.contains("clinical trial")
        || lowered.contains("patient")
    {
        push("Clinical Trials as Topic[MeSH Terms]");
    }

    terms
}

fn date_range_filter_clause(options: QueryPlanBuildOptions) -> Option<String> {
    match (options.min_year, options.max_year) {
        (None, None) => None,
        (Some(min), Some(max)) if min <= max => Some(format!(
            "({}[Date - Publication] : {}[Date - Publication])",
            min, max
        )),
        (Some(min), _) => Some(format!(
            "({}[Date - Publication] : 3000[Date - Publication])",
            min
        )),
        (_, Some(max)) => Some(format!(
            "(0001[Date - Publication] : {}[Date - Publication])",
            max
        )),
    }
}

pub(crate) fn build_search_query_plan(raw_selected: &str) -> Vec<CitationQueryPlanItem> {
    build_search_query_plan_with_options(raw_selected, QueryPlanBuildOptions::default())
}

pub(crate) fn build_search_query_plan_with_options(
    raw_selected: &str,
    options: QueryPlanBuildOptions,
) -> Vec<CitationQueryPlanItem> {
    let cleaned = preprocess_selected_text(raw_selected);
    if cleaned.is_empty() {
        return Vec::new();
    }

    let mut plan: Vec<CitationQueryPlanItem> = Vec::new();
    let mut seen = HashSet::new();

    if let Some(anchor_q) = build_anchor_compact_query(&cleaned, 12) {
        let anchor_q = collapse_whitespace(&anchor_q);
        if !anchor_q.is_empty() && seen.insert(anchor_q.to_lowercase()) {
            plan.push(CitationQueryPlanItem {
                query: anchor_q,
                strategy: "anchor_compact".to_string(),
                source: "rule".to_string(),
                weight: 1.12,
                quality: CitationQueryQualityDebug::default(),
            });
        }
    }

    if let Some(semantic_q) = build_semantic_focus_query(&cleaned, 12) {
        let semantic_q = collapse_whitespace(&semantic_q);
        if !semantic_q.is_empty() && seen.insert(semantic_q.to_lowercase()) {
            plan.push(CitationQueryPlanItem {
                query: semantic_q,
                strategy: "semantic_focus_compact".to_string(),
                source: "rule".to_string(),
                weight: 1.04,
                quality: CitationQueryQualityDebug::default(),
            });
        }
    }

    for (idx, phrase_q) in build_semantic_phrase_queries(&cleaned, 2)
        .into_iter()
        .enumerate()
    {
        let phrase_q = collapse_whitespace(&truncate_chars(&phrase_q, 180));
        if !phrase_q.is_empty() && seen.insert(phrase_q.to_lowercase()) {
            plan.push(CitationQueryPlanItem {
                query: phrase_q,
                strategy: format!("semantic_phrase_{}", idx + 1),
                source: "rule".to_string(),
                weight: 1.0,
                quality: CitationQueryQualityDebug::default(),
            });
        }
    }

    for (idx, sentence) in top_salient_sentences(&cleaned, 2).into_iter().enumerate() {
        let sentence_q = build_semantic_focus_query(&sentence, 10)
            .map(|q| collapse_whitespace(&truncate_chars(&q, 160)))
            .unwrap_or_default();
        if !sentence_q.is_empty() && seen.insert(sentence_q.to_lowercase()) {
            plan.push(CitationQueryPlanItem {
                query: sentence_q,
                strategy: format!("salient_focus_{}", idx + 1),
                source: "rule".to_string(),
                weight: 0.93,
                quality: CitationQueryQualityDebug::default(),
            });
        }
    }

    for (idx, numeric_q) in build_numeric_context_queries(&cleaned, 1)
        .into_iter()
        .enumerate()
    {
        let numeric_q = collapse_whitespace(&truncate_chars(&numeric_q, 160));
        if !numeric_q.is_empty() && seen.insert(numeric_q.to_lowercase()) {
            plan.push(CitationQueryPlanItem {
                query: numeric_q,
                strategy: format!("numeric_context_{}", idx + 1),
                source: "rule".to_string(),
                weight: 0.86,
                quality: CitationQueryQualityDebug::default(),
            });
        }
    }

    if let Some(keyword_q) = build_keyword_query(&cleaned) {
        let keyword_q = collapse_whitespace(&keyword_q);
        if !keyword_q.is_empty() && seen.insert(keyword_q.to_lowercase()) {
            plan.push(CitationQueryPlanItem {
                query: keyword_q,
                strategy: "keyword_compact".to_string(),
                source: "rule".to_string(),
                weight: 0.84,
                quality: CitationQueryQualityDebug::default(),
            });
        }
    }

    if cleaned.chars().count() <= 220 {
        let full = collapse_whitespace(&truncate_chars(&cleaned, 220));
        if !full.is_empty() && seen.insert(full.to_lowercase()) {
            plan.push(CitationQueryPlanItem {
                query: full,
                strategy: "cleaned_fulltext".to_string(),
                source: "rule".to_string(),
                weight: 0.74,
                quality: CitationQueryQualityDebug::default(),
            });
        }
    }

    if options.enable_mesh_expansion {
        for (idx, mesh_term) in mesh_expansion_terms(&cleaned, 3).into_iter().enumerate() {
            let expansion = if let Some(anchor) = plan.first() {
                format!("{} {}", anchor.query, mesh_term)
            } else {
                mesh_term
            };
            append_query_plan_item(
                &mut plan,
                &mut seen,
                &expansion,
                &format!("mesh_expansion_{}", idx + 1),
                "rule",
                0.9,
                240,
            );
        }
    }

    if let Some(date_clause) = date_range_filter_clause(options) {
        if let Some(anchor) = plan.first().cloned() {
            let query = format!("{} {}", anchor.query, date_clause);
            append_query_plan_item(
                &mut plan,
                &mut seen,
                &query,
                "date_range_filtered",
                "rule",
                0.88,
                260,
            );
        }
    }

    plan
}

pub(crate) fn append_query_plan_item(
    plan: &mut Vec<CitationQueryPlanItem>,
    seen: &mut HashSet<String>,
    query: &str,
    strategy: &str,
    source: &str,
    weight: f32,
    max_chars: usize,
) {
    let normalized = collapse_whitespace(&truncate_chars(query, max_chars));
    if normalized.is_empty() {
        return;
    }
    if seen.insert(normalized.to_lowercase()) {
        plan.push(CitationQueryPlanItem {
            query: normalized,
            strategy: strategy.to_string(),
            source: source.to_string(),
            weight,
            quality: CitationQueryQualityDebug::default(),
        });
    }
}

// --- LLM integration ---

fn strip_markdown_json_fence(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.starts_with("```") {
        let without_start = trimmed
            .strip_prefix("```json")
            .or_else(|| trimmed.strip_prefix("```JSON"))
            .or_else(|| trimmed.strip_prefix("```"))
            .unwrap_or(trimmed);
        let without_end = without_start.strip_suffix("```").unwrap_or(without_start);
        return without_end.trim().to_string();
    }
    trimmed.to_string()
}

fn parse_llm_query_payload_from_text(raw: &str) -> Result<LlmQueryPayload, String> {
    let cleaned = strip_markdown_json_fence(raw);
    if let Ok(parsed) = serde_json::from_str::<LlmQueryPayload>(&cleaned) {
        return Ok(parsed);
    }

    let start = cleaned.find('{');
    let end = cleaned.rfind('}');
    if let (Some(s), Some(e)) = (start, end) {
        if s < e {
            let slice = &cleaned[s..=e];
            if let Ok(parsed) = serde_json::from_str::<LlmQueryPayload>(slice) {
                return Ok(parsed);
            }
        }
    }

    Err("LLM output is not valid JSON query payload.".to_string())
}

fn extract_openai_message_content(response: &serde_json::Value) -> Option<String> {
    if let Some(s) = response
        .get("output_text")
        .and_then(serde_json::Value::as_str)
    {
        if !s.trim().is_empty() {
            return Some(s.to_string());
        }
    }

    if let Some(choice0) = response
        .get("choices")
        .and_then(serde_json::Value::as_array)
        .and_then(|arr| arr.first())
    {
        if let Some(content) = choice0
            .get("message")
            .and_then(|m| m.get("content"))
            .and_then(serde_json::Value::as_str)
        {
            if !content.trim().is_empty() {
                return Some(content.to_string());
            }
        }
        if let Some(parts) = choice0
            .get("message")
            .and_then(|m| m.get("content"))
            .and_then(serde_json::Value::as_array)
        {
            let joined = parts
                .iter()
                .filter_map(|p| p.get("text").and_then(serde_json::Value::as_str))
                .collect::<Vec<_>>()
                .join("\n");
            if !joined.trim().is_empty() {
                return Some(joined);
            }
        }
    }

    None
}

pub(crate) async fn generate_llm_query_plan(
    cleaned_text: &str,
    cfg: &settings::CitationLlmRuntimeConfig,
) -> Result<Vec<CitationQueryPlanItem>, String> {
    if !cfg.enabled {
        return Ok(Vec::new());
    }
    let api_key = cfg
        .api_key
        .as_deref()
        .filter(|v| !v.trim().is_empty())
        .ok_or_else(|| {
            "LLM query rewrite is enabled, but API key is not configured.".to_string()
        })?;
    if cleaned_text.trim().is_empty() {
        return Ok(Vec::new());
    }

    let endpoint = Url::parse(&cfg.endpoint)
        .map_err(|e| format!("Invalid LLM endpoint '{}': {}", cfg.endpoint, e))?;
    let timeout = Duration::from_millis(cfg.timeout_ms.clamp(2000, 20000));
    let connect_timeout = Duration::from_millis((cfg.timeout_ms / 3).clamp(1000, 5000));
    let client = reqwest::Client::builder()
        .connect_timeout(connect_timeout)
        .timeout(timeout)
        .build()
        .map_err(|e| format!("Failed to init LLM HTTP client: {}", e))?;

    let prompt = format!(
        "Given the following scientific text, output JSON only with keys \
queries_precise (array of short search queries) and queries_broad (array). \
Keep each query concise (4-14 tokens), no full sentences, no markdown.\n\nText:\n{}",
        truncate_chars(cleaned_text, 1800)
    );
    let body = serde_json::json!({
      "model": cfg.model,
      "temperature": 0.1,
      "messages": [
        {
          "role": "system",
          "content": "You generate scholarly retrieval queries. Output strict JSON only."
        },
        {
          "role": "user",
          "content": prompt
        }
      ],
      "response_format": { "type": "json_object" }
    });

    let response = client
        .post(endpoint)
        .header("Accept", "application/json")
        .header("Content-Type", "application/json")
        .header("Authorization", format!("Bearer {}", api_key))
        .body(body.to_string())
        .send()
        .await
        .map_err(|e| format!("LLM query request failed: {}", e))?;

    if !response.status().is_success() {
        let status = response.status();
        let body_preview = response
            .text()
            .await
            .unwrap_or_default()
            .chars()
            .take(220)
            .collect::<String>();
        let err = if body_preview.is_empty() {
            format!("LLM query request failed with status {}", status)
        } else {
            format!(
                "LLM query request failed with status {}: {}",
                status, body_preview
            )
        };
        return Err(err);
    }

    let text = response
        .text()
        .await
        .map_err(|e| format!("Failed to read LLM response body: {}", e))?;
    let parsed_json: serde_json::Value = serde_json::from_str(&text)
        .map_err(|e| format!("Failed to parse LLM response JSON: {}", e))?;
    let content = extract_openai_message_content(&parsed_json)
        .ok_or_else(|| "LLM response missing message content.".to_string())?;
    let payload = parse_llm_query_payload_from_text(&content)?;

    let mut plan = Vec::<CitationQueryPlanItem>::new();
    let mut seen = HashSet::<String>::new();
    let max_queries = cfg.max_queries.clamp(1, 6);
    for q in payload
        .queries_precise
        .unwrap_or_default()
        .into_iter()
        .take(max_queries)
    {
        append_query_plan_item(&mut plan, &mut seen, &q, "llm_precise", "llm", 1.05, 220);
    }
    if plan.len() < max_queries {
        for q in payload
            .queries
            .unwrap_or_default()
            .into_iter()
            .take(max_queries - plan.len())
        {
            append_query_plan_item(&mut plan, &mut seen, &q, "llm_general", "llm", 0.95, 220);
        }
    }
    if plan.len() < max_queries {
        for q in payload
            .queries_broad
            .unwrap_or_default()
            .into_iter()
            .take(max_queries - plan.len())
        {
            append_query_plan_item(&mut plan, &mut seen, &q, "llm_broad", "llm", 0.76, 220);
        }
    }

    Ok(plan)
}

pub(crate) fn build_compact_query(query: &str, max_tokens: usize, fallback_chars: usize) -> String {
    let mut seen = HashSet::new();
    let tokens = query_tokens(query)
        .into_iter()
        .filter(|t| seen.insert(t.clone()))
        .take(max_tokens)
        .collect::<Vec<_>>();
    if !tokens.is_empty() {
        return tokens.join(" ");
    }
    truncate_chars(&collapse_whitespace(query), fallback_chars)
}

// --- Query compaction ---

pub(crate) fn build_s2_compact_query(query: &str) -> String {
    build_compact_query(query, 16, 140)
}

pub(crate) fn build_openalex_compact_query(query: &str) -> String {
    build_compact_query(query, 18, 180)
}

pub(crate) fn build_crossref_compact_query(query: &str) -> String {
    build_compact_query(query, 14, 160)
}

pub(crate) fn build_pubmed_compact_query(query: &str) -> String {
    build_compact_query(query, 20, 220)
}

// --- Query selection ---

fn query_similarity_for_mmr(a: &[String], b: &[String]) -> f32 {
    super::scoring::token_f1_score(a, b).max(super::scoring::overlap_score(a, b))
}

pub(crate) fn select_execution_query_plan(
    query_plan: &[CitationQueryPlanItem],
    top_n: usize,
    min_quality: f32,
    mmr_lambda: f32,
) -> Vec<CitationQueryPlanItem> {
    if query_plan.is_empty() || top_n == 0 {
        return Vec::new();
    }
    let top_n = top_n.min(query_plan.len());
    let lambda = mmr_lambda.clamp(0.0, 1.0);
    let forced_first = query_plan.first().cloned();

    let mut remaining = query_plan
        .iter()
        .filter(|item| item.quality.total >= min_quality)
        .cloned()
        .map(|item| {
            let tokens = content_tokens(&item.query);
            (item, tokens)
        })
        .collect::<Vec<_>>();

    if remaining.is_empty() {
        if let Some(first) = forced_first {
            return vec![first];
        }
        return Vec::new();
    }

    let mut selected = Vec::<CitationQueryPlanItem>::new();
    let mut selected_tokens = Vec::<Vec<String>>::new();
    while selected.len() < top_n && !remaining.is_empty() {
        let mut best_idx = 0usize;
        let mut best_score = f32::NEG_INFINITY;
        for (idx, (item, tokens)) in remaining.iter().enumerate() {
            let relevance = item.quality.total;
            let diversity_penalty = selected_tokens
                .iter()
                .map(|picked_tokens| query_similarity_for_mmr(tokens, picked_tokens))
                .fold(0.0f32, f32::max);
            let mmr = lambda * relevance - (1.0 - lambda) * diversity_penalty;
            if mmr > best_score {
                best_score = mmr;
                best_idx = idx;
            }
        }
        let (item, tokens) = remaining.remove(best_idx);
        selected_tokens.push(tokens);
        selected.push(item);
    }

    if let Some(first) = forced_first {
        let first_lc = first.query.to_lowercase();
        if selected
            .iter()
            .all(|item| item.query.to_lowercase() != first_lc)
        {
            if selected.len() >= top_n {
                selected.pop();
            }
            selected.insert(0, first);
        }
    }
    selected
}

pub(crate) fn compose_query_quality_total(
    semantic_sim: f32,
    anchor_coverage: f32,
    specificity: f32,
    noise_penalty: f32,
    length_penalty: f32,
) -> f32 {
    (0.45 * semantic_sim + 0.25 * anchor_coverage + 0.20 * specificity
        - 0.15 * noise_penalty
        - 0.05 * length_penalty)
        .clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::{build_search_query_plan_with_options, QueryPlanBuildOptions};

    #[test]
    fn query_plan_supports_mesh_and_date_hooks() {
        let plan = build_search_query_plan_with_options(
            "CRISPR therapy in leukemia",
            QueryPlanBuildOptions {
                enable_mesh_expansion: true,
                min_year: Some(2018),
                max_year: Some(2025),
            },
        );
        assert!(!plan.is_empty());
        assert!(plan
            .iter()
            .any(|item| item.strategy.starts_with("mesh_expansion_")));
        assert!(plan
            .iter()
            .any(|item| item.strategy == "date_range_filtered"));
    }
}
