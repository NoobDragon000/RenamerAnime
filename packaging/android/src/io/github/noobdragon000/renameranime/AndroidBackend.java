package io.github.noobdragon000.renameranime;

import android.app.Activity;
import android.webkit.JavascriptInterface;
import android.webkit.WebView;
import java.io.BufferedReader;
import java.io.File;
import java.io.FileInputStream;
import java.io.FileOutputStream;
import java.io.InputStream;
import java.io.InputStreamReader;
import java.io.OutputStream;
import java.net.HttpURLConnection;
import java.net.URL;
import java.nio.charset.StandardCharsets;
import java.util.ArrayList;
import java.util.Collections;
import java.util.Comparator;
import java.util.HashMap;
import java.util.HashSet;
import java.util.Iterator;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Locale;
import java.util.Map;
import java.util.Set;
import java.util.concurrent.atomic.AtomicBoolean;
import java.util.regex.Matcher;
import java.util.regex.Pattern;
import org.json.JSONArray;
import org.json.JSONObject;

public class AndroidBackend {
    private static final String SAF_DEMO_ROOT_URI =
        "content://com.android.externalstorage.documents/tree/primary%3AAnime_SAF_Demo";
    private static final String ANILIST_ENDPOINT = "https://graphql.anilist.co";
    private static final long POSITIVE_TTL_SECS = 90L * 24 * 3600;
    private static final long NEGATIVE_TTL_SECS = 14L * 24 * 3600;

    private final Activity activity;
    private final WebView webView;
    private final File dataDir;
    private final File rulesFile;
    private final AtomicBoolean cancelFlag = new AtomicBoolean(false);

    private JSONObject activePlan = null;
    private final List<JSONObject> safDemoTree = new ArrayList<JSONObject>();
    private long nextRequestReadyAtMs = 0L;

    public AndroidBackend(Activity activity, WebView webView) {
        this.activity = activity;
        this.webView = webView;
        this.dataDir = new File(activity.getFilesDir(), "renamer-data");
        if (!this.dataDir.exists()) {
            this.dataDir.mkdirs();
        }
        this.rulesFile = new File(this.dataDir, "rules.json");
    }

    @JavascriptInterface
    public String invoke(String cmd, String argsJson) {
        try {
            JSONObject args = (argsJson == null || argsJson.isEmpty())
                ? new JSONObject()
                : new JSONObject(argsJson);

            Object result;
            if ("get_initial_state".equals(cmd)) {
                result = handleGetInitialState();
            } else if ("create_saf_demo_tree".equals(cmd) || "create_demo_sandbox".equals(cmd)) {
                result = handleCreateSafDemoTree();
            } else if ("scan_saf_tree".equals(cmd)) {
                result = handleScanSafTree(args);
            } else if ("apply_saf_demo_selected".equals(cmd)) {
                result = handleApplySafDemoSelected(args);
            } else if ("record_saf_apply".equals(cmd)) {
                result = handleRecordSafApply(args);
            } else if ("get_batch_for_undo".equals(cmd)) {
                result = handleGetBatchForUndo(args.optString("batchId", ""));
            } else if ("complete_saf_undo".equals(cmd)) {
                result = handleCompleteSafUndo(args);
            } else if ("get_transaction_history".equals(cmd)) {
                result = listTransactionBatches();
            } else if ("search_anilist_manual".equals(cmd)) {
                result = searchAniListManual(args.optString("query", ""));
            } else if ("save_manual_override".equals(cmd)) {
                saveManualOverride(args);
                result = JSONObject.NULL;
            } else if ("save_cleanup_rules".equals(cmd)) {
                JSONObject rules = args.optJSONObject("rules");
                writeString(rulesFile, rules != null ? rules.toString() : loadRules().toString());
                result = JSONObject.NULL;
            } else if ("reset_cleanup_rules".equals(cmd)) {
                if (rulesFile.exists()) rulesFile.delete();
                JSONObject def = loadRules();
                writeString(rulesFile, def.toString());
                result = def;
            } else if ("cancel_active_job".equals(cmd)) {
                cancelFlag.set(true);
                result = JSONObject.NULL;
            } else if ("open_data_dir".equals(cmd)) {
                emitProgress("info", "Data stored in " + dataDir.getAbsolutePath());
                result = JSONObject.NULL;
            } else {
                throw new IllegalArgumentException("Unsupported command on Android: " + cmd);
            }

            JSONObject ok = new JSONObject();
            ok.put("ok", true);
            ok.put("data", result == null ? JSONObject.NULL : result);
            return ok.toString();
        } catch (Exception e) {
            try {
                return new JSONObject()
                    .put("ok", false)
                    .put("error", e.getMessage() != null ? e.getMessage() : e.toString())
                    .toString();
            } catch (Exception ignored) {
                return "{\"ok\":false,\"error\":\"Internal Android backend error\"}";
            }
        }
    }

    private void emitProgress(final String phase, final String message) {
        try {
            final String payload = new JSONObject()
                .put("phase", phase)
                .put("message", message)
                .toString();
            webView.post(new Runnable() {
                @Override
                public void run() {
                    webView.evaluateJavascript(
                        "window.__onBackendProgress && window.__onBackendProgress(" + payload + ");",
                        null
                    );
                }
            });
        } catch (Exception ignored) {
        }
    }

    private JSONObject handleGetInitialState() throws Exception {
        JSONObject out = new JSONObject();
        out.put("data_dir", dataDir.getAbsolutePath());
        out.put("rules", loadRules());
        out.put("history", listTransactionBatches());
        return out;
    }

    private synchronized String handleCreateSafDemoTree() throws Exception {
        safDemoTree.clear();
        String[] series = new String[] {
            "[Anime Time] Death Parade + Special [BD 1080p][HEVC 10bit x265][Dual Audio]",
            "[denisplay] Youjo Senki (Season 2) [1080] [HEVC]",
            "[Judas] Kyokou Suiri (In-Spectre) (Season 2) [1080p]",
            "[neoHEVC] Silver Spoon - Gin no Saji [Season 1+2] [BD 1080p x265 10bit]",
            "[Trix] Tenmaku no Jaadugar S01 v2 [WEB 1080p]"
        };
        for (String name : series) {
            safDemoTree.add(makeVirtualNode(safChildUri(SAF_DEMO_ROOT_URI, name), SAF_DEMO_ROOT_URI, name, true, 0));
        }
        String kyokouUri = safChildUri(SAF_DEMO_ROOT_URI, "[Judas] Kyokou Suiri (In-Spectre) (Season 2) [1080p]");
        String seasonUri = safChildUri(kyokouUri, "Season 2");
        safDemoTree.add(makeVirtualNode(seasonUri, kyokouUri, "Season 2", true, 1));
        safDemoTree.add(makeVirtualNode(
            safChildUri(seasonUri, "[Judas] Kyokou Suiri - S02E01v2 [1080p][DEADBEEF].mkv"),
            seasonUri,
            "[Judas] Kyokou Suiri - S02E01v2 [1080p][DEADBEEF].mkv",
            false,
            2
        ));
        safDemoTree.add(makeVirtualNode(
            safChildUri(seasonUri, "02.mkv"),
            seasonUri,
            "02.mkv",
            false,
            2
        ));
        return SAF_DEMO_ROOT_URI;
    }

