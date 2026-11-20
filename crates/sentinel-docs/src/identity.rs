//! Motherboard identity for discovery, built from Phase 2's `BoardIdentity`.
//!
//! Original values are kept verbatim next to their normalised forms. Only
//! non-sensitive identifiers are used: the SKU, BIOS version, serial numbers
//! and UUIDs are never part of the identity (and so never part of a query).

use sentinel_inventory::{Field, IdentificationLevel, SystemInventory};
use sha2::{Digest, Sha256};

use crate::model::{Completeness, IdentityValue, Manufacturer, SearchIdentity};
use crate::vendors;

/// Values firmware uses instead of a real identifier.
const PLACEHOLDERS: &[&str] = &[
    "TOBEFILLEDBYOEM",
    "DEFAULTSTRING",
    "SYSTEMPRODUCTNAME",
    "SYSTEMMANUFACTURER",
    "SYSTEMVERSION",
    "BASEBOARDPRODUCTNAME",
    "TYPE2BOARDPRODUCTNAME1",
    "NOTAPPLICABLE",
    "NA",
    "NONE",
    "XX",
    "XXX",
    "0",
    "OEM",
    "INVALID",
    "UNKNOWN",
    "NOTSPECIFIED",
];

/// Upper-case alphanumeric tokens.
pub fn tokens(text: &str) -> Vec<String> {
    text.split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|t| !t.is_empty())
        .map(str::to_ascii_uppercase)
        .collect()
}

pub fn key_of(text: &str) -> String {
    tokens(text).concat()
}

fn is_placeholder(text: &str) -> bool {
    let key = key_of(text);
    key.is_empty()
        || PLACEHOLDERS.contains(&key.as_str())
        || key.chars().all(|c| c == 'X' || c == '0')
}

fn value(field: &Field<String>) -> Option<IdentityValue> {
    let original = field.value()?.trim().to_string();
    if is_placeholder(&original) {
        return None;
    }
    Some(IdentityValue {
        key: key_of(&original),
        tokens: tokens(&original),
        source: field.source().map(str::to_string),
        original,
    })
}

fn manufacturer(field: &Field<String>) -> Option<Manufacturer> {
    let original = field.value()?.trim().to_string();
    if is_placeholder(&original) {
        return None;
    }
    Some(match vendors::vendor_for(&original) {
        Some(v) => Manufacturer {
            canonical: v.canonical.to_string(),
            key: v.key.to_string(),
            recognised: true,
            original,
        },
        None => Manufacturer {
            canonical: original.clone(),
            key: vendors::normalise_name(&original).replace(' ', "-"),
            recognised: false,
            original,
        },
    })
}

/// Lenovo machine-type model (e.g. `20L5CTO1WW`): the marketing name is in
/// `product_version` instead.
fn looks_like_machine_type(key: &str) -> bool {
    (8..=10).contains(&key.len())
        && key[..2].chars().all(|c| c.is_ascii_digit())
        && key.chars().all(|c| c.is_ascii_alphanumeric())
}

/// A board identifier distinctive enough to search for on its own.
pub fn is_distinctive_board_id(v: &IdentityValue) -> bool {
    v.key.len() >= 4
        && v.key.chars().any(|c| c.is_ascii_digit())
        && v.key.chars().any(|c| c.is_ascii_alphabetic())
}

