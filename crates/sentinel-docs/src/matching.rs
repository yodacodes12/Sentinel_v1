//! Does a document belong to the detected motherboard?
//!
//! Every conclusion comes from inspectable evidence: each signal records
//! what was compared, where it was found and how many points it carries.
//! The class is decided by rules, not by the point total, so a high total
//! can never outweigh a critical contradiction (another manufacturer,
//! another model number). Missing information adds nothing. Where a
//! document is hosted is evidence about its *manufacturer* only; source
//! trust is assessed separately (`trust`).
//!
//! Methodology: docs/DOCUMENT_DISCOVERY.md, "Verification".

use crate::identity::{key_of, tokens};
use crate::model::{
    Effect, Evidence, EvidenceLocation, Indicator, MatchAssessment, MatchClass,
    RevisionCompatibility, SearchIdentity,
};
use crate::vendors;

pub const W_MANUFACTURER: i32 = 2;
pub const W_FAMILY: i32 = 1;
pub const W_MODEL_NUMBER: i32 = 3;
pub const W_SYSTEM_MODEL: i32 = 6;
pub const W_BOARD: i32 = 6;
pub const W_REVISION: i32 = 2;
pub const W_OTHER_MANUFACTURER: i32 = 5;
pub const W_OTHER_MODEL: i32 = 5;
pub const W_OTHER_BOARD: i32 = 5;
pub const W_OTHER_REVISION: i32 = 3;

/// Structured claims of a catalogue entry (exact lists, not free text).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Claims {
    pub manufacturer: Option<String>,
    pub models: Vec<String>,
    pub boards: Vec<String>,
    pub revisions: Vec<String>,
}

/// Everything known about a candidate that could identify its hardware.
#[derive(Debug, Clone, Default)]
pub struct CandidateText {
    pub title: String,
    pub filename: Option<String>,
    /// URL path and query only (the host is judged separately).
    pub url_path: Option<String>,
    pub description: Option<String>,
    pub document_title: Option<String>,
    pub host: Option<String>,
    pub claims: Option<Claims>,
}

/// Words that sit next to model numbers without naming a model.
const GENERIC: &[&str] = &[
    "SCHEMATIC",
    "SCHEMATICS",
    "BOARDVIEW",
    "MANUAL",
    "SERVICE",
    "OWNER",
    "OWNERS",
    "REV",
    "REVISION",
    "VERSION",
    "VER",
    "BOARD",
    "MOTHERBOARD",
    "MAINBOARD",
    "LAPTOP",
    "NOTEBOOK",
    "PAGE",
    "PAGES",
    "PDF",
    "EN",
    "US",
    "MODEL",
    "DIAGRAM",
    "BIOS",
    "FIRMWARE",
    "SPECIFICATIONS",
    "SETUP",
    "AND",
    "THE",
    "FOR",
    "OF",
    "SERIES",
    "PART",
    "NO",
    "DOC",
    "DOCUMENT",
    "ID",
    "FILE",
    "DOWNLOAD",
    "REPAIR",
    "GUIDE",
    "HARDWARE",
    "MAINTENANCE",
    "USER",
    "TECHNICAL",
    "LA",
    "DA",
    "NM",
    "MB",
    "PCB",
    "CPU",
    "GPU",
    "RAM",
    "GB",
    "TB",
    "INCH",
    "IN",
    "HZ",
    "MHZ",
    "GHZ",
    "W",
    "V",
    "DDR4",
    "DDR5",
    "USB",
    "TYPE",
    "WIN",
    "WINDOWS",
    "DETAILS",
    "ITEM",
    "ITEMS",
];

fn is_number_like(t: &str) -> bool {
    t.len() >= 3 && t.chars().any(|c| c.is_ascii_digit())
}

/// Shape of a model number: letters kept, digits as `9` (`T480` -> `T999`).
fn shape(t: &str) -> String {
    t.chars()
        .map(|c| if c.is_ascii_digit() { '9' } else { c })
        .collect()
}

/// True when the token window concatenation equals `key` somewhere in `toks`.
fn contains_key(toks: &[String], key: &str, max_window: usize) -> bool {
    if key.is_empty() {
        return false;
    }
    (0..toks.len()).any(|i| {
        let mut joined = String::new();
        for t in toks.iter().skip(i).take(max_window) {
            joined.push_str(t);
            if joined == key {
                return true;
            }
            if joined.len() >= key.len() {
                break;
            }
        }
        false
    })
}