    private JSONObject makeVirtualNode(String uri, String parentUri, String name, boolean isDir, int depth) throws Exception {
        JSONObject obj = new JSONObject();
        obj.put("uri", uri);
        obj.put("parent_uri", parentUri);
        obj.put("name", name);
        obj.put("is_dir", isDir);
        obj.put("depth", depth);
        return obj;
    }

    private String safChildUri(String parentUri, String name) {
        String encoded = name.replace(" ", "%20");
        if (SAF_DEMO_ROOT_URI.equals(parentUri)) {
            return SAF_DEMO_ROOT_URI + "/document/primary%3AAnime_SAF_Demo%2F" + encoded;
        }
        return parentUri + "%2F" + encoded;
    }

    private JSONObject handleScanSafTree(JSONObject args) throws Exception {
        cancelFlag.set(false);
        String rootUri = args.optString("rootUri", "");
        String mode = args.optString("mode", "english-only");
        boolean cleanTags = args.optBoolean("cleanTags", true);
        boolean recursive = args.optBoolean("recursive", false);
        boolean offline = args.optBoolean("offline", false);
        boolean forceRecheck = args.optBoolean("forceRecheck", false);

        JSONArray rawEntries = args.optJSONArray("entries");
        List<JSONObject> entries = new ArrayList<JSONObject>();
        if ((rawEntries == null || rawEntries.length() == 0) && SAF_DEMO_ROOT_URI.equals(rootUri)) {
            synchronized (this) {
                for (JSONObject n : safDemoTree) {
                    entries.add(new JSONObject(n.toString()));
                }
            }
        } else if (rawEntries != null) {
            for (int i = 0; i < rawEntries.length(); i++) {
                JSONObject obj = rawEntries.optJSONObject(i);
                if (obj != null) entries.add(obj);
            }
        }

        JSONObject rules = loadRules();
        Set<String> ignored = jsonArrayToLowerSet(rules.optJSONArray("ignore_names"));

        List<JSONObject> filtered = new ArrayList<JSONObject>();
        for (JSONObject e : entries) {
            String name = e.optString("name", "");
            int depth = e.optInt("depth", 0);
            if (ignored.contains(name.toLowerCase(Locale.ROOT))) continue;
            if (!recursive && depth > 0) continue;
            filtered.add(e);
        }

        Collections.sort(filtered, new Comparator<JSONObject>() {
            @Override
            public int compare(JSONObject a, JSONObject b) {
                int da = a.optInt("depth", 0);
                int db = b.optInt("depth", 0);
                if (da != db) return da < db ? -1 : 1;
                int pa = a.optString("parent_uri", "").compareTo(b.optString("parent_uri", ""));
                if (pa != 0) return pa;
                return a.optString("name", "").compareTo(b.optString("name", ""));
            }
        });

        JSONObject cache = loadJsonFile(new File(dataDir, "title_cache.json"));
        JSONObject ledger = loadJsonFile(new File(dataDir, "processed_items.json"));
        JSONObject overrides = loadJsonFile(new File(dataDir, "manual_overrides.json"));
        if (!cache.has("entries")) cache.put("entries", new JSONObject());
        if (!ledger.has("records")) ledger.put("records", new JSONObject());
        if (!overrides.has("entries")) overrides.put("entries", new JSONObject());

        Map<String, String> inheritedByUri = new HashMap<String, String>();
        Map<String, Set<String>> existingByParent = new HashMap<String, Set<String>>();
        List<ParsedEntry> parsedEntries = new ArrayList<ParsedEntry>();
        Set<String> pendingQueries = new HashSet<String>();
        long now = System.currentTimeMillis() / 1000L;

        for (JSONObject e : filtered) {
            String uri = e.optString("uri", "");
            String parentUri = e.optString("parent_uri", "");
            String name = e.optString("name", "");
            boolean isDir = e.optBoolean("is_dir", false);
            int depth = e.optInt("depth", 0);

            ParsedEntry pe = parseItemName(name, isDir, rules);
            pe.uri = uri;
            pe.parentUri = parentUri;
            pe.isDir = isDir;
            pe.depth = depth;
            pe.isPureSeasonSubdir = isDir && depth > 0 && pe.baseTitle.isEmpty() && pe.seasonSpec != null;

            if (!pe.searchQuery.isEmpty()) {
                pe.effectiveQuery = pe.searchQuery;
            } else if (inheritedByUri.containsKey(parentUri)) {
                pe.effectiveQuery = inheritedByUri.get(parentUri);
            } else {
                pe.effectiveQuery = "";
            }

            if (isDir && !pe.effectiveQuery.isEmpty()) {
                inheritedByUri.put(uri, pe.effectiveQuery);
            } else if (isDir && inheritedByUri.containsKey(parentUri)) {
                inheritedByUri.put(uri, inheritedByUri.get(parentUri));
            }

            Set<String> sibs = existingByParent.get(parentUri);
            if (sibs == null) {
                sibs = new HashSet<String>();
                existingByParent.put(parentUri, sibs);
            }
            sibs.add(name.toLowerCase(Locale.ROOT));

            if (!pe.isPureSeasonSubdir) {
                boolean hasOv = overrides.optJSONObject("entries").has(normalizeKey(pe.effectiveQuery))
                    || overrides.optJSONObject("entries").has(normalizeKey(name));
                String lKey = ledgerKey(parentUri, name, mode, cleanTags);
                if (!forceRecheck && !hasOv && ledger.optJSONObject("records").has(lKey)) {
                    pe.alreadyProcessed = true;
                } else if (!pe.effectiveQuery.isEmpty() && !offline && !hasOv) {
                    String cKey = normalizeKey(pe.effectiveQuery);
                    JSONObject cEntry = cache.optJSONObject("entries").optJSONObject(cKey);
                    if (cEntry == null || cEntry.optLong("expires_at_unix", 0L) <= now) {
                        pendingQueries.add(pe.effectiveQuery);
                    }
                }
            }
            parsedEntries.add(pe);
        }

        int apiRequests = 0;
        if (!offline && !pendingQueries.isEmpty()) {
            List<String> list = new ArrayList<String>(pendingQueries);
            Collections.sort(list);
            int batchSize = 8;
            for (int i = 0; i < list.size(); i += batchSize) {
                if (cancelFlag.get()) break;
                int end = Math.min(list.size(), i + batchSize);
                List<String> chunk = list.subList(i, end);
                emitProgress("scan", "Querying AniList batch (" + chunk.size() + " titles)...");
                Map<String, List<JSONObject>> respMap = queryAniListBatch(chunk);
                apiRequests++;
                for (String q : chunk) {
                    List<JSONObject> candidates = respMap.get(q);
                    JSONObject cEntry = new JSONObject();
                    cEntry.put("query", q);
                    cEntry.put("normalized_query", normalizeKey(q));
                    cEntry.put("fetched_at_unix", now);
                    if (candidates != null && !candidates.isEmpty()) {
                        cEntry.put("status", "match");
                        cEntry.put("selected", candidates.get(0));
                        cEntry.put("candidates", new JSONArray(candidates));
                        cEntry.put("expires_at_unix", now + POSITIVE_TTL_SECS);
                    } else {
                        cEntry.put("status", "no_match");
                        cEntry.put("selected", JSONObject.NULL);
                        cEntry.put("candidates", new JSONArray());
                        cEntry.put("expires_at_unix", now + NEGATIVE_TTL_SECS);
                    }
                    cache.optJSONObject("entries").put(normalizeKey(q), cEntry);
                }
            }
            saveJsonFile(new File(dataDir, "title_cache.json"), cache);
        }

        Map<String, Set<String>> seenTargets = new HashMap<String, Set<String>>();
        JSONArray planItems = new JSONArray();
        for (int idx = 0; idx < parsedEntries.size(); idx++) {
            ParsedEntry pe = parsedEntries.get(idx);
            JSONObject item = new JSONObject();
            item.put("index", idx);
            item.put("is_dir", pe.isDir);
            item.put("depth", pe.depth);
            item.put("parent_dir", pe.parentUri);
            item.put("current_path", pe.uri);
            item.put("current_name", pe.rawName);
            item.put("effective_query", pe.effectiveQuery);
            item.put("parsed", pe.toJson());

            if (pe.isPureSeasonSubdir) {
                item.put("proposed_name", pe.rawName);
                item.put("proposed_path", pe.uri);
                item.put("anilist_id", JSONObject.NULL);
                item.put("resolved_english", JSONObject.NULL);
                item.put("resolved_romaji", JSONObject.NULL);
                item.put("candidates", new JSONArray());
                item.put("resolution_source", "none");
                item.put("status", "already_clean");
                item.put("status_reason", "Season subfolder");
                planItems.put(item);
                continue;
            }

            if (pe.alreadyProcessed) {
                item.put("proposed_name", pe.rawName);
                item.put("proposed_path", pe.uri);
                item.put("anilist_id", JSONObject.NULL);
                item.put("resolved_english", JSONObject.NULL);
                item.put("resolved_romaji", JSONObject.NULL);
                item.put("candidates", new JSONArray());
                item.put("resolution_source", "already_processed");
                item.put("status", "already_processed");
                item.put("status_reason", "Previously processed");
                planItems.put(item);
                continue;
            }

            if (pe.effectiveQuery.isEmpty()) {
                item.put("proposed_name", pe.rawName);
                item.put("proposed_path", pe.uri);
                item.put("anilist_id", JSONObject.NULL);
                item.put("resolved_english", JSONObject.NULL);
                item.put("resolved_romaji", JSONObject.NULL);
                item.put("candidates", new JSONArray());
                item.put("resolution_source", "none");
                item.put("status", "skipped_empty_query");
                item.put("status_reason", "No extractable title query");
                planItems.put(item);
                continue;
            }

            String resolvedEng = null;
            String resolvedRom = null;
            Long anilistId = null;
            JSONArray candidates = new JSONArray();
            String source = "no_match";

            JSONObject ovMap = overrides.optJSONObject("entries");
            JSONObject ov = ovMap.optJSONObject(normalizeKey(pe.rawName));
            if (ov == null) ov = ovMap.optJSONObject(normalizeKey(pe.effectiveQuery));

            if (ov != null) {
                resolvedEng = ov.optString("english_title", null);
                resolvedRom = ov.isNull("romaji_title") ? null : ov.optString("romaji_title", null);
                anilistId = ov.isNull("anilist_id") ? null : Long.valueOf(ov.optLong("anilist_id"));
                source = "manual_override";
            } else {
                JSONObject ce = cache.optJSONObject("entries").optJSONObject(normalizeKey(pe.effectiveQuery));
                if (ce != null && ce.optLong("expires_at_unix", 0L) > now) {
                    JSONArray arr = ce.optJSONArray("candidates");
                    if (arr != null) candidates = arr;
                    if ("match".equals(ce.optString("status"))) {
                        JSONObject sel = ce.optJSONObject("selected");
                        if (sel != null) {
                            anilistId = Long.valueOf(sel.optLong("id"));
                            resolvedEng = sel.isNull("english") ? null : sel.optString("english", null);
                            resolvedRom = sel.isNull("romaji") ? null : sel.optString("romaji", null);
                        }
                        source = apiRequests > 0 ? "api_match" : "cache_hit";
                    } else if (pe.inlineAlias != null) {
                        source = "inline_alias";
                    }
                } else if (pe.inlineAlias != null) {
                    source = "inline_alias";
                }
            }

            String proposedName = formatProposedName(pe, resolvedEng, resolvedRom, mode, cleanTags);
            String proposedPath = safChildUri(pe.parentUri, proposedName);

            String status = "ready_to_rename";
            String statusReason = null;
            if (proposedName.equals(pe.rawName)) {
                status = "already_clean";
                JSONObject lRec = new JSONObject();
                lRec.put("item_name", pe.rawName);
                lRec.put("mode", mode);
                lRec.put("clean_tags", cleanTags);
                lRec.put("outcome", "already_clean");
                lRec.put("anilist_id", anilistId == null ? JSONObject.NULL : anilistId);
                lRec.put("updated_at_unix", now);
                ledger.optJSONObject("records").put(ledgerKey(pe.parentUri, pe.rawName, mode, cleanTags), lRec);
            } else {
                Set<String> seen = seenTargets.get(pe.parentUri);
                if (seen == null) {
                    seen = new HashSet<String>();
                    seenTargets.put(pe.parentUri, seen);
                }
                String norm = proposedName.toLowerCase(Locale.ROOT);
                boolean caseOnly = proposedName.equalsIgnoreCase(pe.rawName);
                Set<String> existing = existingByParent.get(pe.parentUri);
                if (!seen.add(norm)) {
                    status = "conflict";
                    statusReason = "Duplicate proposed name in folder";
                } else if (existing != null && existing.contains(norm) && !caseOnly) {
                    status = "conflict";
                    statusReason = "Target already exists: " + proposedName;
                }
            }

            item.put("proposed_name", proposedName);
            item.put("proposed_path", proposedPath);
            item.put("anilist_id", anilistId == null ? JSONObject.NULL : anilistId);
            item.put("resolved_english", resolvedEng == null ? JSONObject.NULL : resolvedEng);
            item.put("resolved_romaji", resolvedRom == null ? JSONObject.NULL : resolvedRom);
            item.put("candidates", candidates);
            item.put("resolution_source", source);
            item.put("status", status);
            item.put("status_reason", statusReason == null ? JSONObject.NULL : statusReason);
            planItems.put(item);
        }

        saveJsonFile(new File(dataDir, "processed_items.json"), ledger);

        JSONObject plan = new JSONObject();
        plan.put("root_dir", rootUri);
        plan.put("data_dir", dataDir.getAbsolutePath());
        plan.put("mode", mode);
        plan.put("clean_tags", cleanTags);
        plan.put("items", planItems);
        plan.put("api_requests_made", apiRequests);
        this.activePlan = plan;
        return plan;
    }

