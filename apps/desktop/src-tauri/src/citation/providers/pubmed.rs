use std::time::Duration;

use reqwest::Url;
use serde_json::Value;
use tokio::time::sleep;

use super::super::scoring::{compute_score_explain, extract_evidence_sentences};
use super::super::types::{
    CitationCandidate, PROVIDER_MAX_RETRIES, PROVIDER_PUBMED, PUBMED_CONNECT_TIMEOUT_SECS,
    PUBMED_MIN_INTERVAL_MS, PUBMED_TIMEOUT_SECS,
};
use super::{
    build_http_client, is_retryable_reqwest_error, is_retryable_status, mark_provider_failure,
    mark_provider_success, normalize_doi, reserve_provider_request_slot, retry_after_seconds,
    retry_delay_for_network, retry_delay_for_status,
};

fn parse_pubmed_year(pubdate: Option<&str>) -> Option<u16> {
    let raw = pubdate?.trim();
    let mut digits = String::new();
    for ch in raw.chars() {
        if ch.is_ascii_digit() {
            digits.push(ch);
            if digits.len() == 4 {
                break;
            }
        } else if !digits.is_empty() {
            break;
        }
    }
    if digits.len() != 4 {
        return None;
    }
    let year = digits.parse::<u16>().ok()?;
    if year == 0 {
        None
    } else {
        Some(year)
    }
}

fn pubmed_doi(summary_item: &Value) -> Option<String> {
    let article_ids = summary_item.get("articleids")?.as_array()?;
    for item in article_ids {
        let id_type = item
            .get("idtype")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if id_type.eq_ignore_ascii_case("doi") {
            let value = item
                .get("value")
                .and_then(Value::as_str)
                .map(str::to_string);
            if value.is_some() {
                return normalize_doi(value);
            }
        }
    }
    None
}

