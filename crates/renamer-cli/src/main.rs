use clap::{Args, Parser, Subcommand, ValueEnum};
use renamer_core::{
    build_rename_plan, execute_rename_plan, format_item, list_transaction_batches,
    parse_item_name, resolve_data_dir, undo_transaction_batch, CleanupRules, FormatOptions,
    ManualOverride, ManualOverrides, NamingMode, PlanConfig, PlanItemStatus, ResolutionSource,
};
use std::path::{Path, PathBuf};

#[derive(Debug, Parser)]
#[command(
    name = "renamer-cli",
    version,
    about = "RenamerAnime v2.0 Core CLI"
)]
struct Cli {
    /// Optional path to a custom JSON rules file
    #[arg(long, global = true)]
    rules: Option<PathBuf>,

    /// Custom directory for cache, overrides, and transaction logs
    #[arg(long, global = true)]
    data_dir: Option<PathBuf>,

    #[command(subcommand)]
    command: Option<Commands>,

    #[command(flatten)]
    default_run: DefaultArgs,
}

#[derive(Debug, Args, Clone)]
struct DefaultArgs {
    /// Folder to scan (defaults to current working directory)
    #[arg(long)]
    folder: Option<PathBuf>,

    /// Naming format mode
    #[arg(long, value_enum, default_value_t = CliMode::EnglishOnly)]
    mode: CliMode,

    /// Recurse into subfolders and episode files (OFF by default)
    #[arg(short, long, default_value_t = false)]
    recursive: bool,

    /// Preview only without modifying files (default)
    #[arg(long)]
    dry_run: bool,

    /// Apply renames on disk and write an undo transaction log
    #[arg(long, conflicts_with = "dry_run")]
    apply: bool,

    /// Disable scene/release tag cleanup (only valid in prepend/append modes)
    #[arg(long)]
    no_clean_tags: bool,

    /// Skip AniList network calls and use only local cache / inline aliases
    #[arg(long)]
    offline: bool,

    /// Re-check items even if already recorded in processed_items ledger
    #[arg(long)]
    force: bool,

    /// Revert the latest (or specified) transaction batch
    #[arg(long, num_args = 0..=1, default_missing_value = "")]
    undo: Option<String>,

    /// List saved rename transaction batches
    #[arg(long)]
    history: bool,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum CliMode {
    EnglishOnly,
    Prepend,
    Append,
}

impl From<CliMode> for NamingMode {
    fn from(value: CliMode) -> Self {
        match value {
            CliMode::EnglishOnly => NamingMode::EnglishOnly,
            CliMode::Prepend => NamingMode::Prepend,
            CliMode::Append => NamingMode::Append,
        }
    }
}

#[derive(Debug, Args, Clone)]
struct RunArgs {
    /// Folder to scan (defaults to current working directory)
    folder: Option<PathBuf>,

    /// Naming format mode
    #[arg(long, value_enum, default_value_t = CliMode::EnglishOnly)]
    mode: CliMode,

    /// Recurse into subfolders and episode files (OFF by default)
    #[arg(short, long, default_value_t = false)]
    recursive: bool,

    /// Preview only without modifying files (default)
    #[arg(long)]
    dry_run: bool,

    /// Apply renames on disk and write an undo transaction log
    #[arg(long, conflicts_with = "dry_run")]
    apply: bool,

    /// Disable scene/release tag cleanup (only valid in prepend/append modes)
    #[arg(long)]
    no_clean_tags: bool,

    /// Skip AniList network calls and use only local cache / inline aliases
    #[arg(long)]
    offline: bool,

    /// Re-check items even if already recorded in processed_items ledger
    #[arg(long)]
    force: bool,

    /// Revert the latest (or specified) transaction batch
    #[arg(long, num_args = 0..=1, default_missing_value = "")]
    undo: Option<String>,