    private synchronized JSONObject handleApplySafDemoSelected(JSONObject args) throws Exception {
        if (activePlan == null) {
            throw new IllegalStateException("No active scan plan loaded.");
        }
        Set<Integer> selected = new HashSet<Integer>();
        JSONArray idxArr = args.optJSONArray("selectedIndices");
        if (idxArr != null) {
            for (int i = 0; i < idxArr.length(); i++) {
                selected.add(Integer.valueOf(idxArr.optInt(i)));
            }
        }
        JSONArray items = activePlan.optJSONArray("items");
        List<JSONObject> targets = new ArrayList<JSONObject>();
        for (int i = 0; i < items.length(); i++) {
            JSONObject item = items.optJSONObject(i);
            if ("ready_to_rename".equals(item.optString("status"))
                && selected.contains(Integer.valueOf(item.optInt("index")))) {
                targets.add(item);
            }
        }
        Collections.sort(targets, new Comparator<JSONObject>() {
            @Override
            public int compare(JSONObject a, JSONObject b) {
                int db = b.optInt("depth", 0);
                int da = a.optInt("depth", 0);
                if (db != da) return db < da ? -1 : 1;
                return a.optInt("index", 0) - b.optInt("index", 0);
            }
        });

        JSONArray outcomes = new JSONArray();
        for (JSONObject item : targets) {
            String oldUri = item.optString("current_path");
            String parentUri = item.optString("parent_dir");
            String oldName = item.optString("current_name");
            String newName = item.optString("proposed_name");
            boolean isDir = item.optBoolean("is_dir", false);
            String newUri = safChildUri(parentUri, newName);

            for (JSONObject node : safDemoTree) {
                String nUri = node.optString("uri");
                String nParent = node.optString("parent_uri");
                if (oldUri.equals(nUri)) {
                    node.put("name", newName);
                    node.put("uri", newUri);
                } else if (isDir && nParent.startsWith(oldUri)) {
                    node.put("parent_uri", replacePrefix(nParent, oldUri, newUri));
                    node.put("uri", replacePrefix(nUri, oldUri, newUri));
                }
            }

            JSONObject o = new JSONObject();
            o.put("old_uri", oldUri);
            o.put("new_uri", newUri);
            o.put("parent_uri", parentUri);
            o.put("old_name", oldName);
            o.put("new_name", newName);
            o.put("is_dir", isDir);
            o.put("anilist_id", item.isNull("anilist_id") ? JSONObject.NULL : item.optLong("anilist_id"));
            o.put("resolution_source", item.optString("resolution_source", "none"));
            outcomes.put(o);
        }

        JSONObject recArgs = new JSONObject();
        recArgs.put("outcomes", outcomes);
        recArgs.put("errors", new JSONArray());
        recArgs.put("skippedCount", 0);
        recArgs.put("cancelled", false);
        return handleRecordSafApply(recArgs);
    }

