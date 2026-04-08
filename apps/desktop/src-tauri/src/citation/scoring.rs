use std::collections::{HashMap, HashSet};

use super::query::*;
use super::types::*;

// --- Semantic / embedding functions ---

fn stable_hash_bytes(bytes: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf29ce484222325; // FNV-1a 64-bit offset basis
    for b in bytes {
        hash ^= u64::from(*b);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

fn semantic_stem_token(token: &str) -> String {
    let mut out = token.to_lowercase();
    if out.len() < 4 {
        return out;
    }
    let mut replace_suffix = |suffix: &str, replacement: &str, min_len: usize| {
        if out.len() > min_len && out.ends_with(suffix) {
            let new_len = out.len().saturating_sub(suffix.len());
            out.truncate(new_len);
            out.push_str(replacement);
            true
        } else {
            false
        }
    };

    if replace_suffix("ization", "ize", 9)
        || replace_suffix("isation", "ise", 9)
        || replace_suffix("ational", "ate", 9)
        || replace_suffix("ically", "ic", 8)
        || replace_suffix("ation", "ate", 8)
        || replace_suffix("izing", "ize", 7)
        || replace_suffix("ising", "ise", 7)
        || replace_suffix("ments", "ment", 7)
        || replace_suffix("ment", "", 6)
        || replace_suffix("ing", "", 6)
        || replace_suffix("ied", "y", 6)
        || replace_suffix("ed", "", 5)
    {
        return out;
    }
    if out.len() > 5 && out.ends_with("ies") {
        out.truncate(out.len() - 3);
        out.push('y');
        return out;
    }
    if out.len() > 5 && out.ends_with("es") {
        out.truncate(out.len() - 2);
        return out;
    }
    if out.len() > 4 && out.ends_with('s') {
        out.truncate(out.len() - 1);
    }
    out
}

fn semantic_tokens_for_embedding(text: &str) -> Vec<String> {
    tokenize_lower(text)
        .into_iter()
        .filter(|token| {
            if token.is_empty() {
                return false;
            }
            if is_stop_token(token) {
                return false;
            }
            if is_process_noise_token(token) && !is_anchor_token(token) {
                return false;
            }
            if is_unit_token(token) && !is_anchor_token(token) {
                return false;
            }
            token.len() >= 2 || is_formula_like_token(token)
        })
        .map(|token| semantic_stem_token(&token))
        .filter(|token| !token.is_empty())
        .collect()
}

fn add_hashed_feature(vector: &mut [f32], feature: &str, weight: f32) {
    if vector.is_empty() || feature.is_empty() {
        return;
    }
    let hash = stable_hash_bytes(feature.as_bytes());
    let idx = (hash % vector.len() as u64) as usize;
    let sign = if ((hash >> 63) & 1) == 0 { 1.0 } else { -1.0 };
    vector[idx] += weight * sign;
}

fn build_hashed_semantic_vector(tokens: &[String], dim: usize) -> Vec<f32> {
    let mut vec = vec![0.0f32; dim];
    if tokens.is_empty() || dim == 0 {
        return vec;
    }

    for (idx, token) in tokens.iter().enumerate() {
        let mut token_weight = 1.0f32;
        if is_anchor_token(token) {
            token_weight += 0.35;
        }
        if token.len() >= 8 {
            token_weight += 0.1;
        }
        add_hashed_feature(&mut vec, token, token_weight);

        let chars = token.chars().collect::<Vec<_>>();
        if chars.len() >= 3 {
            for window in chars.windows(3) {
                let trigram = window.iter().collect::<String>();
                add_hashed_feature(&mut vec, &format!("g:{}", trigram), token_weight * 0.30);
            }
        }
        if idx + 1 < tokens.len() {
            let bigram = format!("{} {}", token, tokens[idx + 1]);
            add_hashed_feature(&mut vec, &format!("b:{}", bigram), token_weight * 0.45);
        }
    }
    vec
}

fn l2_normalize(vector: &mut [f32]) -> bool {
    let norm_sq: f32 = vector.iter().map(|v| v * v).sum();
    if norm_sq <= f32::EPSILON {
        return false;
    }
    let inv = norm_sq.sqrt().recip();
    for v in vector.iter_mut() {
        *v *= inv;
    }
    true
}

fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }
    let dot: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
    dot.clamp(-1.0, 1.0)
}

