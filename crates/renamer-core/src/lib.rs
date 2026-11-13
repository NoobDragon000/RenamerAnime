pub mod api;
pub mod cache;
pub mod formatter;
pub mod parser;
pub mod planner;
pub mod rules;
pub mod sanitizer;

pub use api::{
    build_batch_payload, parse_batch_response, resolve_queries_with_cache, select_best_candidate,
    AniListClient, ApiError, RateLimitState,
};
pub use cache::{
    now_unix_secs, resolve_data_dir, CacheEntry, CacheError, CacheStatus, ManualOverride,
    ManualOverrides, MediaCandidate, ProcessedLedger, ProcessedOutcome, TitleCache,
    NEGATIVE_CACHE_TTL_SECS, POSITIVE_CACHE_TTL_SECS,
};
pub use formatter::{format_item, FormatOptions, FormattedResult, NamingMode};
pub use parser::{parse_item_name, split_media_extension, ParsedItem};
pub use planner::{
    build_rename_plan, build_virtual_rename_plan, execute_rename_plan, get_transaction_batch,
    list_transaction_batches, mark_virtual_batch_reverted, record_virtual_batch,
    undo_transaction_batch, ApplyReport, PlanConfig, PlanItem, PlanItemStatus, PlannerError,
    RenamePlan, ResolutionSource, SafRenameOutcome, TransactionBatch, TransactionOp, UndoReport,
    VirtualEntry,
};
pub use rules::{CleanupRules, RulesError};
pub use sanitizer::sanitize_filename_component;

#[cfg(test)]
mod tests {
    use super::*;

    fn rules() -> CleanupRules {
        CleanupRules::default_rules()
    }

    #[test]
    fn screenshot_death_parade() {
        let r = rules();
        let parsed = parse_item_name(
            "[Anime Time] Death Parade + Special [BD 1080p][HEVC 10bit x265][Dual Audio]",
            true,
            &r,
        );
        assert_eq!(parsed.base_title, "Death Parade");
        assert_eq!(parsed.search_query, "Death Parade");
        assert_eq!(parsed.extra_notes, vec!["+ Special"]);

        let eng_only = format_item(&parsed, Some("Death Parade"), Some("Death Parade"), &FormatOptions::default());
        assert_eq!(eng_only.proposed_name, "Death Parade + Special");

        let prepend = format_item(
            &parsed,
            Some("Death Parade"),
            Some("Death Parade"),
            &FormatOptions {
                mode: NamingMode::Prepend,
                ..Default::default()
            },
        );
        assert_eq!(prepend.proposed_name, "Death Parade + Special");
    }

    #[test]
    fn screenshot_project_itoh_and_shingeki() {
        let r = rules();
        let itoh = parse_item_name(
            "[bonkai77] Project Itoh The Empire of Corpses [BD 1080p][HEVC x265 10bit][Dual-Audio]",
            true,
            &r,
        );
        assert_eq!(itoh.base_title, "Project Itoh The Empire of Corpses");
        let res_itoh = format_item(&itoh, Some("The Empire of Corpses"), None, &FormatOptions::default());
        assert_eq!(res_itoh.proposed_name, "The Empire of Corpses");

        let bahamut = parse_item_name(
            "[bonkai77] Shingeki no Bahamut - Virgin Soul [WEB 1080p][HEVC x265]",
            true,
            &r,
        );
        assert_eq!(bahamut.base_title, "Shingeki no Bahamut - Virgin Soul");
        let res_bah = format_item(
            &bahamut,
            Some("Rage of Bahamut: Virgin Soul"),
            Some("Shingeki no Bahamut: Virgin Soul"),
            &FormatOptions::default(),
        );
        assert_eq!(res_bah.proposed_name, "Rage of Bahamut - Virgin Soul");
    }

    #[test]
    fn screenshot_youjo_senki_bare_1080() {
        let r = rules();
        let parsed = parse_item_name("[denisplay] Youjo Senki (Season 2) [1080] [HEVC]", true, &r);
        assert_eq!(parsed.base_title, "Youjo Senki");
        assert_eq!(parsed.season_spec.as_deref(), Some("(Season 2)"));

        let eng = format_item(
            &parsed,
            Some("Saga of Tanya the Evil"),
            Some("Youjo Senki"),
            &FormatOptions::default(),
        );
        assert_eq!(eng.proposed_name, "Saga of Tanya the Evil (Season 2)");

        let prep = format_item(
            &parsed,
            Some("Saga of Tanya the Evil"),
            Some("Youjo Senki"),
            &FormatOptions {
                mode: NamingMode::Prepend,
                ..Default::default()
            },
        );
        assert_eq!(prep.proposed_name, "(Saga of Tanya the Evil) Youjo Senki (Season 2)");
    }

