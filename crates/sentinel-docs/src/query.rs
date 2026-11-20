//! Search queries from a motherboard identity.
//!
//! Queries name the manufacturer together with an exact model or board
//! identifier, never a family or a manufacturer alone, so results cannot be
//! dominated by unrelated hardware. Less identity means fewer queries; an
//! insufficient identity produces none. Queries contain only what firmware
//! reported for the model and board: no SKU, serial number or BIOS version.

use crate::identity::is_distinctive_board_id;
use crate::model::{Completeness, QueryPurpose, SearchIdentity, SearchQuery};

pub const MAX_QUERY_CHARS: usize = 96;

/// Keep letters, digits, spaces and `- . / + ( )`; collapse spaces; bound
/// the length at a word boundary. Quotes and operators are removed so an
/// identifier cannot change a provider's query syntax.
pub fn sanitise(text: &str) -> String {
    let cleaned: String = text
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || " -./+()".contains(c) {
                c
            } else {
                ' '
            }
        })
        .collect();
    let mut out = String::new();
    for word in cleaned.split_whitespace() {
        let word = word.trim_matches(|c: char| "-./+".contains(c));
        if word.is_empty() {
            continue;
        }
        if out.len() + word.len() + 1 > MAX_QUERY_CHARS {
            break;
        }
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(word);
    }
    out
}