fn build_embedding_reference_text(selected_text: &str) -> String {
    let mut parts = Vec::<String>::new();
    let anchors = anchor_focus_tokens(selected_text, 14);
    if !anchors.is_empty() {
        parts.push(anchors.join(" "));
    }
    let salient = top_salient_sentences(selected_text, 2).join(" ");
    let salient = collapse_whitespace(&salient);
    if !salient.is_empty() {
        parts.push(truncate_chars(&salient, 280));
    }
    if parts.is_empty() {
        let cleaned = collapse_whitespace(&truncate_chars(
            &preprocess_selected_text(selected_text),
            320,
        ));
        if cleaned.is_empty() {
            selected_text.to_string()
        } else {
            cleaned
        }
    } else {
        collapse_whitespace(&parts.join(" "))
    }
}

fn local_embedding_semantic_similarity(query: &str, selected_text: &str) -> Result<f32, String> {
    let query_tokens = semantic_tokens_for_embedding(query);
    if query_tokens.is_empty() {
        return Err("empty query tokens for local embedding".to_string());
    }
    let reference_text = build_embedding_reference_text(selected_text);
    let reference_tokens = semantic_tokens_for_embedding(&reference_text);
    if reference_tokens.is_empty() {
        return Err("empty reference tokens for local embedding".to_string());
    }

    let mut query_vec = build_hashed_semantic_vector(&query_tokens, 384);
    let mut reference_vec = build_hashed_semantic_vector(&reference_tokens, 384);
    if !l2_normalize(&mut query_vec) || !l2_normalize(&mut reference_vec) {
        return Err("local embedding vector normalization failed".to_string());
    }
    let cosine = cosine_similarity(&query_vec, &reference_vec);
    Ok(((cosine + 1.0) * 0.5).clamp(0.0, 1.0))
}

// --- Query quality scoring ---

pub(crate) fn score_query_quality(query: &str, selected_text: &str) -> CitationQueryQualityDebug {
    let query_tokens_content = content_tokens(query);
    if query_tokens_content.is_empty() {
        return CitationQueryQualityDebug::default();
    }
    let selected_tokens = content_tokens(selected_text);
    let anchor_reference = anchor_focus_tokens(selected_text, 10);
    let query_raw_tokens = tokenize_lower(query);

    let semantic_sim = token_f1_score(&query_tokens_content, &selected_tokens);

    let anchor_recall = if anchor_reference.is_empty() {
        0.0
    } else {
        overlap_score(&anchor_reference, &query_tokens_content)
    };
    let query_anchor_ratio = query_tokens_content
        .iter()
        .filter(|t| is_anchor_token(t))
        .count() as f32
        / query_tokens_content.len() as f32;
    let anchor_coverage = (0.65 * anchor_recall + 0.35 * query_anchor_ratio).clamp(0.0, 1.0);

    let unique_ratio = query_tokens_content.iter().collect::<HashSet<_>>().len() as f32
        / query_tokens_content.len() as f32;
    let long_ratio = query_tokens_content.iter().filter(|t| t.len() >= 6).count() as f32
        / query_tokens_content.len() as f32;
    let strong_ratio = query_tokens_content
        .iter()
        .filter(|t| is_formula_like_token(t) || is_method_hint_token(t))
        .count() as f32
        / query_tokens_content.len() as f32;
    let specificity =
        (0.45 * unique_ratio + 0.35 * long_ratio + 0.20 * strong_ratio).clamp(0.0, 1.0);

    let noise_penalty = if query_raw_tokens.is_empty() {
        0.0
    } else {
        let noise_count = query_raw_tokens
            .iter()
            .filter(|t| {
                is_stop_token(t)
                    || is_process_noise_token(t)
                    || is_unit_token(t)
                    || is_numeric_token(t)
            })
            .count();
        (noise_count as f32 / query_raw_tokens.len() as f32).clamp(0.0, 1.0)
    };

    let q_len = query_tokens_content.len();
    let length_penalty = if q_len < 3 {
        0.45
    } else if q_len < 5 {
        0.20
    } else if q_len <= 11 {
        0.0
    } else if q_len <= 16 {
        ((q_len - 11) as f32 * 0.04).clamp(0.0, 0.30)
    } else {
        0.35
    };

    let total = compose_query_quality_total(
        semantic_sim,
        anchor_coverage,
        specificity,
        noise_penalty,
        length_penalty,
    );

    CitationQueryQualityDebug {
        total,
        semantic_sim,
        anchor_coverage,
        specificity,
        noise_penalty,
        length_penalty,
    }
}