    #[test]
    fn screenshot_ben_10_no_match_still_cleans_tags_and_normalizes_season() {
        let r = rules();
        let parsed = parse_item_name("[HR] Ben 10 - Ultimate Alien S01-02 COMPLETE [1080p]", true, &r);
        assert_eq!(parsed.base_title, "Ben 10 - Ultimate Alien");
        assert_eq!(parsed.season_spec.as_deref(), Some("(Seasons 1-2)"));
        assert!(parsed.stripped_tags.iter().any(|t| t == "[HR]"));
        assert!(parsed.stripped_tags.iter().any(|t| t == "COMPLETE"));

        let res = format_item(&parsed, None, None, &FormatOptions::default());
        assert_eq!(res.proposed_name, "Ben 10 - Ultimate Alien (Seasons 1-2)");
    }

    #[test]
    fn screenshot_fairy_tail_and_grand_blue_normalization() {
        let r = rules();
        let ft1 = parse_item_name(
            "[Judas] Fairy Tail (2009-2014) (Seasons 1-6) [BD 1080p][HEVC x265 10bit][Dual-Audio]",
            true,
            &r,
        );
        assert_eq!(ft1.base_title, "Fairy Tail");
        assert_eq!(ft1.year_spec.as_deref(), Some("(2009-2014)"));
        assert_eq!(ft1.season_spec.as_deref(), Some("(Seasons 1-6)"));
        let res_ft1 = format_item(&ft1, Some("Fairy Tail"), Some("Fairy Tail"), &FormatOptions::default());
        assert_eq!(res_ft1.proposed_name, "Fairy Tail (2009-2014) (Seasons 1-6)");

        let ft2 = parse_item_name(
            "[Judas] Fairy Tail (2018) (Season 1) [BD 1080p][HEVC x265 10bit]",
            true,
            &r,
        );
        let res_ft2 = format_item(&ft2, Some("Fairy Tail"), None, &FormatOptions::default());
        assert_eq!(res_ft2.proposed_name, "Fairy Tail (2018) (Season 1)");

        let gb = parse_item_name("[Judas] Grand Blue Dreaming (Season 03) [1080p]", true, &r);
        assert_eq!(gb.base_title, "Grand Blue Dreaming");
        assert_eq!(gb.season_spec.as_deref(), Some("(Season 3)"));
        let res_gb = format_item(&gb, Some("Grand Blue Dreaming"), None, &FormatOptions::default());
        assert_eq!(res_gb.proposed_name, "Grand Blue Dreaming (Season 3)");
    }

    #[test]
    fn screenshot_judas_bilingual_inline_aliases() {
        let r = rules();
        let kyokou = parse_item_name(
            "[Judas] Kyokou Suiri (In-Spectre) (Season 2) [1080p]",
            true,
            &r,
        );
        assert_eq!(kyokou.base_title, "Kyokou Suiri");
        assert_eq!(kyokou.inline_alias.as_deref(), Some("In-Spectre"));
        assert_eq!(kyokou.season_spec.as_deref(), Some("(Season 2)"));

        let res_eng = format_item(&kyokou, Some("In-Spectre"), Some("Kyokou Suiri"), &FormatOptions::default());
        assert_eq!(res_eng.proposed_name, "In-Spectre (Season 2)");

        let res_prep = format_item(
            &kyokou,
            Some("In-Spectre"),
            Some("Kyokou Suiri"),
            &FormatOptions {
                mode: NamingMode::Prepend,
                ..Default::default()
            },
        );
        assert_eq!(res_prep.proposed_name, "(In-Spectre) Kyokou Suiri (Season 2)");

        let res_app = format_item(
            &kyokou,
            Some("In-Spectre"),
            Some("Kyokou Suiri"),
            &FormatOptions {
                mode: NamingMode::Append,
                ..Default::default()
            },
        );
        assert_eq!(res_app.proposed_name, "Kyokou Suiri (In-Spectre) (Season 2)");

        let iruma = parse_item_name(
            "[Judas] Mairimashita! Iruma-kun (Welcome to Demon School! Iruma-kun) (Seasons 1-3) [1080p]",
            true,
            &r,
        );
        assert_eq!(iruma.base_title, "Mairimashita! Iruma-kun");
        assert_eq!(
            iruma.inline_alias.as_deref(),
            Some("Welcome to Demon School! Iruma-kun")
        );
        assert_eq!(iruma.season_spec.as_deref(), Some("(Seasons 1-3)"));

        let spice = parse_item_name(
            "[Judas] Ookami to Koushinryou (Spice and Wolf) (Seasons 1-2) [1080p]",
            true,
            &r,
        );
        assert_eq!(spice.base_title, "Ookami to Koushinryou");
        assert_eq!(spice.inline_alias.as_deref(), Some("Spice and Wolf"));
    }

