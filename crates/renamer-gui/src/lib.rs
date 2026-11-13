use renamer_core::{
    build_rename_plan, build_virtual_rename_plan, execute_rename_plan, get_transaction_batch,
    list_transaction_batches, mark_virtual_batch_reverted, record_virtual_batch,
    undo_transaction_batch, AniListClient, ApplyReport, CleanupRules, FormatOptions,
    ManualOverride, ManualOverrides, MediaCandidate, NamingMode, PlanConfig, PlanItemStatus,
    RenamePlan, SafRenameOutcome, TransactionBatch, UndoReport, VirtualEntry,
};
#[cfg(not(target_os = "android"))]
use renamer_core::resolve_data_dir;
use serde::Serialize;
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Emitter, Manager, State};

pub const SAF_DEMO_ROOT_URI: &str =
    "content://com.android.externalstorage.documents/tree/primary%3AAnime_SAF_Demo";

struct AppState {
    data_dir: PathBuf,
    rules_path: PathBuf,
    current_plan: Mutex<Option<RenamePlan>>,
    saf_demo_tree: Mutex<Vec<VirtualEntry>>,
    cancel_flag: Arc<AtomicBool>,
}

#[derive(Debug, Clone, Serialize)]
struct InitialStatePayload {
    data_dir: String,
    rules: CleanupRules,
    history: Vec<TransactionBatch>,
}

#[derive(Debug, Clone, Serialize)]
struct ProgressPayload {
    phase: String,
    message: String,
}

fn load_active_rules(rules_path: &PathBuf) -> CleanupRules {
    if rules_path.exists() {
        CleanupRules::from_path(rules_path).unwrap_or_else(|_| CleanupRules::default_rules())
    } else {
        CleanupRules::default_rules()
    }
}

fn emit_progress(app: &AppHandle, phase: &str, message: &str) {
    let _ = app.emit(
        "renamer://progress",
        ProgressPayload {
            phase: phase.to_string(),
            message: message.to_string(),
        },
    );
}

#[tauri::command]
fn get_initial_state(state: State<'_, AppState>) -> Result<InitialStatePayload, String> {
    let rules = load_active_rules(&state.rules_path);
    let history = list_transaction_batches(&state.data_dir).unwrap_or_default();
    Ok(InitialStatePayload {
        data_dir: state.data_dir.to_string_lossy().to_string(),
        rules,
        history,
    })
}

#[tauri::command]
async fn pick_folder() -> Result<Option<String>, String> {
    #[cfg(not(target_os = "android"))]
    {
        tauri::async_runtime::spawn_blocking(|| {
            let picked = rfd::FileDialog::new()
                .set_title("Select Anime Library Folder")
                .pick_folder();
            picked.map(|p| p.to_string_lossy().to_string())
        })
        .await
        .map_err(|e| e.to_string())
    }
    #[cfg(target_os = "android")]
    {
        Err("Use Android SAF Document Tree picker on mobile.".to_string())
    }
}

