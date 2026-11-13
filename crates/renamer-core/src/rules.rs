use serde::{Deserialize, Serialize};
use std::path::Path;
use thiserror::Error;

const DEFAULT_RULES_JSON: &str = include_str!("../../../rules/default_rules.json");

#[derive(Debug, Error)]
pub enum RulesError {
    #[error("failed to read rules file: {0}")]
    Io(#[from] std::io::Error),
    #[error("failed to parse rules JSON: {0}")]
    Json(#[from] serde_json::Error),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CleanupRules {
    pub version: String,
    pub ignore_names: Vec<String>,
    pub media_extensions: Vec<String>,
    pub subtitle_lang_codes: Vec<String>,
    pub noise_tokens: Vec<String>,
    pub batch_keywords: Vec<String>,
    pub trailing_group_tags: Vec<String>,
    pub extra_note_keywords: Vec<String>,
}

impl CleanupRules {
    /// Load the built-in default ruleset compiled from `rules/default_rules.json`.
    pub fn default_rules() -> Self {
        serde_json::from_str(DEFAULT_RULES_JSON)
            .expect("embedded rules/default_rules.json must be valid JSON")
    }

    /// Load a user-edited ruleset from a JSON file on disk.
    pub fn from_path(path: &Path) -> Result<Self, RulesError> {
        let contents = std::fs::read_to_string(path)?;
        let rules = serde_json::from_str(&contents)?;
        Ok(rules)
    }

    /// Save ruleset to a JSON file on disk (pretty-printed for user editing).
    pub fn save_to_path(&self, path: &Path) -> Result<(), RulesError> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let json = serde_json::to_string_pretty(self)?;
        std::fs::write(path, json)?;
        Ok(())
    }

    /// Returns true if the given entry name matches the ignore list (case-insensitive).
    pub fn is_ignored(&self, name: &str) -> bool {
        let lower = name.trim().to_ascii_lowercase();
        self.ignore_names
            .iter()
            .any(|ignored| ignored.to_ascii_lowercase() == lower)
    }
}
