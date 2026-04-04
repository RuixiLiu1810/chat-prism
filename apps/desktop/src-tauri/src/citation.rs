use std::collections::{HashMap, HashSet};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;
use std::time::Instant;

use crate::settings;
use reqwest::Url;
use serde::de::Deserializer;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::time::sleep;

#[derive(Debug, Serialize, Clone)]
pub struct CitationCandidate {
    pub paper_id: String,
    pub title: String,
    pub year: Option<u16>,
    pub venue: Option<String>,
    pub abstract_text: Option<String>,
    pub doi: Option<String>,
    pub url: Option<String>,
    pub authors: Vec<String>,
    pub citation_count: Option<u32>,
    pub score: f32,
    pub evidence_sentences: Vec<String>,
    pub score_explain: Option<CitationScoreExplain>,
}

#[derive(Debug, Serialize, Clone)]
pub struct CitationScoreExplain {
    pub sem_title: f32,
    pub sem_abstract: f32,
    pub phrase: f32,
    pub recency: f32,
    pub strength: f32,
    pub contradiction_penalty: f32,
    pub formula_penalty: f32,
    pub context_factor: f32,
    pub final_score: f32,
}

#[derive(Debug, Serialize, Clone)]
pub struct CitationSearchAttemptDebug {
    pub query: String,
    pub provider: String,
    pub ok: bool,
    pub error: Option<String>,
    pub result_count: usize,
    pub candidates: Vec<CitationCandidate>,
}

#[derive(Debug, Serialize, Clone)]
pub struct CitationSearchDebug {
    pub selected_text: String,
    pub preprocessed_text: String,
    pub need_decision: CitationNeedDecisionDebug,
    pub latency_ms: u64,
    pub query_plan: Vec<CitationQueryPlanItem>,
    pub queries: Vec<String>,
    pub llm_query_enabled: bool,
    pub llm_query_attempted: bool,
    pub llm_query_error: Option<String>,
    pub query_embedding_provider: String,
    pub query_embedding_timeout_ms: u64,
    pub query_embedding_fallback_count: u32,
    pub query_embedding_error: Option<String>,
    pub query_execution_top_n: usize,
    pub query_execution_mmr_lambda: f32,
    pub query_execution_min_quality: f32,
    pub query_execution_min_hit_ratio: f32,
    pub query_execution_hit_score_threshold: f32,
    pub query_execution_selected_count: usize,
    pub stop_reason: Option<String>,
    pub stop_stage: Option<String>,
    pub stop_hit_ratio: Option<f32>,
    pub stop_quality_hits: usize,
    pub stop_attempted_queries: usize,
    pub stop_merged_count: usize,
    pub per_query_limit: u32,
    pub has_s2_api_key: bool,
    pub s2_rate_limited: bool,
    pub provider_budgets: Vec<CitationProviderBudgetDebug>,
    pub query_execution: Vec<CitationQueryExecutionDebug>,
    pub attempts: Vec<CitationSearchAttemptDebug>,
    pub merged_results: Vec<CitationCandidate>,
    pub final_error: Option<String>,
}

#[derive(Debug, Serialize, Clone)]
pub struct CitationNeedDecisionDebug {
    pub needs_citation: bool,
    pub level: String,
    pub claim_type: String,
    pub recommended_refs: u8,
    pub score: f32,
    pub reasons: Vec<String>,
}

#[derive(Debug, Serialize, Clone)]
pub struct CitationSearchResponse {
    pub results: Vec<CitationCandidate>,
    pub need_decision: CitationNeedDecisionDebug,
}

#[derive(Debug, Serialize, Clone)]
pub struct CitationProviderBudgetDebug {
    pub provider: String,
    pub initial: usize,
    pub used: usize,
    pub skipped_due_to_budget: usize,
    pub skipped_due_to_rate_limit: usize,
}

#[derive(Debug, Serialize, Clone)]
pub struct CitationQueryExecutionDebug {
    pub query: String,
    pub source: String,
    pub strategy: String,
    pub weight: f32,
    pub quality_score: f32,
    pub s2_status: String,
    pub openalex_status: String,
    pub crossref_status: String,
}

#[derive(Debug, Serialize, Clone, Default)]
pub struct CitationQueryQualityDebug {
    pub total: f32,
    pub semantic_sim: f32,
    pub anchor_coverage: f32,
    pub specificity: f32,
    pub noise_penalty: f32,
    pub length_penalty: f32,
}

#[derive(Debug, Serialize, Clone)]
pub struct CitationQueryPlanItem {
    pub query: String,
    pub strategy: String,
    pub source: String,
    pub weight: f32,
    pub quality: CitationQueryQualityDebug,
}

struct CitationSearchRun {
    merged_results: Vec<CitationCandidate>,
    debug: CitationSearchDebug,
    error: Option<String>,
}

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