fn alias_tokens(alias: &str) -> Vec<String> {
    tokens(alias)
}

fn has_alias(toks: &[String], aliases: &[&str]) -> bool {
    aliases.iter().any(|a| {
        let at = alias_tokens(a);
        !at.is_empty() && toks.windows(at.len()).any(|w| w == at.as_slice())
    })
}

struct Located {
    location: EvidenceLocation,
    toks: Vec<String>,
}

fn locations(c: &CandidateText) -> Vec<Located> {
    let mut out = Vec::new();
    let mut add = |location, text: Option<&str>| {
        if let Some(t) = text.filter(|t| !t.trim().is_empty()) {
            out.push(Located {
                location,
                toks: tokens(t),
            });
        }
    };
    add(
        EvidenceLocation::DocumentMetadata,
        c.document_title.as_deref(),
    );
    add(EvidenceLocation::Title, Some(c.title.as_str()));
    add(EvidenceLocation::Filename, c.filename.as_deref());
    add(EvidenceLocation::Description, c.description.as_deref());
    add(EvidenceLocation::Url, c.url_path.as_deref());
    out
}

pub fn assess(id: &SearchIdentity, c: &CandidateText) -> MatchAssessment {
    let mut ev: Vec<Evidence> = Vec::new();
    let mut push = |signal,
                    effect,
                    location,
                    detected: Option<&str>,
                    found: Option<&str>,
                    weight: i32,
                    note: String| {
        if ev
            .iter()
            .any(|e: &Evidence| e.signal == signal && e.effect == effect)
        {
            return;
        }
        ev.push(Evidence {
            signal,
            effect,
            location,
            detected: detected.map(str::to_string),
            found: found.map(str::to_string),
            weight,
            note,
        });
    };
    let locs = locations(c);
    let all: Vec<String> = locs.iter().flat_map(|l| l.toks.clone()).collect();
    let mfr = id.manufacturer.as_ref();
    let our_vendor = mfr.and_then(|m| vendors::by_key(&m.key));
    let board_vendor = id
        .board_manufacturer
        .as_ref()
        .and_then(|m| vendors::by_key(&m.key));
    let our_aliases: Vec<&str> = our_vendor
        .map(|v| v.aliases.to_vec())
        .or_else(|| mfr.map(|m| vec![m.key.as_str()]))
        .unwrap_or_default()
        .into_iter()
        .chain(board_vendor.map(|v| v.aliases.to_vec()).unwrap_or_default())
        .collect();
    let model_key = id
        .system_model
        .as_ref()
        .map(|m| m.key.clone())
        .unwrap_or_default();
    let model_tokens = id
        .system_model
        .as_ref()
        .map(|m| m.tokens.clone())
        .unwrap_or_default();
    let family_tokens: Vec<String> = id
        .system_family
        .as_ref()
        .map(|f| f.tokens.clone())
        .or_else(|| {
            model_tokens
                .iter()
                .find(|t| t.chars().all(|c| c.is_ascii_alphabetic()) && t.len() >= 3)
                .map(|t| vec![t.clone()])
        })
        .unwrap_or_default()
        .into_iter()
        .filter(|t| !t.chars().all(|c| c.is_ascii_digit()))
        .collect();
    let number = id.distinctive_number().map(str::to_string);
    let board_key = id.board_name.as_ref().map(|b| b.key.clone());
    let rev_key = id.board_revision.as_ref().map(|r| r.key.clone());

    // ---- structured catalogue claims: exact lists
    if let Some(claims) = &c.claims {
        let loc = EvidenceLocation::Catalogue;
        if let Some(cm) = &claims.manufacturer {
            match (vendors::vendor_for(cm), our_vendor) {
                (Some(v), Some(o))
                    if v.key == o.key || board_vendor.is_some_and(|b| b.key == v.key) =>
                {
                    push(
                        Indicator::Manufacturer,
                        Effect::Supports,
                        loc,
                        mfr.map(|m| m.original.as_str()),
                        Some(cm),
                        W_MANUFACTURER,
                        "The catalogue lists the same manufacturer.".into(),
                    )
                }
                (Some(v), Some(_)) => push(
                    Indicator::OtherManufacturer,
                    Effect::Contradicts,
                    loc,
                    mfr.map(|m| m.original.as_str()),
                    Some(cm),
                    W_OTHER_MANUFACTURER,
                    format!("The catalogue lists {} as the manufacturer.", v.canonical),
                ),
                _ => {}
            }
        }
        let strip = |k: String| -> String {
            let mut k = k;
            for a in &our_aliases {
                let ak = key_of(a);
                if k.starts_with(&ak) && k.len() > ak.len() {
                    k = k[ak.len()..].to_string();
                }
            }
            k
        };
        if !claims.models.is_empty() && !model_key.is_empty() {
            let ours = strip(model_key.clone());
            if claims.models.iter().any(|m| strip(key_of(m)) == ours) {
                push(
                    Indicator::SystemModel,
                    Effect::Supports,
                    loc,
                    Some(&model_key),
                    Some(&claims.models.join(", ")),
                    W_SYSTEM_MODEL,
                    "The catalogue lists this exact system model.".into(),
                );
            } else if board_key
                .as_ref()
                .is_none_or(|b| !claims.boards.iter().any(|x| key_of(x) == *b))
            {
                push(
                    Indicator::OtherModel,
                    Effect::Contradicts,
                    loc,
                    Some(&model_key),
                    Some(&claims.models.join(", ")),
                    W_OTHER_MODEL,
                    "The catalogue lists other models, not this one.".into(),
                );
            }
        }
        if let Some(b) = &board_key
            && !claims.boards.is_empty()
        {
            if claims.boards.iter().any(|x| key_of(x) == *b) {
                push(
                    Indicator::BoardIdentifier,
                    Effect::Supports,
                    loc,
                    Some(b),
                    Some(&claims.boards.join(", ")),
                    W_BOARD,
                    "The catalogue lists this board identifier.".into(),
                );
            } else {
                push(
                    Indicator::OtherBoardIdentifier,
                    Effect::Contradicts,
                    loc,
                    Some(b),
                    Some(&claims.boards.join(", ")),
                    W_OTHER_BOARD,
                    "The catalogue lists other board identifiers only.".into(),
                );
            }
        }
        if let Some(r) = &rev_key
            && !claims.revisions.is_empty()
        {
            if claims.revisions.iter().any(|x| key_of(x) == *r) {
                push(
                    Indicator::Revision,
                    Effect::Supports,
                    loc,
                    Some(r),
                    Some(&claims.revisions.join(", ")),
                    W_REVISION,
                    "The catalogue lists this board revision.".into(),
                );
            } else {
                push(
                    Indicator::OtherRevision,
                    Effect::Contradicts,
                    loc,
                    Some(r),
                    Some(&claims.revisions.join(", ")),
                    W_OTHER_REVISION,
                    "The catalogue lists other board revisions.".into(),
                );
            }
        }
    }

    // ---- host: evidence about the manufacturer only
    if let Some(host) = &c.host
        && let Some(v) = vendors::vendor_for_host(host)
    {
        if our_vendor.is_some_and(|o| o.key == v.key)
            || board_vendor.is_some_and(|b| b.key == v.key)
        {
            push(
                Indicator::Manufacturer,
                Effect::Supports,
                EvidenceLocation::Host,
                mfr.map(|m| m.original.as_str()),
                Some(host),
                W_MANUFACTURER,
                format!("Published on {host}, the manufacturer's own site."),
            );
        } else if our_vendor.is_some() {
            push(
                Indicator::OtherManufacturer,
                Effect::Contradicts,
                EvidenceLocation::Host,
                mfr.map(|m| m.original.as_str()),
                Some(host),
                W_OTHER_MANUFACTURER,
                format!(
                    "Published on {host}, a {} site: documents there describe {} hardware.",
                    v.canonical, v.canonical
                ),
            );
        }
    }

    // ---- text locations
    let ours_named = has_alias(&all, &our_aliases);
    let our_number_anywhere = number.as_ref().is_some_and(|n| all.contains(n));
    for l in &locs {
        let t = &l.toks;
        let loc = l.location;
        if has_alias(t, &our_aliases) {
            push(
                Indicator::Manufacturer,
                Effect::Supports,
                loc,
                mfr.map(|m| m.original.as_str()),
                mfr.map(|m| m.canonical.as_str()),
                W_MANUFACTURER,
                "Names the detected manufacturer.".into(),
            );
        }
        if !ours_named {
            for v in vendors::VENDORS {
                if our_vendor.is_some_and(|o| o.key == v.key)
                    || board_vendor.is_some_and(|b| b.key == v.key)
                {
                    continue;
                }
                if has_alias(t, v.aliases) {
                    push(
                        Indicator::OtherManufacturer,
                        Effect::Contradicts,
                        loc,
                        mfr.map(|m| m.original.as_str()),
                        Some(v.canonical),
                        W_OTHER_MANUFACTURER,
                        format!("Names {} and not the detected manufacturer.", v.canonical),
                    );
                }
            }
        }
        if vendors::BOARD_DESIGNERS
            .iter()
            .any(|d| t.contains(&d.to_ascii_uppercase()))
        {
            push(Indicator::Designer, Effect::Neutral, loc, None, None, 0, "Names a board design manufacturer (ODM); such names appear on boards of many brands.".into());
        }
        if !model_key.is_empty() && contains_key(t, &model_key, model_tokens.len() + 1) {
            push(
                Indicator::SystemModel,
                Effect::Supports,
                loc,
                id.system_model.as_ref().map(|m| m.original.as_str()),
                id.system_model.as_ref().map(|m| m.original.as_str()),
                W_SYSTEM_MODEL,
                "Names the exact system model.".into(),
            );
        }
        if let Some(n) = &number
            && t.contains(n)
        {
            push(
                Indicator::ModelNumber,
                Effect::Supports,
                loc,
                Some(n),
                Some(n),
                W_MODEL_NUMBER,
                format!("Contains the model number {n} as a whole word."),
            );
        }
        if !family_tokens.is_empty() && family_tokens.iter().all(|f| t.contains(f)) {
            push(
                Indicator::Family,
                Effect::Supports,
                loc,
                Some(&family_tokens.join(" ")),
                Some(&family_tokens.join(" ")),
                W_FAMILY,
                "Names the product family.".into(),
            );
        }
        if let Some(b) = &board_key
            && contains_key(t, b, 3)
        {
            push(
                Indicator::BoardIdentifier,
                Effect::Supports,
                loc,
                Some(b),
                Some(b),
                W_BOARD,
                "Contains the board identifier as a whole word.".into(),
            );
        }
        // Another model number of the same shape next to our family or
        // model words ("Inspiron 15 3530" when this is a 3535).
        if let Some(n) = &number {
            let context: Vec<&String> = family_tokens
                .iter()
                .chain(model_tokens.iter().filter(|m| *m != n))
                .collect();
            for (i, tok) in t.iter().enumerate() {
                if tok == n || shape(tok) != shape(n) {
                    continue;
                }
                let near = t[i.saturating_sub(2)..i]
                    .iter()
                    .any(|p| context.contains(&p));
                if near {
                    if our_number_anywhere {
                        push(
                            Indicator::SeveralModels,
                            Effect::Neutral,
                            loc,
                            Some(n),
                            Some(tok),
                            0,
                            format!("Also names {tok}; the document may cover several models."),
                        );
                    } else {
                        push(
                            Indicator::OtherModelNumber,
                            Effect::Contradicts,
                            loc,
                            Some(n),
                            Some(tok),
                            W_OTHER_MODEL,
                            format!("Names model {tok}, not {n}."),
                        );
                    }
                }
            }
        }
        // A different product of the same manufacturer ("Latitude 5420").
        if ours_named && !our_number_anywhere {
            for w in t.windows(2) {
                let (word, num) = (&w[0], &w[1]);
                let alpha = word.chars().all(|c| c.is_ascii_alphabetic()) && word.len() >= 3;
                if alpha
                    && is_number_like(num)
                    && !GENERIC.contains(&word.as_str())
                    && !family_tokens.contains(word)
                    && !model_tokens.contains(word)
                    && !has_alias(std::slice::from_ref(word), &our_aliases)
                {
                    push(
                        Indicator::OtherModel,
                        Effect::Contradicts,
                        loc,
                        id.system_model.as_ref().map(|m| m.original.as_str()),
                        Some(&format!("{word} {num}")),
                        W_OTHER_MODEL,
                        format!("Names another product, {} {}.", word, num),
                    );
                }
            }
        }
        // Revision markers: "rev A02", "revision 1.0", or right after the board ID.
        if let Some(r) = &rev_key {
            for (i, tok) in t.iter().enumerate() {
                let marker = matches!(tok.as_str(), "REV" | "REVISION" | "R");
                let after_board = board_key.as_ref().is_some_and(|b| tok == b);
                if !(marker || after_board) {
                    continue;
                }
                let Some(next) = t.get(i + 1) else { continue };
                // "rev 1 0" from "rev 1.0": join a digit continuation.
                let value = match t.get(i + 2) {
                    Some(n2)
                        if next.chars().all(|c| c.is_ascii_digit())
                            && n2.chars().all(|c| c.is_ascii_digit())
                            && n2.len() <= 2 =>
                    {
                        format!("{next}{n2}")
                    }
                    _ => next.clone(),
                };
                if value == *r || next == r {
                    push(
                        Indicator::Revision,
                        Effect::Supports,
                        loc,
                        Some(r),
                        Some(&value),
                        W_REVISION,
                        format!("States board revision {value}."),
                    );
                } else if marker && value.len() <= 4 && value.chars().any(|c| c.is_ascii_digit()) {
                    push(
                        Indicator::OtherRevision,
                        Effect::Contradicts,
                        loc,
                        Some(r),
                        Some(&value),
                        W_OTHER_REVISION,
                        format!("States board revision {value}, not {r}."),
                    );
                }
            }
        }
    }

    classify(id, ev)
}