    #[test]
    fn screenshot_mushishi_compound_and_silver_spoon_and_fate_and_tenmaku() {
        let r = rules();
        let mushishi = parse_item_name(
            "[Judas] Mushishi (Seasons 1-3 + Movie + OVA + Specials) [BD 1080p]",
            true,
            &r,
        );
        assert_eq!(mushishi.base_title, "Mushishi");
        assert_eq!(
            mushishi.season_spec.as_deref(),
            Some("(Seasons 1-3 + Movie + OVA + Specials)")
        );
        let res_mushishi = format_item(&mushishi, Some("Mushishi"), None, &FormatOptions::default());
        assert_eq!(
            res_mushishi.proposed_name,
            "Mushishi (Seasons 1-3 + Movie + OVA + Specials)"
        );

        let silver = parse_item_name(
            "[neoHEVC] Silver Spoon - Gin no Saji [Season 1+2] [BD 1080p x265 10bit]",
            true,
            &r,
        );
        assert_eq!(silver.base_title, "Silver Spoon - Gin no Saji");
        assert_eq!(silver.season_spec.as_deref(), Some("(Seasons 1-2)"));
        assert!(silver.fallback_queries.contains(&"Gin no Saji".to_string()));
        let res_silver_eng = format_item(
            &silver,
            Some("Silver Spoon"),
            Some("Gin no Saji"),
            &FormatOptions::default(),
        );
        assert_eq!(res_silver_eng.proposed_name, "Silver Spoon (Seasons 1-2)");
        let res_silver_prep = format_item(
            &silver,
            Some("Silver Spoon"),
            Some("Gin no Saji"),
            &FormatOptions {
                mode: NamingMode::Prepend,
                ..Default::default()
            },
        );
        assert_eq!(
            res_silver_prep.proposed_name,
            "(Silver Spoon) Gin no Saji (Seasons 1-2)"
        );

        let fate = parse_item_name(
            "[Tenrai-Sensei] Fate Stay Night Unlimited Blade Works [1080p]",
            true,
            &r,
        );
        assert_eq!(fate.base_title, "Fate Stay Night Unlimited Blade Works");

        let tenmaku = parse_item_name("[Trix] Tenmaku no Jaadugar S01 v2 [WEB 1080p]", true, &r);
        assert_eq!(tenmaku.base_title, "Tenmaku no Jaadugar");
        assert_eq!(tenmaku.season_spec.as_deref(), Some("(Season 1)"));
        assert!(tenmaku.stripped_tags.iter().any(|t| t == "v2"));
        let res_tenmaku = format_item(
            &tenmaku,
            None,
            Some("Tenmaku no Jaadugar"),
            &FormatOptions::default(),
        );
        assert_eq!(res_tenmaku.proposed_name, "Tenmaku no Jaadugar (Season 1)");
    }