#[derive(Debug, Deserialize)]
struct S2SearchResponse {
    #[serde(
        default,
        alias = "papers",
        deserialize_with = "deserialize_vec_or_empty"
    )]
    data: Vec<S2Paper>,
    #[serde(default)]
    error: Option<String>,
    #[serde(default)]
    message: Option<String>,
    #[serde(default)]
    code: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct S2Paper {
    paper_id: Option<String>,
    title: Option<String>,
    year: Option<u16>,
    venue: Option<String>,
    #[serde(rename = "abstract")]
    abstract_text: Option<String>,
    external_ids: Option<S2ExternalIds>,
    url: Option<String>,
    authors: Option<Vec<S2Author>>,
    citation_count: Option<u32>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct S2ExternalIds {
    doi: Option<String>,
}

#[derive(Debug, Deserialize)]
struct S2Author {
    name: Option<String>,
}

#[derive(Debug, Deserialize)]
struct OpenAlexSearchResponse {
    #[serde(default)]
    results: Vec<OpenAlexWork>,
}

#[derive(Debug, Deserialize)]
struct OpenAlexWork {
    id: Option<String>,
    display_name: Option<String>,
    publication_year: Option<u16>,
    primary_location: Option<OpenAlexPrimaryLocation>,
    doi: Option<String>,
    authorships: Option<Vec<OpenAlexAuthorship>>,
    cited_by_count: Option<u32>,
    abstract_inverted_index: Option<HashMap<String, Vec<usize>>>,
}

#[derive(Debug, Deserialize)]
struct OpenAlexPrimaryLocation {
    source: Option<OpenAlexSource>,
    landing_page_url: Option<String>,
}

#[derive(Debug, Deserialize)]
struct OpenAlexSource {
    display_name: Option<String>,
}

#[derive(Debug, Deserialize)]
struct OpenAlexAuthorship {
    author: Option<OpenAlexAuthor>,
}

#[derive(Debug, Deserialize)]
struct OpenAlexAuthor {
    display_name: Option<String>,
}

#[derive(Debug, Deserialize)]
struct CrossrefSearchResponse {
    #[serde(default)]
    message: CrossrefMessage,
}

#[derive(Debug, Deserialize, Default)]
struct CrossrefMessage {
    #[serde(default)]
    items: Vec<CrossrefWork>,
}

#[derive(Debug, Deserialize)]
struct CrossrefWork {
    #[serde(rename = "DOI")]
    doi: Option<String>,
    title: Option<Vec<String>>,
    author: Option<Vec<CrossrefAuthor>>,
    issued: Option<CrossrefIssued>,
    #[serde(rename = "container-title")]
    container_title: Option<Vec<String>>,
    #[serde(rename = "abstract")]
    abstract_text: Option<String>,
    #[serde(
        rename = "is-referenced-by-count",
        default,
        deserialize_with = "deserialize_opt_u32"
    )]
    is_referenced_by_count: Option<u32>,
    #[serde(rename = "URL")]
    url: Option<String>,
}

#[derive(Debug, Deserialize)]
struct CrossrefAuthor {
    given: Option<String>,
    family: Option<String>,
    name: Option<String>,
}

#[derive(Debug, Deserialize)]
struct CrossrefIssued {
    #[serde(rename = "date-parts")]
    date_parts: Option<Vec<Vec<Value>>>,
}

#[derive(Debug, Deserialize)]
struct LlmQueryPayload {
    queries: Option<Vec<String>>,
    queries_precise: Option<Vec<String>>,
    queries_broad: Option<Vec<String>>,
}

const EN_STOP_WORDS: &[&str] = &[
    "the", "and", "for", "with", "that", "this", "from", "into", "using", "use", "our", "their",
    "were", "was", "are", "is", "be", "can", "also", "than", "have", "has", "had", "between",
    "within", "without", "after", "before", "about", "onto", "over", "under", "such", "via",
    "through", "whose", "which", "while", "where", "when", "then", "them", "they",
];
const UNIT_TOKENS: &[&str] = &[
    "m", "mm", "cm", "nm", "um", "min", "mins", "h", "hr", "hrs", "s", "sec", "wt", "mol", "ph",
    "rpm", "v", "mv", "ma", "c", "k",
];
const DOMAIN_HINT_TOKENS: &[&str] = &[
    "hydrothermal",
    "solvothermal",
    "silar",
    "annealing",
    "calcination",
    "autoclave",
    "aqueous",
    "nanotube",
    "nanotubes",
    "nanoparticle",
    "nanoparticles",
    "substrates",
    "substrate",
    "deposition",
    "synthesized",
    "synthesis",
    "electrochemical",
    "photocatalytic",
    "electrode",
    "electrolyte",
    "doped",
    "composite",
    "oxide",
    "surface",
    "transformation",
    "adsorption",
    "catalyst",
    "catalytic",
];
const METHOD_HINT_TOKENS: &[&str] = &[
    "hydrothermal",
    "solvothermal",
    "silar",
    "anodization",
    "electrodeposition",
    "electrospinning",
    "sonication",
    "ultrasonication",
    "calcination",
    "annealing",
    "topochemical",
    "autoclave",
];
const MORPHOLOGY_HINT_TOKENS: &[&str] = &[
    "nanotube",
    "nanotubes",
    "nanorod",
    "nanorods",
    "nanosheet",
    "nanosheets",
    "nanowire",
    "nanowires",
    "nanoparticle",
    "nanoparticles",
    "film",
    "coating",
    "layer",
    "layers",
];
const CHEMICAL_SHORT_TOKENS: &[&str] = &[
    "naoh", "koh", "hcl", "hno3", "h2so4", "h2o2", "ethanol", "methanol",
];
const PROCESS_NOISE_TOKENS: &[&str] = &[
    "sample",
    "samples",
    "resulting",
    "resulted",
    "process",
    "during",
    "followed",
    "removed",
    "remove",
    "thoroughly",
    "rinsed",
    "washed",
    "dried",
    "drying",
    "contained",
    "sealed",
    "pretreated",
    "polished",
    "immersed",
    "maintained",
    "room",
    "temperature",
    "deionized",
    "water",
    "residual",
    "natural",
    "naturally",
    "method",
];
const CITATION_TRIGGER_PHRASES: &[&str] = &[
    "studies have shown",
    "previous studies",
    "it is known that",
    "has been reported",
    "was reported",
    "according to",
    "evidence suggests",
    "has been demonstrated",
    "research indicates",
    "研究表明",
    "已有研究",
    "据报道",
    "已有文献",
];
const CLAIM_TYPE_METHOD_HINTS: &[&str] = &[
    "method",
    "protocol",
    "assay",
    "silar",
    "hydrothermal",
    "calculated",
    "computed",
    "measured",
];
const CLAIM_TYPE_STAT_HINTS: &[&str] = &[
    "incidence",
    "prevalence",
    "rate",
    "ratio",
    "odds",
    "risk",
    "percent",
    "percentage",
    "cohort",
];
const CLAIM_TYPE_MECHANISM_HINTS: &[&str] = &[
    "mechanism",
    "pathway",
    "promote",
    "inhibit",
    "induce",
    "trigger",
    "mediate",
    "regulate",
];
const CLAIM_TYPE_DEFINITION_HINTS: &[&str] =
    &["defined as", "is defined as", "refers to", "is known as"];
