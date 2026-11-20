//! Motherboard identity: the contract for future schematic discovery.
//!
//! A later phase will look for board documentation. It must consume this
//! type and nothing else, which is why it contains only non-sensitive
//! identifiers: no serial numbers, no UUIDs, no asset tags.
//!
//! Laptop model and board model are different things. A board revision
//! reported by firmware is recorded as firmware-reported and is never
//! "verified" because the laptop model matches something.

use serde::{Deserialize, Serialize};

use crate::provenance::Field;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BoardIdentity {
    pub discovered_at: u64,
    pub system_manufacturer: Field<String>,
    pub system_model: Field<String>,
    pub system_family: Field<String>,
    pub system_sku: Field<String>,
    pub board_manufacturer: Field<String>,
    pub board_name: Field<String>,
    pub board_revision: Field<String>,
    pub bios_vendor: Field<String>,
    pub bios_version: Field<String>,
    pub bios_release_date: Field<String>,
    pub identification: BoardIdentification,
}

/// How completely the board is identified, judged only from what firmware
/// reported. Nothing here is checked against external documentation yet.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BoardIdentification {
    pub level: IdentificationLevel,
    pub explanation: String,
    /// Always false in Phase 2: there is no external verification yet.
    pub externally_verified: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IdentificationLevel {
    /// Board manufacturer and board name are present and not placeholders.
    BoardIdentified,
    /// Only the system (laptop/desktop) model is known, not the board.
    SystemOnly,
    /// Neither is usable.
    Unidentified,
}

impl BoardIdentity {
    /// Judge identification from the fields present.
    pub fn assess(
        board_manufacturer: &Field<String>,
        board_name: &Field<String>,
        system_model: &Field<String>,
    ) -> BoardIdentification {
        let (level, explanation) = if board_manufacturer.is_detected() && board_name.is_detected() {
            (
                IdentificationLevel::BoardIdentified,
                "Board manufacturer and board name were reported by firmware (SMBIOS). They \
                 have not been checked against manufacturer documentation.",
            )
        } else if system_model.is_detected() {
            (
                IdentificationLevel::SystemOnly,
                "Firmware reports the system model but not a usable board name. The board \
                 model cannot be assumed from the system model.",
            )
        } else {
            (
                IdentificationLevel::Unidentified,
                "Firmware did not report usable system or board identifiers.",
            )
        };
        BoardIdentification {
            level,
            explanation: explanation.to_string(),
            externally_verified: false,
        }
    }

    /// Identifiers a future documentation search may use, in order of
    /// specificity. Only detected, non-sensitive values appear.
    pub fn search_terms(&self) -> Vec<String> {
        let mut terms = Vec::new();
        let mut push = |parts: &[&Field<String>]| {
            let values: Vec<&str> = parts
                .iter()
                .filter_map(|f| f.value().map(String::as_str))
                .collect();
            if values.len() == parts.len() {
                let term = values.join(" ");
                if !terms.contains(&term) {
                    terms.push(term);
                }
            }
        };
        push(&[
            &self.board_manufacturer,
            &self.board_name,
            &self.board_revision,
        ]);
        push(&[&self.board_manufacturer, &self.board_name]);
        push(&[&self.system_manufacturer, &self.system_model]);
        terms
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provenance::{Availability, Confidence};

    fn some(v: &str) -> Field<String> {
        Field::detected(v.to_string(), "dmi", Confidence::FirmwareReported)
    }
    fn none() -> Field<String> {
        Field::missing(Availability::Invalid, None)
    }

    fn identity(
        board: Field<String>,
        revision: Field<String>,
        model: Field<String>,
    ) -> BoardIdentity {
        let manufacturer = some("Example Corp");
        let identification = BoardIdentity::assess(&manufacturer, &board, &model);
        BoardIdentity {
            discovered_at: 0,
            system_manufacturer: some("Example Corp"),
            system_model: model,
            system_family: none(),
            system_sku: none(),
            board_manufacturer: manufacturer,
            board_name: board,
            board_revision: revision,
            bios_vendor: none(),
            bios_version: none(),
            bios_release_date: none(),
            identification,
        }
    }

    #[test]
    fn full_identity_is_board_identified_but_never_verified() {
        let id = identity(some("0ABC12"), some("A01"), some("Laptop 15"));
        assert_eq!(
            id.identification.level,
            IdentificationLevel::BoardIdentified
        );
        assert!(!id.identification.externally_verified);
        assert_eq!(
            id.search_terms(),
            [
                "Example Corp 0ABC12 A01",
                "Example Corp 0ABC12",
                "Example Corp Laptop 15"
            ]
        );
    }

    #[test]
    fn system_model_alone_does_not_identify_the_board() {
        let id = identity(none(), none(), some("Laptop 15"));
        assert_eq!(id.identification.level, IdentificationLevel::SystemOnly);
        assert_eq!(id.search_terms(), ["Example Corp Laptop 15"]);
    }

    #[test]
    fn nothing_usable_is_unidentified() {
        let id = identity(none(), none(), none());
        assert_eq!(id.identification.level, IdentificationLevel::Unidentified);
        assert!(id.search_terms().is_empty());
    }
}
