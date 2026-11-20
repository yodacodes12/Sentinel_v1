//! Source trust: who published a document. Independent of whether the
//! document is for this board (see `matching`).
//!
//! Trust comes from the host the document is served from, checked against
//! Sentinel's table of manufacturer domains, never from what a catalogue or
//! web page claims about itself.

use crate::model::{AccessStatus, SearchIdentity, SourceCategory, SourceTrust};
use crate::vendors;

pub struct Trust {
    pub category: SourceCategory,
    pub trust: SourceTrust,
    pub source_name: String,
    pub reason: String,
    pub access: AccessStatus,
    pub usage_note: Option<String>,
}

/// Trust of a remote document hosted on `host`.
pub fn assess_host(host: Option<&str>, identity: &SearchIdentity, https: bool) -> Trust {
    let Some(host) = host else {
        return Trust {
            category: SourceCategory::Unknown,
            trust: SourceTrust::Unverified,
            source_name: "Unknown source".into(),
            reason: "The document has no web address.".into(),
            access: AccessStatus::ReferenceOnly {
                reason: "No address to retrieve it from.".into(),
            },
            usage_note: None,
        };
    };
    let ours = identity.manufacturer.as_ref().map(|m| m.key.as_str());
    let board_ours = identity.board_manufacturer.as_ref().map(|m| m.key.as_str());
    if let Some(v) = vendors::vendor_for_host(host) {
        let mine = Some(v.key) == ours || Some(v.key) == board_ours;
        return Trust {
            category: SourceCategory::Manufacturer,
            trust: if mine {
                SourceTrust::OfficialThisManufacturer
            } else {
                SourceTrust::OfficialOtherManufacturer
            },
            source_name: v.canonical.into(),
            reason: if mine {
                format!(
                    "Served from {host}, a {} domain. This says who published it, not which board it is for.",
                    v.canonical
                )
            } else {
                format!(
                    "Served from {host}, a {} domain, not the detected manufacturer's.",
                    v.canonical
                )
            },
            access: if https {
                AccessStatus::Downloadable
            } else {
                AccessStatus::ReferenceOnly {
                    reason: "Not served over HTTPS.".into(),
                }
            },
            usage_note: Some(format!(
                "Published by {}. Download only for your own reference; check the manufacturer's terms before sharing it.",
                v.canonical
            )),
        };
    }
    if let Some(name) = vendors::community_site(host) {
        return Trust {
            category: SourceCategory::Community,
            trust: SourceTrust::Community,
            source_name: name.into(),
            reason: format!("Hosted on {name} ({host}), where anyone can upload. Sentinel cannot verify who published it or whether it may be shared."),
            access: AccessStatus::ReferenceOnly {
                reason: "Community upload with unverified rights: Sentinel keeps the reference only. Open the source page to review it and its terms.".into(),
            },
            usage_note: Some("Rights to this document are not verified.".into()),
        };
    }
    Trust {
        category: SourceCategory::Unknown,
        trust: SourceTrust::Unverified,
        source_name: host.to_string(),
        reason: format!("{host} is not a known manufacturer or community site."),
        access: AccessStatus::ReferenceOnly {
            reason: "Unknown source: Sentinel keeps the reference only.".into(),
        },
        usage_note: None,
    }
}

/// Trust of a file the user placed in the document library.
pub fn user_library() -> Trust {
    Trust {
        category: SourceCategory::UserLibrary,
        trust: SourceTrust::UserProvided,
        source_name: "Your document library".into(),
        reason: "You added this file to Sentinel's document library. Sentinel does not know where it came from.".into(),
        access: AccessStatus::LocalFile,
        usage_note: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::identity::tests::dell;

    #[test]
    fn official_other_community_unknown() {
        let id = dell();
        assert_eq!(
            assess_host(Some("dl.dell.com"), &id, true).trust,
            SourceTrust::OfficialThisManufacturer
        );
        let other = assess_host(Some("download.lenovo.com"), &id, true);
        assert_eq!(other.trust, SourceTrust::OfficialOtherManufacturer);
        assert_eq!(other.category, SourceCategory::Manufacturer);
        let ia = assess_host(Some("archive.org"), &id, true);
        assert_eq!(ia.trust, SourceTrust::Community);
        assert!(
            matches!(ia.access, AccessStatus::ReferenceOnly { .. }),
            "community uploads are never downloaded"
        );
        assert_eq!(
            assess_host(Some("dell.com.files.example"), &id, true).trust,
            SourceTrust::Unverified
        );
        assert!(matches!(
            assess_host(Some("dell.com"), &id, false).access,
            AccessStatus::ReferenceOnly { .. }
        ));
    }
}
