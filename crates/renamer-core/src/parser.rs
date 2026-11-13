use crate::rules::CleanupRules;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::sync::OnceLock;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ParsedItem {
    pub raw_name: String,
    pub is_dir: bool,
    pub extension: String,
    pub existing_prepended_title: Option<String>,
    pub base_title: String,
    pub inline_alias: Option<String>,
    pub search_query: String,
    pub fallback_queries: Vec<String>,
    pub year_spec: Option<String>,
    pub season_spec: Option<String>,
    pub episode_spec: Option<String>,
    pub extra_notes: Vec<String>,
    pub stripped_tags: Vec<String>,
}

/// Split known media/subtitle extensions (including multi-part `.en.ass`) from filenames.
/// Directories never have extensions split so dots in names like `Dr. Stone` are preserved.
pub fn split_media_extension(raw_name: &str, is_dir: bool, rules: &CleanupRules) -> (String, String) {
    if is_dir {
        return (raw_name.to_string(), String::new());
    }

    let Some(dot_idx) = raw_name.rfind('.') else {
        return (raw_name.to_string(), String::new());
    };

    let ext_no_dot = &raw_name[dot_idx + 1..];
    let ext_lower = ext_no_dot.to_ascii_lowercase();
    if !rules
        .media_extensions
        .iter()
        .any(|m| m.eq_ignore_ascii_case(&ext_lower))
    {
        return (raw_name.to_string(), String::new());
    }

    let stem_before_ext = &raw_name[..dot_idx];
    // Check for optional language code right before subtitle extensions (e.g. `.en.ass`)
    if let Some(lang_dot_idx) = stem_before_ext.rfind('.') {
        let lang_candidate = &stem_before_ext[lang_dot_idx + 1..];
        if rules
            .subtitle_lang_codes
            .iter()
            .any(|c| c.eq_ignore_ascii_case(lang_candidate))
        {
            return (
                stem_before_ext[..lang_dot_idx].to_string(),
                raw_name[lang_dot_idx..].to_string(),
            );
        }
    }

    (stem_before_ext.to_string(), raw_name[dot_idx..].to_string())
}

pub fn parse_item_name(raw_name: &str, is_dir: bool, rules: &CleanupRules) -> ParsedItem {
    let (stem, extension) = split_media_extension(raw_name, is_dir, rules);
    let mut working = stem.trim().to_string();
    let mut stripped_tags: Vec<String> = Vec::new();
    let mut existing_prepended_title: Option<String> = None;

    // 1. Detect already-prepended leading "(Title) Rest..."
    if let Some((prep, rest)) = extract_leading_prepended_title(&working, rules) {
        existing_prepended_title = Some(prep);
        working = rest;
    }

    // 2. Strip leading square-bracket release groups: [Judas], [Anime Time], etc.
    working = strip_leading_square_brackets(&working, &mut stripped_tags);

    // 3. Strip trailing -GROUP tags (e.g., x265-EMBER, -SubsPlease)
    working = strip_trailing_group_tag(&working, rules, &mut stripped_tags);

    // 4. Normalize dots/underscores outside brackets if dot-delimited
    working = normalize_separators_outside_brackets(&working);

    // 5. Process all [...] and (...) spans
    let mut year_spec: Option<String> = None;
    let mut season_spec: Option<String> = None;
    let mut episode_spec: Option<String> = None;
    let mut inline_alias: Option<String> = None;

    working = process_bracketed_spans(
        &working,
        rules,
        &mut year_spec,
        &mut season_spec,
        &mut episode_spec,
        &mut inline_alias,
        &mut stripped_tags,
    );

    // 6. Extract unbracketed "+ Special", "+ Movie", "+ OVA", etc.
    let mut extra_notes: Vec<String> = Vec::new();
    working = extract_extra_notes(&working, &mut extra_notes);

    // 7. Extract unbracketed episode markers (S02E05, - 01, Episode 01, bare 01)
    working = extract_unbracketed_episodes(&working, &mut episode_spec, &mut stripped_tags);

    // 8. Extract unbracketed season markers (S01, S01-02, Season 2)
    working = extract_unbracketed_seasons(&working, &mut season_spec, &mut stripped_tags);

    // 9. Strip remaining unbracketed noise tokens, batch words, and standalone version markers
    working = strip_unbracketed_noise(&working, rules, &mut stripped_tags);

    // 10. Clean dangling punctuation & collapse spaces
    let base_title = clean_title_edges(&working);

    // 11. Build primary query + fallback candidates
    let search_query = base_title.clone();
    let fallback_queries = build_fallback_queries(&base_title, inline_alias.as_deref());

    ParsedItem {
        raw_name: raw_name.to_string(),
        is_dir,
        extension,
        existing_prepended_title,
        base_title,
        inline_alias,
        search_query,
        fallback_queries,
        year_spec,
        season_spec,
        episode_spec,
        extra_notes,
        stripped_tags,
    }
}