#[tauri::command]
fn create_demo_sandbox() -> Result<String, String> {
    let sandbox = std::env::temp_dir().join("RenamerAnime_DemoSandbox");
    if sandbox.exists() {
        std::fs::remove_dir_all(&sandbox).map_err(|e| e.to_string())?;
    }
    let folders = [
        "[Anime Time] Death Parade + Special [BD 1080p][HEVC 10bit x265][Dual Audio]",
        "[bonkai77] Project Itoh The Empire of Corpses [BD 1080p][HEVC x265 10bit][Dual-Audio]",
        "[bonkai77] Shingeki no Bahamut - Virgin Soul [WEB 1080p][HEVC x265]",
        "[denisplay] Youjo Senki (Season 2) [1080] [HEVC]",
        "[HR] Ben 10 - Ultimate Alien S01-02 COMPLETE [1080p]",
        "[Judas] Fairy Tail (2009-2014) (Seasons 1-6) [BD 1080p][HEVC x265 10bit][Dual-Audio]",
        "[Judas] Fairy Tail (2018) (Season 1) [BD 1080p][HEVC x265 10bit]",
        "[Judas] Grand Blue Dreaming (Season 03) [1080p]",
        "[Judas] Kyokou Suiri (In-Spectre) (Season 2) [1080p]",
        "[Judas] Mairimashita! Iruma-kun (Welcome to Demon School! Iruma-kun) (Seasons 1-3) [1080p]",
        "[Judas] Mushishi (Seasons 1-3 + Movie + OVA + Specials) [BD 1080p]",
        "[Judas] Ookami to Koushinryou (Spice and Wolf) (Seasons 1-2) [1080p]",
        "[neoHEVC] Silver Spoon - Gin no Saji [Season 1+2] [BD 1080p x265 10bit]",
        "[Tenrai-Sensei] Fate Stay Night Unlimited Blade Works [1080p]",
        "[Trix] Tenmaku no Jaadugar S01 v2 [WEB 1080p]",
        "[Prejudice-Studio] 记忆管理局 False Memory [01-07][Bilibili WEB-DL 1080p]",
    ];

    for f in folders {
        std::fs::create_dir_all(sandbox.join(f)).map_err(|e| e.to_string())?;
    }

    let ep_dir = sandbox
        .join("[Judas] Kyokou Suiri (In-Spectre) (Season 2) [1080p]")
        .join("Season 2");
    std::fs::create_dir_all(&ep_dir).map_err(|e| e.to_string())?;
    std::fs::write(
        ep_dir.join("[Judas] Kyokou Suiri - S02E01v2 [1080p][DEADBEEF].mkv"),
        b"demo",
    )
    .map_err(|e| e.to_string())?;
    std::fs::write(ep_dir.join("02.mkv"), b"demo").map_err(|e| e.to_string())?;

    Ok(sandbox.to_string_lossy().to_string())
}

fn saf_child_uri(parent_uri: &str, name: &str) -> String {
    let encoded = name.replace(' ', "%20");
    if parent_uri == SAF_DEMO_ROOT_URI {
        format!("{SAF_DEMO_ROOT_URI}/document/primary%3AAnime_SAF_Demo%2F{encoded}")
    } else {
        format!("{parent_uri}%2F{encoded}")
    }
}

#[tauri::command]
fn create_saf_demo_tree(state: State<'_, AppState>) -> Result<String, String> {
    let series_names = [
        "[Anime Time] Death Parade + Special [BD 1080p][HEVC 10bit x265][Dual Audio]",
        "[denisplay] Youjo Senki (Season 2) [1080] [HEVC]",
        "[Judas] Kyokou Suiri (In-Spectre) (Season 2) [1080p]",
        "[neoHEVC] Silver Spoon - Gin no Saji [Season 1+2] [BD 1080p x265 10bit]",
        "[Trix] Tenmaku no Jaadugar S01 v2 [WEB 1080p]",
    ];

    let mut entries = Vec::new();
    for name in series_names {
        entries.push(VirtualEntry {
            uri: saf_child_uri(SAF_DEMO_ROOT_URI, name),
            parent_uri: SAF_DEMO_ROOT_URI.to_string(),
            name: name.to_string(),
            is_dir: true,
            depth: 0,
        });
    }

    let kyokou_uri = saf_child_uri(
        SAF_DEMO_ROOT_URI,
        "[Judas] Kyokou Suiri (In-Spectre) (Season 2) [1080p]",
    );
    let season_uri = saf_child_uri(&kyokou_uri, "Season 2");
    entries.push(VirtualEntry {
        uri: season_uri.clone(),
        parent_uri: kyokou_uri,
        name: "Season 2".to_string(),
        is_dir: true,
        depth: 1,
    });
    entries.push(VirtualEntry {
        uri: saf_child_uri(&season_uri, "[Judas] Kyokou Suiri - S02E01v2 [1080p][DEADBEEF].mkv"),
        parent_uri: season_uri.clone(),
        name: "[Judas] Kyokou Suiri - S02E01v2 [1080p][DEADBEEF].mkv".to_string(),
        is_dir: false,
        depth: 2,
    });
    entries.push(VirtualEntry {
        uri: saf_child_uri(&season_uri, "02.mkv"),
        parent_uri: season_uri,
        name: "02.mkv".to_string(),
        is_dir: false,
        depth: 2,
    });

    let mut guard = state.saf_demo_tree.lock().map_err(|e| e.to_string())?;
    *guard = entries;
    Ok(SAF_DEMO_ROOT_URI.to_string())
}