    /// List saved rename transaction batches
    #[arg(long)]
    history: bool,
}

#[derive(Debug, Subcommand)]
enum Commands {
    /// Scan a folder with AniList batch lookups, cache, and optional apply
    Run(RunArgs),
    /// Revert a previous rename transaction batch
    Undo {
        /// Optional batch ID (defaults to latest)
        batch_id: Option<String>,
    },
    /// Show saved transaction batch history
    History,
    /// Save a manual title override for a query or folder name
    Override {
        query_or_name: String,
        #[arg(long)]
        english: String,
        #[arg(long)]
        romaji: Option<String>,
        #[arg(long)]
        anilist_id: Option<u64>,
    },
    /// Parse and format a single raw folder or file name
    Parse {
        raw_name: String,
        #[arg(long, default_value_t = true, action = clap::ArgAction::Set)]
        is_dir: bool,
        #[arg(long, value_enum, default_value_t = CliMode::EnglishOnly)]
        mode: CliMode,
        #[arg(long)]
        mock_english: Option<String>,
        #[arg(long)]
        mock_romaji: Option<String>,
        #[arg(long)]
        no_clean_tags: bool,
        #[arg(long)]
        json: bool,
    },
    /// Run parser + formatter across all real library examples from the screenshot
    DemoLibrary {
        #[arg(long, value_enum, default_value_t = CliMode::EnglishOnly)]
        mode: CliMode,
        #[arg(long)]
        no_clean_tags: bool,
    },
    /// Offline scan preview (tag cleanup + inline aliases only)
    Scan {
        folder: PathBuf,
        #[arg(long, value_enum, default_value_t = CliMode::EnglishOnly)]
        mode: CliMode,
        #[arg(short, long, default_value_t = false)]
        recursive: bool,
        #[arg(long)]
        no_clean_tags: bool,
    },
}

fn load_rules(path: Option<&Path>) -> CleanupRules {
    match path {
        Some(p) => CleanupRules::from_path(p).unwrap_or_else(|e| {
            eprintln!("Error loading rules from {}: {e}", p.display());
            std::process::exit(1);
        }),
        None => CleanupRules::default_rules(),
    }
}

fn main() {
    let cli = Cli::parse();
    let rules = load_rules(cli.rules.as_deref());
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let data_dir = resolve_data_dir(cli.data_dir.as_deref(), Some(&cwd));

    match cli.command {
        Some(Commands::Run(args)) => run_flow(args, &rules, &data_dir, &cwd),
        Some(Commands::Undo { batch_id }) => run_undo(&data_dir, batch_id.as_deref()),
        Some(Commands::History) => run_history(&data_dir),
        Some(Commands::Override {
            query_or_name,
            english,
            romaji,
            anilist_id,
        }) => {
            let mut overrides = ManualOverrides::load(&data_dir).unwrap_or_default();
            overrides.set(
                &query_or_name,
                ManualOverride {
                    english_title: english.clone(),
                    romaji_title: romaji,
                    anilist_id,
                },
            );
            overrides.save(&data_dir).unwrap_or_else(|e| {
                eprintln!("[ERROR] Failed to save override: {e}");
                std::process::exit(1);
            });
            println!("[OK] Saved manual override: '{query_or_name}' -> '{english}'");
            println!("Data dir: {}", data_dir.display());
        }
        Some(Commands::Parse {
            raw_name,
            is_dir,
            mode,
            mock_english,
            mock_romaji,
            no_clean_tags,
            json,
        }) => {
            let parsed = parse_item_name(&raw_name, is_dir, &rules);
            let opts = FormatOptions {
                mode: mode.into(),
                clean_tags: !no_clean_tags,
                normalize_seasons: true,
            };
            let formatted = format_item(
                &parsed,
                mock_english.as_deref(),
                mock_romaji.as_deref(),
                &opts,
            );

            if json {
                let out = serde_json::json!({
                    "parsed": parsed,
                    "formatted": formatted,
                });
                println!("{}", serde_json::to_string_pretty(&out).unwrap());
            } else {
                println!("Raw Name      : {}", parsed.raw_name);
                println!("Base Title    : {}", parsed.base_title);
                println!("Search Query  : {}", parsed.search_query);
                if !parsed.fallback_queries.is_empty() {
                    println!("Fallbacks     : {}", parsed.fallback_queries.join(" | "));
                }
                if let Some(alias) = &parsed.inline_alias {
                    println!("Inline Alias  : {alias}");
                }
                if let Some(year) = &parsed.year_spec {
                    println!("Year Spec     : {year}");
                }
                if let Some(season) = &parsed.season_spec {
                    println!("Season Spec   : {season}");
                }
                if let Some(ep) = &parsed.episode_spec {
                    println!("Episode Spec  : {ep}");
                }
                if !parsed.extra_notes.is_empty() {
                    println!("Extra Notes   : {}", parsed.extra_notes.join(", "));
                }
                println!("Stripped Tags : {}", parsed.stripped_tags.join(", "));
                println!("Proposed Name : {}", formatted.proposed_name);
            }
        }
        Some(Commands::DemoLibrary { mode, no_clean_tags }) => {
            run_demo_library(mode, no_clean_tags, &rules);
        }
        Some(Commands::Scan {
            folder,
            mode,
            recursive,
            no_clean_tags,
        }) => {
            let args = RunArgs {
                folder: Some(folder),
                mode,
                recursive,
                dry_run: true,
                apply: false,
                no_clean_tags,
                offline: true,
                force: true,
                undo: None,
                history: false,
            };
            run_flow(args, &rules, &data_dir, &cwd);
        }
        None => {
            let d = cli.default_run;
            if let Some(batch_opt) = &d.undo {
                let id = if batch_opt.is_empty() {
                    None
                } else {
                    Some(batch_opt.as_str())
                };
                run_undo(&data_dir, id);
            } else if d.history {
                run_history(&data_dir);
            } else {
                let args = RunArgs {
                    folder: d.folder,
                    mode: d.mode,
                    recursive: d.recursive,
                    dry_run: d.dry_run,
                    apply: d.apply,
                    no_clean_tags: d.no_clean_tags,
                    offline: d.offline,
                    force: d.force,
                    undo: None,
                    history: false,
                };
                run_flow(args, &rules, &data_dir, &cwd);
            }
        }
    }
}

fn run_flow(args: RunArgs, rules: &CleanupRules, data_dir: &Path, cwd: &Path) {
    let target_dir = args.folder.unwrap_or_else(|| cwd.to_path_buf());
    let apply = args.apply;

    println!("[INIT] Target directory : {}", target_dir.display());
    println!("[INIT] Data directory   : {}", data_dir.display());
    println!(
        "[INIT] Mode: {:?} | Recursive: {} | Clean tags: {} | {}",
        args.mode,
        args.recursive,
        !args.no_clean_tags,
        if apply { "APPLY MODE" } else { "DRY-RUN (default)" }
    );
    println!("------------------------------------------------------------");

    let cfg = PlanConfig {
        root_dir: target_dir.clone(),
        format_opts: FormatOptions {
            mode: args.mode.into(),
            clean_tags: !args.no_clean_tags,
            normalize_seasons: true,
        },
        recursive: args.recursive,
        offline: args.offline,
        force_recheck: args.force,
        data_dir: data_dir.to_path_buf(),
    };

    let plan = match build_rename_plan(&cfg, rules, &mut |msg| {
        println!("[PROGRESS] {msg}");
    }) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("[ERROR] {e}");
            std::process::exit(1);
        }
    };

    let mut ready_count = 0usize;
    let mut skipped_count = 0usize;
    let mut conflict_count = 0usize;

    for item in &plan.items {
        let rel = item
            .current_path
            .strip_prefix(&target_dir)
            .unwrap_or(&item.current_path);
        let src_tag = match item.resolution_source {
            ResolutionSource::ApiMatch => "API",
            ResolutionSource::CacheHit => "CACHE",
            ResolutionSource::ManualOverride => "OVERRIDE",
            ResolutionSource::InlineAlias => "INLINE",
            ResolutionSource::AlreadyProcessed => "LEDGER",
            ResolutionSource::NoMatch => "NO-MATCH",
            ResolutionSource::None => "NONE",
        };

        match item.status {
            PlanItemStatus::ReadyToRename => {
                ready_count += 1;
                let tag = if apply { "RENAME" } else { "PREVIEW" };
                println!(
                    "[{tag}] [{src_tag}] {}  -->  {}",
                    rel.display(),
                    item.proposed_name
                );
            }
            PlanItemStatus::AlreadyProcessed => {
                skipped_count += 1;
                println!("[ALREADY PROCESSED] {}", rel.display());
            }
            PlanItemStatus::AlreadyClean => {
                skipped_count += 1;
                println!("[ALREADY CLEAN] [{src_tag}] {}", rel.display());
            }
            PlanItemStatus::SkippedEmptyQuery => {
                skipped_count += 1;
                println!("[SKIP] {} (empty query)", rel.display());
            }
            PlanItemStatus::Conflict => {
                conflict_count += 1;
                println!(
                    "[CONFLICT] {} -> {} ({})",
                    rel.display(),
                    item.proposed_name,
                    item.status_reason.as_deref().unwrap_or("conflict")
                );
            }
        }
    }

    println!("------------------------------------------------------------");
    println!(
        "Plan Summary: {ready_count} to rename | {skipped_count} skipped | {conflict_count} conflicts | API requests: {}",
        plan.api_requests_made
    );

    if apply && ready_count > 0 {
        match execute_rename_plan(&plan, None, None) {
            Ok(report) => {
                println!(
                    "[APPLIED] Batch ID: {} | Renamed: {} | Skipped: {} | Errors: {}",
                    report.batch_id,
                    report.renamed_count,
                    report.skipped_count,
                    report.error_count
                );
                for err in &report.errors {
                    eprintln!("  [ERR] {err}");
                }
                println!("Undo anytime with: renamer-cli undo {}", report.batch_id);
            }
            Err(e) => {
                eprintln!("[ERROR] Failed executing rename batch: {e}");
                std::process::exit(1);
            }
        }
    } else if !apply && ready_count > 0 {
        println!("Dry-run complete. Re-run with `--apply` to rename on disk.");
    }
}