fn extract_leading_prepended_title(input: &str, rules: &CleanupRules) -> Option<(String, String)> {
    static PREPEND_RE: OnceLock<Regex> = OnceLock::new();
    let re = PREPEND_RE.get_or_init(|| Regex::new(r"^\(([^)]+)\)\s+(.+)$").unwrap());
    let caps = re.captures(input)?;
    let inner = caps.get(1)?.as_str().trim();
    let rest = caps.get(2)?.as_str().trim();
    if is_year_spec(inner)
        || parse_season_text(inner).is_some()
        || is_noise_or_batch_span(inner, rules)
        || is_non_alias_paren(inner)
    {
        return None;
    }
    Some((inner.to_string(), rest.to_string()))
}

fn strip_leading_square_brackets(input: &str, stripped_tags: &mut Vec<String>) -> String {
    static LEADING_BRACKET_RE: OnceLock<Regex> = OnceLock::new();
    static EP_RANGE_RE: OnceLock<Regex> = OnceLock::new();
    let re = LEADING_BRACKET_RE.get_or_init(|| Regex::new(r"^(\[[^\]]+\])[\s._-]*").unwrap());
    let ep_re = EP_RANGE_RE.get_or_init(|| Regex::new(r"^\[\d{1,3}[-~]\d{1,3}\]$").unwrap());

    let mut cur = input.trim_start().to_string();
    while let Some(caps) = re.captures(&cur) {
        let tag = caps.get(1).unwrap().as_str();
        if ep_re.is_match(tag) {
            break;
        }
        stripped_tags.push(tag.to_string());
        let consumed = caps.get(0).unwrap().end();
        cur = cur[consumed..].to_string();
    }
    cur
}

fn strip_trailing_group_tag(
    input: &str,
    rules: &CleanupRules,
    stripped_tags: &mut Vec<String>,
) -> String {
    static TRAILING_GROUP_RE: OnceLock<Regex> = OnceLock::new();
    let re = TRAILING_GROUP_RE
        .get_or_init(|| Regex::new(r"(?i)^(.*?)(?:(\s*[-]\s*)([A-Za-z0-9_]+))$").unwrap());

    let Some(caps) = re.captures(input.trim_end()) else {
        return input.to_string();
    };
    let prefix = caps.get(1).unwrap().as_str();
    let group = caps.get(3).unwrap().as_str();

    let is_known_group = rules
        .trailing_group_tags
        .iter()
        .any(|g| g.eq_ignore_ascii_case(group));

    // Also match "-GROUP" immediately following a codec/noise token like "x265-EMBER"
    let prefix_ends_with_codec = prefix
        .split_whitespace()
        .last()
        .map(|w| {
            rules
                .noise_tokens
                .iter()
                .any(|n| n.eq_ignore_ascii_case(w))
        })
        .unwrap_or(false);

    if is_known_group || (prefix_ends_with_codec && !prefix.trim().is_empty()) {
        stripped_tags.push(format!("-{group}"));
        return prefix.trim_end().to_string();
    }

    input.to_string()
}