fn pubmed_authors(summary_item: &Value) -> Vec<String> {
    summary_item
        .get("authors")
        .and_then(Value::as_array)
        .map(|authors| {
            authors
                .iter()
                .filter_map(|author| author.get("name").and_then(Value::as_str))
                .map(str::trim)
                .filter(|name| !name.is_empty())
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default()
}

pub(crate) async fn search_pubmed(
    query: &str,
    score_basis: &str,
    limit: u32,
    min_year: Option<u16>,
    max_year: Option<u16>,
) -> Result<Vec<CitationCandidate>, String> {
    let mut term = query.trim().to_string();
    if term.is_empty() {
        return Ok(Vec::new());
    }
    if let Some(min) = min_year {
        term.push_str(&format!(
            " AND ({}[Date - Publication] : 3000[Date - Publication])",
            min
        ));
    }
    if let Some(max) = max_year {
        term.push_str(&format!(
            " AND (0001[Date - Publication] : {}[Date - Publication])",
            max
        ));
    }

    let pubmed_api_key = std::env::var("PUBMED_API_KEY")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());

    let mut esearch_url = Url::parse("https://eutils.ncbi.nlm.nih.gov/entrez/eutils/esearch.fcgi")
        .map_err(|e| format!("Failed to build PubMed ESearch URL: {}", e))?;
    esearch_url
        .query_pairs_mut()
        .append_pair("db", "pubmed")
        .append_pair("retmode", "json")
        .append_pair("retmax", &limit.to_string())
        .append_pair("sort", "relevance")
        .append_pair("term", &term);
    if let Some(key) = pubmed_api_key.as_ref() {
        esearch_url.query_pairs_mut().append_pair("api_key", key);
    }

    let client = build_http_client(PUBMED_CONNECT_TIMEOUT_SECS, PUBMED_TIMEOUT_SECS)
        .map_err(|e| format!("Failed to initialize PubMed HTTP client: {}", e))?;

    let mut last_err: Option<String> = None;
    for attempt in 0..=PROVIDER_MAX_RETRIES {
        let wait = reserve_provider_request_slot(
            PROVIDER_PUBMED,
            Duration::from_millis(PUBMED_MIN_INTERVAL_MS),
        )?;
        if !wait.is_zero() {
            sleep(wait).await;
        }

        let esearch_response = match client
            .get(esearch_url.clone())
            .header("Accept", "application/json")
            .header("User-Agent", "claude-prism/1.1 (+local)")
            .send()
            .await
        {
            Ok(response) => response,
            Err(err) => {
                let mut err_msg = format!("PubMed ESearch request failed: {}", err);
                let cooldown = mark_provider_failure(PROVIDER_PUBMED, None, None);
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
        };

        let esearch_status = esearch_response.status();
        let esearch_headers = esearch_response.headers().clone();
        if !esearch_status.is_success() {
            let body_preview = esearch_response
                .text()
                .await
                .unwrap_or_default()
                .chars()
                .take(220)
                .collect::<String>();
            let mut err = format!(
                "PubMed ESearch request failed with status {}{}",
                esearch_status,
                if body_preview.is_empty() {
                    "".to_string()
                } else {
                    format!(": {}", body_preview)
                }
            );
            let cooldown = mark_provider_failure(
                PROVIDER_PUBMED,
                Some(esearch_status),
                retry_after_seconds(&esearch_headers),
            );
            if let Some(secs) = cooldown {
                err.push_str(&format!(" Circuit cooldown applied: {}s.", secs));
            }

            if is_retryable_status(esearch_status) && attempt < PROVIDER_MAX_RETRIES {
                last_err = Some(err.clone());
                sleep(retry_delay_for_status(
                    esearch_status,
                    attempt,
                    retry_after_seconds(&esearch_headers),
                ))
                .await;
                continue;
            }
            return Err(err);
        }

        let esearch_body = esearch_response
            .text()
            .await
            .map_err(|e| format!("Failed to read PubMed ESearch response body: {}", e))?;
        let esearch_json: Value = serde_json::from_str(&esearch_body).map_err(|e| {
            let preview = esearch_body.chars().take(220).collect::<String>();
            if preview.is_empty() {
                format!("Failed to parse PubMed ESearch JSON: {}", e)
            } else {
                format!(
                    "Failed to parse PubMed ESearch JSON: {} | body: {}",
                    e, preview
                )
            }
        })?;
        let ids = esearch_json
            .get("esearchresult")
            .and_then(|v| v.get("idlist"))
            .and_then(Value::as_array)
            .map(|arr| {
                arr.iter()
                    .filter_map(Value::as_str)
                    .map(str::trim)
                    .filter(|id| !id.is_empty())
                    .map(str::to_string)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();

        if ids.is_empty() {
            mark_provider_success(PROVIDER_PUBMED);
            return Ok(Vec::new());
        }

        let mut esummary_url =
            Url::parse("https://eutils.ncbi.nlm.nih.gov/entrez/eutils/esummary.fcgi")
                .map_err(|e| format!("Failed to build PubMed ESummary URL: {}", e))?;
        esummary_url
            .query_pairs_mut()
            .append_pair("db", "pubmed")
            .append_pair("retmode", "json")
            .append_pair("id", &ids.join(","));
        if let Some(key) = pubmed_api_key.as_ref() {
            esummary_url.query_pairs_mut().append_pair("api_key", key);
        }

        let wait = reserve_provider_request_slot(
            PROVIDER_PUBMED,
            Duration::from_millis(PUBMED_MIN_INTERVAL_MS),
        )?;
        if !wait.is_zero() {
            sleep(wait).await;
        }

        let esummary_response = match client
            .get(esummary_url)
            .header("Accept", "application/json")
            .header("User-Agent", "claude-prism/1.1 (+local)")
            .send()
            .await
        {
            Ok(response) => response,
            Err(err) => {
                let mut err_msg = format!("PubMed ESummary request failed: {}", err);
                let cooldown = mark_provider_failure(PROVIDER_PUBMED, None, None);
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
        };

        let esummary_status = esummary_response.status();
        let esummary_headers = esummary_response.headers().clone();
        if !esummary_status.is_success() {
            let body_preview = esummary_response
                .text()
                .await
                .unwrap_or_default()
                .chars()
                .take(220)
                .collect::<String>();
            let mut err = format!(
                "PubMed ESummary request failed with status {}{}",
                esummary_status,
                if body_preview.is_empty() {
                    "".to_string()
                } else {
                    format!(": {}", body_preview)
                }
            );
            let cooldown = mark_provider_failure(
                PROVIDER_PUBMED,
                Some(esummary_status),
                retry_after_seconds(&esummary_headers),
            );
            if let Some(secs) = cooldown {
                err.push_str(&format!(" Circuit cooldown applied: {}s.", secs));
            }

            if is_retryable_status(esummary_status) && attempt < PROVIDER_MAX_RETRIES {
                last_err = Some(err.clone());
                sleep(retry_delay_for_status(
                    esummary_status,
                    attempt,
                    retry_after_seconds(&esummary_headers),
                ))
                .await;
                continue;
            }
            return Err(err);
        }

        let esummary_body = esummary_response
            .text()
            .await
            .map_err(|e| format!("Failed to read PubMed ESummary response body: {}", e))?;
        let esummary_json: Value = serde_json::from_str(&esummary_body).map_err(|e| {
            let preview = esummary_body.chars().take(220).collect::<String>();
            if preview.is_empty() {
                format!("Failed to parse PubMed ESummary JSON: {}", e)
            } else {
                format!(
                    "Failed to parse PubMed ESummary JSON: {} | body: {}",
                    e, preview
                )
            }
        })?;
        let result_node = esummary_json.get("result").cloned().unwrap_or(Value::Null);
        let mut candidates = Vec::<CitationCandidate>::new();
        for id in ids {
            let Some(item) = result_node.get(&id) else {
                continue;
            };
            let title = item
                .get("title")
                .and_then(Value::as_str)
                .map(str::trim)
                .unwrap_or_default()
                .to_string();
            if title.is_empty() {
                continue;
            }
            let year = parse_pubmed_year(item.get("pubdate").and_then(Value::as_str));
            let venue = item
                .get("fulljournalname")
                .and_then(Value::as_str)
                .or_else(|| item.get("source").and_then(Value::as_str))
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string);
            let doi = pubmed_doi(item);
            let authors = pubmed_authors(item);
            let url = Some(format!("https://pubmed.ncbi.nlm.nih.gov/{}/", id));
            let score_explain = compute_score_explain(score_basis, &title, "", year, None);
            let evidence_sentences = extract_evidence_sentences(score_basis, &title, "", 2);
            candidates.push(CitationCandidate {
                paper_id: format!("pubmed:{}", id),
                title,
                year,
                venue,
                abstract_text: None,
                doi,
                url,
                authors,
                citation_count: None,
                score: score_explain.final_score,
                evidence_sentences,
                score_explain: Some(score_explain),
            });
        }

        mark_provider_success(PROVIDER_PUBMED);
        candidates.sort_by(|a, b| b.score.total_cmp(&a.score));
        return Ok(candidates);
    }

    Err(last_err.unwrap_or_else(|| "PubMed request failed.".to_string()))
}
