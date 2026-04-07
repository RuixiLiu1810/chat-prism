use std::collections::HashMap;
use std::time::{Duration, Instant};

use reqwest::Url;
use serde_json::Value;
use tokio::time::sleep;

use super::query::*;
use super::scoring::*;
use super::types::*;

// --- Provider runtime state management ---

pub(crate) fn provider_state(
) -> &'static std::sync::Mutex<HashMap<&'static str, ProviderRuntimeState>> {
    PROVIDER_RUNTIME_STATE.get_or_init(|| std::sync::Mutex::new(HashMap::new()))
}

fn provider_label(provider: &'static str) -> &'static str {
    match provider {
        PROVIDER_S2 => "Semantic Scholar",
        PROVIDER_OPENALEX => "OpenAlex",
        PROVIDER_CROSSREF => "Crossref",
        _ => "Provider",
    }
}

pub(crate) fn reserve_provider_request_slot(
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

pub(crate) fn mark_provider_success(provider: &'static str) {
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

pub(crate) fn mark_provider_failure(
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

// --- API response parsing ---

pub(crate) fn normalize_doi(raw: Option<String>) -> Option<String> {
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

pub(crate) fn decode_openalex_abstract(
    index: Option<&HashMap<String, Vec<usize>>>,
) -> Option<String> {
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

pub(crate) fn strip_xml_tags(input: &str) -> String {
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

// --- HTTP & retry logic ---

fn is_retryable_status(status: reqwest::StatusCode) -> bool {
    matches!(status.as_u16(), 408 | 425 | 429 | 500 | 502 | 503 | 504)
}

pub(crate) fn retry_after_seconds(headers: &reqwest::header::HeaderMap) -> Option<u64> {
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

// --- Provider search functions ---

pub(crate) async fn search_semantic_scholar(
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

pub(crate) async fn search_openalex(
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

pub(crate) async fn search_crossref(
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