fn normalize_separators_outside_brackets(input: &str) -> String {
    // Only convert dots to spaces when the outside-bracket text uses dot-separated naming
    // (i.e. contains dots and zero spaces outside brackets).
    let mut has_space_outside = false;
    let mut dot_count_outside = 0usize;
    let mut depth_sq = 0usize;
    let mut depth_par = 0usize;

    for ch in input.chars() {
        match ch {
            '[' => depth_sq += 1,
            ']' => depth_sq = depth_sq.saturating_sub(1),
            '(' => depth_par += 1,
            ')' => depth_par = depth_par.saturating_sub(1),
            _ if depth_sq == 0 && depth_par == 0 => {
                if ch == ' ' {
                    has_space_outside = true;
                } else if ch == '.' {
                    dot_count_outside += 1;
                }
            }
            _ => {}
        }
    }

    let convert_dots = !has_space_outside && dot_count_outside >= 2;
    let mut out = String::with_capacity(input.len());
    let chars: Vec<char> = input.chars().collect();
    depth_sq = 0;
    depth_par = 0;

    for (i, &ch) in chars.iter().enumerate() {
        match ch {
            '[' => {
                depth_sq += 1;
                out.push(ch);
            }
            ']' => {
                depth_sq = depth_sq.saturating_sub(1);
                out.push(ch);
            }
            '(' => {
                depth_par += 1;
                out.push(ch);
            }
            ')' => {
                depth_par = depth_par.saturating_sub(1);
                out.push(ch);
            }
            '_' if depth_sq == 0 && depth_par == 0 => out.push(' '),
            '.' if depth_sq == 0 && depth_par == 0 && convert_dots => {
                let prev_digit = i > 0 && chars[i - 1].is_ascii_digit();
                let next_digit = i + 1 < chars.len() && chars[i + 1].is_ascii_digit();
                let is_h26x = i > 0
                    && chars[i - 1].eq_ignore_ascii_case(&'H')
                    && i + 1 < chars.len()
                    && chars[i + 1] == '2';
                if (prev_digit && next_digit) || is_h26x {
                    out.push('.');
                } else {
                    out.push(' ');
                }
            }
            _ => out.push(ch),
        }
    }
    out
}

fn process_bracketed_spans(
    input: &str,
    rules: &CleanupRules,
    year_spec: &mut Option<String>,
    season_spec: &mut Option<String>,
    episode_spec: &mut Option<String>,
    inline_alias: &mut Option<String>,
    stripped_tags: &mut Vec<String>,
) -> String {
    static SPAN_RE: OnceLock<Regex> = OnceLock::new();
    static CRC_RE: OnceLock<Regex> = OnceLock::new();
    static EP_RANGE_RE: OnceLock<Regex> = OnceLock::new();

    let span_re = SPAN_RE.get_or_init(|| Regex::new(r"(\[[^\]]+\]|\([^)]+\))").unwrap());
    let crc_re = CRC_RE.get_or_init(|| Regex::new(r"^[0-9A-Fa-f]{8}$").unwrap());
    let ep_range_re = EP_RANGE_RE.get_or_init(|| Regex::new(r"^\d{1,3}[-~]\d{1,3}$").unwrap());

    let mut out = String::with_capacity(input.len());
    let mut last_end = 0;

    for m in span_re.find_iter(input) {
        let prefix_segment = &input[last_end..m.start()];
        out.push_str(prefix_segment);
        last_end = m.end();

        let full = m.as_str();
        let is_square = full.starts_with('[');
        let inner = full[1..full.len() - 1].trim();

        // 1. CRC32 hash inside square brackets: [ABCD1234]
        if is_square && crc_re.is_match(inner) {
            stripped_tags.push(full.to_string());
            continue;
        }

        // 2. Year or year range: (2009-2014), (2018)
        if is_year_spec(inner) {
            if year_spec.is_none() {
                *year_spec = Some(format!("({inner})"));
            }
            continue;
        }

        // 3. Compound or simple season marker: (Season 2), (Season 03), [Season 1+2], (Seasons 1-3 + Movie + OVA + Specials)
        if let Some((normalized_season, maybe_version)) = parse_season_text(inner) {
            if let Some(ver) = maybe_version {
                stripped_tags.push(ver);
            }
            if season_spec.is_none() {
                *season_spec = Some(normalized_season);
            }
            continue;
        }

        // 4. Episode range bracket: [01-07] or [01~12]
        if is_square && ep_range_re.is_match(inner) {
            if episode_spec.is_none() {
                *episode_spec = Some(full.to_string());
            }
            continue;
        }

        // 5. Pure noise/batch/version bracket or parens: [BD 1080p], [1080], (1080p), [WEB 1080p]
        if is_noise_or_batch_span(inner, rules) {
            stripped_tags.push(full.to_string());
            continue;
        }

        // 6. Unknown square bracket tag (release group / extra metadata): strip if purely tag-like
        if is_square {
            stripped_tags.push(full.to_string());
            continue;
        }

        // 7. Parenthesized inline bilingual alias following a title prefix: e.g. Kyokou Suiri (In-Spectre)
        let prefix_so_far = out.trim();
        if !prefix_so_far.is_empty()
            && inline_alias.is_none()
            && !is_non_alias_paren(inner)
        {
            *inline_alias = Some(inner.to_string());
            continue;
        }

        // Otherwise keep literal parenthesized text (e.g. (TV), (Part 2))
        out.push_str(full);
    }

    out.push_str(&input[last_end..]);
    out
}

