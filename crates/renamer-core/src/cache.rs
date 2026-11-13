use crate::formatter::NamingMode;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use thiserror::Error;

pub const POSITIVE_CACHE_TTL_SECS: u64 = 90 * 24 * 60 * 60; // 90 days
pub const NEGATIVE_CACHE_TTL_SECS: u64 = 14 * 24 * 60 * 60; // 14 days

#[derive(Debug, Error)]
pub enum CacheError {
    #[error("failed I/O on cache file: {0}")]
    Io(#[from] std::io::Error),
    #[error("failed JSON parse/serialize on cache file: {0}")]
    Json(#[from] serde_json::Error),
}

pub fn now_unix_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

pub fn now_unix_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0)
}

/// Resolve an accessible, permission-friendly directory for cache, overrides, and transaction logs.
pub fn resolve_data_dir(custom: Option<&Path>, workspace_hint: Option<&Path>) -> PathBuf {
    if let Some(p) = custom {
        return p.to_path_buf();
    }

    if let Some(hint) = workspace_hint {
        if hint.join("Cargo.toml").exists() || hint.join("anime.sh").exists() {
            return hint.join(".renamer-data");
        }
    }

    if let Ok(cwd) = std::env::current_dir() {
        if cwd.join("Cargo.toml").exists() || cwd.join("anime.sh").exists() {
            return cwd.join(".renamer-data");
        }
    }

    #[cfg(target_os = "windows")]
    if let Ok(local_app_data) = std::env::var("LOCALAPPDATA") {
        if !local_app_data.trim().is_empty() {
            return PathBuf::from(local_app_data).join("RenamerAnime");
        }
    }

    if let Ok(xdg_data) = std::env::var("XDG_DATA_HOME") {
        if !xdg_data.trim().is_empty() {
            return PathBuf::from(xdg_data).join("renamer-anime");
        }
    }

    if let Ok(home) = std::env::var("HOME") {
        if !home.trim().is_empty() {
            return PathBuf::from(home)
                .join(".local")
                .join("share")
                .join("renamer-anime");
        }
    }

    std::env::temp_dir().join("renamer-anime-data")
}

