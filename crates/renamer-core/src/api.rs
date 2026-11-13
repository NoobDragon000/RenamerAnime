use crate::cache::{now_unix_secs, CacheStatus, MediaCandidate, TitleCache};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::thread;
use std::time::{Duration, Instant};
use thiserror::Error;

pub const ANILIST_ENDPOINT: &str = "https://graphql.anilist.co";
const USER_AGENT: &str = "RenamerAnime/2.0 (Cross-Platform Desktop & CLI)";

#[derive(Debug, Error)]
pub enum ApiError {
    #[error("network/transport error: {0}")]
    Transport(String),
    #[error("AniList HTTP {status}: {message}")]
    Http { status: u16, message: String },
    #[error("invalid JSON response from AniList: {0}")]
    InvalidJson(String),
}

#[derive(Debug, Clone, Default)]
pub struct RateLimitState {
    pub limit: Option<u32>,
    pub remaining: Option<u32>,
    pub reset_at_unix: Option<u64>,
    pub total_requests_made: u32,
}

pub struct AniListClient {
    agent: ureq::Agent,
    endpoint: String,
    pub batch_size: usize,
    pub max_retries: u32,
    pub rate_state: RateLimitState,
    last_request_at: Option<Instant>,
}

impl Default for AniListClient {
    fn default() -> Self {
        Self::new()
    }
}

impl AniListClient {
    pub fn new() -> Self {
        let tls_connector = std::sync::Arc::new(
            native_tls::TlsConnector::new().expect("failed to initialize native TLS connector"),
        );
        let agent = ureq::AgentBuilder::new()
            .tls_connector(tls_connector)
            .timeout_connect(Duration::from_secs(6))
            .timeout_read(Duration::from_secs(15))
            .timeout_write(Duration::from_secs(10))
            .user_agent(USER_AGENT)
            .build();
        Self {
            agent,
            endpoint: ANILIST_ENDPOINT.to_string(),
            batch_size: 8,
            max_retries: 4,
            rate_state: RateLimitState::default(),
            last_request_at: None,
        }
    }

    pub fn search_candidates(
        &mut self,
        query: &str,
        on_progress: &mut dyn FnMut(&str),
    ) -> Result<Vec<MediaCandidate>, ApiError> {
        let mut results = self.query_batch(&[query.to_string()], on_progress)?;
        Ok(results.remove(query).unwrap_or_default())
    }

    /// Query up to `batch_size` distinct search strings in a single GraphQL request.
    pub fn query_batch(
        &mut self,
        queries: &[String],
        on_progress: &mut dyn FnMut(&str),
    ) -> Result<HashMap<String, Vec<MediaCandidate>>, ApiError> {
        if queries.is_empty() {
            return Ok(HashMap::new());
        }

        let mut all_results: HashMap<String, Vec<MediaCandidate>> = HashMap::new();
        let total_chunks = queries.chunks(self.batch_size).count();

        for (chunk_idx, chunk) in queries.chunks(self.batch_size).enumerate() {
            self.pace_before_request(on_progress);
            on_progress(&format!(
                "Querying AniList batch {}/{} ({} titles)...",
                chunk_idx + 1,
                total_chunks,
                chunk.len()
            ));

            let payload = build_batch_payload(chunk);
            let body = self.post_with_retry(&payload, on_progress)?;
            let parsed = parse_batch_response(chunk, &body)?;
            all_results.extend(parsed);
        }

        Ok(all_results)
    }

    fn pace_before_request(&self, on_progress: &mut dyn FnMut(&str)) {
        if let (Some(rem), Some(reset)) =
            (self.rate_state.remaining, self.rate_state.reset_at_unix)
        {
            let now = now_unix_secs();
            if rem <= 2 && reset > now {
                let wait_secs = (reset - now).min(65);
                on_progress(&format!(
                    "AniList rate-limit window nearly full (remaining={rem}); waiting {wait_secs}s..."
                ));
                thread::sleep(Duration::from_secs(wait_secs));
                return;
            }
        }

        if let Some(last) = self.last_request_at {
            let min_gap = match self.rate_state.remaining {
                Some(r) if r <= 10 => Duration::from_millis(2200),
                _ => Duration::from_millis(950),
            };
            let elapsed = last.elapsed();
            if elapsed < min_gap {
                thread::sleep(min_gap - elapsed);
            }
        }
    }