fn is_year_spec(inner: &str) -> bool {
    static YEAR_RE: OnceLock<Regex> = OnceLock::new();
    let re = YEAR_RE.get_or_init(|| Regex::new(r"^(?:19|20)\d{2}(?:\s*[-~]\s*(?:19|20)\d{2})?$").unwrap());
    re.is_match(inner.trim())
}

fn is_non_alias_paren(inner: &str) -> bool {
    static NON_ALIAS_RE: OnceLock<Regex> = OnceLock::new();
    let re = NON_ALIAS_RE.get_or_init(|| {
        Regex::new(r"(?i)^(?:TV|OVA|OAD|ONA|Special|Movie|Part\s*\d+|Cour\s*\d+|\d+(?:st|nd|rd|th)\s+Season|US|UK|JP|CN)$").unwrap()
    });
    re.is_match(inner.trim())
}

/// Parse season spans inside brackets/parens and normalize to `(Season N)` or `(Seasons A-B)`
/// or `(Seasons A-B + Movie + OVA + Specials)`.
fn parse_season_text(inner: &str) -> Option<(String, Option<String>)> {
    static SEASON_RE: OnceLock<Regex> = OnceLock::new();
    let re = SEASON_RE.get_or_init(|| {
        Regex::new(
            r"(?i)^(?:Seasons?\s*|S)(\d{1,2})(?:\s*[-~+&]\s*(?:S)?(\d{1,2}))?(?:\s+(v\d+))?(.*)$",
        )
        .unwrap()
    });

    let caps = re.captures(inner.trim())?;
    let s1: u32 = caps.get(1)?.as_str().parse().ok()?;
    let s2: Option<u32> = caps
        .get(2)
        .and_then(|m| m.as_str().parse().ok());
    let ver = caps.get(3).map(|m| m.as_str().to_string());
    let tail = caps.get(4).map(|m| m.as_str().trim()).unwrap_or("");

    // Reject if trailing text is arbitrary non-extra words unless it starts with '+'
    let extras = if tail.is_empty() {
        String::new()
    } else if tail.starts_with('+') {
        format!(" {tail}")
    } else {
        return None;
    };

    let normalized = match s2 {
        Some(end) if end != s1 => format!("(Seasons {s1}-{end}{extras})"),
        _ => format!("(Season {s1}{extras})"),
    };
    Some((normalized, ver))
}

