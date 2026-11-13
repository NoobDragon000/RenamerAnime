use crate::parser::ParsedItem;
use crate::sanitizer::sanitize_filename_component;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "kebab-case")]
pub enum NamingMode {
    #[default]
    EnglishOnly,
    Prepend,
    Append,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FormatOptions {
    pub mode: NamingMode,
    pub clean_tags: bool,
    pub normalize_seasons: bool,
}

impl Default for FormatOptions {
    fn default() -> Self {
        Self {
            mode: NamingMode::EnglishOnly,
            clean_tags: true,
            normalize_seasons: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FormattedResult {
    pub proposed_name: String,
    pub changed: bool,
    pub used_fallback_romaji: bool,
    pub already_clean_or_matched: bool,
}

pub fn format_item(
    parsed: &ParsedItem,
    resolved_english: Option<&str>,
    resolved_romaji: Option<&str>,
    opts: &FormatOptions,
) -> FormattedResult {
    let eng_candidate = resolved_english
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .or_else(|| parsed.inline_alias.as_deref())
        .or_else(|| parsed.existing_prepended_title.as_deref());

    let romaji_candidate = resolved_romaji.map(str::trim).filter(|s| !s.is_empty());

    let used_fallback_romaji = resolved_english
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .is_none();

    // If clean_tags is false and mode is Prepend/Append, keep the original stem intact
    if !opts.clean_tags && opts.mode != NamingMode::EnglishOnly {
        let raw_stem = if parsed.extension.is_empty() {
            parsed.raw_name.as_str()
        } else {
            parsed
                .raw_name
                .strip_suffix(&parsed.extension)
                .unwrap_or(&parsed.raw_name)
        };

        let assembled = match (opts.mode, eng_candidate) {
            (NamingMode::Prepend, Some(eng))
                if parsed.existing_prepended_title.is_none()
                    && !raw_stem
                        .to_ascii_lowercase()
                        .contains(&eng.to_ascii_lowercase()) =>
            {
                format!("({eng}) {raw_stem}")
            }
            (NamingMode::Append, Some(eng))
                if parsed.existing_prepended_title.is_none()
                    && !raw_stem
                        .to_ascii_lowercase()
                        .contains(&eng.to_ascii_lowercase()) =>
            {
                format!("{raw_stem} ({eng})")
            }
            _ => raw_stem.to_string(),
        };

        let proposed_name = sanitize_filename_component(&assembled, &parsed.extension);
        let changed = proposed_name != parsed.raw_name;
        return FormattedResult {
            proposed_name,
            changed,
            used_fallback_romaji,
            already_clean_or_matched: !changed,
        };
    }

    // Deduplicate bilingual hyphenated titles like "Silver Spoon - Gin no Saji"
    let effective_original = deduplicate_hyphen_bilingual(&parsed.base_title, eng_candidate)
        .or_else(|| {
            if parsed.base_title.is_empty() {
                romaji_candidate.map(str::to_string)
            } else {
                Some(parsed.base_title.clone())
            }
        })
        .unwrap_or_default();

    let display_english = eng_candidate
        .or(romaji_candidate)
        .unwrap_or(&effective_original);

    let title_core = match opts.mode {
        NamingMode::EnglishOnly => display_english.to_string(),
        NamingMode::Prepend => {
            if let Some(eng) = eng_candidate {
                if !effective_original.is_empty()
                    && !eng.eq_ignore_ascii_case(&effective_original)
                {
                    format!("({eng}) {effective_original}")
                } else {
                    display_english.to_string()
                }
            } else {
                display_english.to_string()
            }
        }
        NamingMode::Append => {
            if let Some(eng) = eng_candidate {
                if !effective_original.is_empty()
                    && !eng.eq_ignore_ascii_case(&effective_original)
                {
                    format!("{effective_original} ({eng})")
                } else {
                    display_english.to_string()
                }
            } else {
                display_english.to_string()
            }
        }
    };

    let mut parts: Vec<String> = Vec::new();
    if !title_core.is_empty() {
        parts.push(title_core);
    }

    if let Some(year) = &parsed.year_spec {
        parts.push(year.clone());
    }

    if parsed.is_dir || parsed.episode_spec.is_none() {
        if let Some(season) = &parsed.season_spec {
            parts.push(season.clone());
        }
        for note in &parsed.extra_notes {
            parts.push(note.clone());
        }
    } else if let Some(ep) = &parsed.episode_spec {
        let ep_has_season = ep.starts_with('S') && ep.contains('E');
        if !ep_has_season {
            if let Some(season) = &parsed.season_spec {
                parts.push(season.clone());
            }
        }
        if parts.is_empty() {
            parts.push(ep.clone());
        } else {
            let joined = parts.join(" ");
            parts.clear();
            parts.push(format!("{joined} - {ep}"));
        }
        for note in &parsed.extra_notes {
            parts.push(note.clone());
        }
    }

    let assembled = parts.join(" ");
    let proposed_name = if assembled.is_empty() {
        parsed.raw_name.clone()
    } else {
        sanitize_filename_component(&assembled, &parsed.extension)
    };

    let changed = proposed_name != parsed.raw_name;
    FormattedResult {
        proposed_name,
        changed,
        used_fallback_romaji,
        already_clean_or_matched: !changed,
    }
}

fn deduplicate_hyphen_bilingual(base_title: &str, eng: Option<&str>) -> Option<String> {
    let eng_val = eng?;
    let (left, right) = base_title.split_once(" - ")?;
    if left.trim().eq_ignore_ascii_case(eng_val) && !right.trim().is_empty() {
        return Some(right.trim().to_string());
    }
    if right.trim().eq_ignore_ascii_case(eng_val) && !left.trim().is_empty() {
        return Some(left.trim().to_string());
    }
    None
}