pub fn normalize_query_key(q: &str) -> String {
    let trimmed = q
        .trim()
        .trim_matches(|c: char| !c.is_alphanumeric() && c.is_ascii())
        .to_lowercase();
    trimmed.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MediaCandidate {
    pub id: u64,
    pub romaji: Option<String>,
    pub english: Option<String>,
    pub native: Option<String>,
    #[serde(default)]
    pub synonyms: Vec<String>,
    pub season_year: Option<u32>,
    pub format: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CacheStatus {
    Match,
    NoMatch,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CacheEntry {
    pub query: String,
    pub normalized_query: String,
    pub status: CacheStatus,
    pub selected: Option<MediaCandidate>,
    #[serde(default)]
    pub candidates: Vec<MediaCandidate>,
    pub fetched_at_unix: u64,
    pub expires_at_unix: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TitleCache {
    #[serde(default)]
    pub entries: HashMap<String, CacheEntry>,
}

impl TitleCache {
    pub fn load(data_dir: &Path) -> Result<Self, CacheError> {
        let path = data_dir.join("title_cache.json");
        if !path.exists() {
            return Ok(Self::default());
        }
        let text = std::fs::read_to_string(&path)?;
        let cache = serde_json::from_str(&text)?;
        Ok(cache)
    }

    pub fn save(&self, data_dir: &Path) -> Result<(), CacheError> {
        atomic_write_json(&data_dir.join("title_cache.json"), self)
    }

    pub fn get_valid(&self, query: &str, now_unix: u64) -> Option<&CacheEntry> {
        let key = normalize_query_key(query);
        if key.is_empty() {
            return None;
        }
        let entry = self.entries.get(&key)?;
        if entry.expires_at_unix > now_unix {
            Some(entry)
        } else {
            None
        }
    }

    pub fn upsert_match(
        &mut self,
        query: &str,
        selected: MediaCandidate,
        candidates: Vec<MediaCandidate>,
        now_unix: u64,
    ) {
        let key = normalize_query_key(query);
        if key.is_empty() {
            return;
        }
        self.entries.insert(
            key.clone(),
            CacheEntry {
                query: query.to_string(),
                normalized_query: key,
                status: CacheStatus::Match,
                selected: Some(selected),
                candidates,
                fetched_at_unix: now_unix,
                expires_at_unix: now_unix.saturating_add(POSITIVE_CACHE_TTL_SECS),
            },
        );
    }

    pub fn upsert_no_match(&mut self, query: &str, now_unix: u64) {
        let key = normalize_query_key(query);
        if key.is_empty() {
            return;
        }
        self.entries.insert(
            key.clone(),
            CacheEntry {
                query: query.to_string(),
                normalized_query: key,
                status: CacheStatus::NoMatch,
                selected: None,
                candidates: Vec::new(),
                fetched_at_unix: now_unix,
                expires_at_unix: now_unix.saturating_add(NEGATIVE_CACHE_TTL_SECS),
            },
        );
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ProcessedOutcome {
    Renamed,
    AlreadyClean,
    NoMatchCleaned,
    ManualOverride,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProcessedRecord {
    pub item_name: String,
    pub mode: NamingMode,
    pub clean_tags: bool,
    pub outcome: ProcessedOutcome,
    pub anilist_id: Option<u64>,
    pub updated_at_unix: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ProcessedLedger {
    #[serde(default)]
    pub records: HashMap<String, ProcessedRecord>,
}

impl ProcessedLedger {
    pub fn load(data_dir: &Path) -> Result<Self, CacheError> {
        let path = data_dir.join("processed_items.json");
        if !path.exists() {
            return Ok(Self::default());
        }
        let text = std::fs::read_to_string(&path)?;
        let ledger = serde_json::from_str(&text)?;
        Ok(ledger)
    }

    pub fn save(&self, data_dir: &Path) -> Result<(), CacheError> {
        atomic_write_json(&data_dir.join("processed_items.json"), self)
    }

    pub fn make_key(
        parent_dir: &Path,
        item_name: &str,
        mode: NamingMode,
        clean_tags: bool,
    ) -> String {
        let canonical_parent = std::fs::canonicalize(parent_dir)
            .unwrap_or_else(|_| parent_dir.to_path_buf())
            .to_string_lossy()
            .to_string();
        format!("{canonical_parent}::{item_name}::{mode:?}::{clean_tags}")
    }

    pub fn get(
        &self,
        parent_dir: &Path,
        item_name: &str,
        mode: NamingMode,
        clean_tags: bool,
    ) -> Option<&ProcessedRecord> {
        let key = Self::make_key(parent_dir, item_name, mode, clean_tags);
        self.records.get(&key)
    }

    pub fn record(
        &mut self,
        parent_dir: &Path,
        item_name: &str,
        mode: NamingMode,
        clean_tags: bool,
        outcome: ProcessedOutcome,
        anilist_id: Option<u64>,
        now_unix: u64,
    ) {
        let key = Self::make_key(parent_dir, item_name, mode, clean_tags);
        self.records.insert(
            key,
            ProcessedRecord {
                item_name: item_name.to_string(),
                mode,
                clean_tags,
                outcome,
                anilist_id,
                updated_at_unix: now_unix,
            },
        );
    }

    pub fn remove(
        &mut self,
        parent_dir: &Path,
        item_name: &str,
        mode: NamingMode,
        clean_tags: bool,
    ) -> Option<ProcessedRecord> {
        let key = Self::make_key(parent_dir, item_name, mode, clean_tags);
        self.records.remove(&key)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ManualOverride {
    pub english_title: String,
    pub romaji_title: Option<String>,
    pub anilist_id: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ManualOverrides {
    #[serde(default)]
    pub entries: HashMap<String, ManualOverride>,
}

impl ManualOverrides {
    pub fn load(data_dir: &Path) -> Result<Self, CacheError> {
        let path = data_dir.join("manual_overrides.json");
        if !path.exists() {
            return Ok(Self::default());
        }
        let text = std::fs::read_to_string(&path)?;
        let overrides = serde_json::from_str(&text)?;
        Ok(overrides)
    }

    pub fn save(&self, data_dir: &Path) -> Result<(), CacheError> {
        atomic_write_json(&data_dir.join("manual_overrides.json"), self)
    }

    pub fn get(&self, query_or_name: &str) -> Option<&ManualOverride> {
        let key = normalize_query_key(query_or_name);
        self.entries
            .get(&key)
            .or_else(|| self.entries.get(query_or_name))
    }

    pub fn set(&mut self, query_or_name: &str, entry: ManualOverride) {
        let key = normalize_query_key(query_or_name);
        if !key.is_empty() {
            self.entries.insert(key, entry);
        }
    }
}

pub(crate) fn atomic_write_json<T: Serialize>(path: &Path, value: &T) -> Result<(), CacheError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tmp_path = path.with_extension("json.tmp");
    let json = serde_json::to_string_pretty(value)?;
    std::fs::write(&tmp_path, json)?;
    if path.exists() {
        let _ = std::fs::remove_file(path);
    }
    std::fs::rename(&tmp_path, path)?;
    Ok(())
}