fn is_noise_or_batch_span(inner: &str, rules: &CleanupRules) -> bool {
    static SPLIT_RE: OnceLock<Regex> = OnceLock::new();
    static VER_RE: OnceLock<Regex> = OnceLock::new();

    let split_re = SPLIT_RE.get_or_init(|| Regex::new(r"[\s.,_/+|]+").unwrap());
    let ver_re = VER_RE.get_or_init(|| Regex::new(r"(?i)^v\d+$").unwrap());

    // First remove multi-word noise phrases
    let mut remainder = inner.to_string();
    let mut multi_word: Vec<&String> = rules
        .noise_tokens
        .iter()
        .chain(rules.batch_keywords.iter())
        .chain(rules.trailing_group_tags.iter())
        .filter(|t| t.contains(' ') || t.contains('-') || t.contains('.'))
        .collect();
    multi_word.sort_by_key(|b| std::cmp::Reverse(b.len()));

    for phrase in multi_word {
        let pat = format!(r"(?i)\b{}\b", regex::escape(phrase));
        if let Ok(re) = Regex::new(&pat) {
            remainder = re.replace_all(&remainder, " ").to_string();
        }
    }

    let tokens: Vec<&str> = split_re
        .split(&remainder)
        .filter(|t| !t.is_empty())
        .collect();

    if tokens.is_empty() {
        return !inner.trim().is_empty();
    }

    tokens.iter().all(|tok| {
        ver_re.is_match(tok)
            || rules.noise_tokens.iter().any(|n| n.eq_ignore_ascii_case(tok))
            || rules.batch_keywords.iter().any(|b| b.eq_ignore_ascii_case(tok))
            || rules
                .trailing_group_tags
                .iter()
                .any(|g| g.eq_ignore_ascii_case(tok))
    })
}

fn extract_extra_notes(input: &str, extra_notes: &mut Vec<String>) -> String {
    static EXTRA_RE: OnceLock<Regex> = OnceLock::new();
    let re = EXTRA_RE.get_or_init(|| {
        Regex::new(r"(?i)\s*\+\s*((?:Specials?|Movies?|OVAs?|OADs?|ONAs?|Shorts)(?:\s*\+\s*(?:Specials?|Movies?|OVAs?|OADs?|ONAs?|Shorts))*)").unwrap()
    });

    let mut out = input.to_string();
    while let Some(m) = re.captures(&out) {
        let full_match = m.get(0).unwrap();
        let body = m.get(1).unwrap().as_str().trim();
        let normalized_parts: Vec<&str> = body.split('+').map(|p| p.trim()).collect();
        extra_notes.push(format!("+ {}", normalized_parts.join(" + ")));
        out = format!("{}{}", &out[..full_match.start()], &out[full_match.end()..]);
    }
    out
}