    fn post_with_retry(
        &mut self,
        payload: &Value,
        on_progress: &mut dyn FnMut(&str),
    ) -> Result<Value, ApiError> {
        let mut attempt = 0u32;

        loop {
            self.last_request_at = Some(Instant::now());
            self.rate_state.total_requests_made += 1;

            let resp = self
                .agent
                .post(&self.endpoint)
                .set("Content-Type", "application/json")
                .set("Accept", "application/json")
                .send_json(payload.clone());

            match resp {
                Ok(ok_resp) => {
                    self.update_rate_headers(&ok_resp);
                    let json_val: Value = ok_resp
                        .into_json()
                        .map_err(|e| ApiError::InvalidJson(e.to_string()))?;
                    return Ok(json_val);
                }
                Err(ureq::Error::Status(status, err_resp)) => {
                    self.update_rate_headers(&err_resp);
                    let retry_after_secs = err_resp
                        .header("Retry-After")
                        .and_then(|v| v.trim().parse::<u64>().ok());
                    let body_text = err_resp.into_string().unwrap_or_default();

                    if (status == 429 || (500..=599).contains(&status)) && attempt < self.max_retries
                    {
                        attempt += 1;
                        let backoff = retry_after_secs.unwrap_or_else(|| 1u64 << attempt);
                        let capped = backoff.clamp(1, 65);
                        on_progress(&format!(
                            "HTTP {status} from AniList — backing off {capped}s (retry {attempt}/{})...",
                            self.max_retries
                        ));
                        thread::sleep(Duration::from_secs(capped));
                        continue;
                    }

                    return Err(ApiError::Http {
                        status,
                        message: body_text,
                    });
                }
                Err(ureq::Error::Transport(t)) => {
                    if attempt < self.max_retries {
                        attempt += 1;
                        let backoff = 1u64 << attempt;
                        on_progress(&format!(
                            "Network error ({t}); retrying in {backoff}s ({attempt}/{})...",
                            self.max_retries
                        ));
                        thread::sleep(Duration::from_secs(backoff));
                        continue;
                    }
                    return Err(ApiError::Transport(t.to_string()));
                }
            }
        }
    }

    fn update_rate_headers(&mut self, resp: &ureq::Response) {
        if let Some(lim) = resp
            .header("X-RateLimit-Limit")
            .and_then(|v| v.trim().parse::<u32>().ok())
        {
            self.rate_state.limit = Some(lim);
        }
        if let Some(rem) = resp
            .header("X-RateLimit-Remaining")
            .and_then(|v| v.trim().parse::<u32>().ok())
        {
            self.rate_state.remaining = Some(rem);
        }
        if let Some(rst) = resp
            .header("X-RateLimit-Reset")
            .and_then(|v| v.trim().parse::<u64>().ok())
        {
            self.rate_state.reset_at_unix = Some(rst);
        }
    }
}

/// Build an aliased GraphQL batch query for `queries` (`q0`, `q1`, ...).
pub fn build_batch_payload(queries: &[String]) -> Value {
    let mut var_defs = Vec::with_capacity(queries.len());
    let mut fields = Vec::with_capacity(queries.len());
    let mut vars = serde_json::Map::new();

    for (idx, q) in queries.iter().enumerate() {
        let var_name = format!("q{idx}");
        var_defs.push(format!("${var_name}: String"));
        fields.push(format!(
            "{var_name}: Page(perPage: 5) {{ media(search: ${var_name}, type: ANIME) {{ id title {{ romaji english native }} synonyms seasonYear format }} }}"
        ));
        vars.insert(var_name, Value::String(q.clone()));
    }

    let query_str = format!(
        "query ({}) {{ {} }}",
        var_defs.join(", "),
        fields.join(" ")
    );

    json!({
        "query": query_str,
        "variables": Value::Object(vars),
    })
}

pub fn parse_batch_response(
    queries: &[String],
    body: &Value,
) -> Result<HashMap<String, Vec<MediaCandidate>>, ApiError> {
    let data = body.get("data").and_then(Value::as_object).ok_or_else(|| {
        let err_msg = body
            .get("errors")
            .map(|e| e.to_string())
            .unwrap_or_else(|| "missing data object".to_string());
        ApiError::InvalidJson(err_msg)
    })?;

    let mut out = HashMap::new();
    for (idx, query) in queries.iter().enumerate() {
        let alias = format!("q{idx}");
        let media_arr = data
            .get(&alias)
            .and_then(|p| p.get("media"))
            .and_then(Value::as_array);

        let mut candidates = Vec::new();
        if let Some(arr) = media_arr {
            for item in arr {
                if let Some(c) = parse_media_candidate(item) {
                    candidates.push(c);
                }
            }
        }
        out.insert(query.clone(), candidates);
    }

    Ok(out)
}

