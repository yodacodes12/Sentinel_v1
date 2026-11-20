//! Types shared by the discovery engine, the daemon, the IPC protocol and
//! the frontend (mirrored in `apps/desktop/src/types/documents.ts`).
//!
//! Two dimensions are kept apart everywhere: **source trust** (who
//! published a document) and **hardware match** (whether it is for this
//! board). Neither implies the other.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

// ------------------------------------------------------------------ identity

/// One identifier as firmware reported it, with its normalised form.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IdentityValue {
    /// Exactly as detected. Never altered.
    pub original: String,
    /// Upper-case, separators removed (`INSPIRON153535`); used for comparison.
    pub key: String,
    /// Alphanumeric tokens, upper-case (`["INSPIRON", "15", "3535"]`).
    pub tokens: Vec<String>,
    /// Where it was read, e.g. `/sys/class/dmi/id/board_name`.
    pub source: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Manufacturer {
    /// As detected, e.g. `Dell Inc.`.
    pub original: String,
    /// Common name, e.g. `Dell`; the original when the vendor is not known.
    pub canonical: String,
    /// Stable key, e.g. `dell`.
    pub key: String,
    /// True when the vendor is in Sentinel's table (naming variations and
    /// official domains are known).
    pub recognised: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Completeness {
    /// Manufacturer, system model and board identifier.
    Full,
    /// Manufacturer and board identifier, no usable system model.
    BoardOnly,
    /// Manufacturer and system model, no usable board identifier.
    SystemOnly,
    /// Not enough to search without risking unrelated results.
    Insufficient,
}

/// The motherboard identity used for discovery. Built only from non-sensitive
/// identifiers (no serial numbers, UUIDs, SKUs or BIOS versions).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SearchIdentity {
    pub manufacturer: Option<Manufacturer>,
    pub board_manufacturer: Option<Manufacturer>,
    pub system_model: Option<IdentityValue>,
    pub system_family: Option<IdentityValue>,
    pub board_name: Option<IdentityValue>,
    pub board_revision: Option<IdentityValue>,
    /// Digit-bearing tokens of the system model (`["15", "3535"]`); the
    /// longest is the distinctive model number.
    pub model_numbers: Vec<String>,
    pub completeness: Completeness,
    /// Plain-language notes: what was missing, dropped or contradictory.
    pub notes: Vec<String>,
    /// SHA-256 of the normalised identity; the cache key.
    pub identity_hash: String,
    /// Firmware's own judgement of the board identification (Phase 2).
    pub firmware_level: String,
}

impl SearchIdentity {
    /// The longest digit-bearing token of the system model, e.g. `3535`.
    pub fn distinctive_number(&self) -> Option<&str> {
        self.model_numbers
            .iter()
            .filter(|t| t.len() >= 3)
            .max_by_key(|t| t.len())
            .map(String::as_str)
    }
}

// ------------------------------------------------------------------ queries

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QueryPurpose {
    Schematic,
    Boardview,
    ServiceManual,
    Revision,
    Official,
    /// The identifier alone, for search services that require every word:
    /// document-type words would exclude documents titled differently.
    Identity,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SearchQuery {
    pub text: String,
    pub purpose: QueryPurpose,
    /// Identity fields the query uses, e.g. `["manufacturer", "board_name"]`.
    pub basis: Vec<String>,
}

// ---------------------------------------------------------------- documents

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DocumentType {
    Schematic,
    Boardview,
    ServiceManual,
    HardwareManual,
    SpecificationSheet,
    FirmwareDocumentation,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FileFormat {
    Pdf,
    Zip,
    /// A boardview file recognised by extension (`.brd`, `.bdv`, `.fz`, ...).
    Boardview,
    /// A web page about the document, not the document itself.
    WebPage,
    Unknown,
}

/// How the format was established.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FormatBasis {
    /// Content bytes were checked (downloaded or local files).
    Content,
    /// Only the URL or filename extension; not trusted on its own.
    Extension,
    /// Only a server's `Content-Type`; not trusted on its own.
    ContentType,
    Unknown,
}

// ------------------------------------------------------------------ sources

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceCategory {
    /// Category A: the manufacturer's own sites.
    Manufacturer,
    /// Category B: authorised or otherwise legitimate technical sources.
    Authorized,
    /// Category C: community-maintained resources.
    Community,
    /// A file the user put in Sentinel's document library.
    UserLibrary,
    Unknown,
}

/// Who published the document. Says nothing about which hardware it is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceTrust {
    /// Hosted on a domain of the detected manufacturer.
    OfficialThisManufacturer,
    /// Hosted on a domain of a different manufacturer.
    OfficialOtherManufacturer,
    /// A known community site; uploads are not verified by Sentinel.
    Community,
    /// Supplied by the user; Sentinel does not know where it came from.
    UserProvided,
    Unverified,
}