fn has(ev: &[Evidence], s: Indicator, e: Effect) -> bool {
    ev.iter().any(|x| x.signal == s && x.effect == e)
}

fn classify(id: &SearchIdentity, evidence: Vec<Evidence>) -> MatchAssessment {
    let ev = &evidence;
    let sup = |s| has(ev, s, Effect::Supports);
    let con = |s| has(ev, s, Effect::Contradicts);
    let model_exact = sup(Indicator::SystemModel);
    let board_exact = sup(Indicator::BoardIdentifier);
    let other_mfr = con(Indicator::OtherManufacturer);
    let other_number = con(Indicator::OtherModelNumber);
    let other_model = con(Indicator::OtherModel);
    let other_board = con(Indicator::OtherBoardIdentifier);
    let critical = other_mfr || other_number || other_model || other_board;
    let rev_ok = sup(Indicator::Revision);
    let rev_bad = con(Indicator::OtherRevision);
    let device_rev = id.board_revision.is_some();

    let mut missing = Vec::new();
    if id.system_model.is_none() {
        missing.push("system model (not reported by firmware)".to_string());
    }
    if id.board_name.is_none() {
        missing.push("board identifier (not reported by firmware)".to_string());
    }
    if !device_rev {
        missing.push("board revision (not reported by firmware)".to_string());
    } else if !rev_ok && !rev_bad {
        missing.push("board revision (the document does not state one)".to_string());
    }

    let identity_match = model_exact
        || (board_exact
            && (sup(Indicator::Manufacturer)
                || sup(Indicator::ModelNumber)
                || sup(Indicator::Family)));
    let (class, revision, why) = if critical && !(model_exact || board_exact) {
        if other_number && sup(Indicator::Family) && !other_mfr {
            (
                MatchClass::RelatedModel,
                RevisionCompatibility::NotApplicable,
                "It is for another model of the same family. Similar models are not necessarily electrically compatible.",
            )
        } else {
            (
                MatchClass::Mismatch,
                RevisionCompatibility::NotApplicable,
                "The evidence indicates it is for different hardware.",
            )
        }
    } else if critical {
        (
            MatchClass::Possible,
            RevisionCompatibility::NotApplicable,
            "Some identifiers match, but other evidence contradicts them (for example a matching filename with a title naming another model). Verify it manually.",
        )
    } else if identity_match {
        match (device_rev, rev_ok, rev_bad) {
            (true, true, false) => (
                MatchClass::Exact,
                RevisionCompatibility::Matches,
                "The model or board identifier and the board revision match, and nothing contradicts them.",
            ),
            (true, _, true) => (
                MatchClass::RelatedModel,
                RevisionCompatibility::Conflicts,
                "Same model, but the document is for a different board revision, which may differ electrically.",
            ),
            (true, false, false) => (
                MatchClass::HighConfidence,
                RevisionCompatibility::DocumentUnspecified,
                "The model or board identifier matches; the document does not state a board revision.",
            ),
            (false, _, _) => (
                MatchClass::HighConfidence,
                RevisionCompatibility::DeviceUnknown,
                "The model or board identifier matches; firmware does not report this board's revision.",
            ),
        }
    } else if board_exact
        || sup(Indicator::ModelNumber)
        || (sup(Indicator::Manufacturer) && sup(Indicator::Family))
    {
        (
            MatchClass::Possible,
            RevisionCompatibility::NotApplicable,
            "Some identifiers match, but not enough to establish that it is for this model.",
        )
    } else if sup(Indicator::Manufacturer) {
        (
            MatchClass::Unverified,
            RevisionCompatibility::NotApplicable,
            "Only the manufacturer matches; nothing identifies the model.",
        )
    } else {
        (
            MatchClass::Unverified,
            RevisionCompatibility::NotApplicable,
            "Nothing in the document's title, name or metadata identifies its hardware.",
        )
    };
    let support_points = ev
        .iter()
        .filter(|e| e.effect == Effect::Supports)
        .map(|e| e.weight)
        .sum();
    let contradiction_points = ev
        .iter()
        .filter(|e| e.effect == Effect::Contradicts)
        .map(|e| e.weight)
        .sum();
    MatchAssessment {
        manual_verification_recommended: class != MatchClass::Exact,
        explanation: format!(
            "{why} Sentinel compared names and metadata only, not the document's contents."
        ),
        class,
        evidence,
        support_points,
        contradiction_points,
        missing,
        revision,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::identity::tests::{board, dell, inventory};

    fn doc(title: &str) -> CandidateText {
        CandidateText {
            title: title.into(),
            ..Default::default()
        }
    }

    fn class(id: &SearchIdentity, c: &CandidateText) -> MatchClass {
        assess(id, c).class
    }

    #[test]
    fn exact_model_and_revision() {
        let a = assess(
            &dell(),
            &doc("Dell Inspiron 15 3535 schematic 0N90HF rev A02"),
        );
        assert_eq!(a.class, MatchClass::Exact);
        assert_eq!(a.revision, RevisionCompatibility::Matches);
        assert!(!a.manual_verification_recommended);
        assert_eq!(a.contradiction_points, 0);
        assert!(a.support_points >= W_SYSTEM_MODEL + W_BOARD + W_REVISION);
    }

    #[test]
    fn exact_model_unknown_revision() {
        let a = assess(&dell(), &doc("Inspiron 15 3535 Service Manual"));
        assert_eq!(a.class, MatchClass::HighConfidence);
        assert_eq!(a.revision, RevisionCompatibility::DocumentUnspecified);
        assert!(a.missing.iter().any(|m| m.contains("does not state")));
        let no_rev = SearchIdentity::from_inventory(&inventory(board(
            Some("Dell Inc."),
            Some("Inspiron 15 3535"),
            Some("Inspiron"),
            Some("Dell Inc."),
            Some("0N90HF"),
            None,
        )))
        .unwrap();
        let b = assess(&no_rev, &doc("Dell Inspiron 15 3535 schematic rev A02"));
        assert_eq!(
            b.class,
            MatchClass::HighConfidence,
            "a revision cannot be confirmed when firmware does not report one"
        );
        assert_eq!(b.revision, RevisionCompatibility::DeviceUnknown);
    }

    #[test]
    fn same_family_different_model() {
        let a = assess(&dell(), &doc("Dell Inspiron 15 3530 schematic"));
        assert_eq!(a.class, MatchClass::RelatedModel);
        assert!(
            a.evidence
                .iter()
                .any(|e| e.signal == Indicator::OtherModelNumber
                    && e.found.as_deref() == Some("3530"))
        );
    }

    #[test]
    fn same_manufacturer_unrelated_model() {
        assert_eq!(
            class(&dell(), &doc("Dell Latitude 5420 schematic")),
            MatchClass::Mismatch
        );
        assert_eq!(
            class(&dell(), &doc("Lenovo ThinkPad T480 schematic")),
            MatchClass::Mismatch
        );
    }

    #[test]
    fn conflicting_revision() {
        let a = assess(
            &dell(),
            &doc("Dell Inspiron 15 3535 0N90HF schematic rev A00"),
        );
        assert_eq!(a.class, MatchClass::RelatedModel);
        assert_eq!(a.revision, RevisionCompatibility::Conflicts);
    }

    #[test]
    fn missing_model_identifiers() {
        let id = SearchIdentity::from_inventory(&inventory(board(
            Some("Dell Inc."),
            None,
            None,
            None,
            None,
            None,
        )))
        .unwrap();
        let a = assess(&id, &doc("Dell Inspiron 15 3535 schematic"));
        assert_eq!(
            a.class,
            MatchClass::Unverified,
            "missing identity is not a match"
        );
        assert!(a.missing.len() >= 3);
    }

    #[test]
    fn misleading_filenames_and_titles() {
        // Our board ID in the filename, another model in the title.
        let mut c = doc("Dell Inspiron 15 3530 schematic");
        c.filename = Some("0N90HF.pdf".into());
        assert_eq!(class(&dell(), &c), MatchClass::Possible);
        // A longer number containing ours is not ours.
        assert_eq!(
            class(&dell(), &doc("Inspiron 35350 schematic")),
            MatchClass::Unverified
        );
        assert_eq!(
            class(&dell(), &doc("Dell 35350 schematic")),
            MatchClass::Unverified
        );
        // Model number alone, without manufacturer or family: only possible.
        assert_eq!(class(&dell(), &doc("3535 schematic")), MatchClass::Possible);
        // "Inspiron 3535 vs 3530 comparison" covers several models: not a contradiction.
        let several = assess(
            &dell(),
            &doc("Dell Inspiron 15 3535 / Inspiron 15 3530 service manual"),
        );
        assert_eq!(several.class, MatchClass::HighConfidence);
        assert!(
            several
                .evidence
                .iter()
                .any(|e| e.signal == Indicator::SeveralModels)
        );
    }

    #[test]
    fn official_source_but_wrong_hardware() {
        let mut c = doc("Inspiron 15 3530 Owner's Manual");
        c.host = Some("dl.dell.com".into());
        let a = assess(&dell(), &c);
        assert_eq!(
            a.class,
            MatchClass::RelatedModel,
            "an official host is not a model match"
        );
        let mut lenovo = doc("ThinkPad T480 Hardware Maintenance Manual");
        lenovo.host = Some("download.lenovo.com".into());
        assert_eq!(class(&dell(), &lenovo), MatchClass::Mismatch);
    }

    #[test]
    fn community_source_with_strong_model_evidence() {
        let mut c = doc("Dell Inspiron 15 3535 0N90HF A02 schematic");
        c.host = Some("archive.org".into());
        c.description = Some("Board 0N90HF rev A02".into());
        assert_eq!(
            class(&dell(), &c),
            MatchClass::Exact,
            "hardware match does not depend on source trust"
        );
    }

    #[test]
    fn unverified_documents() {
        assert_eq!(
            class(&dell(), &doc("Laptop schematic collection")),
            MatchClass::Unverified
        );
        assert_eq!(
            class(&dell(), &doc("Dell service manuals")),
            MatchClass::Unverified
        );
    }

    #[test]
    fn catalogue_claims_are_exact_lists() {
        let mut c = doc("Board documentation");
        c.claims = Some(Claims {
            manufacturer: Some("Dell Inc.".into()),
            models: vec!["Inspiron 15 3535".into()],
            boards: vec!["0N90HF".into()],
            revisions: vec!["A02".into()],
        });
        assert_eq!(class(&dell(), &c), MatchClass::Exact);
        c.claims = Some(Claims {
            manufacturer: Some("Dell".into()),
            models: vec!["Inspiron 15 3530".into()],
            boards: vec![],
            revisions: vec![],
        });
        assert_eq!(class(&dell(), &c), MatchClass::Mismatch);
        c.claims = Some(Claims {
            manufacturer: Some("Dell".into()),
            models: vec!["Inspiron 15 3535".into()],
            boards: vec!["0X1234".into()],
            revisions: vec![],
        });
        assert_eq!(
            class(&dell(), &c),
            MatchClass::Possible,
            "same model, other board: contradiction kept"
        );
    }

    #[test]
    fn a_high_score_never_overrides_a_critical_mismatch() {
        let mut c = doc("Inspiron 15 3535 0N90HF rev A02 schematic");
        c.host = Some("download.lenovo.com".into());
        let a = assess(&dell(), &c);
        assert!(a.support_points > a.contradiction_points);
        assert_ne!(a.class, MatchClass::Exact);
        assert!(a.manual_verification_recommended);
    }

    #[test]
    fn odm_names_are_context_not_contradictions() {
        let a = assess(&dell(), &doc("Dell Inspiron 15 3535 Compal schematic"));
        assert_eq!(a.class, MatchClass::HighConfidence);
        assert!(
            a.evidence
                .iter()
                .any(|e| e.signal == Indicator::Designer && e.effect == Effect::Neutral)
        );
    }
}