    private JSONObject handleRecordSafApply(JSONObject args) throws Exception {
        if (activePlan == null) {
            throw new IllegalStateException("No active scan plan loaded.");
        }
        String mode = activePlan.optString("mode", "english-only");
        boolean cleanTags = activePlan.optBoolean("clean_tags", true);
        String rootDir = activePlan.optString("root_dir", "");

        long nowMs = System.currentTimeMillis();
        long nowSecs = nowMs / 1000L;
        String batchId = "batch_" + nowMs;

        JSONArray outcomes = args.optJSONArray("outcomes");
        if (outcomes == null) outcomes = new JSONArray();
        JSONArray errors = args.optJSONArray("errors");
        if (errors == null) errors = new JSONArray();

        JSONArray ops = new JSONArray();
        List<String[]> dirRenames = new ArrayList<String[]>();
        for (int i = 0; i < outcomes.length(); i++) {
            JSONObject o = outcomes.optJSONObject(i);
            boolean isDir = o.optBoolean("is_dir", false);
            String oldUri = o.optString("old_uri");
            String newUri = o.optString("new_uri");
            if (isDir) {
                dirRenames.add(new String[] { oldUri, newUri });
            }
            JSONObject op = new JSONObject();
            op.put("old_path", oldUri);
            op.put("new_path", newUri);
            op.put("parent_dir", o.optString("parent_uri"));
            op.put("old_name", o.optString("old_name"));
            op.put("new_name", o.optString("new_name"));
            op.put("is_dir", isDir);
            op.put("anilist_id", o.isNull("anilist_id") ? JSONObject.NULL : o.optLong("anilist_id"));
            op.put("outcome", "renamed");
            ops.put(op);
        }

        JSONObject batch = new JSONObject();
        batch.put("batch_id", batchId);
        batch.put("created_at_unix", nowSecs);
        batch.put("root_dir", rootDir);
        batch.put("mode", mode);
        batch.put("clean_tags", cleanTags);
        batch.put("reverted", false);
        batch.put("operations", ops);

        File txDir = new File(dataDir, "transactions");
        if (!txDir.exists()) txDir.mkdirs();
        saveJsonFile(new File(txDir, batchId + ".json"), batch);
        saveJsonFile(new File(txDir, "latest.json"), batch);

        JSONObject ledger = loadJsonFile(new File(dataDir, "processed_items.json"));
        if (!ledger.has("records")) ledger.put("records", new JSONObject());
        for (int i = 0; i < ops.length(); i++) {
            JSONObject op = ops.optJSONObject(i);
            String remappedParent = remapSafParent(op.optString("parent_dir"), dirRenames);
            JSONObject rec = new JSONObject();
            rec.put("item_name", op.optString("new_name"));
            rec.put("mode", mode);
            rec.put("clean_tags", cleanTags);
            rec.put("outcome", "renamed");
            rec.put("anilist_id", op.isNull("anilist_id") ? JSONObject.NULL : op.optLong("anilist_id"));
            rec.put("updated_at_unix", nowSecs);
            ledger.optJSONObject("records").put(ledgerKey(remappedParent, op.optString("new_name"), mode, cleanTags), rec);
        }
        saveJsonFile(new File(dataDir, "processed_items.json"), ledger);

        JSONObject report = new JSONObject();
        report.put("batch_id", batchId);
        report.put("renamed_count", outcomes.length());
        report.put("skipped_count", args.optInt("skippedCount", 0));
        report.put("error_count", errors.length());
        report.put("cancelled", args.optBoolean("cancelled", false));
        report.put("errors", errors);
        return report;
    }