fn extract_unbracketed_episodes(
    input: &str,
    episode_spec: &mut Option<String>,
    stripped_tags: &mut Vec<String>,
) -> String {
    static S_E_RE: OnceLock<Regex> = OnceLock::new();
    static DASH_EP_RE: OnceLock<Regex> = OnceLock::new();
    static BARE_EP_RE: OnceLock<Regex> = OnceLock::new();

    let se_re = S_E_RE.get_or_init(|| {
        Regex::new(r"(?i)(?:^|\s+-\s+|\s+)S(\d{1,2})E(\d{1,3}(?:\s*-\s*E?\d{1,3})?)(v\d+)?(?:\s|$)")
            .unwrap()
    });
    let dash_ep_re = DASH_EP_RE.get_or_init(|| {
        Regex::new(r"(?i)(?:\s+-\s+|\s+(?:Ep\.?|Episode)\s*)(\d{1,3}(?:-\d{1,3})?)(v\d+)?(?:\s|$)")
            .unwrap()
    });
    let bare_ep_re =
        BARE_EP_RE.get_or_init(|| Regex::new(r"(?i)^(?:Episode\s*|Ep\.?\s*)?(\d{1,3})(v\d+)?$").unwrap());

    let trimmed = input.trim();
    if let Some(caps) = bare_ep_re.captures(trimmed) {
        let ep = caps.get(1).unwrap().as_str().to_string();
        if let Some(ver) = caps.get(2) {
            stripped_tags.push(ver.as_str().to_string());
        }
        if episode_spec.is_none() {
            *episode_spec = Some(ep);
        }
        return String::new();
    }

    let mut out = input.to_string();
    if let Some(caps) = se_re.captures(&out) {
        let m0 = caps.get(0).unwrap();
        let s: u32 = caps.get(1).unwrap().as_str().parse().unwrap_or(1);
        let e_raw = caps.get(2).unwrap().as_str().replace(' ', "");
        let formatted = if let Some((e1, e2)) = e_raw.split_once('-') {
            let n1: u32 = e1.trim_start_matches(['E', 'e']).parse().unwrap_or(1);
            let n2: u32 = e2.trim_start_matches(['E', 'e']).parse().unwrap_or(1);
            format!("S{s:02}E{n1:02}-E{n2:02}")
        } else {
            let n1: u32 = e_raw.parse().unwrap_or(1);
            format!("S{s:02}E{n1:02}")
        };
        if let Some(ver) = caps.get(3) {
            stripped_tags.push(ver.as_str().to_string());
        }
        if episode_spec.is_none() {
            *episode_spec = Some(formatted);
        }
        out = format!("{} {}", &out[..m0.start()], &out[m0.end()..]);
        return out;
    }

    if let Some(caps) = dash_ep_re.captures(&out) {
        let m0 = caps.get(0).unwrap();
        let ep = caps.get(1).unwrap().as_str().to_string();
        if let Some(ver) = caps.get(2) {
            stripped_tags.push(ver.as_str().to_string());
        }
        if episode_spec.is_none() {
            *episode_spec = Some(ep);
        }
        out = format!("{} {}", &out[..m0.start()], &out[m0.end()..]);
    }

    out
}

fn extract_unbracketed_seasons(
    input: &str,
    season_spec: &mut Option<String>,
    stripped_tags: &mut Vec<String>,
) -> String {
    static SEASON_UNB_RE: OnceLock<Regex> = OnceLock::new();
    let re = SEASON_UNB_RE.get_or_init(|| {
        Regex::new(
            r"(?i)(?:^|\s+-\s+|\s+)(?:Seasons?\s+|S)(\d{1,2})(?:\s*[-~+&]\s*(?:S)?(\d{1,2}))?(?:\s+(v\d+))?(?:\s|$)",
        )
        .unwrap()
    });

    let mut out = input.to_string();
    if let Some(caps) = re.captures(&out) {
        let m0 = caps.get(0).unwrap();
        let s1: u32 = caps.get(1).unwrap().as_str().parse().unwrap_or(1);
        let s2: Option<u32> = caps.get(2).and_then(|m| m.as_str().parse().ok());
        if let Some(ver) = caps.get(3) {
            stripped_tags.push(ver.as_str().to_string());
        }
        if season_spec.is_none() {
            *season_spec = Some(match s2 {
                Some(end) if end != s1 => format!("(Seasons {s1}-{end})"),
                _ => format!("(Season {s1})"),
            });
        }
        out = format!("{} {}", &out[..m0.start()], &out[m0.end()..]);
    }
    out
}