    #[test]
    fn mixed_cjk_fallback_and_unbracketed_ember_and_episode_files() {
        let r = rules();
        let donghua = parse_item_name(
            "[Prejudice-Studio] 记忆管理局 False Memory [01-07][Bilibili WEB-DL 1080p]",
            true,
            &r,
        );
        assert_eq!(donghua.base_title, "记忆管理局 False Memory");
        assert_eq!(donghua.episode_spec.as_deref(), Some("[01-07]"));
        assert!(donghua.fallback_queries.contains(&"记忆管理局".to_string()));

        let ember = parse_item_name(
            "Hortensia Saga S01 1080p Dual Audio BDRip 10 bits DD x265-EMBER",
            true,
            &r,
        );
        assert_eq!(ember.base_title, "Hortensia Saga");
        assert_eq!(ember.season_spec.as_deref(), Some("(Season 1)"));

        let ep = parse_item_name(
            "[Judas] Kyokou Suiri (In-Spectre) - S02E05v2 [1080p][DEADBEEF].en.ass",
            false,
            &r,
        );
        assert_eq!(ep.base_title, "Kyokou Suiri");
        assert_eq!(ep.episode_spec.as_deref(), Some("S02E05"));
        assert_eq!(ep.extension, ".en.ass");
        assert!(ep.stripped_tags.iter().any(|t| t == "[DEADBEEF]"));
        assert!(ep.stripped_tags.iter().any(|t| t == "v2"));

        let ep_eng = format_item(&ep, Some("In-Spectre"), Some("Kyokou Suiri"), &FormatOptions::default());
        assert_eq!(ep_eng.proposed_name, "In-Spectre - S02E05.en.ass");

        let ep_prep = format_item(
            &ep,
            Some("In-Spectre"),
            Some("Kyokou Suiri"),
            &FormatOptions {
                mode: NamingMode::Prepend,
                ..Default::default()
            },
        );
        assert_eq!(
            ep_prep.proposed_name,
            "(In-Spectre) Kyokou Suiri - S02E05.en.ass"
        );
    }

    #[test]
    fn graphql_batch_payload_and_response_parsing() {
        let queries = vec!["Youjo Senki".to_string(), "Ben 10 - Ultimate Alien".to_string()];
        let payload = build_batch_payload(&queries);
        let q_str = payload["query"].as_str().unwrap();
        assert!(q_str.contains("q0: Page(perPage: 5)"));
        assert!(q_str.contains("q1: Page(perPage: 5)"));
        assert_eq!(payload["variables"]["q0"], "Youjo Senki");

        let fake_resp = serde_json::json!({
            "data": {
                "q0": {
                    "media": [
                        {
                            "id": 21613,
                            "title": {
                                "romaji": "Youjo Senki",
                                "english": "Saga of Tanya the Evil",
                                "native": "幼女戦記"
                            },
                            "synonyms": [],
                            "seasonYear": 2017,
                            "format": "TV"
                        }
                    ]
                },
                "q1": {
                    "media": []
                }
            }
        });

        let parsed = parse_batch_response(&queries, &fake_resp).unwrap();
        let best = select_best_candidate("Youjo Senki", &parsed["Youjo Senki"]).unwrap();
        assert_eq!(best.id, 21613);
        assert_eq!(best.english.as_deref(), Some("Saga of Tanya the Evil"));
        assert!(parsed["Ben 10 - Ultimate Alien"].is_empty());
    }