fn run_undo(data_dir: &Path, batch_id: Option<&str>) {
    match undo_transaction_batch(data_dir, batch_id) {
        Ok(report) => {
            println!(
                "[UNDO OK] Batch: {} | Restored: {} | Errors: {}",
                report.batch_id, report.restored_count, report.error_count
            );
            for (new_name, old_name) in &report.restored_items {
                println!("  {new_name}  -->  {old_name}");
            }
            for err in &report.errors {
                eprintln!("  [ERR] {err}");
            }
        }
        Err(e) => {
            eprintln!("[ERROR] Undo failed: {e}");
            std::process::exit(1);
        }
    }
}

fn run_history(data_dir: &Path) {
    match list_transaction_batches(data_dir) {
        Ok(batches) => {
            if batches.is_empty() {
                println!("No transaction batches recorded in {}.", data_dir.display());
                return;
            }
            println!("=== Transaction History ({}) ===", data_dir.display());
            for b in batches {
                println!(
                    "- {} | ops: {} | mode: {:?} | reverted: {} | root: {}",
                    b.batch_id,
                    b.operations.len(),
                    b.mode,
                    b.reverted,
                    b.root_dir.display()
                );
            }
        }
        Err(e) => {
            eprintln!("[ERROR] Failed to read history: {e}");
            std::process::exit(1);
        }
    }
}