fn strip_unbracketed_noise(
    input: &str,
    rules: &CleanupRules,
    stripped_tags: &mut Vec<String>,
) -> String {
    let mut out = input.to_string();

    // 1. Multi-word noise phrases first (longest first)
    let mut multi_word: Vec<&String> = rules
        .noise_tokens
        .iter()
        .chain(rules.batch_keywords.iter())
        .filter(|t| t.contains(' ') || t.contains('-'))
        .collect();
    multi_word.sort_by_key(|b| std::cmp::Reverse(b.len()));

    for phrase in multi_word {
        let pat = format!(r"(?i)(?:^|\s)({})(?:\s|$)", regex::escape(phrase));
        if let Ok(re) = Regex::new(&pat) {
            let mut changed = false;
            out = re
                .replace_all(&out, |caps: &regex::Captures| {
                    changed = true;
                    stripped_tags.push(caps[1].to_string());
                    " "
                })
                .to_string();
            let _ = changed;
        }
    }

    // 2. Single-word noise, batch keywords, and standalone version markers (v2, v3)
    static VER_WORD_RE: OnceLock<Regex> = OnceLock::new();
    let ver_re = VER_WORD_RE.get_or_init(|| Regex::new(r"(?i)^v\d+$").unwrap());

    let mut kept_words: Vec<&str> = Vec::new();
    for word in out.split_whitespace() {
        let is_noise = rules.noise_tokens.iter().any(|n| n.eq_ignore_ascii_case(word))
            || rules
                .batch_keywords
                .iter()
                .any(|b| b.eq_ignore_ascii_case(word))
            || ver_re.is_match(word);

        if is_noise && (!kept_words.is_empty() || ver_re.is_match(word)) {
            stripped_tags.push(word.to_string());
        } else {
            kept_words.push(word);
        }
    }

    kept_words.join(" ")
}

fn clean_title_edges(input: &str) -> String {
    static WS_RE: OnceLock<Regex> = OnceLock::new();
    let ws_re = WS_RE.get_or_init(|| Regex::new(r"\s+").unwrap());
    let collapsed = ws_re.replace_all(input, " ");
    collapsed
        .trim()
        .trim_matches(|c: char| matches!(c, '-' | '+' | '~' | ' '))
        .trim()
        .to_string()
}

fn build_fallback_queries(base_title: &str, inline_alias: Option<&str>) -> Vec<String> {
    let mut fallbacks = Vec::new();

    if let Some(alias) = inline_alias {
        let alias_trim = alias.trim();
        if !alias_trim.is_empty() && !alias_trim.eq_ignore_ascii_case(base_title) {
            fallbacks.push(alias_trim.to_string());
        }
    }

    // Mixed CJK + ASCII query fallback (e.g. "记忆管理局 False Memory" -> "记忆管理局")
    let has_cjk = base_title.chars().any(is_cjk_char);
    let has_ascii_alpha = base_title.chars().any(|c| c.is_ascii_alphabetic());
    if has_cjk && has_ascii_alpha {
        let cjk_only: String = base_title
            .chars()
            .filter(|&c| !c.is_ascii() || c.is_whitespace())
            .collect();
        let cjk_trimmed = clean_title_edges(&cjk_only);
        if !cjk_trimmed.is_empty() && !fallbacks.contains(&cjk_trimmed) {
            fallbacks.push(cjk_trimmed);
        }

        let ascii_only: String = base_title
            .chars()
            .filter(|&c| c.is_ascii())
            .collect();
        let ascii_trimmed = clean_title_edges(&ascii_only);
        if !ascii_trimmed.is_empty() && !fallbacks.contains(&ascii_trimmed) {
            fallbacks.push(ascii_trimmed);
        }
    }

    // Hyphenated bilingual fallback (e.g. "Silver Spoon - Gin no Saji" -> "Gin no Saji", "Silver Spoon")
    if let Some((left, right)) = base_title.split_once(" - ") {
        let right_t = right.trim();
        let left_t = left.trim();
        if !right_t.is_empty() && !fallbacks.iter().any(|f| f.eq_ignore_ascii_case(right_t)) {
            fallbacks.push(right_t.to_string());
        }
        if !left_t.is_empty() && !fallbacks.iter().any(|f| f.eq_ignore_ascii_case(left_t)) {
            fallbacks.push(left_t.to_string());
        }
    }

    fallbacks
}

fn is_cjk_char(c: char) -> bool {
    matches!(
        c as u32,
        0x4E00..=0x9FFF
            | 0x3400..=0x4DBF
            | 0x3040..=0x309F
            | 0x30A0..=0x30FF
            | 0xAC00..=0xD7AF
    )
}