    #[test]
    fn title_cache_positive_and_negative_ttl_expiry() {
        let tmp = std::env::temp_dir().join(format!(
            "renamer_cache_test_{}",
            crate::cache::now_unix_millis()
        ));
        std::fs::create_dir_all(&tmp).unwrap();

        let mut cache = TitleCache::default();
        let now = 1_700_000_000u64;
        cache.upsert_match(
            "Youjo Senki",
            MediaCandidate {
                id: 21613,
                romaji: Some("Youjo Senki".to_string()),
                english: Some("Saga of Tanya the Evil".to_string()),
                native: None,
                synonyms: vec![],
                season_year: Some(2017),
                format: Some("TV".to_string()),
            },
            vec![],
            now,
        );
        cache.upsert_no_match("Ben 10 - Ultimate Alien", now);
        cache.save(&tmp).unwrap();

        let loaded = TitleCache::load(&tmp).unwrap();
        assert!(loaded.get_valid("youjo senki", now + 100).is_some());
        assert_eq!(
            loaded
                .get_valid("Ben 10 - Ultimate Alien", now + 100)
                .unwrap()
                .status,
            CacheStatus::NoMatch
        );
        // Negative TTL expires after 14 days while positive TTL (90 days) remains valid
        assert!(loaded
            .get_valid("Ben 10 - Ultimate Alien", now + NEGATIVE_CACHE_TTL_SECS + 1)
            .is_none());
        assert!(loaded
            .get_valid("Youjo Senki", now + NEGATIVE_CACHE_TTL_SECS + 1)
            .is_some());

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn end_to_end_bottom_up_rename_recheck_skip_and_undo() {
        let base = std::env::temp_dir().join(format!(
            "renamer_e2e_test_{}",
            crate::cache::now_unix_millis()
        ));
        let media_root = base.join("media");
        let data_dir = base.join("data");
        let series_dir = media_root.join("[Judas] Kyokou Suiri (In-Spectre) (Season 2) [1080p]");
        let season_sub = series_dir.join("Season 2");
        std::fs::create_dir_all(&season_sub).unwrap();
        std::fs::write(
            season_sub.join("[Judas] Kyokou Suiri - S02E01v2 [1080p][DEADBEEF].mkv"),
            b"ep1",
        )
        .unwrap();
        std::fs::write(season_sub.join("02.mkv"), b"ep2").unwrap();

        // Seed cache so test runs 100% offline deterministically
        let mut cache = TitleCache::default();
        cache.upsert_match(
            "Kyokou Suiri",
            MediaCandidate {
                id: 118579,
                romaji: Some("Kyokou Suiri Season 2".to_string()),
                english: Some("In-Spectre".to_string()),
                native: None,
                synonyms: vec![],
                season_year: Some(2023),
                format: Some("TV".to_string()),
            },
            vec![],
            now_unix_secs(),
        );
        cache.save(&data_dir).unwrap();

        let rules = rules();
        let cfg = PlanConfig {
            root_dir: media_root.clone(),
            format_opts: FormatOptions::default(),
            recursive: true,
            offline: true,
            force_recheck: false,
            data_dir: data_dir.clone(),
        };

        let plan = build_rename_plan(&cfg, &rules, &mut |_| {}).unwrap();
        let ready_count = plan
            .items
            .iter()
            .filter(|i| i.status == PlanItemStatus::ReadyToRename)
            .count();
        assert_eq!(ready_count, 3); // root series dir + ep1 + ep2 (Season 2 subdir stays untouched)

        let report = execute_rename_plan(&plan, None, None).unwrap();
        assert_eq!(report.renamed_count, 3);
        assert_eq!(report.error_count, 0);

        let expected_dir = media_root.join("In-Spectre (Season 2)");
        assert!(expected_dir.is_dir());
        assert!(expected_dir.join("Season 2").join("In-Spectre - S02E01.mkv").is_file());
        assert!(expected_dir.join("Season 2").join("In-Spectre - 02.mkv").is_file());

        // Second run must flag all renamed items as AlreadyProcessed
        let rerun_plan = build_rename_plan(&cfg, &rules, &mut |_| {}).unwrap();
        assert_eq!(rerun_plan.api_requests_made, 0);
        assert!(rerun_plan
            .items
            .iter()
            .filter(|i| i.status == PlanItemStatus::ReadyToRename)
            .count() == 0);
        assert!(rerun_plan
            .items
            .iter()
            .any(|i| i.status == PlanItemStatus::AlreadyProcessed));

        // Undo restores exact original folder & episode paths
        let undo = undo_transaction_batch(&data_dir, None).unwrap();
        assert_eq!(undo.restored_count, 3);
        assert_eq!(undo.error_count, 0);
        assert!(series_dir.is_dir());
        assert!(season_sub
            .join("[Judas] Kyokou Suiri - S02E01v2 [1080p][DEADBEEF].mkv")
            .is_file());
        assert!(season_sub.join("02.mkv").is_file());

        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn saf_content_uri_virtual_plan_and_ledger_roundtrip() {
        let base = std::env::temp_dir().join(format!(
            "renamer_saf_test_{}",
            crate::cache::now_unix_millis()
        ));
        let data_dir = base.join("data");

        let mut cache = TitleCache::default();
        cache.upsert_match(
            "Kyokou Suiri",
            MediaCandidate {
                id: 118579,
                romaji: Some("Kyokou Suiri Season 2".to_string()),
                english: Some("In-Spectre".to_string()),
                native: None,
                synonyms: vec![],
                season_year: Some(2023),
                format: Some("TV".to_string()),
            },
            vec![],
            now_unix_secs(),
        );
        cache.save(&data_dir).unwrap();

        let root_uri = "content://com.android.externalstorage.documents/tree/primary%3AAnime";
        let series_uri = format!("{root_uri}/document/primary%3AAnime%2FKyokouRaw");
        let ep_uri = format!("{series_uri}%2F01.mkv");

        let entries = vec![
            VirtualEntry {
                uri: series_uri.clone(),
                parent_uri: root_uri.to_string(),
                name: "[Judas] Kyokou Suiri (In-Spectre) (Season 2) [1080p]".to_string(),
                is_dir: true,
                depth: 0,
            },
            VirtualEntry {
                uri: ep_uri.clone(),
                parent_uri: series_uri.clone(),
                name: "[Judas] Kyokou Suiri - S02E01v2 [1080p][DEADBEEF].mkv".to_string(),
                is_dir: false,
                depth: 1,
            },
        ];

        let rules = rules();
        let cfg = PlanConfig {
            root_dir: std::path::PathBuf::from(root_uri),
            format_opts: FormatOptions::default(),
            recursive: true,
            offline: true,
            force_recheck: false,
            data_dir: data_dir.clone(),
        };

        let plan = build_virtual_rename_plan(root_uri, &entries, &cfg, &rules, &mut |_| {}).unwrap();
        assert_eq!(plan.items.len(), 2);
        assert_eq!(plan.items[0].proposed_name, "In-Spectre (Season 2)");
        assert_eq!(plan.items[1].proposed_name, "In-Spectre - S02E01.mkv");
        assert!(plan.items.iter().all(|i| i.status == PlanItemStatus::ReadyToRename));

        let new_series_uri = format!("{root_uri}/document/primary%3AAnime%2FIn-Spectre");
        let new_ep_uri = format!("{new_series_uri}%2FIn-Spectre%20-%20S02E01.mkv");
        let outcomes = vec![
            SafRenameOutcome {
                old_uri: ep_uri,
                new_uri: new_ep_uri.clone(),
                parent_uri: series_uri.clone(),
                old_name: "[Judas] Kyokou Suiri - S02E01v2 [1080p][DEADBEEF].mkv".to_string(),
                new_name: "In-Spectre - S02E01.mkv".to_string(),
                is_dir: false,
                anilist_id: Some(118579),
                resolution_source: ResolutionSource::CacheHit,
            },
            SafRenameOutcome {
                old_uri: series_uri.clone(),
                new_uri: new_series_uri.clone(),
                parent_uri: root_uri.to_string(),
                old_name: "[Judas] Kyokou Suiri (In-Spectre) (Season 2) [1080p]".to_string(),
                new_name: "In-Spectre (Season 2)".to_string(),
                is_dir: true,
                anilist_id: Some(118579),
                resolution_source: ResolutionSource::CacheHit,
            },
        ];

        let apply_report = record_virtual_batch(&plan, &outcomes, vec![], 0, false).unwrap();
        assert_eq!(apply_report.renamed_count, 2);

        // Re-scanning updated SAF URIs must mark both as AlreadyProcessed with 0 API calls
        let renamed_entries = vec![
            VirtualEntry {
                uri: new_series_uri.clone(),
                parent_uri: root_uri.to_string(),
                name: "In-Spectre (Season 2)".to_string(),
                is_dir: true,
                depth: 0,
            },
            VirtualEntry {
                uri: new_ep_uri,
                parent_uri: new_series_uri,
                name: "In-Spectre - S02E01.mkv".to_string(),
                is_dir: false,
                depth: 1,
            },
        ];
        let rerun = build_virtual_rename_plan(root_uri, &renamed_entries, &cfg, &rules, &mut |_| {}).unwrap();
        assert_eq!(rerun.api_requests_made, 0);
        assert!(rerun.items.iter().all(|i| i.status == PlanItemStatus::AlreadyProcessed));

        let undo_report = mark_virtual_batch_reverted(
            &data_dir,
            Some(&apply_report.batch_id),
            vec![
                (
                    "In-Spectre (Season 2)".to_string(),
                    "[Judas] Kyokou Suiri (In-Spectre) (Season 2) [1080p]".to_string(),
                ),
                (
                    "In-Spectre - S02E01.mkv".to_string(),
                    "[Judas] Kyokou Suiri - S02E01v2 [1080p][DEADBEEF].mkv".to_string(),
                ),
            ],
            vec![],
        )
        .unwrap();
        assert_eq!(undo_report.restored_count, 2);
        assert_eq!(undo_report.error_count, 0);

        let _ = std::fs::remove_dir_all(&base);
    }
}
