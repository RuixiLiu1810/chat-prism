use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::Instant;

use serde::de::Deserializer;
use serde::{Deserialize, Serialize};
use serde_json::Value;

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

pub(crate) struct CitationSearchRun {
    pub merged_results: Vec<CitationCandidate>,
    pub debug: CitationSearchDebug,
    pub error: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct S2SearchResponse {
    #[serde(
        default,
        alias = "papers",
        deserialize_with = "deserialize_vec_or_empty"
    )]
    pub data: Vec<S2Paper>,
    #[serde(default)]
    pub error: Option<String>,
    #[serde(default)]
    pub message: Option<String>,
    #[serde(default)]
    pub code: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct S2Paper {
    pub paper_id: Option<String>,
    pub title: Option<String>,
    pub year: Option<u16>,
    pub venue: Option<String>,
    #[serde(rename = "abstract")]
    pub abstract_text: Option<String>,
    pub external_ids: Option<S2ExternalIds>,
    pub url: Option<String>,
    pub authors: Option<Vec<S2Author>>,
    pub citation_count: Option<u32>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct S2ExternalIds {
    pub doi: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct S2Author {
    pub name: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct OpenAlexSearchResponse {
    #[serde(default)]
    pub results: Vec<OpenAlexWork>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct OpenAlexWork {
    pub id: Option<String>,
    pub display_name: Option<String>,
    pub publication_year: Option<u16>,
    pub primary_location: Option<OpenAlexPrimaryLocation>,
    pub doi: Option<String>,
    pub authorships: Option<Vec<OpenAlexAuthorship>>,
    pub cited_by_count: Option<u32>,
    pub abstract_inverted_index: Option<HashMap<String, Vec<usize>>>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct OpenAlexPrimaryLocation {
    pub source: Option<OpenAlexSource>,
    pub landing_page_url: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct OpenAlexSource {
    pub display_name: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct OpenAlexAuthorship {
    pub author: Option<OpenAlexAuthor>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct OpenAlexAuthor {
    pub display_name: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct CrossrefSearchResponse {
    #[serde(default)]
    pub message: CrossrefMessage,
}

#[derive(Debug, Deserialize, Default)]
pub(crate) struct CrossrefMessage {
    #[serde(default)]
    pub items: Vec<CrossrefWork>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct CrossrefWork {
    #[serde(rename = "DOI")]
    pub doi: Option<String>,
    pub title: Option<Vec<String>>,
    pub author: Option<Vec<CrossrefAuthor>>,
    pub issued: Option<CrossrefIssued>,
    #[serde(rename = "container-title")]
    pub container_title: Option<Vec<String>>,
    #[serde(rename = "abstract")]
    pub abstract_text: Option<String>,
    #[serde(
        rename = "is-referenced-by-count",
        default,
        deserialize_with = "deserialize_opt_u32"
    )]
    pub is_referenced_by_count: Option<u32>,
    #[serde(rename = "URL")]
    pub url: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct CrossrefAuthor {
    pub given: Option<String>,
    pub family: Option<String>,
    pub name: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct CrossrefIssued {
    #[serde(rename = "date-parts")]
    pub date_parts: Option<Vec<Vec<Value>>>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct LlmQueryPayload {
    pub queries: Option<Vec<String>>,
    pub queries_precise: Option<Vec<String>>,
    pub queries_broad: Option<Vec<String>>,
}

// --- Constants ---

pub(crate) const EN_STOP_WORDS: &[&str] = &[
    "the", "and", "for", "with", "that", "this", "from", "into", "using", "use", "our", "their",
    "were", "was", "are", "is", "be", "can", "also", "than", "have", "has", "had", "between",
    "within", "without", "after", "before", "about", "onto", "over", "under", "such", "via",
    "through", "whose", "which", "while", "where", "when", "then", "them", "they",
];
pub(crate) const UNIT_TOKENS: &[&str] = &[
    "m", "mm", "cm", "nm", "um", "min", "mins", "h", "hr", "hrs", "s", "sec", "wt", "mol", "ph",
    "rpm", "v", "mv", "ma", "c", "k",
];
pub(crate) const DOMAIN_HINT_TOKENS: &[&str] = &[
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
pub(crate) const METHOD_HINT_TOKENS: &[&str] = &[
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
pub(crate) const MORPHOLOGY_HINT_TOKENS: &[&str] = &[
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
pub(crate) const CHEMICAL_SHORT_TOKENS: &[&str] = &[
    "naoh", "koh", "hcl", "hno3", "h2so4", "h2o2", "ethanol", "methanol",
];
pub(crate) const PROCESS_NOISE_TOKENS: &[&str] = &[
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
pub(crate) const CITATION_TRIGGER_PHRASES: &[&str] = &[
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
pub(crate) const CLAIM_TYPE_METHOD_HINTS: &[&str] = &[
    "method",
    "protocol",
    "assay",
    "silar",
    "hydrothermal",
    "calculated",
    "computed",
    "measured",
];
pub(crate) const CLAIM_TYPE_STAT_HINTS: &[&str] = &[
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
pub(crate) const CLAIM_TYPE_MECHANISM_HINTS: &[&str] = &[
    "mechanism",
    "pathway",
    "promote",
    "inhibit",
    "induce",
    "trigger",
    "mediate",
    "regulate",
];
pub(crate) const CLAIM_TYPE_DEFINITION_HINTS: &[&str] =
    &["defined as", "is defined as", "refers to", "is known as"];
pub(crate) const SELF_AUTHORED_PHRASES: &[&str] = &[
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
pub(crate) const CONTROVERSY_HINTS: &[&str] = &[
    "controversial",
    "conflicting",
    "inconsistent",
    "whereas",
    "争议",
    "不一致",
];

pub(crate) const S2_TIMEOUT_SECS: u64 = 8;
pub(crate) const S2_CONNECT_TIMEOUT_SECS: u64 = 3;
pub(crate) const S2_MIN_INTERVAL_NO_KEY_MS: u64 = 1200;
pub(crate) const S2_MIN_INTERVAL_WITH_KEY_MS: u64 = 250;
pub(crate) const OPENALEX_TIMEOUT_SECS: u64 = 8;
pub(crate) const OPENALEX_CONNECT_TIMEOUT_SECS: u64 = 3;
pub(crate) const OPENALEX_MIN_INTERVAL_MS: u64 = 120;
pub(crate) const CROSSREF_TIMEOUT_SECS: u64 = 8;
pub(crate) const CROSSREF_CONNECT_TIMEOUT_SECS: u64 = 3;
pub(crate) const CROSSREF_MIN_INTERVAL_MS: u64 = 180;
pub(crate) const PROVIDER_MAX_RETRIES: usize = 1; // total attempts = 2
pub(crate) const PROVIDER_CIRCUIT_THRESHOLD: u32 = 3;
pub(crate) const PROVIDER_CIRCUIT_BASE_COOLDOWN_SECS: u64 = 8;
pub(crate) const PROVIDER_CIRCUIT_MAX_COOLDOWN_SECS: u64 = 90;
pub(crate) const PROVIDER_S2: &str = "semantic_scholar";
pub(crate) const PROVIDER_OPENALEX: &str = "openalex";
pub(crate) const PROVIDER_CROSSREF: &str = "crossref";
pub(crate) const QUERY_EMBEDDING_PROVIDER_NONE: &str = "none";
pub(crate) const QUERY_EMBEDDING_PROVIDER_LOCAL: &str = "local_embedding";
pub(crate) const QUERY_EXECUTION_DEFAULT_TOP_N: usize = 5;
pub(crate) const QUERY_EXECUTION_DEFAULT_MIN_QUALITY: f32 = 0.24;
pub(crate) const QUERY_EXECUTION_DEFAULT_MMR_LAMBDA: f32 = 0.72;
pub(crate) const QUERY_EXECUTION_DEFAULT_MIN_HIT_RATIO: f32 = 0.45;
pub(crate) const QUERY_EXECUTION_DEFAULT_HIT_SCORE_THRESHOLD: f32 = 0.58;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum QueryEmbeddingProvider {
    None,
    LocalEmbedding,
}

impl QueryEmbeddingProvider {
    pub fn from_raw(raw: &str) -> Self {
        if raw.eq_ignore_ascii_case(QUERY_EMBEDDING_PROVIDER_LOCAL) {
            Self::LocalEmbedding
        } else {
            Self::None
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::None => QUERY_EMBEDDING_PROVIDER_NONE,
            Self::LocalEmbedding => QUERY_EMBEDDING_PROVIDER_LOCAL,
        }
    }
}

#[derive(Debug)]
pub(crate) struct ProviderRuntimeState {
    pub last_request_at: Option<Instant>,
    pub blocked_until: Option<Instant>,
    pub consecutive_failures: u32,
}

pub(crate) static PROVIDER_RUNTIME_STATE: OnceLock<
    Mutex<HashMap<&'static str, ProviderRuntimeState>>,
> = OnceLock::new();

// --- Helper deserialization / utility functions ---

pub(crate) fn non_empty(s: Option<String>) -> Option<String> {
    s.and_then(|v| if v.trim().is_empty() { None } else { Some(v) })
}

pub(crate) fn deserialize_vec_or_empty<'de, D, T>(deserializer: D) -> Result<Vec<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    Ok(Option::<Vec<T>>::deserialize(deserializer)?.unwrap_or_default())
}

pub(crate) fn deserialize_opt_u32<'de, D>(deserializer: D) -> Result<Option<u32>, D::Error>
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

#[derive(Default)]
pub(crate) struct PolarityProfile {
    pub upward: bool,
    pub downward: bool,
    pub positive: bool,
    pub negative: bool,
    pub significant: bool,
    pub nonsignificant: bool,
}