    private JSONObject handleGetBatchForUndo(String batchId) throws Exception {
        File txDir = new File(dataDir, "transactions");
        File f = (batchId == null || batchId.isEmpty())
            ? new File(txDir, "latest.json")
            : new File(txDir, batchId + ".json");
        if (!f.exists()) {
            throw new IllegalStateException("Transaction batch not found");
        }
        return loadJsonFile(f);
    }

    private synchronized JSONObject handleCompleteSafUndo(JSONObject args) throws Exception {
        String reqId = args.optString("batchId", "");
        JSONObject batch = handleGetBatchForUndo(reqId);
        if (batch.optBoolean("reverted", false)) {
            throw new IllegalStateException("Batch already reverted");
        }
        String batchId = batch.optString("batch_id");
        String mode = batch.optString("mode", "english-only");
        boolean cleanTags = batch.optBoolean("clean_tags", true);
        JSONArray ops = batch.optJSONArray("operations");
        if (ops == null) ops = new JSONArray();

        JSONArray restored = args.optJSONArray("restoredItems");
        if (restored == null) restored = new JSONArray();
        JSONArray errors = args.optJSONArray("errors");
        if (errors == null) errors = new JSONArray();

        if (SAF_DEMO_ROOT_URI.equals(batch.optString("root_dir"))) {
            restored = new JSONArray();
            for (int i = ops.length() - 1; i >= 0; i--) {
                JSONObject op = ops.optJSONObject(i);
                String oldUri = op.optString("old_path");
                String newUri = op.optString("new_path");
                String oldName = op.optString("old_name");
                String newName = op.optString("new_name");
                boolean isDir = op.optBoolean("is_dir", false);
                for (JSONObject node : safDemoTree) {
                    String nUri = node.optString("uri");
                    String nParent = node.optString("parent_uri");
                    if (newUri.equals(nUri)) {
                        node.put("name", oldName);
                        node.put("uri", oldUri);
                    } else if (isDir && nParent.startsWith(newUri)) {
                        node.put("parent_uri", replacePrefix(nParent, newUri, oldUri));
                        node.put("uri", replacePrefix(nUri, newUri, oldUri));
                    }
                }
                JSONArray pair = new JSONArray();
                pair.put(newName);
                pair.put(oldName);
                restored.put(pair);
            }
        }

        List<String[]> dirRenames = new ArrayList<String[]>();
        for (int i = 0; i < ops.length(); i++) {
            JSONObject op = ops.optJSONObject(i);
            if (op.optBoolean("is_dir", false)) {
                dirRenames.add(new String[] { op.optString("old_path"), op.optString("new_path") });
            }
        }

        JSONObject ledger = loadJsonFile(new File(dataDir, "processed_items.json"));
        JSONObject recs = ledger.optJSONObject("records");
        if (recs != null) {
            for (int i = 0; i < ops.length(); i++) {
                JSONObject op = ops.optJSONObject(i);
                String p = op.optString("parent_dir");
                String n = op.optString("new_name");
                String rp = remapSafParent(p, dirRenames);
                recs.remove(ledgerKey(rp, n, mode, cleanTags));
                recs.remove(ledgerKey(p, n, mode, cleanTags));
            }
            saveJsonFile(new File(dataDir, "processed_items.json"), ledger);
        }

        batch.put("reverted", errors.length() == 0);
        File txDir = new File(dataDir, "transactions");
        saveJsonFile(new File(txDir, batchId + ".json"), batch);
        File latest = new File(txDir, "latest.json");
        if (latest.exists()) {
            JSONObject lBatch = loadJsonFile(latest);
            if (batchId.equals(lBatch.optString("batch_id"))) {
                saveJsonFile(latest, batch);
            }
        }

        JSONObject report = new JSONObject();
        report.put("batch_id", batchId);
        report.put("restored_count", restored.length());
        report.put("error_count", errors.length());
        report.put("restored_items", restored);
        report.put("errors", errors);
        return report;
    }

    private JSONArray listTransactionBatches() throws Exception {
        File txDir = new File(dataDir, "transactions");
        List<JSONObject> list = new ArrayList<JSONObject>();
        if (txDir.exists()) {
            File[] files = txDir.listFiles();
            if (files != null) {
                for (File f : files) {
                    String n = f.getName();
                    if ("latest.json".equals(n) || !n.endsWith(".json")) continue;
                    try {
                        list.add(loadJsonFile(f));
                    } catch (Exception ignored) {
                    }
                }
            }
        }
        Collections.sort(list, new Comparator<JSONObject>() {
            @Override
            public int compare(JSONObject a, JSONObject b) {
                long ta = a.optLong("created_at_unix", 0L);
                long tb = b.optLong("created_at_unix", 0L);
                if (ta != tb) return tb < ta ? -1 : 1;
                return b.optString("batch_id", "").compareTo(a.optString("batch_id", ""));
            }
        });
        return new JSONArray(list);
    }

