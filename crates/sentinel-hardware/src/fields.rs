//! Turning raw reads into [`Field`]s with provenance.

use std::str::FromStr;

use sentinel_inventory::{Availability, Confidence, Field};

use crate::source::{ReadError, SysRoot};

/// A text attribute.
pub fn text(root: &SysRoot, rel: &str, confidence: Confidence) -> Field<String> {
    from_read(root, rel, root.read_trimmed(rel), confidence)
}

/// A firmware (DMI) string, with vendor placeholders rejected.
pub fn firmware_text(root: &SysRoot, rel: &str) -> Field<String> {
    text(root, rel, Confidence::FirmwareReported).validate(
        |value| !is_placeholder(value),
        "firmware reports a placeholder value instead of real information",
    )
}

/// A numeric attribute parsed as `T`.
pub fn number<T: FromStr>(root: &SysRoot, rel: &str, confidence: Confidence) -> Field<T> {
    match root.read_trimmed(rel) {
        Ok(raw) => match raw.parse::<T>() {
            Ok(value) => Field::detected(value, root.display(rel), confidence),
            Err(_) => Field::missing(Availability::Invalid, Some(root.display(rel)))
                .with_note(format!("could not parse {raw:?} as a number")),
        },
        Err(error) => missing(root, rel, &error),
    }
}

/// A `0`/`1` attribute.
pub fn flag(root: &SysRoot, rel: &str, confidence: Confidence) -> Field<bool> {
    match root.read_trimmed(rel) {
        Ok(raw) => match raw.as_str() {
            "1" => Field::detected(true, root.display(rel), confidence),
            "0" => Field::detected(false, root.display(rel), confidence),
            _ => Field::missing(Availability::Invalid, Some(root.display(rel)))
                .with_note(format!("expected 0 or 1, found {raw:?}")),
        },
        Err(error) => missing(root, rel, &error),
    }
}

pub fn from_read(
    root: &SysRoot,
    rel: &str,
    read: Result<String, ReadError>,
    confidence: Confidence,
) -> Field<String> {
    match read {
        Ok(value) => Field::detected(value, root.display(rel), confidence),
        Err(error) => missing(root, rel, &error),
    }
}

pub fn missing<T>(root: &SysRoot, rel: &str, error: &ReadError) -> Field<T> {
    let field = Field::missing(error.availability(), Some(root.display(rel)));
    match error {
        ReadError::PermissionDenied => field.with_note(
            "readable only by administrators; Sentinel does not request elevated privileges",
        ),
        ReadError::Missing => field.with_note("not provided by this system"),
        ReadError::Empty => field.with_note("present but empty"),
        ReadError::Refused => field.with_note("not read: identifies this specific machine"),
        ReadError::TooLarge => field.with_note("unexpectedly large; not read"),
        ReadError::Other(reason) => field.with_note(format!("read failed: {reason}")),
    }
}

/// Values firmware vendors leave in DMI fields they did not fill in.
pub fn is_placeholder(value: &str) -> bool {
    let lower = value.trim().to_ascii_lowercase();
    if lower.is_empty() {
        return true;
    }
    const PLACEHOLDERS: &[&str] = &[
        "to be filled by o.e.m.",
        "to be filled by oem",
        "default string",
        "system product name",
        "system manufacturer",
        "system version",
        "system name",
        "system sku",
        "base board product name",
        "base board version",
        "baseboard product name",
        "chassis manufacturer",
        "chassis version",
        "not specified",
        "not applicable",
        "not available",
        "none",
        "n/a",
        "na",
        "unknown",
        "oem",
        "o.e.m.",
        "invalid",
        "empty",
        "default",
        "type1productconfigid",
        "type2 - board product name1",
        "type2 - board vendor name1",
        "type2 - board version",
        "123456789",
        "0123456789",
        "1234567890",
        "x.x",
        "0x00",
        "0x0000",
    ];
    if PLACEHOLDERS.contains(&lower.as_str()) {
        return true;
    }
    // "0", "00000", "xxxx", "......" and similar runs of one character.
    let mut chars = lower.chars();
    let first = chars.next().unwrap_or(' ');
    chars.all(|c| c == first) && matches!(first, '0' | 'x' | '.' | '-' | '*' | ' ' | 'f')
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn placeholders_are_recognised() {
        for value in [
            "To Be Filled By O.E.M.",
            "Default string",
            "System Product Name",
            "Not Specified",
            "0",
            "00000000",
            "xxxx",
            "  ",
            "N/A",
        ] {
            assert!(is_placeholder(value), "{value:?}");
        }
        for value in [
            "Dell Inc.",
            "0N90HF",
            "A02",
            "Inspiron 15 3535",
            "1.32.0",
            "X1",
        ] {
            assert!(!is_placeholder(value), "{value:?}");
        }
    }

    #[test]
    fn fields_carry_availability() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("vendor"), "Default string\n").unwrap();
        fs::write(dir.path().join("size"), "abc\n").unwrap();
        fs::write(dir.path().join("count"), "12\n").unwrap();
        fs::write(dir.path().join("flag"), "1\n").unwrap();
        let root = SysRoot::at(dir.path());

        let vendor = firmware_text(&root, "vendor");
        assert_eq!(vendor.availability(), Availability::Invalid);
        assert_eq!(vendor.value(), None);

        let size: Field<u64> = number(&root, "size", Confidence::KernelReported);
        assert_eq!(size.availability(), Availability::Invalid);

        let count: Field<u32> = number(&root, "count", Confidence::KernelReported);
        assert_eq!(count.value(), Some(&12));
        assert_eq!(count.source(), Some("/count"));

        assert_eq!(flag(&root, "flag", Confidence::Direct).value(), Some(&true));
        assert_eq!(
            flag(&root, "missing", Confidence::Direct).availability(),
            Availability::Unsupported
        );
    }
}