impl SearchIdentity {
    /// The identity of the inventory's motherboard, or why there is none.
    pub fn from_inventory(inv: &SystemInventory) -> Result<SearchIdentity, String> {
        let board = inv
            .motherboard
            .as_ref()
            .ok_or_else(|| "The hardware scan did not identify a motherboard.".to_string())?;
        let mut notes = Vec::new();
        let system_mfr = manufacturer(&board.system_manufacturer);
        let board_mfr = manufacturer(&board.board_manufacturer);
        let mut system_model = value(&board.system_model);
        let system_family = value(&board.system_family);
        let board_name = value(&board.board_name);
        let mut board_revision = value(&board.board_revision);

        let mfr = system_mfr.clone().or_else(|| board_mfr.clone());
        if let (Some(s), Some(b)) = (&system_mfr, &board_mfr)
            && s.key != b.key
        {
            notes.push(format!(
                "Firmware names {} as the system manufacturer and {} as the board manufacturer. Searches use the system manufacturer; documents from either are compared.",
                s.original, b.original
            ));
        }

        // Lenovo reports the machine type as the product name.
        if mfr.as_ref().is_some_and(|m| m.key == "lenovo")
            && let Some(model) = &system_model
            && looks_like_machine_type(&model.key)
        {
            let version = inv.system.as_ref().and_then(|s| value(&s.product_version));
            match version {
                Some(v) => {
                    notes.push(format!(
                        "{} is a Lenovo machine type; the model name {} (product_version) is used.",
                        model.original, v.original
                    ));
                    system_model = Some(v);
                }
                None => notes.push(format!(
                    "{} is a Lenovo machine type and no model name was reported.",
                    model.original
                )),
            }
        }

        // A "revision" equal to the BIOS version is a firmware version.
        if let (Some(rev), Some(bios)) = (&board_revision, board.bios_version.value())
            && rev.key == key_of(bios)
        {
            notes.push(format!(
                "The board version {} equals the BIOS version, so it is not used as a hardware revision.",
                rev.original
            ));
            board_revision = None;
        }
        if board_revision.is_none() {
            notes.push("Firmware does not report a usable board revision, so no document can be confirmed for this exact revision.".into());
        }
        let board_name = match board_name {
            Some(b) if system_model.as_ref().is_some_and(|m| m.key == b.key) => {
                notes.push("The board name equals the system model; it is not a separate board identifier.".into());
                None
            }
            other => other,
        };

        let model_numbers: Vec<String> = system_model
            .as_ref()
            .map(|m| {
                m.tokens
                    .iter()
                    .filter(|t| t.chars().any(|c| c.is_ascii_digit()))
                    .cloned()
                    .collect()
            })
            .unwrap_or_default();
        let has_board = board_name.as_ref().is_some_and(is_distinctive_board_id);
        if board_name.is_some() && !has_board {
            notes.push("The board name is too generic to search for on its own.".into());
        }
        let has_model = system_model.as_ref().is_some_and(|m| !m.tokens.is_empty());
        let completeness = match (&mfr, has_model, has_board) {
            (None, _, _) => {
                notes.push("No manufacturer was reported; searching without one would associate documents from unrelated hardware.".into());
                Completeness::Insufficient
            }
            (Some(_), true, true) => Completeness::Full,
            (Some(_), false, true) => Completeness::BoardOnly,
            (Some(_), true, false) => Completeness::SystemOnly,
            (Some(_), false, false) => {
                notes.push("Neither a system model nor a board identifier was reported.".into());
                Completeness::Insufficient
            }
        };
        let firmware_level = match board.identification.level {
            IdentificationLevel::BoardIdentified => "board_identified",
            IdentificationLevel::SystemOnly => "system_only",
            IdentificationLevel::Unidentified => "unidentified",
        };

        let mut hasher = Sha256::new();
        for part in [
            mfr.as_ref().map(|m| m.key.as_str()),
            system_model.as_ref().map(|v| v.key.as_str()),
            board_mfr.as_ref().map(|m| m.key.as_str()),
            board_name.as_ref().map(|v| v.key.as_str()),
            board_revision.as_ref().map(|v| v.key.as_str()),
        ] {
            hasher.update(part.unwrap_or("-").as_bytes());
            hasher.update(b"|");
        }
        let identity_hash = format!("{:x}", hasher.finalize());

        Ok(SearchIdentity {
            manufacturer: mfr,
            board_manufacturer: board_mfr,
            system_model,
            system_family,
            board_name: board_name.filter(|_| has_board),
            board_revision,
            model_numbers,
            completeness,
            notes,
            identity_hash,
            firmware_level: firmware_level.into(),
        })
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use sentinel_inventory::{Availability, BoardIdentity, Confidence};

    fn some(v: &str) -> Field<String> {
        Field::detected(
            v.to_string(),
            "/sys/class/dmi/id/x",
            Confidence::FirmwareReported,
        )
    }
    fn none() -> Field<String> {
        Field::missing(Availability::Unavailable, None)
    }

    pub fn board(
        sys_mfr: Option<&str>,
        model: Option<&str>,
        family: Option<&str>,
        board_mfr: Option<&str>,
        name: Option<&str>,
        rev: Option<&str>,
    ) -> BoardIdentity {
        let f = |v: Option<&str>| v.map(some).unwrap_or_else(none);
        let (bm, bn, sm) = (f(board_mfr), f(name), f(model));
        BoardIdentity {
            discovered_at: 1,
            identification: BoardIdentity::assess(&bm, &bn, &sm),
            system_manufacturer: f(sys_mfr),
            system_model: sm,
            system_family: f(family),
            system_sku: some("0C3E"),
            board_manufacturer: bm,
            board_name: bn,
            board_revision: f(rev),
            bios_vendor: f(sys_mfr),
            bios_version: some("1.32.0"),
            bios_release_date: some("09/02/2026"),
        }
    }

    pub fn inventory(b: BoardIdentity) -> SystemInventory {
        let json = include_str!("../tests/empty_inventory.json");
        let mut inv: SystemInventory = serde_json::from_str(json).unwrap();
        inv.motherboard = Some(b);
        inv
    }

    pub fn dell() -> SearchIdentity {
        SearchIdentity::from_inventory(&inventory(board(
            Some("Dell Inc."),
            Some("Inspiron 15 3535"),
            Some("Inspiron"),
            Some("Dell Inc."),
            Some("0N90HF"),
            Some("A02"),
        )))
        .unwrap()
    }

    #[test]
    fn exact_identity_keeps_originals_and_normalises() {
        let id = dell();
        let m = id.manufacturer.as_ref().unwrap();
        assert_eq!(
            (m.original.as_str(), m.canonical.as_str(), m.key.as_str()),
            ("Dell Inc.", "Dell", "dell")
        );
        let model = id.system_model.as_ref().unwrap();
        assert_eq!(model.original, "Inspiron 15 3535");
        assert_eq!(model.key, "INSPIRON153535");
        assert_eq!(id.board_name.as_ref().unwrap().key, "0N90HF");
        assert_eq!(id.board_revision.as_ref().unwrap().original, "A02");
        assert_eq!(id.model_numbers, ["15", "3535"]);
        assert_eq!(id.distinctive_number(), Some("3535"));
        assert_eq!(id.completeness, Completeness::Full);
        assert_eq!(id.identity_hash.len(), 64);
        // Nothing sensitive or firmware-versioned in the identity.
        let text = serde_json::to_string(&id).unwrap();
        assert!(!text.contains("0C3E") && !text.contains("1.32.0"));
    }

    #[test]
    fn naming_variations_give_the_same_identity() {
        let a = dell();
        let b = SearchIdentity::from_inventory(&inventory(board(
            Some("DELL INC"),
            Some("Inspiron 15  3535"),
            Some("Inspiron"),
            Some("Dell Inc."),
            Some("0n90hf"),
            Some("a02"),
        )))
        .unwrap();
        assert_eq!(a.identity_hash, b.identity_hash);
        assert_ne!(
            a.system_model.unwrap().original,
            b.system_model.unwrap().original,
            "originals are preserved"
        );
    }

    #[test]
    fn distinct_models_never_merge() {
        let a = dell();
        let b = SearchIdentity::from_inventory(&inventory(board(
            Some("Dell Inc."),
            Some("Inspiron 15 3530"),
            Some("Inspiron"),
            Some("Dell Inc."),
            Some("0N90HF"),
            Some("A02"),
        )))
        .unwrap();
        assert_ne!(a.identity_hash, b.identity_hash);
    }

    #[test]
    fn missing_manufacturer_model_or_revision() {
        let no_mfr = SearchIdentity::from_inventory(&inventory(board(
            None,
            Some("Inspiron 15 3535"),
            None,
            None,
            Some("0N90HF"),
            None,
        )))
        .unwrap();
        assert_eq!(no_mfr.completeness, Completeness::Insufficient);
        let no_model = SearchIdentity::from_inventory(&inventory(board(
            Some("Dell Inc."),
            None,
            None,
            Some("Dell Inc."),
            Some("0N90HF"),
            Some("A02"),
        )))
        .unwrap();
        assert_eq!(no_model.completeness, Completeness::BoardOnly);
        let no_board = SearchIdentity::from_inventory(&inventory(board(
            Some("Dell Inc."),
            Some("Inspiron 15 3535"),
            None,
            None,
            None,
            None,
        )))
        .unwrap();
        assert_eq!(no_board.completeness, Completeness::SystemOnly);
        assert!(no_board.board_revision.is_none());
        assert!(no_board.notes.iter().any(|n| n.contains("board revision")));
        let nothing =
            SearchIdentity::from_inventory(&inventory(board(None, None, None, None, None, None)))
                .unwrap();
        assert_eq!(nothing.completeness, Completeness::Insufficient);
        let mut inv = inventory(board(None, None, None, None, None, None));
        inv.motherboard = None;
        assert!(SearchIdentity::from_inventory(&inv).is_err());
    }

    #[test]
    fn placeholders_and_firmware_versions_are_not_identifiers() {
        let id = SearchIdentity::from_inventory(&inventory(board(
            Some("To Be Filled By O.E.M."),
            Some("System Product Name"),
            None,
            Some("ASUSTeK COMPUTER INC."),
            Some("PRIME B450M-A"),
            Some("1.32.0"),
        )))
        .unwrap();
        assert!(id.system_model.is_none());
        assert_eq!(
            id.manufacturer.as_ref().unwrap().key,
            "asus",
            "board manufacturer used when system one is a placeholder"
        );
        assert!(
            id.board_revision.is_none(),
            "revision equal to the BIOS version is dropped"
        );
        assert_eq!(id.completeness, Completeness::BoardOnly);
        let generic = SearchIdentity::from_inventory(&inventory(board(
            Some("Dell Inc."),
            Some("Inspiron 15 3535"),
            None,
            Some("Dell Inc."),
            Some("Base"),
            None,
        )))
        .unwrap();
        assert!(generic.board_name.is_none());
        assert_eq!(generic.completeness, Completeness::SystemOnly);
    }

    #[test]
    fn conflicting_manufacturers_are_reported_not_merged() {
        let id = SearchIdentity::from_inventory(&inventory(board(
            Some("Dell Inc."),
            Some("Inspiron 15 3535"),
            None,
            Some("Quanta Computer"),
            Some("0N90HF"),
            None,
        )))
        .unwrap();
        assert_eq!(id.manufacturer.as_ref().unwrap().key, "dell");
        assert!(id.notes.iter().any(|n| n.contains("Quanta Computer")));
    }

    #[test]
    fn lenovo_machine_type_uses_the_model_name() {
        let mut inv = inventory(board(
            Some("LENOVO"),
            Some("20L5CTO1WW"),
            Some("ThinkPad T480"),
            Some("LENOVO"),
            Some("20L5CTO1WW"),
            Some("SDK0J40697 WIN"),
        ));
        inv.system.as_mut().unwrap().product_version = some("ThinkPad T480");
        let id = SearchIdentity::from_inventory(&inv).unwrap();
        assert_eq!(id.system_model.as_ref().unwrap().original, "ThinkPad T480");
        assert_eq!(id.distinctive_number(), Some("T480"));
    }
}