    private void saveManualOverride(JSONObject args) throws Exception {
        String key = normalizeKey(args.optString("queryOrName", ""));
        if (key.isEmpty()) return;
        JSONObject overrides = loadJsonFile(new File(dataDir, "manual_overrides.json"));
        if (!overrides.has("entries")) overrides.put("entries", new JSONObject());
        JSONObject entry = new JSONObject();
        entry.put("english_title", args.optString("englishTitle", ""));
        entry.put("romaji_title", args.isNull("romajiTitle") ? JSONObject.NULL : args.optString("romajiTitle", ""));
        entry.put("anilist_id", args.isNull("anilistId") ? JSONObject.NULL : args.optLong("anilistId", 0L));
        overrides.optJSONObject("entries").put(key, entry);
        saveJsonFile(new File(dataDir, "manual_overrides.json"), overrides);
    }

    private JSONArray searchAniListManual(String query) throws Exception {
        List<String> one = new ArrayList<String>();
        one.add(query);
        Map<String, List<JSONObject>> map = queryAniListBatch(one);
        List<JSONObject> list = map.get(query);
        return list != null ? new JSONArray(list) : new JSONArray();
    }

    private Map<String, List<JSONObject>> queryAniListBatch(List<String> queries) throws Exception {
        StringBuilder qBuilder = new StringBuilder();
        qBuilder.append("query (");
        for (int i = 0; i < queries.size(); i++) {
            if (i > 0) qBuilder.append(", ");
            qBuilder.append("$q").append(i).append(": String");
        }
        qBuilder.append(") { ");
        for (int i = 0; i < queries.size(); i++) {
            qBuilder.append("q").append(i)
                .append(": Page(page: 1, perPage: 5) { media(search: $q")
                .append(i)
                .append(", type: ANIME) { id title { romaji english native } synonyms seasonYear format } } ");
        }
        qBuilder.append("}");

        JSONObject vars = new JSONObject();
        for (int i = 0; i < queries.size(); i++) {
            vars.put("q" + i, queries.get(i));
        }

        JSONObject payload = new JSONObject();
        payload.put("query", qBuilder.toString());
        payload.put("variables", vars);

        JSONObject root = postAniListWithRateLimit(payload.toString());
        JSONObject data = root.optJSONObject("data");
        Map<String, List<JSONObject>> out = new LinkedHashMap<String, List<JSONObject>>();

        for (int i = 0; i < queries.size(); i++) {
            String orig = queries.get(i);
            List<JSONObject> list = new ArrayList<JSONObject>();
            if (data != null) {
                JSONObject page = data.optJSONObject("q" + i);
                JSONArray media = page != null ? page.optJSONArray("media") : null;
                if (media != null) {
                    for (int m = 0; m < media.length(); m++) {
                        JSONObject item = media.optJSONObject(m);
                        if (item == null) continue;
                        JSONObject t = item.optJSONObject("title");
                        JSONObject cand = new JSONObject();
                        cand.put("id", item.optLong("id"));
                        cand.put("romaji", t != null && !t.isNull("romaji") ? t.optString("romaji") : JSONObject.NULL);
                        cand.put("english", t != null && !t.isNull("english") ? t.optString("english") : JSONObject.NULL);
                        cand.put("native", t != null && !t.isNull("native") ? t.optString("native") : JSONObject.NULL);
                        cand.put("synonyms", item.optJSONArray("synonyms") != null ? item.optJSONArray("synonyms") : new JSONArray());
                        cand.put("season_year", item.isNull("seasonYear") ? JSONObject.NULL : item.optInt("seasonYear"));
                        cand.put("format", item.isNull("format") ? JSONObject.NULL : item.optString("format"));
                        list.add(cand);
                    }
                }
            }
            out.put(orig, list);
        }
        return out;
    }

    private JSONObject postAniListWithRateLimit(String body) throws Exception {
        long backoffMs = 2000L;
        for (int attempt = 0; attempt < 5; attempt++) {
            long wait = nextRequestReadyAtMs - System.currentTimeMillis();
            if (wait > 0) {
                Thread.sleep(wait);
            }

            HttpURLConnection conn = (HttpURLConnection) new URL(ANILIST_ENDPOINT).openConnection();
            conn.setRequestMethod("POST");
            conn.setConnectTimeout(10000);
            conn.setReadTimeout(20000);
            conn.setDoOutput(true);
            conn.setRequestProperty("Content-Type", "application/json");
            conn.setRequestProperty("Accept", "application/json");
            conn.setRequestProperty("User-Agent", "RenamerAnime/2.0 (Android SAF)");

            OutputStream os = conn.getOutputStream();
            os.write(body.getBytes(StandardCharsets.UTF_8));
            os.close();

            int code = conn.getResponseCode();
            String remaining = conn.getHeaderField("X-RateLimit-Remaining");
            String retryAfter = conn.getHeaderField("Retry-After");

            if (remaining != null) {
                try {
                    int rem = Integer.parseInt(remaining.trim());
                    if (rem <= 5) {
                        nextRequestReadyAtMs = System.currentTimeMillis() + 2000L;
                    } else if (rem <= 15) {
                        nextRequestReadyAtMs = System.currentTimeMillis() + 800L;
                    }
                } catch (NumberFormatException ignored) {
                }
            }

            if (code == 429 || (code >= 500 && code < 600)) {
                long sleepMs = backoffMs;
                if (retryAfter != null) {
                    try {
                        sleepMs = Math.max(sleepMs, Long.parseLong(retryAfter.trim()) * 1000L);
                    } catch (NumberFormatException ignored) {
                    }
                }
                emitProgress("scan", "AniList HTTP " + code + " rate limit; retrying in " + (sleepMs / 1000) + "s...");
                Thread.sleep(sleepMs);
                backoffMs *= 2L;
                continue;
            }

            InputStream is = (code >= 200 && code < 300) ? conn.getInputStream() : conn.getErrorStream();
            String respText = readStream(is);
            if (code < 200 || code >= 300) {
                throw new IllegalStateException("AniList HTTP " + code + ": " + respText);
            }
            return new JSONObject(respText);
        }
        throw new IllegalStateException("AniList API failed after retries.");
    }

    // ---- Tag cleanup & formatter matching shared rules/default_rules.json semantics ----

    private static class ParsedEntry {
        String rawName;
        String uri;
        String parentUri;
        boolean isDir;
        int depth;
        String extension = "";
        String existingPrepended = null;
        String baseTitle = "";
        String inlineAlias = null;
        String searchQuery = "";
        String effectiveQuery = "";
        String yearSpec = null;
        String seasonSpec = null;
        String episodeSpec = null;
        List<String> extraNotes = new ArrayList<String>();
        List<String> strippedTags = new ArrayList<String>();
        boolean isPureSeasonSubdir = false;
        boolean alreadyProcessed = false;