pub fn generate(id: &SearchIdentity) -> Vec<SearchQuery> {
    let mut out: Vec<SearchQuery> = Vec::new();
    let mut push = |text: String, purpose: QueryPurpose, basis: &[&str]| {
        let text = sanitise(&text);
        if text.split_whitespace().count() < 2 {
            return;
        }
        if out.iter().any(|q| q.text.eq_ignore_ascii_case(&text)) {
            return;
        }
        out.push(SearchQuery {
            text,
            purpose,
            basis: basis.iter().map(|s| s.to_string()).collect(),
        });
    };
    if id.completeness == Completeness::Insufficient {
        return Vec::new();
    }
    let Some(mfr) = &id.manufacturer else {
        return Vec::new();
    };
    let mfr = mfr.canonical.as_str();

    if let Some(model) = &id.system_model {
        // A model name that already starts with the manufacturer is not
        // repeated ("HP EliteBook" stays as reported).
        let name = if model
            .original
            .to_lowercase()
            .starts_with(&mfr.to_lowercase())
        {
            model.original.clone()
        } else {
            format!("{mfr} {}", model.original)
        };
        push(
            format!("{name} schematic"),
            QueryPurpose::Schematic,
            &["manufacturer", "system_model"],
        );
        push(
            format!("{name} boardview"),
            QueryPurpose::Boardview,
            &["manufacturer", "system_model"],
        );
        push(
            format!("{name} service manual"),
            QueryPurpose::ServiceManual,
            &["manufacturer", "system_model"],
        );
        push(
            format!("{name} technical specifications"),
            QueryPurpose::Official,
            &["manufacturer", "system_model"],
        );
        push(
            name.clone(),
            QueryPurpose::Identity,
            &["manufacturer", "system_model"],
        );
        // The model alone only when it carries a distinctive model number
        // ("ThinkPad T480", "Inspiron 15 3535"), never a bare family name.
        if id.distinctive_number().is_some() && model.tokens.len() >= 2 {
            push(
                model.original.clone(),
                QueryPurpose::Identity,
                &["system_model"],
            );
        }
    }
    if let Some(board) = id
        .board_name
        .as_ref()
        .filter(|b| is_distinctive_board_id(b))
    {
        let board_mfr = id
            .board_manufacturer
            .as_ref()
            .map(|m| m.canonical.as_str())
            .unwrap_or(mfr);
        push(
            format!("{board_mfr} {} schematic", board.original),
            QueryPurpose::Schematic,
            &["board_manufacturer", "board_name"],
        );
        push(
            format!("{board_mfr} {} boardview", board.original),
            QueryPurpose::Boardview,
            &["board_manufacturer", "board_name"],
        );
        push(
            format!("{board_mfr} {}", board.original),
            QueryPurpose::Identity,
            &["board_manufacturer", "board_name"],
        );
        if let Some(rev) = &id.board_revision {
            push(
                format!("{board_mfr} {} rev {}", board.original, rev.original),
                QueryPurpose::Revision,
                &["board_manufacturer", "board_name", "board_revision"],
            );
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::identity::tests::{board, dell, inventory};

    fn texts(id: &SearchIdentity) -> Vec<String> {
        generate(id).into_iter().map(|q| q.text).collect()
    }

    #[test]
    fn exact_model_and_board_queries() {
        let q = texts(&dell());
        assert_eq!(
            q,
            [
                "Dell Inspiron 15 3535 schematic",
                "Dell Inspiron 15 3535 boardview",
                "Dell Inspiron 15 3535 service manual",
                "Dell Inspiron 15 3535 technical specifications",
                "Dell Inspiron 15 3535",
                "Inspiron 15 3535",
                "Dell 0N90HF schematic",
                "Dell 0N90HF boardview",
                "Dell 0N90HF",
                "Dell 0N90HF rev A02",
            ]
        );
    }

    #[test]
    fn missing_revision_drops_the_revision_query() {
        let id = SearchIdentity::from_inventory(&inventory(board(
            Some("Dell Inc."),
            Some("Inspiron 15 3535"),
            None,
            Some("Dell Inc."),
            Some("0N90HF"),
            None,
        )))
        .unwrap();
        let q = texts(&id);
        assert_eq!(q.len(), 9);
        assert!(!q.iter().any(|t| t.contains(" rev ")));
    }

    #[test]
    fn incomplete_identity_is_conservative() {
        let system_only = SearchIdentity::from_inventory(&inventory(board(
            Some("Dell Inc."),
            Some("Inspiron 15 3535"),
            Some("Inspiron"),
            None,
            None,
            None,
        )))
        .unwrap();
        let q = texts(&system_only);
        assert_eq!(q.len(), 6);
        assert!(
            q.iter().all(|t| t.contains("Inspiron 15 3535")),
            "never family-only: {q:?}"
        );
        // A model without a distinctive number is never searched alone.
        let generic = SearchIdentity::from_inventory(&inventory(board(
            Some("Framework"),
            Some("Laptop 13"),
            None,
            None,
            None,
            None,
        )))
        .unwrap();
        assert!(!texts(&generic).iter().any(|t| t == "Laptop 13"));
        let no_mfr = SearchIdentity::from_inventory(&inventory(board(
            None,
            Some("Inspiron 15 3535"),
            None,
            None,
            Some("0N90HF"),
            None,
        )))
        .unwrap();
        assert!(texts(&no_mfr).is_empty());
        let nothing = SearchIdentity::from_inventory(&inventory(board(
            Some("Dell Inc."),
            None,
            None,
            None,
            None,
            None,
        )))
        .unwrap();
        assert!(texts(&nothing).is_empty());
    }

    #[test]
    fn duplicates_are_not_generated() {
        // Board name equal to the model: no duplicate board queries.
        let id = SearchIdentity::from_inventory(&inventory(board(
            Some("Framework"),
            Some("Laptop 13 (AMD Ryzen 7040Series)"),
            None,
            Some("Framework"),
            Some("FRANMDCP07"),
            Some("A7"),
        )))
        .unwrap();
        let q = texts(&id);
        let mut dedup = q.clone();
        dedup.sort();
        dedup.dedup();
        assert_eq!(dedup.len(), q.len());
        assert!(q[0].starts_with("Framework Laptop 13 (AMD Ryzen 7040Series)"));
    }

    #[test]
    fn identifiers_cannot_inject_query_syntax() {
        assert_eq!(
            sanitise("Model \"X\" OR site:evil.example"),
            "Model X OR site evil.example"
        );
        assert_eq!(sanitise("a\u{0}b\nc  d"), "a b c d");
        assert!(sanitise(&"word ".repeat(40)).len() <= MAX_QUERY_CHARS);
        assert_eq!(sanitise("--./"), "");
    }
}