const SELF_AUTHORED_PHRASES: &[&str] = &[
    "in this work",
    "in this paper",
    "in this study",
    "we propose",
    "we investigate",
    "we report",
    "our work",
    "本文",
    "本研究",
    "我们",
];
const CONTROVERSY_HINTS: &[&str] = &[
    "controversial",
    "conflicting",
    "inconsistent",
    "whereas",
    "争议",
    "不一致",
];

const S2_TIMEOUT_SECS: u64 = 8;
const S2_CONNECT_TIMEOUT_SECS: u64 = 3;
const S2_MIN_INTERVAL_NO_KEY_MS: u64 = 1200;
const S2_MIN_INTERVAL_WITH_KEY_MS: u64 = 250;
const OPENALEX_TIMEOUT_SECS: u64 = 8;
const OPENALEX_CONNECT_TIMEOUT_SECS: u64 = 3;
const OPENALEX_MIN_INTERVAL_MS: u64 = 120;
const CROSSREF_TIMEOUT_SECS: u64 = 8;
const CROSSREF_CONNECT_TIMEOUT_SECS: u64 = 3;
const CROSSREF_MIN_INTERVAL_MS: u64 = 180;
const PROVIDER_MAX_RETRIES: usize = 1; // total attempts = 2
const PROVIDER_CIRCUIT_THRESHOLD: u32 = 3;
const PROVIDER_CIRCUIT_BASE_COOLDOWN_SECS: u64 = 8;
const PROVIDER_CIRCUIT_MAX_COOLDOWN_SECS: u64 = 90;
const PROVIDER_S2: &str = "semantic_scholar";
const PROVIDER_OPENALEX: &str = "openalex";
const PROVIDER_CROSSREF: &str = "crossref";
const QUERY_EMBEDDING_PROVIDER_NONE: &str = "none";
const QUERY_EMBEDDING_PROVIDER_LOCAL: &str = "local_embedding";
const QUERY_EXECUTION_DEFAULT_TOP_N: usize = 5;
const QUERY_EXECUTION_DEFAULT_MIN_QUALITY: f32 = 0.24;
const QUERY_EXECUTION_DEFAULT_MMR_LAMBDA: f32 = 0.72;
const QUERY_EXECUTION_DEFAULT_MIN_HIT_RATIO: f32 = 0.45;
const QUERY_EXECUTION_DEFAULT_HIT_SCORE_THRESHOLD: f32 = 0.58;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum QueryEmbeddingProvider {
    None,
    LocalEmbedding,
}

impl QueryEmbeddingProvider {
    fn from_raw(raw: &str) -> Self {
        if raw.eq_ignore_ascii_case(QUERY_EMBEDDING_PROVIDER_LOCAL) {
            Self::LocalEmbedding
        } else {
            Self::None
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::None => QUERY_EMBEDDING_PROVIDER_NONE,
            Self::LocalEmbedding => QUERY_EMBEDDING_PROVIDER_LOCAL,
        }
    }
}

#[derive(Debug)]
struct ProviderRuntimeState {
    last_request_at: Option<Instant>,
    blocked_until: Option<Instant>,
    consecutive_failures: u32,
}

static PROVIDER_RUNTIME_STATE: OnceLock<Mutex<HashMap<&'static str, ProviderRuntimeState>>> =
    OnceLock::new();

fn provider_state() -> &'static Mutex<HashMap<&'static str, ProviderRuntimeState>> {
    PROVIDER_RUNTIME_STATE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn provider_label(provider: &'static str) -> &'static str {
    match provider {
        PROVIDER_S2 => "Semantic Scholar",
        PROVIDER_OPENALEX => "OpenAlex",
        PROVIDER_CROSSREF => "Crossref",
        _ => "Provider",
    }
}

fn reserve_provider_request_slot(
    provider: &'static str,
    min_interval: Duration,
) -> Result<Duration, String> {
    let now = Instant::now();
    let mut guard = provider_state()
        .lock()
        .map_err(|_| format!("{} runtime lock poisoned", provider_label(provider)))?;
    let entry = guard.entry(provider).or_insert(ProviderRuntimeState {
        last_request_at: None,
        blocked_until: None,
        consecutive_failures: 0,
    });

    if let Some(until) = entry.blocked_until {
        if until > now {
            let remain = until.duration_since(now).as_secs().max(1);
            return Err(format!(
                "{} is temporarily unavailable (circuit cooldown {}s).",
                provider_label(provider),
                remain
            ));
        }
    }

    let wait = guard
        .get(provider)
        .and_then(|s| s.last_request_at)
        .and_then(|last| {
            let next = last + min_interval;
            if next > now {
                Some(next.duration_since(now))
            } else {
                None
            }
        })
        .unwrap_or_default();
    if let Some(state) = guard.get_mut(provider) {
        state.last_request_at = Some(now + wait);
    }
    Ok(wait)
}

fn mark_provider_success(provider: &'static str) {
    if let Ok(mut guard) = provider_state().lock() {
        let state = guard.entry(provider).or_insert(ProviderRuntimeState {
            last_request_at: None,
            blocked_until: None,
            consecutive_failures: 0,
        });
        state.consecutive_failures = 0;
        state.blocked_until = None;
    }
}

fn mark_provider_failure(
    provider: &'static str,
    status: Option<reqwest::StatusCode>,
    retry_after_secs: Option<u64>,
) -> Option<u64> {
    let mut cooldown: Option<u64> = None;
    if let Ok(mut guard) = provider_state().lock() {
        let state = guard.entry(provider).or_insert(ProviderRuntimeState {
            last_request_at: None,
            blocked_until: None,
            consecutive_failures: 0,
        });

        if status.is_some_and(|s| s.as_u16() == 429) {
            state.consecutive_failures = state.consecutive_failures.saturating_add(1).min(10);
            let secs = retry_after_secs
                .unwrap_or(PROVIDER_CIRCUIT_BASE_COOLDOWN_SECS)
                .clamp(6, PROVIDER_CIRCUIT_MAX_COOLDOWN_SECS);
            state.blocked_until = Some(Instant::now() + Duration::from_secs(secs));
            cooldown = Some(secs);
        } else {
            state.consecutive_failures = state.consecutive_failures.saturating_add(1).min(10);
            if state.consecutive_failures >= PROVIDER_CIRCUIT_THRESHOLD {
                let exp = state.consecutive_failures - PROVIDER_CIRCUIT_THRESHOLD;
                let secs = (PROVIDER_CIRCUIT_BASE_COOLDOWN_SECS
                    .saturating_mul(2u64.saturating_pow(exp)))
                .clamp(
                    PROVIDER_CIRCUIT_BASE_COOLDOWN_SECS,
                    PROVIDER_CIRCUIT_MAX_COOLDOWN_SECS,
                );
                state.blocked_until = Some(Instant::now() + Duration::from_secs(secs));
                cooldown = Some(secs);
            }
        }
    }
    cooldown
}

fn non_empty(s: Option<String>) -> Option<String> {
    s.and_then(|v| if v.trim().is_empty() { None } else { Some(v) })
}

fn deserialize_vec_or_empty<'de, D, T>(deserializer: D) -> Result<Vec<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    Ok(Option::<Vec<T>>::deserialize(deserializer)?.unwrap_or_default())
}

fn deserialize_opt_u32<'de, D>(deserializer: D) -> Result<Option<u32>, D::Error>
where
    D: Deserializer<'de>,
{
    let raw = Option::<Value>::deserialize(deserializer)?;
    let Some(value) = raw else {
        return Ok(None);
    };

    let parsed = match value {
        Value::Number(n) => n.as_u64().and_then(|v| u32::try_from(v).ok()),
        Value::String(s) => s.trim().parse::<u32>().ok(),
        _ => None,
    };
    Ok(parsed)
}

fn tokenize_lower(s: &str) -> Vec<String> {
    s.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|t| !t.is_empty())
        .map(ToString::to_string)
        .collect()
}