        JSONObject toJson() throws Exception {
            JSONObject obj = new JSONObject();
            obj.put("raw_name", rawName);
            obj.put("is_dir", isDir);
            obj.put("extension", extension);
            obj.put("existing_prepended_title", existingPrepended == null ? JSONObject.NULL : existingPrepended);
            obj.put("base_title", baseTitle);
            obj.put("inline_alias", inlineAlias == null ? JSONObject.NULL : inlineAlias);
            obj.put("search_query", searchQuery);
            obj.put("fallback_queries", new JSONArray());
            obj.put("year_spec", yearSpec == null ? JSONObject.NULL : yearSpec);
            obj.put("season_spec", seasonSpec == null ? JSONObject.NULL : seasonSpec);
            obj.put("episode_spec", episodeSpec == null ? JSONObject.NULL : episodeSpec);
            obj.put("extra_notes", new JSONArray(extraNotes));
            obj.put("stripped_tags", new JSONArray(strippedTags));
            return obj;
        }
    }

    private ParsedEntry parseItemName(String rawName, boolean isDir, JSONObject rules) {
        ParsedEntry pe = new ParsedEntry();
        pe.rawName = rawName;
        pe.isDir = isDir;

        String stem = rawName;
        if (!isDir) {
            int dot = rawName.lastIndexOf('.');
            if (dot > 0) {
                String ext = rawName.substring(dot + 1).toLowerCase(Locale.ROOT);
                Set<String> exts = jsonArrayToLowerSet(rules.optJSONArray("media_extensions"));
                if (exts.contains(ext)) {
                    pe.extension = "." + rawName.substring(dot + 1);
                    stem = rawName.substring(0, dot);
                }
            }
        }

        Matcher leadPrep = Pattern.compile("^\\(([^)]+)\\)\\s+(.+)$").matcher(stem);
        if (leadPrep.matches()) {
            pe.existingPrepended = leadPrep.group(1).trim();
            stem = leadPrep.group(2).trim();
        }

        // Collect all [...] groups: strip leading release group & technical tags
        Matcher sq = Pattern.compile("\\[([^\\]]+)\\]").matcher(stem);
        StringBuffer afterSq = new StringBuffer();
        boolean isFirst = true;
        while (sq.find()) {
            String full = sq.group(0);
            String inner = sq.group(1).trim();
            if (isFirst && sq.start() == 0) {
                pe.strippedTags.add(full);
                sq.appendReplacement(afterSq, " ");
            } else if (inner.matches("(?i)^(?:Seasons?|S)\\s*\\d+.*")) {
                pe.seasonSpec = normalizeSeasonString(inner);
                sq.appendReplacement(afterSq, " ");
            } else {
                pe.strippedTags.add(full);
                sq.appendReplacement(afterSq, " ");
            }
            isFirst = false;
        }
        sq.appendTail(afterSq);
        String work = afterSq.toString().replaceAll("\\s+", " ").trim();

        // Check (...) groups for years, seasons, inline aliases
        Matcher par = Pattern.compile("\\(([^)]+)\\)").matcher(work);
        StringBuffer afterPar = new StringBuffer();
        while (par.find()) {
            String full = par.group(0);
            String inner = par.group(1).trim();
            if (inner.matches("^\\d{4}(?:-\\d{4})?$")) {
                pe.yearSpec = "(" + inner + ")";
                par.appendReplacement(afterPar, " ");
            } else if (inner.matches("(?i)^(?:Seasons?|S)\\s*\\d+.*")) {
                pe.seasonSpec = normalizeSeasonString(inner);
                par.appendReplacement(afterPar, " ");
            } else if (inner.matches("^[A-Za-z0-9'!,.:\\- ]{3,}$")) {
                pe.inlineAlias = inner;
                par.appendReplacement(afterPar, " ");
            } else {
                par.appendReplacement(afterPar, Matcher.quoteReplacement(full));
            }
        }
        par.appendTail(afterPar);
        work = afterPar.toString().replaceAll("\\s+", " ").trim();

        // Bare S01 / S01-02 / COMPLETE / v2 tokens
        Matcher sBare = Pattern.compile("(?i)\\b(S0*(\\d+)(?:\\s*[-~+]\\s*0*(\\d+))?)(?:\\s*v\\d+)?\\b").matcher(work);
        if (sBare.find()) {
            if (pe.seasonSpec == null) {
                String a = Integer.valueOf(Integer.parseInt(sBare.group(2))).toString();
                String b = sBare.group(3);
                pe.seasonSpec = (b != null)
                    ? "(Seasons " + a + "-" + Integer.parseInt(b) + ")"
                    : "(Season " + a + ")";
            }
            pe.strippedTags.add(sBare.group(0));
            work = (work.substring(0, sBare.start()) + " " + work.substring(sBare.end())).trim();
        }

        Matcher epBare = Pattern.compile("(?i)\\bS\\d{1,2}E(\\d{1,4})(?:v\\d+)?\\b").matcher(work);
        if (epBare.find()) {
            pe.episodeSpec = epBare.group(0).replaceAll("(?i)v\\d+$", "");
            work = (work.substring(0, epBare.start()) + " " + work.substring(epBare.end())).trim();
        } else if (!isDir && work.matches("^\\d{1,4}$")) {
            pe.episodeSpec = work;
            work = "";
        }

        for (String kw : jsonArrayToList(rules.optJSONArray("batch_keywords"))) {
            if (work.matches("(?i).*\\b" + Pattern.quote(kw) + "\\b.*")) {
                pe.strippedTags.add(kw);
                work = work.replaceAll("(?i)\\b" + Pattern.quote(kw) + "\\b", " ").trim();
            }
        }

        // Preserve "+ Special / OVA / Movie"
        Matcher plusNote = Pattern.compile("(?i)\\+\\s*(Specials?|OVAs?|Movies?)(?:\\s*\\+\\s*(?:Specials?|OVAs?|Movies?))*").matcher(work);
        if (plusNote.find()) {
            pe.extraNotes.add(plusNote.group(0).replaceAll("\\s+", " ").trim());
            work = (work.substring(0, plusNote.start()) + " " + work.substring(plusNote.end())).trim();
        }

        work = work.replaceAll("(?i)\\bv\\d+\\b", " ").replaceAll("\\s+-\\s*$", "").replaceAll("\\s+", " ").trim();
        pe.baseTitle = work;
        pe.searchQuery = work;
        return pe;
    }

