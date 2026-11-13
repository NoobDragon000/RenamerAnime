use crate::api::{resolve_queries_with_cache, AniListClient, ApiError};
use crate::cache::{
    atomic_write_json, now_unix_millis, now_unix_secs, CacheError, CacheStatus, ManualOverrides,
    MediaCandidate, ProcessedLedger, ProcessedOutcome, TitleCache,
};
use crate::formatter::{format_item, FormatOptions, NamingMode};
use crate::parser::{parse_item_name, ParsedItem};
use crate::rules::CleanupRules;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum PlannerError {
    #[error("root directory does not exist or is not a directory: {0}")]
    InvalidRoot(PathBuf),
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("cache error: {0}")]
    Cache(#[from] CacheError),
    #[error("API error: {0}")]
    Api(#[from] ApiError),
    #[error("transaction batch not found: {0}")]
    BatchNotFound(String),
    #[error("transaction batch already reverted: {0}")]
    AlreadyReverted(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanConfig {
    pub root_dir: PathBuf,
    pub format_opts: FormatOptions,
    pub recursive: bool,
    pub offline: bool,
    pub force_recheck: bool,
    pub data_dir: PathBuf,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PlanItemStatus {
    ReadyToRename,
    AlreadyClean,
    AlreadyProcessed,
    Conflict,
    SkippedEmptyQuery,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ResolutionSource {
    None,
    AlreadyProcessed,
    ManualOverride,
    CacheHit,
    ApiMatch,
    InlineAlias,
    NoMatch,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanItem {
    pub index: usize,
    pub is_dir: bool,
    pub depth: usize,
    pub parent_dir: PathBuf,
    pub current_path: PathBuf,
    pub current_name: String,
    pub proposed_name: String,
    pub proposed_path: PathBuf,
    pub parsed: ParsedItem,
    pub effective_query: String,
    pub anilist_id: Option<u64>,
    pub resolved_english: Option<String>,
    pub resolved_romaji: Option<String>,
    pub candidates: Vec<MediaCandidate>,
    pub resolution_source: ResolutionSource,
    pub status: PlanItemStatus,
    pub status_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RenamePlan {
    pub root_dir: PathBuf,
    pub data_dir: PathBuf,
    pub mode: NamingMode,
    pub clean_tags: bool,
    pub items: Vec<PlanItem>,
    pub api_requests_made: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransactionOp {
    pub old_path: PathBuf,
    pub new_path: PathBuf,
    pub parent_dir: PathBuf,
    pub old_name: String,
    pub new_name: String,
    pub is_dir: bool,
    pub anilist_id: Option<u64>,
    pub outcome: ProcessedOutcome,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransactionBatch {
    pub batch_id: String,
    pub created_at_unix: u64,
    pub root_dir: PathBuf,
    pub mode: NamingMode,
    pub clean_tags: bool,
    pub reverted: bool,
    pub operations: Vec<TransactionOp>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApplyReport {
    pub batch_id: String,
    pub renamed_count: usize,
    pub skipped_count: usize,
    pub error_count: usize,
    pub cancelled: bool,
    pub errors: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UndoReport {
    pub batch_id: String,
    pub restored_count: usize,
    pub error_count: usize,
    pub restored_items: Vec<(String, String)>,
    pub errors: Vec<String>,
}

struct ScannedEntry {
    is_dir: bool,
    depth: usize,
    parent_dir: PathBuf,
    current_path: PathBuf,
    current_name: String,
    parsed: ParsedItem,
    effective_query: String,
    effective_fallbacks: Vec<String>,
    is_pure_season_subdir: bool,
    already_processed_record: Option<ProcessedOutcome>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VirtualEntry {
    pub uri: String,
    pub parent_uri: String,
    pub name: String,
    pub is_dir: bool,
    pub depth: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SafRenameOutcome {
    pub old_uri: String,
    pub new_uri: String,
    pub parent_uri: String,
    pub old_name: String,
    pub new_name: String,
    pub is_dir: bool,
    pub anilist_id: Option<u64>,
    pub resolution_source: ResolutionSource,
}

pub fn build_rename_plan(
    config: &PlanConfig,
    rules: &CleanupRules,
    on_progress: &mut dyn FnMut(&str),
) -> Result<RenamePlan, PlannerError> {
    if !config.root_dir.is_dir() {
        return Err(PlannerError::InvalidRoot(config.root_dir.clone()));
    }

    let mut scanned = Vec::new();
    collect_entries(
        &config.root_dir,
        0,
        None,
        &[],
        config.recursive,
        rules,
        &mut scanned,
    )?;

    build_plan_from_scanned(&config.root_dir, scanned, config, None, on_progress)
}

/// Build a dry-run `RenamePlan` from a virtual filesystem listing (such as Android SAF `content://` URIs).
pub fn build_virtual_rename_plan(
    root_uri: &str,
    entries: &[VirtualEntry],
    config: &PlanConfig,
    rules: &CleanupRules,
    on_progress: &mut dyn FnMut(&str),
) -> Result<RenamePlan, PlannerError> {
    let mut sorted: Vec<&VirtualEntry> = entries
        .iter()
        .filter(|e| !rules.is_ignored(&e.name))
        .filter(|e| config.recursive || e.depth == 0)
        .collect();
    sorted.sort_by(|a, b| {
        a.depth
            .cmp(&b.depth)
            .then_with(|| a.parent_uri.cmp(&b.parent_uri))
            .then_with(|| a.name.cmp(&b.name))
    });

    let mut inherited_by_uri: HashMap<String, (String, Vec<String>)> = HashMap::new();
    let mut existing_names_per_parent: HashMap<PathBuf, HashSet<String>> = HashMap::new();
    let mut scanned = Vec::with_capacity(sorted.len());

    for entry in sorted {
        let parsed = parse_item_name(&entry.name, entry.is_dir, rules);
        let is_pure_season_subdir = entry.is_dir
            && entry.depth > 0
            && parsed.base_title.is_empty()
            && parsed.season_spec.is_some();

        let (inherited_q, inherited_fb) = inherited_by_uri
            .get(&entry.parent_uri)
            .map(|(q, fb)| (Some(q.as_str()), fb.as_slice()))
            .unwrap_or((None, &[]));

        let (effective_query, effective_fallbacks) = if !parsed.search_query.is_empty() {
            (parsed.search_query.clone(), parsed.fallback_queries.clone())
        } else if let Some(parent_q) = inherited_q {
            (parent_q.to_string(), inherited_fb.to_vec())
        } else {
            (String::new(), Vec::new())
        };

        if entry.is_dir && !effective_query.is_empty() {
            inherited_by_uri.insert(
                entry.uri.clone(),
                (effective_query.clone(), effective_fallbacks.clone()),
            );
        } else if entry.is_dir {
            if let Some(parent_pair) = inherited_by_uri.get(&entry.parent_uri).cloned() {
                inherited_by_uri.insert(entry.uri.clone(), parent_pair);
            }
        }

        let parent_path = PathBuf::from(&entry.parent_uri);
        existing_names_per_parent
            .entry(parent_path.clone())
            .or_default()
            .insert(entry.name.to_ascii_lowercase());

        scanned.push(ScannedEntry {
            is_dir: entry.is_dir,
            depth: entry.depth,
            parent_dir: parent_path,
            current_path: PathBuf::from(&entry.uri),
            current_name: entry.name.clone(),
            parsed,
            effective_query,
            effective_fallbacks,
            is_pure_season_subdir,
            already_processed_record: None,
        });
    }

    build_plan_from_scanned(
        Path::new(root_uri),
        scanned,
        config,
        Some(&existing_names_per_parent),
        on_progress,
    )
}

fn join_virtual_or_fs_child(parent: &Path, child: &str) -> PathBuf {
    let parent_str = parent.to_string_lossy();
    if parent_str.starts_with("content://") {
        let trimmed = parent_str.trim_end_matches('/');
        PathBuf::from(format!("{trimmed}/{child}"))
    } else {
        parent.join(child)
    }
}

fn build_plan_from_scanned(
    root_path: &Path,
    mut scanned: Vec<ScannedEntry>,
    config: &PlanConfig,
    virtual_existing_names: Option<&HashMap<PathBuf, HashSet<String>>>,
    on_progress: &mut dyn FnMut(&str),
) -> Result<RenamePlan, PlannerError> {
    let mut cache = TitleCache::load(&config.data_dir)?;
    let mut ledger = ProcessedLedger::load(&config.data_dir)?;
    let overrides = ManualOverrides::load(&config.data_dir)?;

    // Check ProcessedLedger and determine which queries actually need API lookups
    let mut queries_to_resolve: Vec<(String, Vec<String>)> = Vec::new();

    for entry in &mut scanned {
        if entry.is_pure_season_subdir {
            continue;
        }

        // Check manual override first; if present, don't skip via ledger so override takes effect
        let has_override = overrides.get(&entry.effective_query).is_some()
            || overrides.get(&entry.current_name).is_some();

        if !config.force_recheck && !has_override {
            if let Some(rec) = ledger.get(
                &entry.parent_dir,
                &entry.current_name,
                config.format_opts.mode,
                config.format_opts.clean_tags,
            ) {
                entry.already_processed_record = Some(rec.outcome);
                continue;
            }
        }

        if !entry.effective_query.is_empty() {
            queries_to_resolve.push((
                entry.effective_query.clone(),
                entry.effective_fallbacks.clone(),
            ));
        }
    }

    let mut client = AniListClient::new();
    let api_requests_made = resolve_queries_with_cache(
        &mut client,
        &mut cache,
        &queries_to_resolve,
        config.offline,
        on_progress,
    )?;

    if api_requests_made > 0 {
        cache.save(&config.data_dir)?;
    }

    let now = now_unix_secs();
    let mut plan_items = Vec::with_capacity(scanned.len());
    let mut seen_targets_per_dir: HashMap<PathBuf, HashSet<String>> = HashMap::new();

    for (idx, entry) in scanned.into_iter().enumerate() {
        if entry.is_pure_season_subdir {
            plan_items.push(PlanItem {
                index: idx,
                is_dir: entry.is_dir,
                depth: entry.depth,
                parent_dir: entry.parent_dir,
                current_path: entry.current_path.clone(),
                current_name: entry.current_name.clone(),
                proposed_name: entry.current_name,
                proposed_path: entry.current_path,
                parsed: entry.parsed,
                effective_query: entry.effective_query,
                anilist_id: None,
                resolved_english: None,
                resolved_romaji: None,
                candidates: Vec::new(),
                resolution_source: ResolutionSource::None,
                status: PlanItemStatus::AlreadyClean,
                status_reason: Some("Season subfolder".to_string()),
            });
            continue;
        }

        if let Some(outcome) = entry.already_processed_record {
            plan_items.push(PlanItem {
                index: idx,
                is_dir: entry.is_dir,
                depth: entry.depth,
                parent_dir: entry.parent_dir,
                current_path: entry.current_path.clone(),
                current_name: entry.current_name.clone(),
                proposed_name: entry.current_name,
                proposed_path: entry.current_path,
                parsed: entry.parsed,
                effective_query: entry.effective_query,
                anilist_id: None,
                resolved_english: None,
                resolved_romaji: None,
                candidates: Vec::new(),
                resolution_source: ResolutionSource::AlreadyProcessed,
                status: PlanItemStatus::AlreadyProcessed,
                status_reason: Some(format!("Previously processed ({outcome:?})")),
            });
            continue;
        }

        if entry.effective_query.is_empty() {
            plan_items.push(PlanItem {
                index: idx,
                is_dir: entry.is_dir,
                depth: entry.depth,
                parent_dir: entry.parent_dir,
                current_path: entry.current_path.clone(),
                current_name: entry.current_name.clone(),
                proposed_name: entry.current_name,
                proposed_path: entry.current_path,
                parsed: entry.parsed,
                effective_query: entry.effective_query,
                anilist_id: None,
                resolved_english: None,
                resolved_romaji: None,
                candidates: Vec::new(),
                resolution_source: ResolutionSource::None,
                status: PlanItemStatus::SkippedEmptyQuery,
                status_reason: Some("No extractable title query".to_string()),
            });
            continue;
        }

        let mut anilist_id = None;
        let mut resolved_english: Option<String> = None;
        let mut resolved_romaji: Option<String> = None;
        let mut candidates: Vec<MediaCandidate> = Vec::new();
        let mut source = ResolutionSource::NoMatch;

        if let Some(ov) = overrides
            .get(&entry.current_name)
            .or_else(|| overrides.get(&entry.effective_query))
        {
            anilist_id = ov.anilist_id;
            resolved_english = Some(ov.english_title.clone());
            resolved_romaji = ov.romaji_title.clone();
            source = ResolutionSource::ManualOverride;
        } else if let Some(cached) = cache.get_valid(&entry.effective_query, now) {
            candidates = cached.candidates.clone();
            match cached.status {
                CacheStatus::Match => {
                    if let Some(sel) = &cached.selected {
                        anilist_id = Some(sel.id);
                        resolved_english = sel.english.clone();
                        resolved_romaji = sel.romaji.clone();
                    }
                    source = if api_requests_made > 0 && cached.fetched_at_unix >= now.saturating_sub(60) {
                        ResolutionSource::ApiMatch
                    } else {
                        ResolutionSource::CacheHit
                    };
                }
                CacheStatus::NoMatch => {
                    source = if entry.parsed.inline_alias.is_some() {
                        ResolutionSource::InlineAlias
                    } else {
                        ResolutionSource::NoMatch
                    };
                }
            }
        } else if entry.parsed.inline_alias.is_some() {
            source = ResolutionSource::InlineAlias;
        }

        let formatted = format_item(
            &entry.parsed,
            resolved_english.as_deref(),
            resolved_romaji.as_deref(),
            &config.format_opts,
        );

        let proposed_name = formatted.proposed_name;
        let proposed_path = join_virtual_or_fs_child(&entry.parent_dir, &proposed_name);

        let (status, status_reason) = if proposed_name == entry.current_name {
            // Record already-clean items so subsequent runs make zero API calls
            if !config.offline || source != ResolutionSource::NoMatch {
                let outcome = if source == ResolutionSource::NoMatch {
                    ProcessedOutcome::NoMatchCleaned
                } else {
                    ProcessedOutcome::AlreadyClean
                };
                ledger.record(
                    &entry.parent_dir,
                    &entry.current_name,
                    config.format_opts.mode,
                    config.format_opts.clean_tags,
                    outcome,
                    anilist_id,
                    now,
                );
            }
            (PlanItemStatus::AlreadyClean, None)
        } else {
            let dir_set = seen_targets_per_dir
                .entry(entry.parent_dir.clone())
                .or_default();
            let norm_target = proposed_name.to_ascii_lowercase();
            let is_case_only_rename = proposed_name.eq_ignore_ascii_case(&entry.current_name);
            let target_exists = match virtual_existing_names {
                Some(map) => map
                    .get(&entry.parent_dir)
                    .map(|set| set.contains(&norm_target))
                    .unwrap_or(false),
                None => proposed_path.exists(),
            };

            if !dir_set.insert(norm_target) {
                (
                    PlanItemStatus::Conflict,
                    Some("Duplicate proposed name in the same folder".to_string()),
                )
            } else if target_exists && !is_case_only_rename {
                (
                    PlanItemStatus::Conflict,
                    Some(format!("Target already exists: {proposed_name}")),
                )
            } else {
                (PlanItemStatus::ReadyToRename, None)
            }
        };

        plan_items.push(PlanItem {
            index: idx,
            is_dir: entry.is_dir,
            depth: entry.depth,
            parent_dir: entry.parent_dir,
            current_path: entry.current_path,
            current_name: entry.current_name,
            proposed_name,
            proposed_path,
            parsed: entry.parsed,
            effective_query: entry.effective_query,
            anilist_id,
            resolved_english,
            resolved_romaji,
            candidates,
            resolution_source: source,
            status,
            status_reason,
        });
    }

    ledger.save(&config.data_dir)?;

    Ok(RenamePlan {
        root_dir: root_path.to_path_buf(),
        data_dir: config.data_dir.clone(),
        mode: config.format_opts.mode,
        clean_tags: config.format_opts.clean_tags,
        items: plan_items,
        api_requests_made,
    })
}

fn collect_entries(
    dir: &Path,
    depth: usize,
    inherited_query: Option<&str>,
    inherited_fallbacks: &[String],
    recursive: bool,
    rules: &CleanupRules,
    out: &mut Vec<ScannedEntry>,
) -> Result<(), PlannerError> {
    let mut entries: Vec<_> = std::fs::read_dir(dir)?.filter_map(Result::ok).collect();
    entries.sort_by_key(|e| e.file_name());

    for entry in entries {
        let name = entry.file_name().to_string_lossy().to_string();
        if rules.is_ignored(&name) {
            continue;
        }
        let path = entry.path();
        let is_dir = path.is_dir();
        let parsed = parse_item_name(&name, is_dir, rules);

        let is_pure_season_subdir =
            is_dir && depth > 0 && parsed.base_title.is_empty() && parsed.season_spec.is_some();

        let (effective_query, effective_fallbacks) = if !parsed.search_query.is_empty() {
            (parsed.search_query.clone(), parsed.fallback_queries.clone())
        } else if let Some(parent_q) = inherited_query {
            (parent_q.to_string(), inherited_fallbacks.to_vec())
        } else {
            (String::new(), Vec::new())
        };

        let next_inherited_query = if !effective_query.is_empty() {
            Some(effective_query.clone())
        } else {
            inherited_query.map(str::to_string)
        };
        let next_inherited_fallbacks = if !effective_fallbacks.is_empty() {
            effective_fallbacks.clone()
        } else {
            inherited_fallbacks.to_vec()
        };

        out.push(ScannedEntry {
            is_dir,
            depth,
            parent_dir: dir.to_path_buf(),
            current_path: path.clone(),
            current_name: name,
            parsed,
            effective_query,
            effective_fallbacks,
            is_pure_season_subdir,
            already_processed_record: None,
        });

        if is_dir && recursive {
            collect_entries(
                &path,
                depth + 1,
                next_inherited_query.as_deref(),
                &next_inherited_fallbacks,
                true,
                rules,
                out,
            )?;
        }
    }

    Ok(())
}

/// Apply selected items from a `RenamePlan` in bottom-up order (deepest children first),
/// flushing every completed operation to the JSON transaction log for safe rollback.
pub fn execute_rename_plan(
    plan: &RenamePlan,
    selected_indices: Option<&[usize]>,
    cancel_flag: Option<&AtomicBool>,
) -> Result<ApplyReport, PlannerError> {
    let selected_set: Option<HashSet<usize>> =
        selected_indices.map(|slice| slice.iter().copied().collect());

    let mut targets: Vec<&PlanItem> = plan
        .items
        .iter()
        .filter(|item| item.status == PlanItemStatus::ReadyToRename)
        .filter(|item| {
            selected_set
                .as_ref()
                .map(|s| s.contains(&item.index))
                .unwrap_or(true)
        })
        .collect();

    // Bottom-up ordering: deepest paths first (episodes -> subfolders -> root folders)
    targets.sort_by(|a, b| b.depth.cmp(&a.depth).then_with(|| a.index.cmp(&b.index)));

    let batch_id = format!("batch_{}", now_unix_millis());
    let mut batch = TransactionBatch {
        batch_id: batch_id.clone(),
        created_at_unix: now_unix_secs(),
        root_dir: plan.root_dir.clone(),
        mode: plan.mode,
        clean_tags: plan.clean_tags,
        reverted: false,
        operations: Vec::new(),
    };

    let tx_dir = plan.data_dir.join("transactions");
    let batch_path = tx_dir.join(format!("{batch_id}.json"));
    let latest_path = tx_dir.join("latest.json");

    let mut ledger = ProcessedLedger::load(&plan.data_dir)?;
    let mut renamed_count = 0usize;
    let mut skipped_count = 0usize;
    let mut errors = Vec::new();
    let mut cancelled = false;
    let mut dir_renames: Vec<(PathBuf, PathBuf)> = Vec::new();

    for item in targets {
        if let Some(flag) = cancel_flag {
            if flag.load(Ordering::Relaxed) {
                cancelled = true;
                break;
            }
        }

        let is_case_only = item.proposed_name.eq_ignore_ascii_case(&item.current_name);
        if item.proposed_path.exists() && !is_case_only {
            skipped_count += 1;
            errors.push(format!(
                "Skipped '{}' -> '{}': destination already exists",
                item.current_name, item.proposed_name
            ));
            continue;
        }

        match std::fs::rename(&item.current_path, &item.proposed_path) {
            Ok(()) => {
                renamed_count += 1;
                let outcome = match item.resolution_source {
                    ResolutionSource::ManualOverride => ProcessedOutcome::ManualOverride,
                    ResolutionSource::NoMatch => ProcessedOutcome::NoMatchCleaned,
                    _ => ProcessedOutcome::Renamed,
                };
                if item.is_dir {
                    dir_renames.push((item.current_path.clone(), item.proposed_path.clone()));
                }
                batch.operations.push(TransactionOp {
                    old_path: item.current_path.clone(),
                    new_path: item.proposed_path.clone(),
                    parent_dir: item.parent_dir.clone(),
                    old_name: item.current_name.clone(),
                    new_name: item.proposed_name.clone(),
                    is_dir: item.is_dir,
                    anilist_id: item.anilist_id,
                    outcome,
                });
                atomic_write_json(&batch_path, &batch)?;
                atomic_write_json(&latest_path, &batch)?;
            }
            Err(e) => {
                errors.push(format!(
                    "Failed '{}' -> '{}': {e}",
                    item.current_name, item.proposed_name
                ));
            }
        }
    }

    // Update ProcessedLedger using final post-rename parent paths so nested children stay matched
    let now = now_unix_secs();
    for op in &batch.operations {
        let final_parent = remap_parent_path(&op.parent_dir, &dir_renames);
        ledger.record(
            &final_parent,
            &op.new_name,
            plan.mode,
            plan.clean_tags,
            op.outcome,
            op.anilist_id,
            now,
        );
    }
    ledger.save(&plan.data_dir)?;

    Ok(ApplyReport {
        batch_id,
        renamed_count,
        skipped_count,
        error_count: errors.len(),
        cancelled,
        errors,
    })
}

fn outcome_from_resolution(source: ResolutionSource) -> ProcessedOutcome {
    match source {
        ResolutionSource::ManualOverride => ProcessedOutcome::ManualOverride,
        ResolutionSource::NoMatch => ProcessedOutcome::NoMatchCleaned,
        _ => ProcessedOutcome::Renamed,
    }
}

fn remap_parent_path(parent: &Path, dir_renames: &[(PathBuf, PathBuf)]) -> PathBuf {
    let mut current = parent.to_path_buf();
    for (old_dir, new_dir) in dir_renames {
        let old_s = old_dir.to_string_lossy();
        if old_s.starts_with("content://") {
            let cur_s = current.to_string_lossy().to_string();
            if cur_s == old_s.as_ref() {
                current = new_dir.clone();
            } else if let Some(rest) = cur_s.strip_prefix(old_s.as_ref()) {
                if rest.starts_with("%2F") || rest.starts_with("%2f") || rest.starts_with('/') {
                    let new_s = new_dir.to_string_lossy();
                    current = PathBuf::from(format!("{new_s}{rest}"));
                }
            }
            continue;
        }
        if current == *old_dir {
            current = new_dir.clone();
        } else if let Ok(rel) = current.strip_prefix(old_dir) {
            current = if rel.as_os_str().is_empty() {
                new_dir.clone()
            } else {
                new_dir.join(rel)
            };
        }
    }
    current
}

/// Persist completed Android SAF (`content://`) rename outcomes to the transaction log and ledger.
pub fn record_virtual_batch(
    plan: &RenamePlan,
    outcomes: &[SafRenameOutcome],
    errors: Vec<String>,
    skipped_count: usize,
    cancelled: bool,
) -> Result<ApplyReport, PlannerError> {
    let batch_id = format!("batch_{}", now_unix_millis());
    let mut batch = TransactionBatch {
        batch_id: batch_id.clone(),
        created_at_unix: now_unix_secs(),
        root_dir: plan.root_dir.clone(),
        mode: plan.mode,
        clean_tags: plan.clean_tags,
        reverted: false,
        operations: Vec::with_capacity(outcomes.len()),
    };

    let mut dir_renames: Vec<(PathBuf, PathBuf)> = Vec::new();
    for o in outcomes {
        let outcome = outcome_from_resolution(o.resolution_source);
        if o.is_dir {
            dir_renames.push((PathBuf::from(&o.old_uri), PathBuf::from(&o.new_uri)));
        }
        batch.operations.push(TransactionOp {
            old_path: PathBuf::from(&o.old_uri),
            new_path: PathBuf::from(&o.new_uri),
            parent_dir: PathBuf::from(&o.parent_uri),
            old_name: o.old_name.clone(),
            new_name: o.new_name.clone(),
            is_dir: o.is_dir,
            anilist_id: o.anilist_id,
            outcome,
        });
    }

    let tx_dir = plan.data_dir.join("transactions");
    let batch_path = tx_dir.join(format!("{batch_id}.json"));
    let latest_path = tx_dir.join("latest.json");
    atomic_write_json(&batch_path, &batch)?;
    atomic_write_json(&latest_path, &batch)?;

    let mut ledger = ProcessedLedger::load(&plan.data_dir)?;
    let now = now_unix_secs();
    for op in &batch.operations {
        let final_parent = remap_parent_path(&op.parent_dir, &dir_renames);
        ledger.record(
            &final_parent,
            &op.new_name,
            plan.mode,
            plan.clean_tags,
            op.outcome,
            op.anilist_id,
            now,
        );
    }
    ledger.save(&plan.data_dir)?;

    Ok(ApplyReport {
        batch_id,
        renamed_count: outcomes.len(),
        skipped_count,
        error_count: errors.len(),
        cancelled,
        errors,
    })
}

pub fn get_transaction_batch(
    data_dir: &Path,
    batch_id: Option<&str>,
) -> Result<TransactionBatch, PlannerError> {
    let tx_dir = data_dir.join("transactions");
    let batch_path = match batch_id {
        Some(id) => tx_dir.join(format!("{id}.json")),
        None => tx_dir.join("latest.json"),
    };

    if !batch_path.exists() {
        return Err(PlannerError::BatchNotFound(
            batch_id.unwrap_or("latest").to_string(),
        ));
    }

    let text = std::fs::read_to_string(&batch_path)?;
    let batch: TransactionBatch = serde_json::from_str(&text).map_err(CacheError::Json)?;
    Ok(batch)
}

/// Update transaction batch & processed ledger after an Android SAF (`content://`) undo pass.
pub fn mark_virtual_batch_reverted(
    data_dir: &Path,
    batch_id: Option<&str>,
    restored_items: Vec<(String, String)>,
    errors: Vec<String>,
) -> Result<UndoReport, PlannerError> {
    let tx_dir = data_dir.join("transactions");
    let mut batch = get_transaction_batch(data_dir, batch_id)?;
    if batch.reverted {
        return Err(PlannerError::AlreadyReverted(batch.batch_id));
    }

    let mut ledger = ProcessedLedger::load(data_dir)?;
    let mut dir_renames: Vec<(PathBuf, PathBuf)> = Vec::new();
    for op in &batch.operations {
        if op.is_dir {
            dir_renames.push((op.old_path.clone(), op.new_path.clone()));
        }
    }

    let restored_set: HashSet<(String, String)> = restored_items.iter().cloned().collect();
    for op in &batch.operations {
        if restored_set.contains(&(op.new_name.clone(), op.old_name.clone())) {
            let remapped_parent = remap_parent_path(&op.parent_dir, &dir_renames);
            ledger.remove(&remapped_parent, &op.new_name, batch.mode, batch.clean_tags);
            ledger.remove(&op.parent_dir, &op.new_name, batch.mode, batch.clean_tags);
        }
    }

    batch.reverted = errors.is_empty();
    let specific_batch_path = tx_dir.join(format!("{}.json", batch.batch_id));
    atomic_write_json(&specific_batch_path, &batch)?;
    let latest_path = tx_dir.join("latest.json");
    if latest_path.exists() {
        if let Ok(latest_txt) = std::fs::read_to_string(&latest_path) {
            if let Ok(latest) = serde_json::from_str::<TransactionBatch>(&latest_txt) {
                if latest.batch_id == batch.batch_id {
                    atomic_write_json(&latest_path, &batch)?;
                }
            }
        }
    }
    ledger.save(data_dir)?;

    Ok(UndoReport {
        batch_id: batch.batch_id,
        restored_count: restored_items.len(),
        error_count: errors.len(),
        restored_items,
        errors,
    })
}

pub fn list_transaction_batches(data_dir: &Path) -> Result<Vec<TransactionBatch>, PlannerError> {
    let tx_dir = data_dir.join("transactions");
    if !tx_dir.exists() {
        return Ok(Vec::new());
    }
    let mut batches = Vec::new();
    for entry in std::fs::read_dir(&tx_dir)?.filter_map(Result::ok) {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        if name == "latest.json" || !name.ends_with(".json") {
            continue;
        }
        if let Ok(text) = std::fs::read_to_string(&path) {
            if let Ok(batch) = serde_json::from_str::<TransactionBatch>(&text) {
                batches.push(batch);
            }
        }
    }
    batches.sort_by(|a, b| b.created_at_unix.cmp(&a.created_at_unix).then_with(|| b.batch_id.cmp(&a.batch_id)));
    Ok(batches)
}

/// Undo a completed transaction batch in exact reverse order (top-down: parents before children).
pub fn undo_transaction_batch(
    data_dir: &Path,
    batch_id: Option<&str>,
) -> Result<UndoReport, PlannerError> {
    let tx_dir = data_dir.join("transactions");
    let batch_path = match batch_id {
        Some(id) => tx_dir.join(format!("{id}.json")),
        None => tx_dir.join("latest.json"),
    };

    if !batch_path.exists() {
        return Err(PlannerError::BatchNotFound(
            batch_id.unwrap_or("latest").to_string(),
        ));
    }

    let text = std::fs::read_to_string(&batch_path)?;
    let mut batch: TransactionBatch =
        serde_json::from_str(&text).map_err(CacheError::Json)?;

    if batch.reverted {
        return Err(PlannerError::AlreadyReverted(batch.batch_id));
    }

    let mut ledger = ProcessedLedger::load(data_dir)?;
    let mut dir_renames: Vec<(PathBuf, PathBuf)> = Vec::new();
    for op in &batch.operations {
        if op.is_dir {
            dir_renames.push((op.old_path.clone(), op.new_path.clone()));
        }
    }

    let mut restored_items = Vec::new();
    let mut errors = Vec::new();

    // Reverse order restores root folders first, then subfolders, then episode files
    for op in batch.operations.iter().rev() {
        let is_case_only = op.old_name.eq_ignore_ascii_case(&op.new_name);
        if !op.new_path.exists() {
            errors.push(format!("Missing target to revert: {}", op.new_path.display()));
            continue;
        }
        if op.old_path.exists() && !is_case_only {
            errors.push(format!(
                "Cannot revert '{}' -> '{}': original path already exists",
                op.new_name, op.old_name
            ));
            continue;
        }

        match std::fs::rename(&op.new_path, &op.old_path) {
            Ok(()) => {
                let remapped_parent = remap_parent_path(&op.parent_dir, &dir_renames);
                ledger.remove(&remapped_parent, &op.new_name, batch.mode, batch.clean_tags);
                ledger.remove(&op.parent_dir, &op.new_name, batch.mode, batch.clean_tags);
                restored_items.push((op.new_name.clone(), op.old_name.clone()));
            }
            Err(e) => {
                errors.push(format!("Failed revert '{}' -> '{}': {e}", op.new_name, op.old_name));
            }
        }
    }

    batch.reverted = errors.is_empty();
    let specific_batch_path = tx_dir.join(format!("{}.json", batch.batch_id));
    atomic_write_json(&specific_batch_path, &batch)?;
    let latest_path = tx_dir.join("latest.json");
    if latest_path.exists() {
        if let Ok(latest_txt) = std::fs::read_to_string(&latest_path) {
            if let Ok(latest) = serde_json::from_str::<TransactionBatch>(&latest_txt) {
                if latest.batch_id == batch.batch_id {
                    atomic_write_json(&latest_path, &batch)?;
                }
            }
        }
    }
    ledger.save(data_dir)?;

    Ok(UndoReport {
        batch_id: batch.batch_id,
        restored_count: restored_items.len(),
        error_count: errors.len(),
        restored_items,
        errors,
    })
}