/// Whether Sentinel may retrieve the document, and why not.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum AccessStatus {
    /// A public HTTPS document on a source whose terms Sentinel treats as
    /// permitting a user-requested download.
    Downloadable,
    /// Discoverable, but only the reference is kept.
    ReferenceOnly { reason: String },
    /// Already on this computer (the user's library).
    LocalFile,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceInfo {
    pub provider_id: String,
    pub provider_name: String,
    /// Publisher or site name, e.g. `Dell`, `Internet Archive`.
    pub source_name: String,
    pub domain: Option<String>,
    pub page_url: Option<String>,
    pub document_url: Option<String>,
    pub category: SourceCategory,
    pub trust: SourceTrust,
    pub trust_reason: String,
    pub access: AccessStatus,
    pub usage_note: Option<String>,
}

// ----------------------------------------------------------------- matching

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MatchClass {
    /// Model (or board) and revision match; nothing contradicts.
    Exact,
    /// Model or board matches; revision unknown or unspecified.
    HighConfidence,
    /// Same family or same model with another revision: not this board.
    RelatedModel,
    /// Some identifiers match; not enough for more.
    Possible,
    /// No identifying evidence either way.
    Unverified,
    /// The evidence says it is for different hardware.
    Mismatch,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Effect {
    Supports,
    Contradicts,
    /// Neither: context (an ODM name, several models covered).
    Neutral,
}

/// Where in the candidate a signal was found.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceLocation {
    Title,
    Filename,
    Url,
    Description,
    /// Structured fields of a catalogue entry.
    Catalogue,
    /// The PDF's own document information (title, subject).
    DocumentMetadata,
    /// The domain the document is hosted on.
    Host,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Indicator {
    Manufacturer,
    OtherManufacturer,
    SystemModel,
    ModelNumber,
    OtherModelNumber,
    BoardIdentifier,
    OtherBoardIdentifier,
    Revision,
    OtherRevision,
    Family,
    OtherModel,
    Designer,
    SeveralModels,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Evidence {
    pub signal: Indicator,
    pub effect: Effect,
    pub location: EvidenceLocation,
    /// The detected value compared, e.g. `0N90HF`.
    pub detected: Option<String>,
    /// What the document says, e.g. `0N90HF` or `3530`.
    pub found: Option<String>,
    /// Points contributed (documented in docs/DOCUMENT_DISCOVERY.md).
    pub weight: i32,
    pub note: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RevisionCompatibility {
    Matches,
    Conflicts,
    /// The board revision is known but the document does not state one.
    DocumentUnspecified,
    /// Firmware did not report a board revision.
    DeviceUnknown,
    /// Not established: the document is not for this model.
    NotApplicable,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MatchAssessment {
    pub class: MatchClass,
    pub evidence: Vec<Evidence>,
    /// Sum of supporting weights.
    pub support_points: i32,
    /// Sum of contradicting weights (as a positive number).
    pub contradiction_points: i32,
    /// Identity fields that could not be compared.
    pub missing: Vec<String>,
    pub revision: RevisionCompatibility,
    pub explanation: String,
    pub manual_verification_recommended: bool,
}

// --------------------------------------------------------------- candidates

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum RetrievalStatus {
    NotDownloaded,
    ReferenceOnly {
        reason: String,
    },
    /// Stored as `file` in Sentinel's download directory.
    Downloaded {
        file: String,
        sha256: String,
        size_bytes: u64,
        at: u64,
    },
    Failed {
        reason: String,
        at: u64,
    },
    /// A file in the user's library; never copied or moved.
    InLibrary {
        file: String,
        size_bytes: u64,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DocumentCandidate {
    /// Database row ID; 0 until saved.
    pub id: u64,
    /// De-duplication key (normalised document or page URL, or library file).
    pub key: String,
    pub title: String,
    /// Original filename, sanitised; metadata only, never used as a path.
    pub filename: Option<String>,
    pub extension: Option<String>,
    pub mime_type: Option<String>,
    pub size_bytes: Option<u64>,
    pub doc_type: DocumentType,
    pub format: FileFormat,
    pub format_basis: FormatBasis,
    pub source: SourceInfo,
    pub assessment: MatchAssessment,
    pub retrieval: RetrievalStatus,
    pub discovered_at: u64,
    pub verified_at: u64,
    /// Basic metadata (description, PDF title, query that found it, ...).
    pub metadata: BTreeMap<String, String>,
}

// ----------------------------------------------------------------- sessions

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Stage {
    PreparingIdentity,
    GeneratingQueries,
    SearchingProviders,
    CollectingCandidates,
    IdentifyingTypes,
    VerifyingMatches,
    SavingResults,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderState {
    Ok,
    Empty,
    Failed,
    /// Online provider while online discovery is off.
    SkippedOffline,
    /// Not usable as configured (e.g. no catalogue URL).
    NotConfigured,
    Cancelled,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderOutcome {
    pub provider_id: String,
    pub provider_name: String,
    pub state: ProviderState,
    pub results: u32,
    pub requests: u32,
    pub duration_ms: u64,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionStatus {
    Running,
    Completed,
    /// Some providers failed; the others' results were kept.
    Partial,
    Failed,
    Cancelled,
    /// The daemon stopped while it was running.
    Interrupted,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionRecord {
    pub id: u64,
    pub identity_hash: String,
    pub started_at: u64,
    pub finished_at: Option<u64>,
    pub status: SessionStatus,
    pub online: bool,
    pub queries: Vec<SearchQuery>,
    pub providers: Vec<ProviderOutcome>,
    pub candidates: u32,
    pub error: Option<String>,
}

/// Provider description for the UI.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderInfo {
    pub id: String,
    pub name: String,
    pub category: SourceCategory,
    pub online: bool,
    pub enabled: bool,
    pub configured: bool,
    pub description: String,
}

// ------------------------------------------------------------- daemon view

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JobKind {
    Search,
    Download,
}

/// What the document coordinator is doing right now.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DocumentProgress {
    pub running: bool,
    pub kind: Option<JobKind>,
    pub stage: Option<Stage>,
    pub started_at: Option<u64>,
    pub session_id: Option<u64>,
    pub document_id: Option<u64>,
    pub cancel_requested: bool,
}

/// Everything the documentation page shows, in one answer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DocumentsView {
    pub progress: DocumentProgress,
    pub identity: Option<SearchIdentity>,
    /// Why there is no usable identity (no scan yet, no motherboard).
    pub identity_problem: Option<String>,
    pub online: bool,
    pub auto_download: bool,
    pub providers: Vec<ProviderInfo>,
    pub candidates: Vec<DocumentCandidate>,
    pub sessions: Vec<SessionRecord>,
    pub last_search_at: Option<u64>,
    /// When cached results stop being reused by a normal search.
    pub cache_expires_at: Option<u64>,
    pub storage_available: bool,
    pub library_dir: String,
    pub download_dir: String,
    /// The outcome of the last request (cache reused, download finished...).
    pub notice: Option<String>,
}
