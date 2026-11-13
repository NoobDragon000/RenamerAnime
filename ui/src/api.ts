import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type {
  ApplyReport,
  CleanupRules,
  InitialState,
  MediaCandidate,
  NamingMode,
  ProgressEvent,
  RenamePlan,
  SafRenameOutcome,
  TransactionBatch,
  UndoReport,
  VirtualEntry,
} from "./types";

export interface ScanRequest {
  folder: string;
  mode: NamingMode;
  cleanTags: boolean;
  recursive: boolean;
  offline: boolean;
  forceRecheck: boolean;
}

export function isSafUri(value: string): boolean {
  return value.trim().startsWith("content://");
}

async function callBackend<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  const native = window.AndroidNativeBackend;
  if (native) {
    const raw = native.invoke(cmd, JSON.stringify(args ?? {}));
    const res = JSON.parse(raw) as { ok: boolean; data?: T; error?: string };
    if (!res.ok) {
      throw new Error(res.error || `Android backend error in ${cmd}`);
    }
    return res.data as T;
  }
  return invoke<T>(cmd, args);
}

export async function getInitialState(): Promise<InitialState> {
  return callBackend<InitialState>("get_initial_state");
}

export async function pickFolder(): Promise<string | null> {
  const bridge = window.AndroidSafBridge;
  if (bridge) {
    return new Promise<string | null>((resolve) => {
      window.__onSafTreePicked = (uri: string | null) => {
        window.__onSafTreePicked = undefined;
        resolve(uri);
      };
      bridge.pickDocumentTree();
    });
  }
  return callBackend<string | null>("pick_folder");
}

export async function createDemoSandbox(): Promise<string> {
  return callBackend<string>("create_demo_sandbox");
}

export async function createSafDemoTree(): Promise<string> {
  return callBackend<string>("create_saf_demo_tree");
}

export async function scanDirectory(req: ScanRequest): Promise<RenamePlan> {
  if (isSafUri(req.folder)) {
    let entries: VirtualEntry[] = [];
    const bridge = window.AndroidSafBridge;
    if (bridge && !req.folder.includes("Anime_SAF_Demo")) {
      const rawJson = bridge.listTreeEntries(req.folder, req.recursive);
      const parsed = JSON.parse(rawJson) as VirtualEntry[] | { error: string };
      if (!Array.isArray(parsed)) {
        throw new Error(parsed.error || "Failed to read Android SAF folder tree");
      }
      entries = parsed;
    }
    return callBackend<RenamePlan>("scan_saf_tree", {
      rootUri: req.folder,
      entries,
      mode: req.mode,
      cleanTags: req.cleanTags,
      recursive: req.recursive,
      offline: req.offline,
      forceRecheck: req.forceRecheck,
    });
  }

  return callBackend<RenamePlan>("scan_directory", {
    folder: req.folder,
    mode: req.mode,
    cleanTags: req.cleanTags,
    recursive: req.recursive,
    offline: req.offline,
    forceRecheck: req.forceRecheck,
  });
}

export async function applySelected(
  selectedIndices: number[],
  currentPlan: RenamePlan | null
): Promise<ApplyReport> {
  if (currentPlan && isSafUri(currentPlan.root_dir)) {
    const bridge = window.AndroidSafBridge;
    if (!bridge || currentPlan.root_dir.includes("Anime_SAF_Demo")) {
      return callBackend<ApplyReport>("apply_saf_demo_selected", { selectedIndices });
    }

    const selected = new Set(selectedIndices);
    const targets = currentPlan.items
      .filter((i) => i.status === "ready_to_rename" && selected.has(i.index))
      .sort((a, b) => b.depth - a.depth || a.index - b.index);

    const outcomes: SafRenameOutcome[] = [];
    const errors: string[] = [];

    for (const item of targets) {
      const res = JSON.parse(
        bridge!.renameDocument(item.current_path, item.proposed_name)
      ) as { ok: boolean; new_uri?: string; error?: string };

      if (res.ok && res.new_uri) {
        outcomes.push({
          old_uri: item.current_path,
          new_uri: res.new_uri,
          parent_uri: item.parent_dir,
          old_name: item.current_name,
          new_name: item.proposed_name,
          is_dir: item.is_dir,
          anilist_id: item.anilist_id,
          resolution_source: item.resolution_source,
        });
      } else {
        errors.push(
          `Failed SAF rename '${item.current_name}' -> '${item.proposed_name}': ${res.error ?? "unknown error"}`
        );
      }
    }

    return callBackend<ApplyReport>("record_saf_apply", {
      outcomes,
      errors,
      skippedCount: 0,
      cancelled: false,
    });
  }

  return callBackend<ApplyReport>("apply_selected", { selectedIndices });
}

export async function cancelActiveJob(): Promise<void> {
  return callBackend<void>("cancel_active_job");
}

export async function undoBatch(batchId: string | null): Promise<UndoReport> {
  const batch = await callBackend<TransactionBatch>("get_batch_for_undo", { batchId });
  if (isSafUri(batch.root_dir)) {
    const bridge = window.AndroidSafBridge;
    if (!bridge || batch.root_dir.includes("Anime_SAF_Demo")) {
      return callBackend<UndoReport>("complete_saf_undo", {
        batchId: batch.batch_id,
        restoredItems: [],
        errors: [],
      });
    }

    const restoredItems: [string, string][] = [];
    const errors: string[] = [];
    for (const op of [...batch.operations].reverse()) {
      const res = JSON.parse(bridge.renameDocument(op.new_path, op.old_name)) as {
        ok: boolean;
        new_uri?: string;
        error?: string;
      };
      if (res.ok) {
        restoredItems.push([op.new_name, op.old_name]);
      } else {
        errors.push(
          `Failed SAF revert '${op.new_name}' -> '${op.old_name}': ${res.error ?? "unknown error"}`
        );
      }
    }

    return callBackend<UndoReport>("complete_saf_undo", {
      batchId: batch.batch_id,
      restoredItems,
      errors,
    });
  }

  return callBackend<UndoReport>("undo_batch", { batchId });
}

export async function getTransactionHistory(): Promise<TransactionBatch[]> {
  return callBackend<TransactionBatch[]>("get_transaction_history");
}

export async function searchAniListManual(query: string): Promise<MediaCandidate[]> {
  return callBackend<MediaCandidate[]>("search_anilist_manual", { query });
}

export async function saveManualOverride(params: {
  queryOrName: string;
  englishTitle: string;
  romajiTitle: string | null;
  anilistId: number | null;
}): Promise<void> {
  return callBackend<void>("save_manual_override", params);
}

export async function saveCleanupRules(rules: CleanupRules): Promise<void> {
  return callBackend<void>("save_cleanup_rules", { rules });
}

export async function resetCleanupRules(): Promise<CleanupRules> {
  return callBackend<CleanupRules>("reset_cleanup_rules");
}

export async function openDataDir(): Promise<void> {
  return callBackend<void>("open_data_dir");
}

export function onProgress(cb: (ev: ProgressEvent) => void): Promise<UnlistenFn> {
  if (window.AndroidNativeBackend) {
    window.__onBackendProgress = cb;
    return Promise.resolve(() => {
      window.__onBackendProgress = undefined;
    });
  }
  return listen<ProgressEvent>("renamer://progress", (event) => cb(event.payload));
}