    private String normalizeSeasonString(String inner) {
        String s = inner.trim();
        Matcher plus = Pattern.compile("(?i)^Season\\s*0*(\\d+)\\s*\\+\\s*0*(\\d+)$").matcher(s);
        if (plus.matches()) {
            return "(Seasons " + Integer.parseInt(plus.group(1)) + "-" + Integer.parseInt(plus.group(2)) + ")";
        }
        Matcher range = Pattern.compile("(?i)^Seasons?\\s*0*(\\d+)\\s*-\\s*0*(\\d+)(.*)$").matcher(s);
        if (range.matches()) {
            return "(Seasons " + Integer.parseInt(range.group(1)) + "-" + Integer.parseInt(range.group(2)) + range.group(3) + ")";
        }
        Matcher single = Pattern.compile("(?i)^Seasons?\\s*0*(\\d+)(.*)$").matcher(s);
        if (single.matches()) {
            return "(Season " + Integer.parseInt(single.group(1)) + single.group(2) + ")";
        }
        Matcher shortS = Pattern.compile("(?i)^S0*(\\d+)$").matcher(s);
        if (shortS.matches()) {
            return "(Season " + Integer.parseInt(shortS.group(1)) + ")";
        }
        return "(" + s + ")";
    }

    private String formatProposedName(
        ParsedEntry pe,
        String englishTitle,
        String romajiTitle,
        String mode,
        boolean cleanTags
    ) {
        String chosen = englishTitle;
        if ((chosen == null || chosen.isEmpty()) && pe.inlineAlias != null) chosen = pe.inlineAlias;
        if (chosen == null || chosen.isEmpty()) chosen = romajiTitle;
        if (chosen == null || chosen.isEmpty()) chosen = pe.baseTitle;
        chosen = sanitizeComponent(chosen);

        StringBuilder out = new StringBuilder();
        if ("prepend".equals(mode)) {
            if (!chosen.equalsIgnoreCase(pe.baseTitle) && !pe.baseTitle.isEmpty()) {
                out.append("(").append(chosen).append(") ").append(sanitizeComponent(pe.baseTitle));
            } else {
                out.append(chosen);
            }
        } else if ("append".equals(mode)) {
            if (!chosen.equalsIgnoreCase(pe.baseTitle) && !pe.baseTitle.isEmpty()) {
                out.append(sanitizeComponent(pe.baseTitle)).append(" (").append(chosen).append(")");
            } else {
                out.append(chosen);
            }
        } else {
            out.append(chosen);
        }

        if (pe.yearSpec != null) out.append(" ").append(pe.yearSpec);
        if (pe.seasonSpec != null && pe.isDir) out.append(" ").append(pe.seasonSpec);
        if (pe.episodeSpec != null && !pe.isDir) out.append(" - ").append(pe.episodeSpec);
        for (String note : pe.extraNotes) out.append(" ").append(note);
        if (!pe.isDir && !pe.extension.isEmpty()) out.append(pe.extension);
        return out.toString().replaceAll("\\s+", " ").trim();
    }

    private String sanitizeComponent(String s) {
        if (s == null) return "";
        return s.replace(':', '-')
            .replace('/', '-')
            .replace('\\', '-')
            .replaceAll("[*?\"<>|]", "")
            .replaceAll("\\s+", " ")
            .trim();
    }

    // ---- File & JSON utilities ----

    private JSONObject loadRules() throws Exception {
        if (rulesFile.exists()) {
            return loadJsonFile(rulesFile);
        }
        InputStream is = activity.getAssets().open("default_rules.json");
        try {
            return new JSONObject(readStream(is));
        } finally {
            is.close();
        }
    }

    private JSONObject loadJsonFile(File f) throws Exception {
        if (!f.exists()) return new JSONObject();
        FileInputStream fis = new FileInputStream(f);
        try {
            String txt = readStream(fis);
            return txt.trim().isEmpty() ? new JSONObject() : new JSONObject(txt);
        } finally {
            fis.close();
        }
    }

    private void saveJsonFile(File f, JSONObject obj) throws Exception {
        if (f.getParentFile() != null && !f.getParentFile().exists()) {
            f.getParentFile().mkdirs();
        }
        writeString(f, obj.toString());
    }

    private void writeString(File f, String content) throws Exception {
        File tmp = new File(f.getParentFile(), f.getName() + ".tmp");
        FileOutputStream fos = new FileOutputStream(tmp);
        try {
            fos.write(content.getBytes(StandardCharsets.UTF_8));
        } finally {
            fos.close();
        }
        if (f.exists()) f.delete();
        tmp.renameTo(f);
    }

    private String readStream(InputStream is) throws Exception {
        if (is == null) return "";
        BufferedReader br = new BufferedReader(new InputStreamReader(is, StandardCharsets.UTF_8));
        StringBuilder sb = new StringBuilder();
        String line;
        while ((line = br.readLine()) != null) {
            sb.append(line).append('\n');
        }
        return sb.toString();
    }

    private String normalizeKey(String q) {
        return q == null ? "" : q.trim().toLowerCase(Locale.ROOT).replaceAll("\\s+", " ");
    }

    private String ledgerKey(String parent, String name, String mode, boolean cleanTags) {
        return parent + "::" + name + "::" + mode + "::" + cleanTags;
    }

    private String replacePrefix(String value, String oldP, String newP) {
        return value.startsWith(oldP) ? newP + value.substring(oldP.length()) : value;
    }

    private String remapSafParent(String parent, List<String[]> dirRenames) {
        String cur = parent;
        for (String[] pair : dirRenames) {
            String oldDir = pair[0];
            String newDir = pair[1];
            if (cur.equals(oldDir)) {
                cur = newDir;
            } else if (cur.startsWith(oldDir)) {
                String rest = cur.substring(oldDir.length());
                if (rest.startsWith("%2F") || rest.startsWith("%2f") || rest.startsWith("/")) {
                    cur = newDir + rest;
                }
            }
        }
        return cur;
    }

    private Set<String> jsonArrayToLowerSet(JSONArray arr) {
        Set<String> set = new HashSet<String>();
        if (arr != null) {
            for (int i = 0; i < arr.length(); i++) {
                set.add(arr.optString(i, "").toLowerCase(Locale.ROOT));
            }
        }
        return set;
    }

    private List<String> jsonArrayToList(JSONArray arr) {
        List<String> list = new ArrayList<String>();
        if (arr != null) {
            for (int i = 0; i < arr.length(); i++) {
                list.add(arr.optString(i, ""));
            }
        }
        return list;
    }
}
