// Mirrors crates/sentinel-docs/src/model.rs (Phase 6 document discovery).

export type Completeness = "full" | "board_only" | "system_only" | "insufficient";

export interface IdentityValue {
  original: string;
  key: string;
  tokens: string[];
  source: string | null;
}

export interface Manufacturer {
  original: string;
  canonical: string;
  key: string;
  recognised: boolean;
}

export interface SearchIdentity {
  manufacturer: Manufacturer | null;
  board_manufacturer: Manufacturer | null;
  system_model: IdentityValue | null;
  system_family: IdentityValue | null;
  board_name: IdentityValue | null;
  board_revision: IdentityValue | null;
  model_numbers: string[];
  completeness: Completeness;
  notes: string[];
  identity_hash: string;
  firmware_level: string;
}

export type QueryPurpose = "schematic" | "boardview" | "service_manual" | "revision" | "official" | "identity";

export interface SearchQuery {
  text: string;
  purpose: QueryPurpose;
  basis: string[];
}

export type DocumentType = "schematic" | "boardview" | "service_manual" | "hardware_manual" | "specification_sheet" | "firmware_documentation" | "unknown";
export type FileFormat = "pdf" | "zip" | "boardview" | "web_page" | "unknown";
export type FormatBasis = "content" | "extension" | "content_type" | "unknown";
export type SourceCategory = "manufacturer" | "authorized" | "community" | "user_library" | "unknown";
export type SourceTrust = "official_this_manufacturer" | "official_other_manufacturer" | "community" | "user_provided" | "unverified";

export type AccessStatus = { state: "downloadable" } | { state: "reference_only"; reason: string } | { state: "local_file" };

export interface SourceInfo {
  provider_id: string;
  provider_name: string;
  source_name: string;
  domain: string | null;
  page_url: string | null;
  document_url: string | null;
  category: SourceCategory;
  trust: SourceTrust;
  trust_reason: string;
  access: AccessStatus;
  usage_note: string | null;
}

export type MatchClass = "exact" | "high_confidence" | "related_model" | "possible" | "unverified" | "mismatch";
export type Effect = "supports" | "contradicts" | "neutral";
export type EvidenceLocation = "title" | "filename" | "url" | "description" | "catalogue" | "document_metadata" | "host";
export type Indicator =
  | "manufacturer"
  | "other_manufacturer"
  | "system_model"
  | "model_number"
  | "other_model_number"
  | "board_identifier"
  | "other_board_identifier"
  | "revision"
  | "other_revision"
  | "family"
  | "other_model"
  | "designer"
  | "several_models";

export interface Evidence {
  signal: Indicator;
  effect: Effect;
  location: EvidenceLocation;
  detected: string | null;
  found: string | null;
  weight: number;
  note: string;
}

export type RevisionCompatibility = "matches" | "conflicts" | "document_unspecified" | "device_unknown" | "not_applicable";

export interface MatchAssessment {
  class: MatchClass;
  evidence: Evidence[];
  support_points: number;
  contradiction_points: number;
  missing: string[];
  revision: RevisionCompatibility;
  explanation: string;
  manual_verification_recommended: boolean;
}

export type RetrievalStatus =
  | { state: "not_downloaded" }
  | { state: "reference_only"; reason: string }
  | { state: "downloaded"; file: string; sha256: string; size_bytes: number; at: number }
  | { state: "failed"; reason: string; at: number }
  | { state: "in_library"; file: string; size_bytes: number };

export interface DocumentCandidate {
  id: number;
  key: string;
  title: string;
  filename: string | null;
  extension: string | null;
  mime_type: string | null;
  size_bytes: number | null;
  doc_type: DocumentType;
  format: FileFormat;
  format_basis: FormatBasis;
  source: SourceInfo;
  assessment: MatchAssessment;
  retrieval: RetrievalStatus;
  discovered_at: number;
  verified_at: number;
  metadata: Record<string, string>;
}

export type Stage = "preparing_identity" | "generating_queries" | "searching_providers" | "collecting_candidates" | "identifying_types" | "verifying_matches" | "saving_results";
export type ProviderState = "ok" | "empty" | "failed" | "skipped_offline" | "not_configured" | "cancelled";
export type SessionStatus = "running" | "completed" | "partial" | "failed" | "cancelled" | "interrupted";

export interface ProviderOutcome {
  provider_id: string;
  provider_name: string;
  state: ProviderState;
  results: number;
  requests: number;
  duration_ms: number;
  error: string | null;
}

export interface SessionRecord {
  id: number;
  identity_hash: string;
  started_at: number;
  finished_at: number | null;
  status: SessionStatus;
  online: boolean;
  queries: SearchQuery[];
  providers: ProviderOutcome[];
  candidates: number;
  error: string | null;
}

export interface ProviderInfo {
  id: string;
  name: string;
  category: SourceCategory;
  online: boolean;
  enabled: boolean;
  configured: boolean;
  description: string;
}

export interface DocumentProgress {
  running: boolean;
  kind: "search" | "download" | null;
  stage: Stage | null;
  started_at: number | null;
  session_id: number | null;
  document_id: number | null;
  cancel_requested: boolean;
}

export interface DocumentsView {
  progress: DocumentProgress;
  identity: SearchIdentity | null;
  identity_problem: string | null;
  online: boolean;
  auto_download: boolean;
  providers: ProviderInfo[];
  candidates: DocumentCandidate[];
  sessions: SessionRecord[];
  last_search_at: number | null;
  cache_expires_at: number | null;
  storage_available: boolean;
  library_dir: string;
  download_dir: string;
  notice: string | null;
}

export type OpenTarget = "source_page" | "document_address" | "local_copy";

export interface DocumentSettings {
  online_discovery: boolean;
  internet_archive: boolean;
  catalogue_urls: string[];
  request_timeout_s: number;
  min_request_interval_ms: number;
  cache_days: number;
  max_download_mb: number;
  auto_download: boolean;
}