fn run_demo_library(mode: CliMode, no_clean_tags: bool, rules: &CleanupRules) {
    let samples: &[(&str, Option<&str>, Option<&str>)] = &[
        ("[Anime Time] Death Parade + Special [BD 1080p][HEVC 10bit x265][Dual Audio]", Some("Death Parade"), Some("Death Parade")),
        ("[bonkai77] Project Itoh The Empire of Corpses [BD 1080p][HEVC x265 10bit][Dual-Audio]", Some("The Empire of Corpses"), Some("Shisha no Teikoku")),
        ("[bonkai77] Shingeki no Bahamut - Virgin Soul [WEB 1080p][HEVC x265]", Some("Rage of Bahamut: Virgin Soul"), Some("Shingeki no Bahamut: Virgin Soul")),
        ("[denisplay] Youjo Senki (Season 2) [1080] [HEVC]", Some("Saga of Tanya the Evil"), Some("Youjo Senki")),
        ("[HR] Ben 10 - Ultimate Alien S01-02 COMPLETE [1080p]", None, None),
        ("[Judas] Fairy Tail (2009-2014) (Seasons 1-6) [BD 1080p][HEVC x265 10bit][Dual-Audio]", Some("Fairy Tail"), Some("Fairy Tail")),
        ("[Judas] Fairy Tail (2018) (Season 1) [BD 1080p][HEVC x265 10bit]", Some("Fairy Tail"), Some("Fairy Tail")),
        ("[Judas] Grand Blue Dreaming (Season 03) [1080p]", Some("Grand Blue Dreaming"), Some("Grand Blue")),
        ("[Judas] Kyokou Suiri (In-Spectre) (Season 1) [1080p]", Some("In-Spectre"), Some("Kyokou Suiri")),
        ("[Judas] Kyokou Suiri (In-Spectre) (Season 2) [1080p]", Some("In-Spectre"), Some("Kyokou Suiri")),
        ("[Judas] Mairimashita! Iruma-kun (Welcome to Demon School! Iruma-kun) (Seasons 1-3) [1080p]", Some("Welcome to Demon School! Iruma-kun"), Some("Mairimashita! Iruma-kun")),
        ("[Judas] Mushishi (Seasons 1-3 + Movie + OVA + Specials) [BD 1080p]", Some("Mushishi"), Some("Mushishi")),
        ("[Judas] Ookami to Koushinryou (Spice and Wolf) (Seasons 1-2) [1080p]", Some("Spice and Wolf"), Some("Ookami to Koushinryou")),
        ("[neoHEVC] Silver Spoon - Gin no Saji [Season 1+2] [BD 1080p x265 10bit]", Some("Silver Spoon"), Some("Gin no Saji")),
        ("[Tenrai-Sensei] Fate Stay Night Unlimited Blade Works [1080p]", Some("Fate/stay night [Unlimited Blade Works]"), Some("Fate/stay night: Unlimited Blade Works")),
        ("[Trix] Tenmaku no Jaadugar S01 v2 [WEB 1080p]", None, Some("Tenmaku no Jaadugar")),
    ];

    let opts = FormatOptions {
        mode: mode.into(),
        clean_tags: !no_clean_tags,
        normalize_seasons: true,
    };

    println!("=== Demo Library Preview (Mode: {mode:?}, Clean Tags: {}) ===", !no_clean_tags);
    for (raw, eng, rom) in samples {
        let parsed = parse_item_name(raw, true, rules);
        let formatted = format_item(&parsed, *eng, *rom, &opts);
        println!("\nCurrent  : {raw}");
        println!(
            "Query    : '{}'  |  Stripped: [{}]",
            parsed.search_query,
            parsed.stripped_tags.join(", ")
        );
        println!("Proposed : {}", formatted.proposed_name);
    }
}