pub(crate) fn score_query_quality_with_embedding_provider(
    query: &str,
    selected_text: &str,
    provider: QueryEmbeddingProvider,
) -> Result<CitationQueryQualityDebug, String> {
    let mut quality = score_query_quality(query, selected_text);
    if provider == QueryEmbeddingProvider::None {
        return Ok(quality);
    }

    let embedding_sem = local_embedding_semantic_similarity(query, selected_text)?;
    quality.semantic_sim = (0.58 * embedding_sem + 0.42 * quality.semantic_sim).clamp(0.0, 1.0);
    quality.total = compose_query_quality_total(
        quality.semantic_sim,
        quality.anchor_coverage,
        quality.specificity,
        quality.noise_penalty,
        quality.length_penalty,
    );
    Ok(quality)
}

pub(crate) fn recency_score(year: Option<u16>) -> f32 {
    let Some(y) = year else {
        return 0.3;
    };
    // Favor recent papers while keeping older classics available.
    if y >= 2024 {
        1.0
    } else if y >= 2020 {
        0.85
    } else if y >= 2015 {
        0.65
    } else if y >= 2010 {
        0.45
    } else {
        0.25
    }
}

// --- Candidate scoring ---

pub(crate) fn citation_strength_score(citation_count: Option<u32>) -> f32 {
    let Some(c) = citation_count else {
        return 0.2;
    };
    let capped = c.min(5000) as f32;
    // ln(1 + c) normalized to ~[0,1]
    ((1.0 + capped).ln() / (1.0 + 5000.0f32).ln()).clamp(0.0, 1.0)
}

fn extract_query_phrases(selected_text: &str, max_count: usize) -> Vec<String> {
    let tokens = content_tokens(selected_text);
    if tokens.len() < 2 {
        return Vec::new();
    }
    let mut seen = HashSet::new();
    let mut phrases = Vec::new();
    for i in 0..(tokens.len() - 1) {
        let phrase = format!("{} {}", tokens[i], tokens[i + 1]);
        if seen.insert(phrase.clone()) {
            phrases.push(phrase);
        }
        if phrases.len() >= max_count {
            break;
        }
    }
    phrases
}

pub(crate) fn phrase_hit_score(selected_text: &str, title: &str, abstract_text: &str) -> f32 {
    let phrases = extract_query_phrases(selected_text, 8);
    if phrases.is_empty() {
        return 0.0;
    }
    let haystack = format!("{} {}", title.to_lowercase(), abstract_text.to_lowercase());
    let hit = phrases
        .iter()
        .filter(|p| haystack.contains(p.as_str()))
        .count() as f32;
    (hit / phrases.len() as f32).clamp(0.0, 1.0)
}

