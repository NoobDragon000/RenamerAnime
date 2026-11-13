import { useEffect, useMemo, useState } from "react";
import {
  Check,
  Edit3,
  FileVideo,
  Folder,
  FolderOpen,
  FolderSearch,
  History,
  Play,
  RotateCcw,
  Search,
  Settings,
  Sparkles,
  Square,
  X,
} from "lucide-react";
import {
  applySelected,
  cancelActiveJob,
  createDemoSandbox,
  createSafDemoTree,
  getInitialState,
  getTransactionHistory,
  isSafUri,
  onProgress,
  openDataDir,
  pickFolder,
  resetCleanupRules,
  saveCleanupRules,
  saveManualOverride,
  scanDirectory,
  searchAniListManual,
  undoBatch,
} from "./api";
import type {
  CleanupRules,
  MediaCandidate,
  NamingMode,
  PlanItem,
  RenamePlan,
  TransactionBatch,
} from "./types";

type FilterTab = "all" | "ready" | "clean" | "nomatch" | "conflict";
type DrawerTab = "logs" | "history";

interface LogEntry {
  id: number;
  kind: "info" | "success" | "error";
  text: string;
}

export default function App() {
  const [folder, setFolder] = useState("");
  const [mode, setMode] = useState<NamingMode>("english-only");
  const [cleanTags, setCleanTags] = useState(true);
  const [recursive, setRecursive] = useState(false);
  const [offline, setOffline] = useState(false);
  const [forceRecheck, setForceRecheck] = useState(false);

  const [plan, setPlan] = useState<RenamePlan | null>(null);
  const [selectedIndices, setSelectedIndices] = useState<Set<number>>(new Set());
  const [filterTab, setFilterTab] = useState<FilterTab>("all");
  const [searchFilter, setSearchFilter] = useState("");

  const [busy, setBusy] = useState(false);
  const [progressText, setProgressText] = useState("");
  const [dataDir, setDataDir] = useState("");
  const [rules, setRules] = useState<CleanupRules | null>(null);
  const [history, setHistory] = useState<TransactionBatch[]>([]);
  const [drawerTab, setDrawerTab] = useState<DrawerTab>("logs");
  const [logs, setLogs] = useState<LogEntry[]>([]);

  // Override Modal State
  const [overrideTarget, setOverrideTarget] = useState<PlanItem | null>(null);
  const [manualQuery, setManualQuery] = useState("");
  const [manualCandidates, setManualCandidates] = useState<MediaCandidate[]>([]);
  const [manualEnglishInput, setManualEnglishInput] = useState("");
  const [searchingManual, setSearchingManual] = useState(false);

  // Rules Modal State
  const [rulesModalOpen, setRulesModalOpen] = useState(false);
  const [rulesJsonText, setRulesJsonText] = useState("");
  const [rulesError, setRulesError] = useState<string | null>(null);

  const appendLog = (kind: LogEntry["kind"], text: string) => {
    setLogs((prev) => [{ id: Date.now() + Math.random(), kind, text }, ...prev].slice(0, 300));
  };

  useEffect(() => {
    getInitialState()
      .then((init) => {
        setDataDir(init.data_dir);
        setRules(init.rules);
        setHistory(init.history);
        appendLog("info", `Ready. Data directory: ${init.data_dir}`);
      })
      .catch((err) => appendLog("error", `Init failed: ${String(err)}`));

    let unlisten: (() => void) | undefined;
    onProgress((ev) => {
      setProgressText(ev.message);
      appendLog("info", `[${ev.phase.toUpperCase()}] ${ev.message}`);
    }).then((fn) => {
      unlisten = fn;
    });

    return () => {
      if (unlisten) unlisten();
    };
  }, []);

  const refreshHistory = async () => {
    try {
      const h = await getTransactionHistory();
      setHistory(h);
    } catch (e) {
      appendLog("error", `Failed to refresh history: ${String(e)}`);
    }
  };

  const runScan = async (overrideFolder?: string) => {
    const target = (overrideFolder ?? folder).trim();
    if (!target) {
      appendLog("error", "Please select or enter a folder path first.");
      return;
    }
    setBusy(true);
    setProgressText("Scanning folder and batching AniList lookups...");
    try {
      const nextPlan = await scanDirectory({
        folder: target,
        mode,
        cleanTags,
        recursive,
        offline,
        forceRecheck,
      });
      setPlan(nextPlan);
      const ready = new Set<number>();
      for (const item of nextPlan.items) {
        if (item.status === "ready_to_rename") {
          ready.add(item.index);
        }
      }
      setSelectedIndices(ready);
      appendLog(
        "success",
        `Dry-run complete: ${ready.size} ready, ${nextPlan.items.length - ready.size} skipped/clean, ${nextPlan.api_requests_made} API requests.`
      );
      await refreshHistory();
    } catch (e) {
      appendLog("error", `Scan failed: ${String(e)}`);
    } finally {
      setBusy(false);
      setProgressText("");
    }
  };

  const handleBrowse = async () => {
    try {
      const picked = await pickFolder();
      if (picked) {
        setFolder(picked);
        await runScan(picked);
      }
    } catch (e) {
      appendLog("error", `Folder picker error: ${String(e)}`);
    }
  };

  const handleLoadSandbox = async () => {
    try {
      const sandbox = await createDemoSandbox();
      setFolder(sandbox);
      appendLog("info", `Created demo sandbox at: ${sandbox}`);
      await runScan(sandbox);
    } catch (e) {
      appendLog("error", `Failed creating demo sandbox: ${String(e)}`);
    }
  };

  const handleLoadSafDemo = async () => {
    try {
      const uri = await createSafDemoTree();
      setFolder(uri);
      appendLog("info", `Loaded simulated Android SAF tree: ${uri}`);
      await runScan(uri);
    } catch (e) {
      appendLog("error", `Failed creating SAF demo tree: ${String(e)}`);
    }
  };

  const handleApply = async () => {
    if (!plan) return;
    const indices = Array.from(selectedIndices);
    if (indices.length === 0) return;

    setBusy(true);
    setProgressText(`Applying ${indices.length} renames (bottom-up)...`);
    try {
      const report = await applySelected(indices, plan);
      appendLog(
        report.error_count > 0 ? "error" : "success",
        `Batch ${report.batch_id}: ${report.renamed_count} renamed, ${report.skipped_count} skipped, ${report.error_count} errors${report.cancelled ? " (CANCELLED)" : ""}.`
      );
      for (const err of report.errors) {
        appendLog("error", err);
      }
      await refreshHistory();
      await runScan();
    } catch (e) {
      appendLog("error", `Apply failed: ${String(e)}`);
    } finally {
      setBusy(false);
      setProgressText("");
    }
  };

  const handleUndo = async (batchId: string | null) => {
    setBusy(true);
    setProgressText("Reverting rename batch...");
    try {
      const report = await undoBatch(batchId);
      appendLog(
        report.error_count > 0 ? "error" : "success",
        `Undo ${report.batch_id}: restored ${report.restored_count} items (${report.error_count} errors).`
      );
      for (const err of report.errors) {
        appendLog("error", err);
      }
      await refreshHistory();
      if (folder.trim()) {
        await runScan();
      }
    } catch (e) {
      appendLog("error", `Undo failed: ${String(e)}`);
    } finally {
      setBusy(false);
      setProgressText("");
    }
  };

  const openOverrideModal = (item: PlanItem) => {
    setOverrideTarget(item);
    const initialQ = item.effective_query || item.parsed.base_title || item.current_name;
    setManualQuery(initialQ);
    setManualCandidates(item.candidates ?? []);
    setManualEnglishInput(item.resolved_english ?? item.parsed.inline_alias ?? "");
  };

  const handleManualSearch = async () => {
    if (!manualQuery.trim()) return;
    setSearchingManual(true);
    try {
      const results = await searchAniListManual(manualQuery.trim());
      setManualCandidates(results);
    } catch (e) {
      appendLog("error", `Manual search failed: ${String(e)}`);
    } finally {
      setSearchingManual(false);
    }
  };

  const applyCandidateOverride = async (
    englishTitle: string,
    romajiTitle: string | null,
    anilistId: number | null
  ) => {
    if (!overrideTarget || !englishTitle.trim()) return;
    const key = overrideTarget.effective_query || overrideTarget.current_name;
    try {
      await saveManualOverride({
        queryOrName: key,
        englishTitle: englishTitle.trim(),
        romajiTitle,
        anilistId,
      });
      appendLog("success", `Saved override '${key}' -> '${englishTitle.trim()}'`);
      setOverrideTarget(null);
      await runScan();
    } catch (e) {
      appendLog("error", `Save override failed: ${String(e)}`);
    }
  };

  const openRulesEditor = () => {
    if (!rules) return;
    setRulesJsonText(JSON.stringify(rules, null, 2));
    setRulesError(null);
    setRulesModalOpen(true);
  };

  const handleSaveRules = async () => {
    try {
      const parsed = JSON.parse(rulesJsonText) as CleanupRules;
      await saveCleanupRules(parsed);
      setRules(parsed);
      setRulesModalOpen(false);
      appendLog("success", "Saved updated tag cleanup rules.");
      if (folder.trim()) await runScan();
    } catch (e) {
      setRulesError(String(e));
    }
  };

  const handleResetRules = async () => {
    try {
      const defaults = await resetCleanupRules();
      setRules(defaults);
      setRulesJsonText(JSON.stringify(defaults, null, 2));
      setRulesError(null);
      appendLog("info", "Reset rules to built-in defaults.");
    } catch (e) {
      setRulesError(String(e));
    }
  };

  const counts = useMemo(() => {
    const items = plan?.items ?? [];
    return {
      all: items.length,
      ready: items.filter((i) => i.status === "ready_to_rename").length,
      clean: items.filter(
        (i) => i.status === "already_clean" || i.status === "already_processed"
      ).length,
      nomatch: items.filter((i) => i.resolution_source === "no_match").length,
      conflict: items.filter((i) => i.status === "conflict").length,
    };
  }, [plan]);

  const filteredItems = useMemo(() => {
    if (!plan) return [];
    const q = searchFilter.trim().toLowerCase();
    return plan.items.filter((item) => {
      if (filterTab === "ready" && item.status !== "ready_to_rename") return false;
      if (
        filterTab === "clean" &&
        item.status !== "already_clean" &&
        item.status !== "already_processed"
      )
        return false;
      if (filterTab === "nomatch" && item.resolution_source !== "no_match") return false;
      if (filterTab === "conflict" && item.status !== "conflict") return false;
      if (!q) return true;
      return (
        item.current_name.toLowerCase().includes(q) ||
        item.proposed_name.toLowerCase().includes(q) ||
        item.effective_query.toLowerCase().includes(q)
      );
    });
  }, [plan, filterTab, searchFilter]);

  const readySelectedCount = useMemo(() => {
    if (!plan) return 0;
    return plan.items.filter(
      (i) => i.status === "ready_to_rename" && selectedIndices.has(i.index)
    ).length;
  }, [plan, selectedIndices]);

  const toggleAllVisibleReady = () => {
    const readyVisible = filteredItems.filter((i) => i.status === "ready_to_rename");
    const allChecked =
      readyVisible.length > 0 && readyVisible.every((i) => selectedIndices.has(i.index));
    const next = new Set(selectedIndices);
    for (const item of readyVisible) {
      if (allChecked) next.delete(item.index);
      else next.add(item.index);
    }
    setSelectedIndices(next);
  };

  const latestRevertable = history.find((b) => !b.reverted && b.operations.length > 0);

  return (
    <div className="app-shell">
      {/* Top Toolbar */}
      <header className="topbar">
        <div className="topbar-row">
          <div className="brand">
            <Sparkles size={18} color="#818cf8" />
            <span>RenamerAnime</span>
            <span className="brand-badge">v2.0</span>
            {isSafUri(plan?.root_dir || folder) && (
              <span className="badge badge-saf" title="Operating via Android SAF content:// tree">
                SAF content://
              </span>
            )}
          </div>

          <input
            className="folder-input"
            type="text"
            placeholder="Select folder path or Android SAF content:// URI..."
            value={folder}
            onChange={(e) => setFolder(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter" && !busy) runScan();
            }}
          />

          <button className="btn" onClick={handleBrowse} disabled={busy}>
            <FolderOpen size={14} /> Browse
          </button>

          <button
            className="btn"
            onClick={handleLoadSandbox}
            disabled={busy}
            title="Create a safe temporary folder populated with real release examples for testing"
          >
            <FolderSearch size={14} /> Demo Sandbox
          </button>

          <button
            className="btn"
            onClick={handleLoadSafDemo}
            disabled={busy}
            title="Simulate an Android Storage Access Framework content:// document tree"
          >
            <FolderSearch size={14} /> SAF (content://) Demo
          </button>

          <button className="btn btn-primary" onClick={() => runScan()} disabled={busy || !folder.trim()}>
            <Search size={14} /> Scan (Dry Run)
          </button>

          <button
            className="btn btn-success"
            onClick={handleApply}
            disabled={busy || readySelectedCount === 0}
          >
            <Play size={14} /> Apply Selected ({readySelectedCount})
          </button>

          <button
            className="btn"
            onClick={() => handleUndo(null)}
            disabled={busy || !latestRevertable}
            title="Revert the most recent rename batch"
          >
            <RotateCcw size={14} /> Undo Last
          </button>
        </div>

        <div className="topbar-row">
          <label style={{ display: "inline-flex", alignItems: "center", gap: 6, color: "var(--muted)" }}>
            <span>Naming Mode:</span>
            <select
              className="select-input"
              value={mode}
              onChange={(e) => setMode(e.target.value as NamingMode)}
            >
              <option value="english-only">English Title Only (In-Spectre (Season 2))</option>
              <option value="prepend">Prepend ((In-Spectre) Kyokou Suiri)</option>
              <option value="append">Append (Kyokou Suiri (In-Spectre))</option>
            </select>
          </label>

          <label className="toggle-chip">
            <input
              type="checkbox"
              checked={cleanTags}
              onChange={(e) => setCleanTags(e.target.checked)}
            />
            Clean Scene Tags
          </label>

          <label className="toggle-chip">
            <input
              type="checkbox"
              checked={recursive}
              onChange={(e) => setRecursive(e.target.checked)}
            />
            Subfolders & Episodes
          </label>

          <label className="toggle-chip">
            <input
              type="checkbox"
              checked={offline}
              onChange={(e) => setOffline(e.target.checked)}
            />
            Offline Only
          </label>

          <label className="toggle-chip">
            <input
              type="checkbox"
              checked={forceRecheck}
              onChange={(e) => setForceRecheck(e.target.checked)}
            />
            Force Recheck
          </label>

          <div style={{ marginLeft: "auto", display: "flex", gap: 8 }}>
            <button className="btn" onClick={openRulesEditor}>
              <Settings size={14} /> Rules Config
            </button>
            <button
              className="btn"
              onClick={() => openDataDir()}
              title={`Open cache & transaction folder: ${dataDir}`}
            >
              <Folder size={14} /> Data Folder
            </button>
          </div>
        </div>
      </header>

      {/* Progress Strip */}
      {busy && (
        <div className="progress-strip">
          <div className="progress-left">
            <div className="spinner" />
            <span>{progressText || "Working..."}</span>
          </div>
          <button
            className="btn btn-danger btn-sm"
            onClick={() => cancelActiveJob()}
          >
            <Square size={12} /> Cancel
          </button>
        </div>
      )}

      {/* Filter & Summary Bar */}
      <div className="filter-bar">
        <div className="filter-tabs">
          <button
            className={`tab-btn ${filterTab === "all" ? "active" : ""}`}
            onClick={() => setFilterTab("all")}
          >
            All ({counts.all})
          </button>
          <button
            className={`tab-btn ${filterTab === "ready" ? "active" : ""}`}
            onClick={() => setFilterTab("ready")}
          >
            Ready ({counts.ready})
          </button>
          <button
            className={`tab-btn ${filterTab === "clean" ? "active" : ""}`}
            onClick={() => setFilterTab("clean")}
          >
            Processed / Clean ({counts.clean})
          </button>
          <button
            className={`tab-btn ${filterTab === "nomatch" ? "active" : ""}`}
            onClick={() => setFilterTab("nomatch")}
          >
            No Match ({counts.nomatch})
          </button>
          <button
            className={`tab-btn ${filterTab === "conflict" ? "active" : ""}`}
            onClick={() => setFilterTab("conflict")}
          >
            Conflicts ({counts.conflict})
          </button>
        </div>

        <input
          className="search-input"
          type="text"
          placeholder="Filter preview rows..."
          value={searchFilter}
          onChange={(e) => setSearchFilter(e.target.value)}
          style={{ width: 220 }}
        />
      </div>

      {/* Preview Table */}
      <main className="table-wrap">
        {!plan ? (
          <div className="empty-state">
            <FolderOpen size={36} color="#4b5563" />
            <div style={{ fontSize: 14, fontWeight: 600, color: "var(--text)" }}>
              Choose a folder to preview renames (Dry Run is always default)
            </div>
            <div style={{ maxWidth: 480 }}>
              Click <strong>Browse</strong> to open your anime folder, or click{" "}
              <strong>Demo Sandbox</strong> to test on a safe temporary folder populated with
              your real library examples.
            </div>
          </div>
        ) : (
          <table>
            <thead>
              <tr>
                <th style={{ width: 38 }}>
                  <input
                    type="checkbox"
                    checked={
                      filteredItems.some((i) => i.status === "ready_to_rename") &&
                      filteredItems
                        .filter((i) => i.status === "ready_to_rename")
                        .every((i) => selectedIndices.has(i.index))
                    }
                    onChange={toggleAllVisibleReady}
                  />
                </th>
                <th style={{ width: 72 }}>Type</th>
                <th style={{ width: "37%" }}>Current Name (Stripped Tags Highlighted)</th>
                <th style={{ width: "37%" }}>Proposed Name</th>
                <th style={{ width: 135 }}>Status</th>
                <th style={{ width: 92 }}>Title</th>
              </tr>
            </thead>
            <tbody>
              {filteredItems.map((item) => {
                const isReady = item.status === "ready_to_rename";
                const checked = selectedIndices.has(item.index);
                return (
                  <tr key={item.index}>
                    <td>
                      <input
                        type="checkbox"
                        disabled={!isReady}
                        checked={isReady && checked}
                        onChange={(e) => {
                          const next = new Set(selectedIndices);
                          if (e.target.checked) next.add(item.index);
                          else next.delete(item.index);
                          setSelectedIndices(next);
                        }}
                      />
                    </td>
                    <td>
                      <span
                        className="badge badge-type"
                        style={{ marginLeft: item.depth * 8 }}
                      >
                        {item.is_dir ? <Folder size={10} /> : <FileVideo size={10} />}
                        {item.is_dir ? "DIR" : "FILE"}
                      </span>
                    </td>
                    <td>
                      <div className="mono-name">{item.current_name}</div>
                      {item.parsed.stripped_tags.length > 0 && (
                        <div className="tag-row">
                          {item.parsed.stripped_tags.map((t, idx) => (
                            <span key={`${t}-${idx}`} className="tag-stripped">
                              {t}
                            </span>
                          ))}
                        </div>
                      )}
                    </td>
                    <td>
                      <div
                        className={
                          item.proposed_name !== item.current_name
                            ? "proposed-name"
                            : "proposed-unchanged"
                        }
                      >
                        {item.proposed_name}
                      </div>
                      <div className="tag-row">
                        {item.parsed.year_spec && (
                          <span className="tag-kept">{item.parsed.year_spec}</span>
                        )}
                        {item.parsed.season_spec && (
                          <span className="tag-kept">{item.parsed.season_spec}</span>
                        )}
                        {item.parsed.episode_spec && (
                          <span className="tag-kept">Ep {item.parsed.episode_spec}</span>
                        )}
                        {item.parsed.extra_notes.map((n, i) => (
                          <span key={i} className="tag-kept">
                            {n}
                          </span>
                        ))}
                      </div>
                    </td>
                    <td>
                      <StatusBadge item={item} />
                      {item.status_reason && (
                        <div style={{ fontSize: 11, color: "var(--muted)", marginTop: 3 }}>
                          {item.status_reason}
                        </div>
                      )}
                    </td>
                    <td>
                      <button
                        className="btn btn-sm"
                        onClick={() => openOverrideModal(item)}
                        disabled={busy || !item.effective_query}
                        title="Search AniList or manually override title"
                      >
                        <Edit3 size={11} /> Override
                      </button>
                    </td>
                  </tr>
                );
              })}
            </tbody>
          </table>
        )}
      </main>

      {/* Bottom Log & Undo History Drawer */}
      <section className="bottom-drawer">
        <div className="drawer-header">
          <div className="filter-tabs">
            <button
              className={`tab-btn ${drawerTab === "logs" ? "active" : ""}`}
              onClick={() => setDrawerTab("logs")}
            >
              Execution Log ({logs.length})
            </button>
            <button
              className={`tab-btn ${drawerTab === "history" ? "active" : ""}`}
              onClick={() => setDrawerTab("history")}
            >
              <History size={12} style={{ verticalAlign: "middle", marginRight: 4 }} />
              Undo History ({history.length})
            </button>
          </div>
          {plan && (
            <span style={{ fontSize: 11, color: "var(--muted)" }}>
              Last scan API requests: <strong>{plan.api_requests_made}</strong>
            </span>
          )}
        </div>

        <div className="drawer-body">
          {drawerTab === "logs" ? (
            logs.length === 0 ? (
              <div style={{ color: "var(--muted)" }}>No activity yet.</div>
            ) : (
              logs.map((l) => (
                <div key={l.id} className={`log-line ${l.kind}`}>
                  {l.text}
                </div>
              ))
            )
          ) : history.length === 0 ? (
            <div style={{ color: "var(--muted)" }}>No rename batches applied yet.</div>
          ) : (
            history.map((b) => (
              <div key={b.batch_id} className="history-row">
                <div>
                  <strong style={{ fontFamily: "var(--mono)" }}>{b.batch_id}</strong>{" "}
                  <span style={{ color: "var(--muted)" }}>
                    • {b.operations.length} items • {b.mode} • {b.root_dir}
                  </span>
                </div>
                <div>
                  {b.reverted ? (
                    <span className="badge badge-clean">Reverted</span>
                  ) : (
                    <button
                      className="btn btn-sm"
                      disabled={busy || b.operations.length === 0}
                      onClick={() => handleUndo(b.batch_id)}
                    >
                      <RotateCcw size={11} /> Undo Batch
                    </button>
                  )}
                </div>
              </div>
            ))
          )}
        </div>
      </section>

      {/* Manual Search / Override Modal */}
      {overrideTarget && (
        <div className="modal-backdrop" onClick={() => setOverrideTarget(null)}>
          <div className="modal-card" onClick={(e) => e.stopPropagation()}>
            <div className="modal-header">
              <span>Search / Override Title</span>
              <button className="btn btn-sm" onClick={() => setOverrideTarget(null)}>
                <X size={13} />
              </button>
            </div>
            <div className="modal-body">
              <div style={{ fontSize: 12, color: "var(--muted)" }}>
                Item: <strong style={{ color: "var(--text)" }}>{overrideTarget.current_name}</strong>
              </div>

              <div style={{ display: "flex", gap: 8 }}>
                <input
                  className="modal-input"
                  style={{ flex: 1 }}
                  value={manualQuery}
                  onChange={(e) => setManualQuery(e.target.value)}
                  placeholder="Search AniList by title..."
                  onKeyDown={(e) => {
                    if (e.key === "Enter") handleManualSearch();
                  }}
                />
                <button
                  className="btn btn-primary"
                  onClick={handleManualSearch}
                  disabled={searchingManual}
                >
                  <Search size={13} /> {searchingManual ? "Searching..." : "Search AniList"}
                </button>
              </div>

              <div style={{ display: "flex", gap: 8 }}>
                <input
                  className="modal-input"
                  style={{ flex: 1 }}
                  value={manualEnglishInput}
                  onChange={(e) => setManualEnglishInput(e.target.value)}
                  placeholder="Or enter a custom English title..."
                />
                <button
                  className="btn btn-success"
                  disabled={!manualEnglishInput.trim()}
                  onClick={() =>
                    applyCandidateOverride(
                      manualEnglishInput,
                      overrideTarget.resolved_romaji,
                      overrideTarget.anilist_id
                    )
                  }
                >
                  <Check size={13} /> Apply Custom
                </button>
              </div>

              <div style={{ fontSize: 11, fontWeight: 700, color: "var(--muted)", marginTop: 4 }}>
                ANILIST CANDIDATES ({manualCandidates.length})
              </div>

              {manualCandidates.length === 0 ? (
                <div style={{ color: "var(--muted)", fontSize: 12 }}>
                  No candidates loaded. Try searching a different title above.
                </div>
              ) : (
                manualCandidates.map((c) => {
                  const chosenTitle = c.english || c.romaji || c.native || "";
                  return (
                    <div key={c.id} className="candidate-item">
                      <div>
                        <div style={{ fontWeight: 600 }}>
                          {c.english || c.romaji || "(No title)"}{" "}
                          <span style={{ color: "var(--muted)", fontWeight: 400 }}>
                            {c.season_year ? `(${c.season_year})` : ""} {c.format || ""}
                          </span>
                        </div>
                        {c.romaji && c.english && c.romaji !== c.english && (
                          <div style={{ fontSize: 11, color: "var(--muted)" }}>
                            Romaji: {c.romaji}
                          </div>
                        )}
                      </div>
                      <button
                        className="btn btn-sm btn-primary"
                        disabled={!chosenTitle}
                        onClick={() => applyCandidateOverride(chosenTitle, c.romaji, c.id)}
                      >
                        Select
                      </button>
                    </div>
                  );
                })
              )}
            </div>
          </div>
        </div>
      )}

      {/* Data-Driven Rules Editor Modal */}
      {rulesModalOpen && (
        <div className="modal-backdrop" onClick={() => setRulesModalOpen(false)}>
          <div className="modal-card" onClick={(e) => e.stopPropagation()}>
            <div className="modal-header">
              <span>Tag Cleanup Rules Editor (JSON)</span>
              <button className="btn btn-sm" onClick={() => setRulesModalOpen(false)}>
                <X size={13} />
              </button>
            </div>
            <div className="modal-body">
              <div style={{ fontSize: 12, color: "var(--muted)" }}>
                Edit release groups, codec/quality tokens, batch words, or ignored folders. Saved
                to <code>{dataDir}/rules.json</code>.
              </div>
              <textarea
                className="rules-textarea"
                value={rulesJsonText}
                onChange={(e) => setRulesJsonText(e.target.value)}
              />
              {rulesError && <div style={{ color: "#fca5a5", fontSize: 12 }}>{rulesError}</div>}
              <div style={{ display: "flex", justifyContent: "space-between" }}>
                <button className="btn" onClick={handleResetRules}>
                  Reset to Defaults
                </button>
                <div style={{ display: "flex", gap: 8 }}>
                  <button className="btn" onClick={() => setRulesModalOpen(false)}>
                    Cancel
                  </button>
                  <button className="btn btn-primary" onClick={handleSaveRules}>
                    Save Rules
                  </button>
                </div>
              </div>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}

function StatusBadge({ item }: { item: PlanItem }) {
  const src = item.resolution_source.replace("_", " ").toUpperCase();
  if (item.status === "conflict") {
    return <span className="badge badge-conflict">Conflict</span>;
  }
  if (item.status === "already_processed") {
    return <span className="badge badge-processed">Processed</span>;
  }
  if (item.status === "already_clean") {
    return <span className="badge badge-clean">Clean • {src}</span>;
  }
  if (item.resolution_source === "no_match") {
    return <span className="badge badge-nomatch">Ready • No Match</span>;
  }
  return <span className="badge badge-ready">Ready • {src}</span>;
}