fn is_stop_token(token: &str) -> bool {
    EN_STOP_WORDS.contains(&token)
}

fn is_unit_token(token: &str) -> bool {
    UNIT_TOKENS.contains(&token)
}

fn is_method_hint_token(token: &str) -> bool {
    METHOD_HINT_TOKENS.contains(&token) || token.ends_with("thermal")
}

fn is_morphology_hint_token(token: &str) -> bool {
    MORPHOLOGY_HINT_TOKENS.contains(&token)
}

fn is_process_noise_token(token: &str) -> bool {
    PROCESS_NOISE_TOKENS.contains(&token)
}

fn is_formula_like_token(token: &str) -> bool {
    has_digit(token)
        && token.len() >= 4
        && token.chars().any(|c| c.is_ascii_alphabetic())
        && !token.chars().all(|c| c.is_ascii_digit())
}

fn is_anchor_token(token: &str) -> bool {
    is_formula_like_token(token)
        || is_method_hint_token(token)
        || is_morphology_hint_token(token)
        || CHEMICAL_SHORT_TOKENS.contains(&token)
}

fn has_digit(token: &str) -> bool {
    token.chars().any(|c| c.is_ascii_digit())
}

fn is_numeric_token(token: &str) -> bool {
    !token.is_empty() && token.chars().all(|c| c.is_ascii_digit())
}