#[tauri::command]
async fn scan_directory(
    app: AppHandle,
    state: State<'_, AppState>,
    folder: String,
    mode: NamingMode,
    clean_tags: bool,
    recursive: bool,
    offline: bool,
    force_recheck: bool,
) -> Result<RenamePlan, String> {
    state.cancel_flag.store(false, Ordering::Relaxed);
    let data_dir = state.data_dir.clone();
    let rules = load_active_rules(&state.rules_path);
    let config = PlanConfig {
        root_dir: PathBuf::from(folder),
        format_opts: FormatOptions {
            mode,
            clean_tags,
            normalize_seasons: true,
        },
        recursive,
        offline,
        force_recheck,
        data_dir,
    };

    let app_clone = app.clone();
    let plan = tauri::async_runtime::spawn_blocking(move || {
        build_rename_plan(&config, &rules, &mut |msg| {
            emit_progress(&app_clone, "scan", msg);
        })
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| e.to_string())?;

    let mut guard = state.current_plan.lock().map_err(|e| e.to_string())?;
    *guard = Some(plan.clone());
    Ok(plan)
}

#[tauri::command]
async fn scan_saf_tree(
    app: AppHandle,
    state: State<'_, AppState>,
    root_uri: String,
    entries: Vec<VirtualEntry>,
    mode: NamingMode,
    clean_tags: bool,
    recursive: bool,
    offline: bool,
    force_recheck: bool,
) -> Result<RenamePlan, String> {
    state.cancel_flag.store(false, Ordering::Relaxed);
    let active_entries = if entries.is_empty() && root_uri == SAF_DEMO_ROOT_URI {
        let guard = state.saf_demo_tree.lock().map_err(|e| e.to_string())?;
        if guard.is_empty() {
            return Err("SAF demo tree is empty. Click SAF (content://) Demo first.".to_string());
        }
        guard.clone()
    } else {
        entries
    };

    let data_dir = state.data_dir.clone();
    let rules = load_active_rules(&state.rules_path);
    let config = PlanConfig {
        root_dir: PathBuf::from(&root_uri),
        format_opts: FormatOptions {
            mode,
            clean_tags,
            normalize_seasons: true,
        },
        recursive,
        offline,
        force_recheck,
        data_dir,
    };

    let app_clone = app.clone();
    let plan = tauri::async_runtime::spawn_blocking(move || {
        build_virtual_rename_plan(&root_uri, &active_entries, &config, &rules, &mut |msg| {
            emit_progress(&app_clone, "scan", msg);
        })
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| e.to_string())?;

    let mut guard = state.current_plan.lock().map_err(|e| e.to_string())?;
    *guard = Some(plan.clone());
    Ok(plan)
}

#[tauri::command]
async fn apply_selected(
    app: AppHandle,
    state: State<'_, AppState>,
    selected_indices: Vec<usize>,
) -> Result<ApplyReport, String> {
    let plan_opt = {
        let guard = state.current_plan.lock().map_err(|e| e.to_string())?;
        guard.clone()
    };
    let Some(plan) = plan_opt else {
        return Err("No active scan plan loaded. Run Scan first.".to_string());
    };

    state.cancel_flag.store(false, Ordering::Relaxed);
    let cancel_flag = Arc::clone(&state.cancel_flag);
    emit_progress(&app, "apply", "Applying selected renames (bottom-up)...");

    let report = tauri::async_runtime::spawn_blocking(move || {
        execute_rename_plan(&plan, Some(&selected_indices), Some(&cancel_flag))
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| e.to_string())?;

    Ok(report)
}

#[tauri::command]
async fn apply_saf_demo_selected(
    app: AppHandle,
    state: State<'_, AppState>,
    selected_indices: Vec<usize>,
) -> Result<ApplyReport, String> {
    let plan = {
        let guard = state.current_plan.lock().map_err(|e| e.to_string())?;
        guard
            .clone()
            .ok_or_else(|| "No active SAF scan plan loaded.".to_string())?
    };

    emit_progress(&app, "apply", "Applying SAF content:// renames (bottom-up)...");

    let selected: HashSet<usize> = selected_indices.into_iter().collect();
    let mut targets: Vec<_> = plan
        .items
        .iter()
        .filter(|i| i.status == PlanItemStatus::ReadyToRename && selected.contains(&i.index))
        .cloned()
        .collect();
    targets.sort_by(|a, b| b.depth.cmp(&a.depth).then_with(|| a.index.cmp(&b.index)));

    let mut outcomes = Vec::with_capacity(targets.len());
    {
        let mut tree = state.saf_demo_tree.lock().map_err(|e| e.to_string())?;
        for item in &targets {
            let old_uri = item.current_path.to_string_lossy().to_string();
            let parent_uri = item.parent_dir.to_string_lossy().to_string();
            let new_uri = saf_child_uri(&parent_uri, &item.proposed_name);

            for node in tree.iter_mut() {
                if node.uri == old_uri {
                    node.name = item.proposed_name.clone();
                    node.uri = new_uri.clone();
                } else if item.is_dir && node.parent_uri.starts_with(&old_uri) {
                    node.parent_uri = node.parent_uri.replacen(&old_uri, &new_uri, 1);
                    node.uri = node.uri.replacen(&old_uri, &new_uri, 1);
                }
            }

            outcomes.push(SafRenameOutcome {
                old_uri,
                new_uri,
                parent_uri,
                old_name: item.current_name.clone(),
                new_name: item.proposed_name.clone(),
                is_dir: item.is_dir,
                anilist_id: item.anilist_id,
                resolution_source: item.resolution_source,
            });
        }
    }

    record_virtual_batch(&plan, &outcomes, Vec::new(), 0, false).map_err(|e| e.to_string())
}

#[tauri::command]
fn record_saf_apply(
    state: State<'_, AppState>,
    outcomes: Vec<SafRenameOutcome>,
    errors: Vec<String>,
    skipped_count: usize,
    cancelled: bool,
) -> Result<ApplyReport, String> {
    let plan = {
        let guard = state.current_plan.lock().map_err(|e| e.to_string())?;
        guard
            .clone()
            .ok_or_else(|| "No active SAF scan plan loaded.".to_string())?
    };

    record_virtual_batch(&plan, &outcomes, errors, skipped_count, cancelled)
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn get_batch_for_undo(
    state: State<'_, AppState>,
    batch_id: Option<String>,
) -> Result<TransactionBatch, String> {
    get_transaction_batch(&state.data_dir, batch_id.as_deref()).map_err(|e| e.to_string())
}

#[tauri::command]
fn complete_saf_undo(
    state: State<'_, AppState>,
    batch_id: Option<String>,
    restored_items: Vec<(String, String)>,
    errors: Vec<String>,
) -> Result<UndoReport, String> {
    let batch = get_transaction_batch(&state.data_dir, batch_id.as_deref())
        .map_err(|e| e.to_string())?;

    let mut final_restored = restored_items;
    if batch.root_dir.to_string_lossy() == SAF_DEMO_ROOT_URI {
        let mut tree = state.saf_demo_tree.lock().map_err(|e| e.to_string())?;
        final_restored.clear();
        for op in batch.operations.iter().rev() {
            let new_uri = op.new_path.to_string_lossy().to_string();
            let old_uri = op.old_path.to_string_lossy().to_string();
            for node in tree.iter_mut() {
                if node.uri == new_uri {
                    node.name = op.old_name.clone();
                    node.uri = old_uri.clone();
                } else if op.is_dir && node.parent_uri.starts_with(&new_uri) {
                    node.parent_uri = node.parent_uri.replacen(&new_uri, &old_uri, 1);
                    node.uri = node.uri.replacen(&new_uri, &old_uri, 1);
                }
            }
            final_restored.push((op.new_name.clone(), op.old_name.clone()));
        }
    }

    mark_virtual_batch_reverted(
        &state.data_dir,
        Some(&batch.batch_id),
        final_restored,
        errors,
    )
    .map_err(|e| e.to_string())
}

#[tauri::command]
fn cancel_active_job(state: State<'_, AppState>) {
    state.cancel_flag.store(true, Ordering::Relaxed);
}

#[tauri::command]
async fn undo_batch(
    state: State<'_, AppState>,
    batch_id: Option<String>,
) -> Result<UndoReport, String> {
    let data_dir = state.data_dir.clone();
    tauri::async_runtime::spawn_blocking(move || {
        undo_transaction_batch(&data_dir, batch_id.as_deref())
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| e.to_string())
}

#[tauri::command]
fn get_transaction_history(state: State<'_, AppState>) -> Result<Vec<TransactionBatch>, String> {
    list_transaction_batches(&state.data_dir).map_err(|e| e.to_string())
}

#[tauri::command]
async fn search_anilist_manual(
    app: AppHandle,
    query: String,
) -> Result<Vec<MediaCandidate>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let mut client = AniListClient::new();
        client.search_candidates(&query, &mut |msg| {
            emit_progress(&app, "search", msg);
        })
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| e.to_string())
}

#[tauri::command]
fn save_manual_override(
    state: State<'_, AppState>,
    query_or_name: String,
    english_title: String,
    romaji_title: Option<String>,
    anilist_id: Option<u64>,
) -> Result<(), String> {
    let mut overrides = ManualOverrides::load(&state.data_dir).map_err(|e| e.to_string())?;
    overrides.set(
        &query_or_name,
        ManualOverride {
            english_title,
            romaji_title,
            anilist_id,
        },
    );
    overrides.save(&state.data_dir).map_err(|e| e.to_string())
}

#[tauri::command]
fn save_cleanup_rules(state: State<'_, AppState>, rules: CleanupRules) -> Result<(), String> {
    rules.save_to_path(&state.rules_path).map_err(|e| e.to_string())
}

#[tauri::command]
fn reset_cleanup_rules(state: State<'_, AppState>) -> Result<CleanupRules, String> {
    let defaults = CleanupRules::default_rules();
    defaults
        .save_to_path(&state.rules_path)
        .map_err(|e| e.to_string())?;
    Ok(defaults)
}

#[tauri::command]
fn open_data_dir(state: State<'_, AppState>) -> Result<(), String> {
    std::fs::create_dir_all(&state.data_dir).map_err(|e| e.to_string())?;
    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("explorer")
            .arg(&state.data_dir)
            .spawn()
            .map_err(|e| e.to_string())?;
    }
    #[cfg(target_os = "linux")]
    {
        std::process::Command::new("xdg-open")
            .arg(&state.data_dir)
            .spawn()
            .map_err(|e| e.to_string())?;
    }
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg(&state.data_dir)
            .spawn()
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // Prevent WebKitGTK DMABUF rendering glitches on Linux Wayland/KDE Plasma
    #[cfg(target_os = "linux")]
    if std::env::var_os("WEBKIT_DISABLE_DMABUF_RENDERER").is_none() {
        unsafe {
            std::env::set_var("WEBKIT_DISABLE_DMABUF_RENDERER", "1");
        }
    }

    tauri::Builder::default()
        .setup(|app| {
            #[cfg(target_os = "android")]
            let data_dir = app
                .path()
                .app_local_data_dir()
                .unwrap_or_else(|_| PathBuf::from("/data/local/tmp/renamer-anime"));

            #[cfg(not(target_os = "android"))]
            let data_dir = {
                let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
                resolve_data_dir(None, Some(&cwd))
            };

            let rules_path = data_dir.join("rules.json");
            app.manage(AppState {
                data_dir,
                rules_path,
                current_plan: Mutex::new(None),
                saf_demo_tree: Mutex::new(Vec::new()),
                cancel_flag: Arc::new(AtomicBool::new(false)),
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_initial_state,
            pick_folder,
            create_demo_sandbox,
            create_saf_demo_tree,
            scan_directory,
            scan_saf_tree,
            apply_selected,
            apply_saf_demo_selected,
            record_saf_apply,
            get_batch_for_undo,
            complete_saf_undo,
            cancel_active_job,
            undo_batch,
            get_transaction_history,
            search_anilist_manual,
            save_manual_override,
            save_cleanup_rules,
            reset_cleanup_rules,
            open_data_dir,
        ])
        .run(tauri::generate_context!())
        .expect("error while running RenamerAnime GUI");
}
