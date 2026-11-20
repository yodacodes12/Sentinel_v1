//! The on-disk form of cached scans.
//!
//! A cache entry is a JSON document stored by `sentinel-storage`. It is
//! accepted only if its schema version matches this build exactly; anything
//! else is reported as stale and ignored rather than guessed at.

use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::hardware::{INVENTORY_SCHEMA_VERSION, SystemInventory};
use crate::utilities::{UTILITY_SCHEMA_VERSION, UtilityCatalog};

/// Cache keys in the storage table.
pub const HARDWARE_CACHE_KEY: &str = "hardware_inventory";
pub const UTILITIES_CACHE_KEY: &str = "utility_catalog";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CacheProblem {
    /// Written by a different version of the inventory format.
    SchemaMismatch { found: u32, expected: u32 },
    /// Not a valid document (corrupt, truncated, or violates an invariant).
    Unreadable(String),
}

impl std::fmt::Display for CacheProblem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CacheProblem::SchemaMismatch { found, expected } => write!(
                f,
                "cache uses inventory format {found}; this Sentinel uses {expected}"
            ),
            CacheProblem::Unreadable(reason) => write!(f, "cache is unreadable: {reason}"),
        }
    }
}

/// Something that can be cached: it knows its key and schema version.
pub trait Cacheable: Serialize + DeserializeOwned {
    const KEY: &'static str;
    const SCHEMA_VERSION: u32;
    fn schema_version(&self) -> u32;
    fn discovered_at(&self) -> u64;
}

impl Cacheable for SystemInventory {
    const KEY: &'static str = HARDWARE_CACHE_KEY;
    const SCHEMA_VERSION: u32 = INVENTORY_SCHEMA_VERSION;
    fn schema_version(&self) -> u32 {
        self.schema_version
    }
    fn discovered_at(&self) -> u64 {
        self.discovered_at
    }
}

impl Cacheable for UtilityCatalog {
    const KEY: &'static str = UTILITIES_CACHE_KEY;
    const SCHEMA_VERSION: u32 = UTILITY_SCHEMA_VERSION;
    fn schema_version(&self) -> u32 {
        self.schema_version
    }
    fn discovered_at(&self) -> u64 {
        self.discovered_at
    }
}

pub fn encode<T: Cacheable>(value: &T) -> String {
    serde_json::to_string(value).expect("inventory types serialise")
}

/// Decode a cache document stored with `stored_version`.
pub fn decode<T: Cacheable>(stored_version: u32, payload: &str) -> Result<T, CacheProblem> {
    if stored_version != T::SCHEMA_VERSION {
        return Err(CacheProblem::SchemaMismatch {
            found: stored_version,
            expected: T::SCHEMA_VERSION,
        });
    }
    let value: T =
        serde_json::from_str(payload).map_err(|e| CacheProblem::Unreadable(e.to_string()))?;
    if value.schema_version() != T::SCHEMA_VERSION {
        return Err(CacheProblem::SchemaMismatch {
            found: value.schema_version(),
            expected: T::SCHEMA_VERSION,
        });
    }
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn empty_catalog() -> UtilityCatalog {
        UtilityCatalog {
            schema_version: UTILITY_SCHEMA_VERSION,
            discovered_at: 1_700_000_000,
            duration_ms: 5,
            root_override: None,
            utilities: vec![],
            services: vec![],
            package_managers: vec![],
            discovery_status: vec![],
        }
    }

    #[test]
    fn round_trip() {
        let catalog = empty_catalog();
        let text = encode(&catalog);
        let back: UtilityCatalog = decode(UTILITY_SCHEMA_VERSION, &text).unwrap();
        assert_eq!(back, catalog);
    }

    #[test]
    fn other_schema_versions_are_refused() {
        let text = encode(&empty_catalog());
        assert_eq!(
            decode::<UtilityCatalog>(UTILITY_SCHEMA_VERSION + 1, &text).unwrap_err(),
            CacheProblem::SchemaMismatch {
                found: UTILITY_SCHEMA_VERSION + 1,
                expected: UTILITY_SCHEMA_VERSION
            }
        );
        let mut catalog = empty_catalog();
        catalog.schema_version = 99;
        assert!(matches!(
            decode::<UtilityCatalog>(UTILITY_SCHEMA_VERSION, &encode(&catalog)),
            Err(CacheProblem::SchemaMismatch { found: 99, .. })
        ));
    }

    #[test]
    fn corrupt_documents_are_refused() {
        assert!(matches!(
            decode::<UtilityCatalog>(UTILITY_SCHEMA_VERSION, "{\"schema_version\":1"),
            Err(CacheProblem::Unreadable(_))
        ));
    }
}
