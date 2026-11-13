export type NamingMode = "english-only" | "prepend" | "append";

export type PlanItemStatus =
  | "ready_to_rename"
  | "already_clean"
  | "already_processed"
  | "conflict"
  | "skipped_empty_query";

export type ResolutionSource =
  | "none"
  | "already_processed"
  | "manual_override"
  | "cache_hit"
  | "api_match"
  | "inline_alias"
  | "no_match";

export interface ParsedItem {
  raw_name: string;
  is_dir: boolean;
  extension: string;
  existing_prepended_title: string | null;
  base_title: string;
  inline_alias: string | null;
  search_query: string;
  fallback_queries: string[];
  year_spec: string | null;
  season_spec: string | null;
  episode_spec: string | null;
  extra_notes: string[];
  stripped_tags: string[];
}

export interface MediaCandidate {
  id: number;
  romaji: string | null;
  english: string | null;
  native: string | null;
  synonyms: string[];
  season_year: number | null;
  format: string | null;
}

export interface PlanItem {
  index: number;
  is_dir: boolean;
  depth: number;
  parent_dir: string;
  current_path: string;
  current_name: string;
  proposed_name: string;
  proposed_path: string;
  parsed: ParsedItem;
  effective_query: string;
  anilist_id: number | null;
  resolved_english: string | null;
  resolved_romaji: string | null;
  candidates: MediaCandidate[];
  resolution_source: ResolutionSource;
  status: PlanItemStatus;
  status_reason: string | null;
}

export interface RenamePlan {
  root_dir: string;
  data_dir: string;
  mode: NamingMode;
  clean_tags: boolean;
  items: PlanItem[];
  api_requests_made: number;
}

export interface ApplyReport {
  batch_id: string;
  renamed_count: number;
  skipped_count: number;
  error_count: number;
  cancelled: boolean;
  errors: string[];
}

export interface UndoReport {
  batch_id: string;
  restored_count: number;
  error_count: number;
  restored_items: [string, string][];
  errors: string[];
}

export interface TransactionOp {
  old_path: string;
  new_path: string;
  parent_dir: string;
  old_name: string;
  new_name: string;
  is_dir: boolean;
  anilist_id: number | null;
  outcome: string;
}

export interface TransactionBatch {
  batch_id: string;
  created_at_unix: number;
  root_dir: string;
  mode: NamingMode;
  clean_tags: boolean;
  reverted: boolean;
  operations: TransactionOp[];
}

export interface CleanupRules {
  version: string;
  ignore_names: string[];
  media_extensions: string[];
  subtitle_lang_codes: string[];
  noise_tokens: string[];
  batch_keywords: string[];
  trailing_group_tags: string[];
  extra_note_keywords: string[];
}

export interface InitialState {
  data_dir: string;
  rules: CleanupRules;
  history: TransactionBatch[];
}

export interface ProgressEvent {
  phase: "scan" | "apply" | "search";
  message: string;
}

export interface VirtualEntry {
  uri: string;
  parent_uri: string;
  name: string;
  is_dir: boolean;
  depth: number;
}

export interface SafRenameOutcome {
  old_uri: string;
  new_uri: string;
  parent_uri: string;
  old_name: string;
  new_name: string;
  is_dir: boolean;
  anilist_id: number | null;
  resolution_source: ResolutionSource;
}

export interface AndroidSafBridge {
  pickDocumentTree(): void;
  listTreeEntries(treeUriString: string, recursive: boolean): string;
  renameDocument(documentUriString: string, newDisplayName: string): string;
}

export interface AndroidNativeBackend {
  invoke(cmd: string, argsJson: string): string;
}

declare global {
  interface Window {
    AndroidSafBridge?: AndroidSafBridge;
    AndroidNativeBackend?: AndroidNativeBackend;
    __onSafTreePicked?: (uri: string | null) => void;
    __onBackendProgress?: (ev: ProgressEvent) => void;
  }
}