pub(crate) fn anchor_focus_tokens(text: &str, max_count: usize) -> Vec<String> {
    let mut scored = HashMap::<String, f32>::new();
    for (idx, token) in query_tokens(text).into_iter().enumerate() {
        if !is_anchor_token(&token) && !DOMAIN_HINT_TOKENS.contains(&token.as_str()) {
            continue;
        }
        let mut w = semantic_token_weight(&token, idx);
        if is_anchor_token(&token) {
            w += 0.6;
        }
        *scored.entry(token).or_insert(0.0) += w;
    }
    let mut items = scored.into_iter().collect::<Vec<_>>();
    items.sort_by(|a, b| b.1.total_cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    items.into_iter().take(max_count).map(|(t, _)| t).collect()
}

fn anchor_mismatch_penalty(selected_text: &str, title: &str, abstract_text: &str) -> f32 {
    let anchors = anchor_focus_tokens(selected_text, 8);
    if anchors.len() < 2 {
        return 0.0;
    }
    let evidence_tokens = content_tokens(&format!("{} {}", title, abstract_text));
    let overlap = overlap_score(&anchors, &evidence_tokens);
    if overlap >= 0.34 {
        0.0
    } else if overlap >= 0.24 {
        0.04
    } else if overlap >= 0.16 {
        0.08
    } else if overlap >= 0.10 {
        0.13
    } else {
        0.19
    }
}

fn formula_signal_penalty(selected_text: &str, title: &str, abstract_text: &str) -> f32 {
    let claim_len = content_tokens(selected_text).len();
    if claim_len == 0 {
        return 0.0;
    }
    let claim_elements = extract_formula_elements(selected_text, 12);
    if claim_elements.len() < 2 {
        return 0.0;
    }
    let evidence_elements = extract_formula_elements(&format!("{} {}", title, abstract_text), 16);
    if evidence_elements.is_empty() {
        return if claim_len <= 22 && claim_elements.len() >= 3 {
            0.14f32
        } else {
            0.11f32
        };
    }
    let hit = claim_elements.intersection(&evidence_elements).count() as f32;
    let overlap = (hit / claim_elements.len() as f32).clamp(0.0, 1.0);
    let mut base_penalty: f32 = if overlap >= 0.66 {
        0.0f32
    } else if overlap >= 0.45 {
        0.02f32
    } else if overlap >= 0.30 {
        0.06f32
    } else if overlap >= 0.16 {
        0.10f32
    } else {
        0.14f32
    };

    // For short, chemistry-dense claims we need a stronger guardrail:
    // method-word overlap (e.g. hydrothermal/nanotube) should not outrank
    // material-matched papers when formula elements barely intersect.
    if claim_len <= 24 && claim_elements.len() >= 3 {
        if overlap < 0.20 {
            base_penalty = base_penalty.max(0.16f32);
        } else if overlap < 0.34 {
            base_penalty = base_penalty.max(0.13f32);
        } else if overlap < 0.45 {
            base_penalty = base_penalty.max(0.10f32);
        }
    }

    // Keep long-claim behavior conservative to avoid overpowering richer context.
    let length_scale: f32 = if claim_len <= 18 {
        1.0f32
    } else if claim_len <= 42 {
        0.82f32
    } else {
        0.58f32
    };
    (base_penalty * length_scale).clamp(0.0f32, 0.16f32)
}

fn sentence_match_score(selected_text: &str, selected_tokens: &[String], sentence: &str) -> f32 {
    let sentence_tokens = content_tokens(sentence);
    if sentence_tokens.is_empty() {
        return 0.0;
    }
    let overlap = overlap_score(selected_tokens, &sentence_tokens);
    let phrase = phrase_hit_score(selected_text, sentence, "");
    (0.78 * overlap + 0.22 * phrase).clamp(0.0, 1.0)
}

pub(crate) fn extract_evidence_sentences(
    selected_text: &str,
    title: &str,
    abstract_text: &str,
    max_n: usize,
) -> Vec<String> {
    let selected_tokens = content_tokens(selected_text);
    if selected_tokens.is_empty() {
        return Vec::new();
    }

    let mut ranked = Vec::<(f32, String)>::new();
    let title_clean = collapse_whitespace(title);
    if !title_clean.is_empty() {
        let s = sentence_match_score(selected_text, &selected_tokens, &title_clean);
        ranked.push((s, truncate_chars(&title_clean, 260)));
    }
    for sentence in split_sentences(abstract_text) {
        let clean = collapse_whitespace(&sentence);
        if clean.len() < 18 {
            continue;
        }
        let s = sentence_match_score(selected_text, &selected_tokens, &clean);
        if s >= 0.08 {
            ranked.push((s, truncate_chars(&clean, 260)));
        }
    }
    ranked.sort_by(|a, b| b.0.total_cmp(&a.0).then_with(|| b.1.len().cmp(&a.1.len())));

    let mut out = Vec::<String>::new();
    let mut seen = HashSet::<String>::new();
    for (_, sentence) in ranked {
        let key = sentence.to_lowercase();
        if seen.insert(key) {
            out.push(sentence);
        }
        if out.len() >= max_n {
            break;
        }
    }
    out
}

// --- Contradiction detection ---

pub(crate) fn build_polarity_profile(text: &str) -> PolarityProfile {
    let tokens = tokenize_lower(text)
        .into_iter()
        .collect::<HashSet<String>>();
    let has_any = |terms: &[&str]| terms.iter().any(|term| tokens.contains(*term));

    PolarityProfile {
        upward: has_any(&[
            "increase",
            "increases",
            "improve",
            "improves",
            "enhance",
            "enhances",
            "higher",
            "elevated",
            "rise",
            "upregulate",
        ]),
        downward: has_any(&[
            "decrease",
            "decreases",
            "reduce",
            "reduces",
            "inhibit",
            "inhibits",
            "suppress",
            "suppresses",
            "lower",
            "downregulate",
            "decline",
        ]),
        positive: has_any(&[
            "beneficial",
            "effective",
            "promising",
            "improvement",
            "improved",
            "superior",
        ]),
        negative: has_any(&[
            "detrimental",
            "adverse",
            "worse",
            "inferior",
            "ineffective",
            "toxic",
            "toxicity",
        ]),
        significant: has_any(&["significant", "significantly", "robust"]),
        nonsignificant: has_any(&[
            "insignificant",
            "nonsignificant",
            "nonsignificantly",
            "marginal",
            "limited",
        ]),
    }
}

pub(crate) fn contradiction_penalty_score(
    selected_text: &str,
    title: &str,
    abstract_text: &str,
) -> f32 {
    let claim_tokens = content_tokens(selected_text);
    let evidence_tokens = content_tokens(&format!("{} {}", title, abstract_text));
    let anchor_overlap = overlap_score(&claim_tokens, &evidence_tokens);
    let anchor_hits = {
        let c = claim_tokens.iter().collect::<HashSet<_>>();
        let e = evidence_tokens.iter().collect::<HashSet<_>>();
        c.intersection(&e).count()
    };

    // Avoid penalizing weakly related candidates.
    if anchor_overlap < 0.16 || anchor_hits < 2 {
        return 0.0;
    }

    let claim = build_polarity_profile(selected_text);
    let evidence = build_polarity_profile(&format!("{} {}", title, abstract_text));

    let mut penalty = 0.0f32;
    if (claim.upward && evidence.downward) || (claim.downward && evidence.upward) {
        penalty += 0.12;
    }
    if (claim.positive && evidence.negative) || (claim.negative && evidence.positive) {
        penalty += 0.08;
    }
    if (claim.significant && evidence.nonsignificant)
        || (claim.nonsignificant && evidence.significant)
    {
        penalty += 0.07;
    }

    let overlap_factor = (0.65 + anchor_overlap.clamp(0.0, 1.0) * 0.55).clamp(0.65, 1.2);
    (penalty * overlap_factor).clamp(0.0, 0.26)
}

pub(crate) fn compute_score_explain(
    selected_text: &str,
    title: &str,
    abstract_text: &str,
    year: Option<u16>,
    citation_count: Option<u32>,
) -> CitationScoreExplain {
    let claim_tokens = content_tokens(selected_text);
    let title_tokens = content_tokens(title);
    let abs_tokens = content_tokens(abstract_text);
    let sem_title = overlap_score(&claim_tokens, &title_tokens);
    let sem_abs = overlap_score(&claim_tokens, &abs_tokens);
    let phrase = phrase_hit_score(selected_text, title, abstract_text);
    let sem = 0.54 * sem_title + 0.27 * sem_abs + 0.19 * phrase;
    let recency = recency_score(year);
    let strength = citation_strength_score(citation_count);
    let contradiction_penalty = contradiction_penalty_score(selected_text, title, abstract_text);
    let anchor_penalty = anchor_mismatch_penalty(selected_text, title, abstract_text);
    let formula_penalty = formula_signal_penalty(selected_text, title, abstract_text);
    let base_score = (0.74 * sem + 0.16 * recency + 0.10 * strength).clamp(0.0, 1.0);
    let final_score =
        (base_score - contradiction_penalty - anchor_penalty - formula_penalty).clamp(0.0, 1.0);
    CitationScoreExplain {
        sem_title,
        sem_abstract: sem_abs,
        phrase,
        recency,
        strength,
        contradiction_penalty,
        formula_penalty,
        context_factor: 1.0,
        final_score,
    }
}

// --- Provider/source/strategy score factors ---

pub(crate) fn provider_score_factor(provider: &str) -> f32 {
    match provider {
        PROVIDER_S2 => 1.02,
        PROVIDER_OPENALEX => 1.0,
        PROVIDER_CROSSREF => 0.98,
        PROVIDER_PUBMED => 1.01,
        _ => 1.0,
    }
}

pub(crate) fn source_score_factor(source: &str) -> f32 {
    match source {
        "rule" => 1.0,
        "llm" => 0.97,
        _ => 1.0,
    }
}

pub(crate) fn strategy_score_factor(strategy: &str) -> f32 {
    if strategy == "anchor_compact" {
        1.05
    } else if strategy == "semantic_focus_compact" {
        1.03
    } else if strategy == "cleaned_fulltext" {
        0.92
    } else if strategy.starts_with("salient_focus") {
        0.99
    } else if strategy == "keyword_compact" {
        0.94
    } else if strategy == "llm_precise" {
        1.03
    } else if strategy == "llm_general" {
        0.99
    } else if strategy == "llm_broad" {
        0.92
    } else if strategy.starts_with("mesh_expansion_") {
        1.01
    } else if strategy == "date_range_filtered" {
        0.96
    } else {
        1.0
    }
}

pub(crate) fn apply_query_context_scores(
    mut candidates: Vec<CitationCandidate>,
    query_plan_item: &CitationQueryPlanItem,
    provider: &'static str,
) -> Vec<CitationCandidate> {
    let quality_weight = query_plan_item.quality.total.clamp(0.0, 1.0);
    let query_weight = (0.55 * query_plan_item.weight.clamp(0.0, 1.2).min(1.0)
        + 0.45 * quality_weight)
        .clamp(0.0, 1.0);
    let provider_factor = provider_score_factor(provider);
    let source_factor = source_score_factor(&query_plan_item.source);
    let strategy_factor = strategy_score_factor(&query_plan_item.strategy);
    for c in &mut candidates {
        let base_score = c.score;
        let blended = 0.9 * base_score + 0.1 * query_weight;
        let adjusted =
            (blended * provider_factor * source_factor * strategy_factor).clamp(0.0, 1.0);
        c.score = adjusted;
        if let Some(explain) = c.score_explain.as_mut() {
            explain.context_factor = if base_score > 0.0 {
                (adjusted / base_score).clamp(0.0, 1.2)
            } else {
                (provider_factor * source_factor * strategy_factor).clamp(0.0, 1.2)
            };
            explain.final_score = adjusted;
        }
    }
    candidates.sort_by(|a, b| b.score.total_cmp(&a.score));
    candidates
}

// --- Result merging ---

pub(crate) fn normalize_title_for_key(title: &str) -> String {
    title
        .to_lowercase()
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .collect()
}

pub(crate) fn candidate_key(c: &CitationCandidate) -> String {
    if let Some(doi) = c.doi.as_ref() {
        return format!("doi:{}", doi.to_lowercase());
    }
    if !c.paper_id.trim().is_empty() {
        return format!("pid:{}", c.paper_id.to_lowercase());
    }
    format!(
        "title:{}:{}",
        normalize_title_for_key(&c.title),
        c.year.unwrap_or(0)
    )
}

pub(crate) fn merge_candidates(
    existing: Vec<CitationCandidate>,
    incoming: Vec<CitationCandidate>,
) -> Vec<CitationCandidate> {
    let mut map: HashMap<String, (CitationCandidate, usize)> = HashMap::new();
    for c in existing.into_iter().chain(incoming.into_iter()) {
        let key = candidate_key(&c);
        match map.get_mut(&key) {
            Some((prev, support_count)) => {
                *support_count += 1;
                if c.score > prev.score {
                    prev.score = c.score;
                    if c.score_explain.is_some() {
                        prev.score_explain = c.score_explain.clone();
                    }
                }
                if prev.abstract_text.is_none() && c.abstract_text.is_some() {
                    prev.abstract_text = c.abstract_text.clone();
                }
                if prev.url.is_none() && c.url.is_some() {
                    prev.url = c.url.clone();
                }
                if prev.doi.is_none() && c.doi.is_some() {
                    prev.doi = c.doi.clone();
                }
                if prev.authors.is_empty() && !c.authors.is_empty() {
                    prev.authors = c.authors.clone();
                }
                if prev.venue.is_none() && c.venue.is_some() {
                    prev.venue = c.venue.clone();
                }
                if prev.evidence_sentences.is_empty() && !c.evidence_sentences.is_empty() {
                    prev.evidence_sentences = c.evidence_sentences.clone();
                } else if !c.evidence_sentences.is_empty() {
                    let mut seen = prev
                        .evidence_sentences
                        .iter()
                        .map(|s| s.to_lowercase())
                        .collect::<HashSet<_>>();
                    for sentence in &c.evidence_sentences {
                        if seen.insert(sentence.to_lowercase()) {
                            prev.evidence_sentences.push(sentence.clone());
                        }
                        if prev.evidence_sentences.len() >= 3 {
                            break;
                        }
                    }
                }
            }
            None => {
                map.insert(key, (c, 1));
            }
        }
    }
    let mut out = map
        .into_values()
        .map(|(mut c, support_count)| {
            if support_count > 1 {
                // Repeated hits across queries/providers are weak positive evidence.
                let boost = ((support_count - 1) as f32 * 0.03).min(0.09);
                c.score = (c.score + boost).min(1.0);
            }
            c
        })
        .collect::<Vec<_>>();
    out.sort_by(|a, b| b.score.total_cmp(&a.score));
    out
}

pub(crate) fn short_query(q: &str) -> String {
    let clipped = truncate_chars(q, 60);
    if q.chars().count() > 60 {
        format!("{}...", clipped)
    } else {
        clipped
    }
}

pub(crate) fn should_stop_early(
    merged: &[CitationCandidate],
    limit: usize,
    quality_hit_queries: usize,
    attempted_queries: usize,
    min_hit_ratio: f32,
) -> Option<f32> {
    if limit == 0 {
        return Some(0.0);
    }
    if attempted_queries < 2 {
        return None;
    }
    if merged.len() < limit.saturating_mul(2) {
        return None;
    }
    let head_n = merged.len().min(limit.saturating_add(2));
    let strong = merged
        .iter()
        .take(head_n)
        .filter(|c| c.score >= 0.52)
        .count();
    if strong < ((limit as f32 * 0.6).ceil() as usize).max(3) {
        return None;
    }
    let hit_ratio = quality_hit_queries as f32 / attempted_queries as f32;
    if hit_ratio >= min_hit_ratio.clamp(0.0, 1.0) {
        Some(hit_ratio)
    } else {
        None
    }
}

pub(crate) fn has_quality_hit(candidates: &[CitationCandidate], hit_score_threshold: f32) -> bool {
    candidates
        .iter()
        .take(6)
        .any(|c| c.score >= hit_score_threshold.clamp(0.0, 1.0))
}

pub(crate) fn overlap_score(a: &[String], b: &[String]) -> f32 {
    if a.is_empty() || b.is_empty() {
        return 0.0;
    }
    let a_set: HashSet<&String> = a.iter().collect();
    let b_set: HashSet<&String> = b.iter().collect();
    let hit = a_set.intersection(&b_set).count() as f32;
    hit / a_set.len() as f32
}

pub(crate) fn token_f1_score(a: &[String], b: &[String]) -> f32 {
    if a.is_empty() || b.is_empty() {
        return 0.0;
    }
    let a_set: HashSet<&String> = a.iter().collect();
    let b_set: HashSet<&String> = b.iter().collect();
    let hit = a_set.intersection(&b_set).count() as f32;
    if hit == 0.0 {
        return 0.0;
    }
    let precision = hit / a_set.len() as f32;
    let recall = hit / b_set.len() as f32;
    let denom = precision + recall;
    if denom <= f32::EPSILON {
        0.0
    } else {
        (2.0 * precision * recall / denom).clamp(0.0, 1.0)
    }
}
