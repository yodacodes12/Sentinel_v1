//! Source providers. Each provider independently submits its searches,
//! reads results, identifies its source, extracts metadata and validates
//! URLs; rate limiting, robots.txt and address checks come from the shared
//! [`Client`](crate::net::Client). A failing provider never stops the others.

use crate::matching::Claims;
use crate::model::{FileFormat, FormatBasis, ProviderInfo, SearchIdentity, SearchQuery};
use crate::net::{Cancel, Client, NetError};

/// A result as a provider found it, before classification and matching.
#[derive(Debug, Clone, Default)]
pub struct RawCandidate {
    pub title: String,
    pub document_url: Option<String>,
    pub page_url: Option<String>,
    pub filename: Option<String>,
    pub mime_type: Option<String>,
    pub size_bytes: Option<u64>,
    pub description: Option<String>,
    /// Title from the document's own metadata (local PDFs).
    pub document_title: Option<String>,
    /// Structured claims (catalogue entries).
    pub claims: Option<Claims>,
    /// Publisher named by the source (informational; trust is decided from
    /// the host, not from this).
    pub publisher: Option<String>,
    /// A file in the user's library: name and size.
    pub library_file: Option<(String, u64)>,
    /// Format established by the provider (content-checked for local files).
    pub format: Option<(FileFormat, FormatBasis)>,
    pub query: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ProviderError {
    #[error(transparent)]
    Net(#[from] NetError),
    #[error("unexpected response: {0}")]
    InvalidResponse(String),
    #[error("not configured: {0}")]
    NotConfigured(String),
    #[error("{0}")]
    Io(String),
}

pub struct SearchContext<'a> {
    pub identity: &'a SearchIdentity,
    pub queries: &'a [SearchQuery],
    /// `None` when online discovery is off.
    pub client: Option<&'a Client>,
    pub cancel: &'a Cancel,
}

pub trait Provider: Send + Sync {
    fn info(&self) -> ProviderInfo;
    fn search(&self, ctx: &SearchContext) -> Result<Vec<RawCandidate>, ProviderError>;
}