fn parse_media_candidate(v: &Value) -> Option<MediaCandidate> {
    let id = v.get("id")?.as_u64()?;
    let title_obj = v.get("title");
    let romaji = title_obj
        .and_then(|t| t.get("romaji"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string);
    let english = title_obj
        .and_then(|t| t.get("english"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string);
    let native = title_obj
        .and_then(|t| t.get("native"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string);
    let synonyms = v
        .get("synonyms")
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();
    let season_year = v
        .get("seasonYear")
        .and_then(Value::as_u64)
        .map(|y| y as u32);
    let format = v
        .get("format")
        .and_then(Value::as_str)
        .map(str::to_string);

    Some(MediaCandidate {
        id,
        romaji,
        english,
        native,
        synonyms,
        season_year,
        format,
    })
}

pub fn select_best_candidate(query: &str, candidates: &[MediaCandidate]) -> Option<MediaCandidate> {
    if candidates.is_empty() {
        return None;
    }

    let norm_q = normalize_for_match(query);
    if !norm_q.is_empty() {
        for cand in candidates {
            let matches_title = [
                cand.english.as_deref(),
                cand.romaji.as_deref(),
                cand.native.as_deref(),
            ]
            .into_iter()
            .flatten()
            .any(|t| normalize_for_match(t) == norm_q);

            let matches_synonym = cand
                .synonyms
                .iter()
                .any(|s| normalize_for_match(s) == norm_q);

            if matches_title || matches_synonym {
                return Some(cand.clone());
            }
        }
    }

    candidates.first().cloned()
}

fn normalize_for_match(s: &str) -> String {
    s.chars()
        .map(|c| {
            if c.is_alphanumeric() || !c.is_ascii() {
                c.to_ascii_lowercase()
            } else {
                ' '
            }
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// Resolve all uncached primary queries (and secondary fallback queries if primary misses)
/// using batched AniList lookups and updating `cache` in place.
pub fn resolve_queries_with_cache(
    client: &mut AniListClient,
    cache: &mut TitleCache,
    query_specs: &[(String, Vec<String>)],
    offline: bool,
    on_progress: &mut dyn FnMut(&str),
) -> Result<u32, ApiError> {
    if offline {
        return Ok(0);
    }

    let now = now_unix_secs();
    let initial_requests = client.rate_state.total_requests_made;

    // Stage 1: Deduplicated uncached primary queries
    let mut seen_primary = HashSet::new();
    let mut uncached_primary = Vec::new();
    for (primary, _) in query_specs {
        let norm = crate::cache::normalize_query_key(primary);
        if norm.is_empty() || !seen_primary.insert(norm) {
            continue;
        }
        if cache.get_valid(primary, now).is_none() {
            uncached_primary.push(primary.clone());
        }
    }

    if !uncached_primary.is_empty() {
        let stage1_results = client.query_batch(&uncached_primary, on_progress)?;
        for query in &uncached_primary {
            let candidates = stage1_results.get(query).cloned().unwrap_or_default();
            if let Some(best) = select_best_candidate(query, &candidates) {
                cache.upsert_match(query, best, candidates, now);
            } else {
                cache.upsert_no_match(query, now);
            }
        }
    }

    // Stage 2: Fallback queries for any primary query that yielded NoMatch
    let mut seen_fallback = HashSet::new();
    let mut uncached_fallbacks = Vec::new();
    let mut fallback_to_primaries: HashMap<String, Vec<String>> = HashMap::new();

    for (primary, fallbacks) in query_specs {
        let is_no_match = cache
            .get_valid(primary, now)
            .map(|e| e.status == CacheStatus::NoMatch)
            .unwrap_or(false);
        if !is_no_match {
            continue;
        }
        for fb in fallbacks {
            let norm_fb = crate::cache::normalize_query_key(fb);
            if norm_fb.is_empty() {
                continue;
            }
            fallback_to_primaries
                .entry(fb.clone())
                .or_default()
                .push(primary.clone());

            if let Some(existing_fb) = cache.get_valid(fb, now) {
                if existing_fb.status == CacheStatus::Match {
                    if let Some(sel) = &existing_fb.selected {
                        cache.upsert_match(
                            primary,
                            sel.clone(),
                            existing_fb.candidates.clone(),
                            now,
                        );
                    }
                }
                continue;
            }

            if seen_fallback.insert(norm_fb) {
                uncached_fallbacks.push(fb.clone());
            }
        }
    }

    if !uncached_fallbacks.is_empty() {
        let stage2_results = client.query_batch(&uncached_fallbacks, on_progress)?;
        for fb in &uncached_fallbacks {
            let candidates = stage2_results.get(fb).cloned().unwrap_or_default();
            if let Some(best) = select_best_candidate(fb, &candidates) {
                cache.upsert_match(fb, best.clone(), candidates.clone(), now);
                if let Some(primaries) = fallback_to_primaries.get(fb) {
                    for primary in primaries {
                        // Only promote the first matching fallback for a given primary
                        let still_no_match = cache
                            .get_valid(primary, now)
                            .map(|e| e.status == CacheStatus::NoMatch)
                            .unwrap_or(true);
                        if still_no_match {
                            cache.upsert_match(primary, best.clone(), candidates.clone(), now);
                        }
                    }
                }
            } else {
                cache.upsert_no_match(fb, now);
            }
        }
    }

    Ok(client
        .rate_state
        .total_requests_made
        .saturating_sub(initial_requests))
}