fn content_tokens(s: &str) -> Vec<String> {
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

fn parse_formula_elements(token: &str) -> Option<Vec<String>> {
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

fn extract_formula_elements(text: &str, max_elements: usize) -> HashSet<String> {
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

fn query_tokens(s: &str) -> Vec<String> {
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

fn collapse_whitespace(input: &str) -> String {
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

fn preprocess_selected_text(input: &str) -> String {
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

fn split_sentences(text: &str) -> Vec<String> {
    text.split(|c: char| ".!?;\n。！？；".contains(c))
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(ToString::to_string)
        .collect()
}

fn contains_any_phrase(haystack: &str, phrases: &[&str]) -> bool {
    phrases.iter().any(|phrase| haystack.contains(phrase))
}

fn contains_any_token(tokens: &[String], hints: &[&str]) -> bool {
    tokens.iter().any(|t| hints.contains(&t.as_str()))
}

fn classify_claim_type(lower_text: &str, tokens: &[String]) -> String {
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

fn classify_citation_need(text: &str) -> CitationNeedDecisionDebug {
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

fn build_keyword_query(text: &str) -> Option<String> {
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

fn truncate_chars(s: &str, max_chars: usize) -> String {
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

fn top_salient_sentences(text: &str, max_n: usize) -> Vec<String> {
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

fn semantic_token_weight(token: &str, position: usize) -> f32 {
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

fn build_anchor_compact_query(text: &str, max_tokens: usize) -> Option<String> {
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

fn build_semantic_focus_query(text: &str, max_tokens: usize) -> Option<String> {
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

fn build_search_query_plan(raw_selected: &str) -> Vec<CitationQueryPlanItem> {
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

    plan
}

fn append_query_plan_item(
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

async fn generate_llm_query_plan(
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

fn build_compact_query(query: &str, max_tokens: usize, fallback_chars: usize) -> String {
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

fn build_s2_compact_query(query: &str) -> String {
    build_compact_query(query, 16, 140)
}

fn build_openalex_compact_query(query: &str) -> String {
    build_compact_query(query, 18, 180)
}

fn build_crossref_compact_query(query: &str) -> String {
    build_compact_query(query, 14, 160)
}

fn normalize_title_for_key(title: &str) -> String {
    title
        .to_lowercase()
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .collect()
}

fn candidate_key(c: &CitationCandidate) -> String {
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

fn merge_candidates(
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

fn short_query(q: &str) -> String {
    let clipped = truncate_chars(q, 60);
    if q.chars().count() > 60 {
        format!("{}...", clipped)
    } else {
        clipped
    }
}

fn should_stop_early(
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

fn has_quality_hit(candidates: &[CitationCandidate], hit_score_threshold: f32) -> bool {
    candidates
        .iter()
        .take(6)
        .any(|c| c.score >= hit_score_threshold.clamp(0.0, 1.0))
}

fn query_similarity_for_mmr(a: &[String], b: &[String]) -> f32 {
    token_f1_score(a, b).max(overlap_score(a, b))
}

fn select_execution_query_plan(
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

fn overlap_score(a: &[String], b: &[String]) -> f32 {
    if a.is_empty() || b.is_empty() {
        return 0.0;
    }
    let a_set: HashSet<&String> = a.iter().collect();
    let b_set: HashSet<&String> = b.iter().collect();
    let hit = a_set.intersection(&b_set).count() as f32;
    hit / a_set.len() as f32
}

fn token_f1_score(a: &[String], b: &[String]) -> f32 {
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

fn compose_query_quality_total(
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

fn score_query_quality(query: &str, selected_text: &str) -> CitationQueryQualityDebug {
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

fn score_query_quality_with_embedding_provider(
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

fn recency_score(year: Option<u16>) -> f32 {
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

fn citation_strength_score(citation_count: Option<u32>) -> f32 {
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

fn phrase_hit_score(selected_text: &str, title: &str, abstract_text: &str) -> f32 {
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

fn anchor_focus_tokens(text: &str, max_count: usize) -> Vec<String> {
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

fn extract_evidence_sentences(
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

#[derive(Default)]
struct PolarityProfile {
    upward: bool,
    downward: bool,
    positive: bool,
    negative: bool,
    significant: bool,
    nonsignificant: bool,
}

fn build_polarity_profile(text: &str) -> PolarityProfile {
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

fn contradiction_penalty_score(selected_text: &str, title: &str, abstract_text: &str) -> f32 {
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

fn compute_score_explain(
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

fn provider_score_factor(provider: &str) -> f32 {
    match provider {
        PROVIDER_S2 => 1.02,
        PROVIDER_OPENALEX => 1.0,
        PROVIDER_CROSSREF => 0.98,
        _ => 1.0,
    }
}

fn source_score_factor(source: &str) -> f32 {
    match source {
        "rule" => 1.0,
        "llm" => 0.97,
        _ => 1.0,
    }
}

fn strategy_score_factor(strategy: &str) -> f32 {
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
    } else {
        1.0
    }
}

fn apply_query_context_scores(
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

fn normalize_doi(raw: Option<String>) -> Option<String> {
    let doi = raw?.trim().to_string();
    if doi.is_empty() {
        return None;
    }
    let lower = doi.to_lowercase();
    if let Some(stripped) = lower.strip_prefix("https://doi.org/") {
        return Some(stripped.to_string());
    }
    if let Some(stripped) = lower.strip_prefix("http://doi.org/") {
        return Some(stripped.to_string());
    }
    Some(doi)
}

fn decode_openalex_abstract(index: Option<&HashMap<String, Vec<usize>>>) -> Option<String> {
    let idx = index?;
    let max_pos = idx.values().flat_map(|v| v.iter().copied()).max()?;
    let mut words = vec![String::new(); max_pos + 1];
    for (word, positions) in idx {
        for pos in positions {
            if *pos < words.len() && words[*pos].is_empty() {
                words[*pos] = word.clone();
            }
        }
    }
    let joined = words
        .into_iter()
        .filter(|w| !w.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    if joined.trim().is_empty() {
        None
    } else {
        Some(joined)
    }
}

fn strip_xml_tags(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut in_tag = false;
    for ch in input.chars() {
        if ch == '<' {
            in_tag = true;
            out.push(' ');
            continue;
        }
        if ch == '>' {
            in_tag = false;
            out.push(' ');
            continue;
        }
        if !in_tag {
            out.push(ch);
        }
    }
    collapse_whitespace(&out)
}

fn crossref_year(issued: Option<&CrossrefIssued>) -> Option<u16> {
    let year_i32 = issued
        .and_then(|i| i.date_parts.as_ref())
        .and_then(|parts| parts.first())
        .and_then(|part| part.first())
        .and_then(|v| match v {
            Value::Number(n) => n.as_i64().and_then(|x| i32::try_from(x).ok()),
            Value::String(s) => s.trim().parse::<i32>().ok(),
            _ => None,
        })?;
    if year_i32 <= 0 || year_i32 > u16::MAX as i32 {
        None
    } else {
        Some(year_i32 as u16)
    }
}

fn crossref_title(work: &CrossrefWork) -> Option<String> {
    work.title
        .as_ref()
        .and_then(|titles| titles.iter().find(|t| !t.trim().is_empty()))
        .map(|t| t.trim().to_string())
}

fn crossref_venue(work: &CrossrefWork) -> Option<String> {
    work.container_title
        .as_ref()
        .and_then(|titles| titles.iter().find(|t| !t.trim().is_empty()))
        .map(|t| t.trim().to_string())
}

fn crossref_authors(work: &CrossrefWork) -> Vec<String> {
    work.author
        .as_ref()
        .map(|authors| {
            authors
                .iter()
                .filter_map(|a| {
                    if let Some(name) = a.name.as_ref().map(|n| n.trim()).filter(|n| !n.is_empty())
                    {
                        return Some(name.to_string());
                    }
                    let given = a.given.as_deref().unwrap_or("").trim();
                    let family = a.family.as_deref().unwrap_or("").trim();
                    let full = format!("{} {}", given, family).trim().to_string();
                    if full.is_empty() {
                        None
                    } else {
                        Some(full)
                    }
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default()
}

fn is_retryable_status(status: reqwest::StatusCode) -> bool {
    matches!(status.as_u16(), 408 | 425 | 429 | 500 | 502 | 503 | 504)
}

fn retry_after_seconds(headers: &reqwest::header::HeaderMap) -> Option<u64> {
    let raw = headers
        .get(reqwest::header::RETRY_AFTER)?
        .to_str()
        .ok()?
        .trim();
    raw.parse::<u64>().ok().map(|s| s.clamp(1, 30))
}

fn retry_delay_for_status(
    status: reqwest::StatusCode,
    attempt: usize,
    retry_after_secs: Option<u64>,
) -> Duration {
    if status.as_u16() == 429 {
        if let Some(secs) = retry_after_secs {
            return Duration::from_secs(secs);
        }
        return Duration::from_millis((700 + attempt as u64 * 900).min(3_000));
    }
    Duration::from_millis((350 + attempt as u64 * 700).min(2_000))
}

fn retry_delay_for_network(attempt: usize) -> Duration {
    Duration::from_millis((400 + attempt as u64 * 800).min(2_500))
}

fn is_retryable_reqwest_error(err: &reqwest::Error) -> bool {
    err.is_timeout() || err.is_connect() || err.is_request()
}

fn build_http_client(
    connect_timeout_secs: u64,
    timeout_secs: u64,
) -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(connect_timeout_secs))
        .timeout(Duration::from_secs(timeout_secs))
        .build()
        .map_err(|e| format!("Failed to initialize HTTP client: {}", e))
}

async fn search_semantic_scholar(
    query: &str,
    score_basis: &str,
    limit: u32,
    semantic_scholar_api_key: Option<&str>,
) -> Result<Vec<CitationCandidate>, String> {
    let mut url = Url::parse("https://api.semanticscholar.org/graph/v1/paper/search")
        .map_err(|e| format!("Failed to build Semantic Scholar URL: {}", e))?;
    url.query_pairs_mut()
        .append_pair("query", query)
        .append_pair("limit", &limit.to_string())
        .append_pair(
            "fields",
            "paperId,title,year,venue,abstract,externalIds,url,authors,citationCount",
        );

    let has_s2_api_key = semantic_scholar_api_key.is_some();

    let client = build_http_client(S2_CONNECT_TIMEOUT_SECS, S2_TIMEOUT_SECS)
        .map_err(|e| format!("Failed to initialize Semantic Scholar HTTP client: {}", e))?;

    let mut last_err: Option<String> = None;

    for attempt in 0..=PROVIDER_MAX_RETRIES {
        let min_interval = if has_s2_api_key {
            Duration::from_millis(S2_MIN_INTERVAL_WITH_KEY_MS)
        } else {
            Duration::from_millis(S2_MIN_INTERVAL_NO_KEY_MS)
        };
        let wait = reserve_provider_request_slot(PROVIDER_S2, min_interval)?;
        if !wait.is_zero() {
            sleep(wait).await;
        }

        let mut request = client
            .get(url.clone())
            .header("Accept", "application/json")
            .header("User-Agent", "claude-prism/1.1 (+local)");
        if let Some(key) = semantic_scholar_api_key {
            request = request.header("x-api-key", key);
        }

        match request.send().await {
            Ok(response) => {
                let status = response.status();
                let headers = response.headers().clone();
                if !status.is_success() {
                    let body_preview = response
                        .text()
                        .await
                        .unwrap_or_default()
                        .chars()
                        .take(220)
                        .collect::<String>();
                    let mut err = format!(
                        "Semantic Scholar request failed with status {}{}",
                        status,
                        if body_preview.is_empty() {
                            "".to_string()
                        } else {
                            format!(": {}", body_preview)
                        }
                    );
                    if status.as_u16() == 429 && !has_s2_api_key {
                        err.push_str(
                            ". Tip: set S2_API_KEY to increase rate limits (https://www.semanticscholar.org/product/api).",
                        );
                    }
                    let cooldown = mark_provider_failure(
                        PROVIDER_S2,
                        Some(status),
                        retry_after_seconds(&headers),
                    );
                    if let Some(secs) = cooldown {
                        err.push_str(&format!(" Circuit cooldown applied: {}s.", secs));
                    }

                    if is_retryable_status(status) && attempt < PROVIDER_MAX_RETRIES {
                        let delay =
                            retry_delay_for_status(status, attempt, retry_after_seconds(&headers));
                        last_err = Some(err.clone());
                        sleep(delay).await;
                        continue;
                    }
                    return Err(err);
                }

                let body = response
                    .text()
                    .await
                    .map_err(|e| format!("Failed to read Semantic Scholar response body: {}", e))?;
                let parsed: S2SearchResponse = serde_json::from_str(&body).map_err(|e| {
                    let preview = body.chars().take(220).collect::<String>();
                    if preview.is_empty() {
                        format!("Failed to parse Semantic Scholar response JSON: {}", e)
                    } else {
                        format!(
                            "Failed to parse Semantic Scholar response JSON: {} | body: {}",
                            e, preview
                        )
                    }
                })?;
                let detail = non_empty(parsed.message)
                    .or(non_empty(parsed.error))
                    .or(non_empty(parsed.code));
                if parsed.data.is_empty() {
                    if let Some(reason) = detail {
                        return Err(format!("Semantic Scholar response error: {}", reason));
                    }
                }
                let papers = parsed.data;

                let mut candidates: Vec<CitationCandidate> = papers
                    .into_iter()
                    .filter_map(|p| {
                        let title = p.title.unwrap_or_default();
                        if title.trim().is_empty() {
                            return None;
                        }
                        let abstract_text = p.abstract_text.unwrap_or_default();
                        let score_explain = compute_score_explain(
                            score_basis,
                            &title,
                            &abstract_text,
                            p.year,
                            p.citation_count,
                        );
                        let score = score_explain.final_score;
                        let evidence_sentences =
                            extract_evidence_sentences(score_basis, &title, &abstract_text, 2);

                        Some(CitationCandidate {
                            paper_id: p.paper_id.unwrap_or_default(),
                            title,
                            year: p.year,
                            venue: non_empty(p.venue),
                            abstract_text: if abstract_text.trim().is_empty() {
                                None
                            } else {
                                Some(abstract_text)
                            },
                            doi: p.external_ids.and_then(|ids| ids.doi),
                            url: non_empty(p.url),
                            authors: p
                                .authors
                                .unwrap_or_default()
                                .into_iter()
                                .filter_map(|a| a.name)
                                .filter(|n| !n.trim().is_empty())
                                .collect(),
                            citation_count: p.citation_count,
                            score,
                            evidence_sentences,
                            score_explain: Some(score_explain),
                        })
                    })
                    .collect();

                mark_provider_success(PROVIDER_S2);
                candidates.sort_by(|a, b| b.score.total_cmp(&a.score));
                return Ok(candidates);
            }
            Err(err) => {
                let mut err_msg = format!("Semantic Scholar request failed: {}", err);
                let cooldown = mark_provider_failure(PROVIDER_S2, None, None);
                if is_retryable_reqwest_error(&err) && attempt < PROVIDER_MAX_RETRIES {
                    last_err = Some(err_msg.clone());
                    sleep(retry_delay_for_network(attempt)).await;
                    continue;
                }
                if let Some(secs) = cooldown {
                    err_msg.push_str(&format!(" Circuit cooldown applied: {}s.", secs));
                }
                return Err(err_msg);
            }
        }
    }

    Err(last_err.unwrap_or_else(|| "Semantic Scholar request failed.".to_string()))
}

async fn search_openalex(
    query: &str,
    score_basis: &str,
    limit: u32,
) -> Result<Vec<CitationCandidate>, String> {
    let mut url = Url::parse("https://api.openalex.org/works")
        .map_err(|e| format!("Failed to build OpenAlex URL: {}", e))?;
    url.query_pairs_mut()
        .append_pair("search", query)
        .append_pair("per-page", &limit.to_string());

    let client = build_http_client(OPENALEX_CONNECT_TIMEOUT_SECS, OPENALEX_TIMEOUT_SECS)
        .map_err(|e| format!("Failed to initialize OpenAlex HTTP client: {}", e))?;

    let mut last_err: Option<String> = None;
    for attempt in 0..=PROVIDER_MAX_RETRIES {
        let wait = reserve_provider_request_slot(
            PROVIDER_OPENALEX,
            Duration::from_millis(OPENALEX_MIN_INTERVAL_MS),
        )?;
        if !wait.is_zero() {
            sleep(wait).await;
        }

        match client
            .get(url.clone())
            .header("Accept", "application/json")
            .header("User-Agent", "claude-prism/1.1 (+local)")
            .send()
            .await
        {
            Ok(response) => {
                let status = response.status();
                let headers = response.headers().clone();
                if !status.is_success() {
                    let body_preview = response
                        .text()
                        .await
                        .unwrap_or_default()
                        .chars()
                        .take(220)
                        .collect::<String>();
                    let mut err = format!(
                        "OpenAlex request failed with status {}{}",
                        status,
                        if body_preview.is_empty() {
                            "".to_string()
                        } else {
                            format!(": {}", body_preview)
                        }
                    );
                    let cooldown = mark_provider_failure(
                        PROVIDER_OPENALEX,
                        Some(status),
                        retry_after_seconds(&headers),
                    );
                    if let Some(secs) = cooldown {
                        err.push_str(&format!(" Circuit cooldown applied: {}s.", secs));
                    }

                    if is_retryable_status(status) && attempt < PROVIDER_MAX_RETRIES {
                        last_err = Some(err.clone());
                        sleep(retry_delay_for_status(
                            status,
                            attempt,
                            retry_after_seconds(&headers),
                        ))
                        .await;
                        continue;
                    }
                    return Err(err);
                }

                let body = response
                    .text()
                    .await
                    .map_err(|e| format!("Failed to read OpenAlex response body: {}", e))?;
                let parsed: OpenAlexSearchResponse = serde_json::from_str(&body)
                    .map_err(|e| format!("Failed to parse OpenAlex response JSON: {}", e))?;

                let mut candidates: Vec<CitationCandidate> = parsed
                    .results
                    .into_iter()
                    .filter_map(|w| {
                        let title = w.display_name.unwrap_or_default();
                        if title.trim().is_empty() {
                            return None;
                        }

                        let abstract_text =
                            decode_openalex_abstract(w.abstract_inverted_index.as_ref());
                        let abstract_for_score = abstract_text.as_deref().unwrap_or("");
                        let score_explain = compute_score_explain(
                            score_basis,
                            &title,
                            abstract_for_score,
                            w.publication_year,
                            w.cited_by_count,
                        );
                        let score = score_explain.final_score;
                        let evidence_sentences =
                            extract_evidence_sentences(score_basis, &title, abstract_for_score, 2);

                        let venue = w
                            .primary_location
                            .as_ref()
                            .and_then(|loc| loc.source.as_ref())
                            .and_then(|src| src.display_name.clone());
                        let url = w
                            .primary_location
                            .as_ref()
                            .and_then(|loc| loc.landing_page_url.clone())
                            .or(w.id.clone());
                        let authors = w
                            .authorships
                            .unwrap_or_default()
                            .into_iter()
                            .filter_map(|a| a.author.and_then(|x| x.display_name))
                            .filter(|n| !n.trim().is_empty())
                            .collect::<Vec<_>>();

                        Some(CitationCandidate {
                            paper_id: w.id.unwrap_or_default(),
                            title,
                            year: w.publication_year,
                            venue: non_empty(venue),
                            abstract_text: abstract_text.filter(|t| !t.trim().is_empty()),
                            doi: normalize_doi(w.doi),
                            url: non_empty(url),
                            authors,
                            citation_count: w.cited_by_count,
                            score,
                            evidence_sentences,
                            score_explain: Some(score_explain),
                        })
                    })
                    .collect();

                mark_provider_success(PROVIDER_OPENALEX);
                candidates.sort_by(|a, b| b.score.total_cmp(&a.score));
                return Ok(candidates);
            }
            Err(err) => {
                let mut err_msg = format!("OpenAlex request failed: {}", err);
                let cooldown = mark_provider_failure(PROVIDER_OPENALEX, None, None);
                if is_retryable_reqwest_error(&err) && attempt < PROVIDER_MAX_RETRIES {
                    last_err = Some(err_msg.clone());
                    sleep(retry_delay_for_network(attempt)).await;
                    continue;
                }
                if let Some(secs) = cooldown {
                    err_msg.push_str(&format!(" Circuit cooldown applied: {}s.", secs));
                }
                return Err(err_msg);
            }
        }
    }

    Err(last_err.unwrap_or_else(|| "OpenAlex request failed.".to_string()))
}

async fn search_crossref(
    query: &str,
    score_basis: &str,
    limit: u32,
) -> Result<Vec<CitationCandidate>, String> {
    let mut url = Url::parse("https://api.crossref.org/works")
        .map_err(|e| format!("Failed to build Crossref URL: {}", e))?;
    url.query_pairs_mut()
        .append_pair("query.bibliographic", query)
        .append_pair("rows", &limit.to_string())
        .append_pair(
            "select",
            "DOI,title,author,issued,container-title,abstract,is-referenced-by-count,URL",
        );
    if let Ok(mailto) = std::env::var("CROSSREF_MAILTO") {
        let trimmed = mailto.trim();
        if !trimmed.is_empty() {
            url.query_pairs_mut().append_pair("mailto", trimmed);
        }
    }

    let client = build_http_client(CROSSREF_CONNECT_TIMEOUT_SECS, CROSSREF_TIMEOUT_SECS)
        .map_err(|e| format!("Failed to initialize Crossref HTTP client: {}", e))?;

    let mut last_err: Option<String> = None;
    for attempt in 0..=PROVIDER_MAX_RETRIES {
        let wait = reserve_provider_request_slot(
            PROVIDER_CROSSREF,
            Duration::from_millis(CROSSREF_MIN_INTERVAL_MS),
        )?;
        if !wait.is_zero() {
            sleep(wait).await;
        }

        match client
            .get(url.clone())
            .header("Accept", "application/json")
            .header("User-Agent", "claude-prism/1.1 (+local)")
            .send()
            .await
        {
            Ok(response) => {
                let status = response.status();
                let headers = response.headers().clone();
                if !status.is_success() {
                    let body_preview = response
                        .text()
                        .await
                        .unwrap_or_default()
                        .chars()
                        .take(220)
                        .collect::<String>();
                    let mut err = format!(
                        "Crossref request failed with status {}{}",
                        status,
                        if body_preview.is_empty() {
                            "".to_string()
                        } else {
                            format!(": {}", body_preview)
                        }
                    );
                    let cooldown = mark_provider_failure(
                        PROVIDER_CROSSREF,
                        Some(status),
                        retry_after_seconds(&headers),
                    );
                    if let Some(secs) = cooldown {
                        err.push_str(&format!(" Circuit cooldown applied: {}s.", secs));
                    }

                    if is_retryable_status(status) && attempt < PROVIDER_MAX_RETRIES {
                        last_err = Some(err.clone());
                        sleep(retry_delay_for_status(
                            status,
                            attempt,
                            retry_after_seconds(&headers),
                        ))
                        .await;
                        continue;
                    }
                    return Err(err);
                }

                let body = response
                    .text()
                    .await
                    .map_err(|e| format!("Failed to read Crossref response body: {}", e))?;
                let parsed: CrossrefSearchResponse = serde_json::from_str(&body).map_err(|e| {
                    let preview = body.chars().take(220).collect::<String>();
                    if preview.is_empty() {
                        format!("Failed to parse Crossref response JSON: {}", e)
                    } else {
                        format!(
                            "Failed to parse Crossref response JSON: {} | body: {}",
                            e, preview
                        )
                    }
                })?;

                let mut candidates: Vec<CitationCandidate> = parsed
                    .message
                    .items
                    .into_iter()
                    .filter_map(|w| {
                        let title = crossref_title(&w)?;
                        let abstract_text = w
                            .abstract_text
                            .as_deref()
                            .map(strip_xml_tags)
                            .filter(|s| !s.trim().is_empty());
                        let year = crossref_year(w.issued.as_ref());
                        let venue = non_empty(crossref_venue(&w));
                        let authors = crossref_authors(&w);
                        let doi = normalize_doi(w.doi.clone());
                        let url = non_empty(w.url.clone());
                        let abstract_for_score = abstract_text.as_deref().unwrap_or("");
                        let score_explain = compute_score_explain(
                            score_basis,
                            &title,
                            abstract_for_score,
                            year,
                            w.is_referenced_by_count,
                        );
                        let score = score_explain.final_score;
                        let evidence_sentences =
                            extract_evidence_sentences(score_basis, &title, abstract_for_score, 2);
                        let paper_id = doi.clone().or_else(|| url.clone()).unwrap_or_else(|| {
                            format!("crossref:{}", normalize_title_for_key(&title))
                        });

                        Some(CitationCandidate {
                            paper_id,
                            title,
                            year,
                            venue,
                            abstract_text,
                            doi,
                            url,
                            authors,
                            citation_count: w.is_referenced_by_count,
                            score,
                            evidence_sentences,
                            score_explain: Some(score_explain),
                        })
                    })
                    .collect();

                mark_provider_success(PROVIDER_CROSSREF);
                candidates.sort_by(|a, b| b.score.total_cmp(&a.score));
                return Ok(candidates);
            }
            Err(err) => {
                let mut err_msg = format!("Crossref request failed: {}", err);
                let cooldown = mark_provider_failure(PROVIDER_CROSSREF, None, None);
                if is_retryable_reqwest_error(&err) && attempt < PROVIDER_MAX_RETRIES {
                    last_err = Some(err_msg.clone());
                    sleep(retry_delay_for_network(attempt)).await;
                    continue;
                }
                if let Some(secs) = cooldown {
                    err_msg.push_str(&format!(" Circuit cooldown applied: {}s.", secs));
                }
                return Err(err_msg);
            }
        }
    }

    Err(last_err.unwrap_or_else(|| "Crossref request failed.".to_string()))
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
    let mut openalex_rate_limited = false;
    let mut crossref_rate_limited = false;
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
            stop_stage = Some("after_crossref".to_string());
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
    ];

    if merged.is_empty() {
        // Semantic Scholar can be rate-limited (429). If OpenAlex/Crossref responded but found no hit,
        // treat as normal "no match" rather than hard failure.
        if openalex_responded || crossref_responded {
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
    use super::compute_score_explain;

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
