//! Where a value came from and how far it can be trusted.
//!
//! Every discovered value is a [`Field`]. A field either holds a value
//! (availability [`Availability::Detected`]) or explains why it does not.
//! The invariant "a value is present exactly when it was detected" is
//! enforced by the constructors and re-checked when a field is deserialised,
//! so a cache file cannot smuggle in a value marked "unavailable" or an
//! "available" field with nothing in it.

use serde::{Deserialize, Serialize};

/// How a value was obtained. Ordered from most to least authoritative.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Confidence {
    /// Read directly from a Linux hardware interface (for example a sysfs
    /// attribute exposed by the device itself).
    Direct,
    /// Reported by SMBIOS/DMI or other firmware tables. Vendors sometimes
    /// fill these in carelessly.
    FirmwareReported,
    /// Reported by the Linux kernel or a driver.
    KernelReported,
    /// Looked up by numeric ID in a hardware database installed on this
    /// machine (`pci.ids`, `usb.ids`). Names can be generic or out of date.
    Database,
    /// Calculated from other detected values by a fixed formula (for
    /// example battery health from full and design capacity). As good as
    /// its inputs, but not itself a reading.
    Derived,
    /// Concluded from indirect evidence (a lookup table, the device's place
    /// in the kernel tree). Can be wrong even when the inputs are right.
    Inferred,
    /// The source or its reliability cannot be established.
    Unknown,
}

impl Confidence {
    pub fn label(self) -> &'static str {
        match self {
            Confidence::Direct => "direct",
            Confidence::FirmwareReported => "firmware-reported",
            Confidence::KernelReported => "kernel-reported",
            Confidence::Database => "ID database",
            Confidence::Derived => "derived",
            Confidence::Inferred => "inferred",
            Confidence::Unknown => "unknown",
        }
    }
}

/// Whether a value could be obtained.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Availability {
    /// The value was obtained.
    Detected,
    /// No source was found and it is not known why.
    Unknown,
    /// This machine, kernel or driver does not provide the information.
    Unsupported,
    /// The interface exists but did not provide a value (for example an
    /// empty attribute).
    Unavailable,
    /// The interface exists but the current user may not read it. Sentinel
    /// never escalates privileges to get around this.
    PermissionDenied,
    /// A value was present but malformed, or a firmware placeholder such as
    /// "To Be Filled By O.E.M.". It is not shown as real information.
    Invalid,
}

impl Availability {
    pub fn label(self) -> &'static str {
        match self {
            Availability::Detected => "detected",
            Availability::Unknown => "unknown",
            Availability::Unsupported => "not supported",
            Availability::Unavailable => "not available",
            Availability::PermissionDenied => "permission denied",
            Availability::Invalid => "invalid value",
        }
    }
}

/// A discovered value together with its provenance.
///
/// Timestamps live on the record that contains the field (each section of
/// the inventory carries `discovered_at`), not on every field.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "RawField<T>", into = "RawField<T>")]
#[serde(bound(
    serialize = "T: Serialize + Clone",
    deserialize = "T: Deserialize<'de>"
))]
pub struct Field<T> {
    value: Option<T>,
    availability: Availability,
    confidence: Confidence,
    source: Option<String>,
    notes: Option<String>,
}

/// Serialised form of [`Field`]; converted with validation.
#[derive(Serialize, Deserialize)]
struct RawField<T> {
    #[serde(skip_serializing_if = "Option::is_none")]
    value: Option<T>,
    availability: Availability,
    confidence: Confidence,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    source: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    notes: Option<String>,
}

impl<T> TryFrom<RawField<T>> for Field<T> {
    type Error = String;

    fn try_from(raw: RawField<T>) -> Result<Self, Self::Error> {
        let detected = raw.availability == Availability::Detected;
        if detected != raw.value.is_some() {
            return Err(format!(
                "inconsistent field: availability {:?} with{} a value",
                raw.availability,
                if raw.value.is_some() { "" } else { "out" }
            ));
        }
        Ok(Field {
            value: raw.value,
            availability: raw.availability,
            confidence: raw.confidence,
            source: raw.source,
            notes: raw.notes,
        })
    }
}

impl<T> From<Field<T>> for RawField<T> {
    fn from(field: Field<T>) -> Self {
        RawField {
            value: field.value,
            availability: field.availability,
            confidence: field.confidence,
            source: field.source,
            notes: field.notes,
        }
    }
}

impl<T> Field<T> {
    /// A value that was obtained from `source`.
    pub fn detected(value: T, source: impl Into<String>, confidence: Confidence) -> Self {
        Self {
            value: Some(value),
            availability: Availability::Detected,
            confidence,
            source: Some(source.into()),
            notes: None,
        }
    }

    /// No value. `availability` must not be [`Availability::Detected`]; if it
    /// is, the field is recorded as [`Availability::Unknown`] instead so the
    /// invariant can never be broken by a caller.
    pub fn missing(availability: Availability, source: Option<String>) -> Self {
        let availability = if availability == Availability::Detected {
            Availability::Unknown
        } else {
            availability
        };
        Self {
            value: None,
            availability,
            confidence: Confidence::Unknown,
            source,
            notes: None,
        }
    }

    pub fn unknown() -> Self {
        Self::missing(Availability::Unknown, None)
    }

    pub fn unsupported(note: impl Into<String>) -> Self {
        Self::missing(Availability::Unsupported, None).with_note(note)
    }

    pub fn with_note(mut self, note: impl Into<String>) -> Self {
        self.notes = Some(note.into());
        self
    }

    pub fn value(&self) -> Option<&T> {
        self.value.as_ref()
    }

    pub fn into_value(self) -> Option<T> {
        self.value
    }

    pub fn availability(&self) -> Availability {
        self.availability
    }

    pub fn confidence(&self) -> Confidence {
        self.confidence
    }

    pub fn source(&self) -> Option<&str> {
        self.source.as_deref()
    }

    pub fn notes(&self) -> Option<&str> {
        self.notes.as_deref()
    }

    pub fn is_detected(&self) -> bool {
        self.availability == Availability::Detected
    }

    /// Transform the value, keeping the provenance.
    pub fn map<U>(self, f: impl FnOnce(T) -> U) -> Field<U> {
        Field {
            value: self.value.map(f),
            availability: self.availability,
            confidence: self.confidence,
            source: self.source,
            notes: self.notes,
        }
    }

    /// Lower the confidence of a detected value, for example when a value is
    /// derived from this one. Confidence is never raised.
    pub fn at_most(mut self, confidence: Confidence) -> Self {
        if self.confidence < confidence {
            self.confidence = confidence;
        }
        self
    }

    /// Keep the field only if `keep` accepts the value; otherwise it becomes
    /// [`Availability::Invalid`] with an explanation.
    pub fn validate(self, keep: impl FnOnce(&T) -> bool, why: &str) -> Self {
        match &self.value {
            Some(value) if !keep(value) => Field {
                value: None,
                availability: Availability::Invalid,
                confidence: Confidence::Unknown,
                source: self.source,
                notes: Some(why.to_string()),
            },
            _ => self,
        }
    }
}

impl<T: Clone> Field<T> {
    pub fn cloned_value(&self) -> Option<T> {
        self.value.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detected_fields_carry_value_and_source() {
        let field = Field::detected(42u32, "/sys/x", Confidence::Direct);
        assert_eq!(field.value(), Some(&42));
        assert!(field.is_detected());
        assert_eq!(field.source(), Some("/sys/x"));
        assert_eq!(field.confidence(), Confidence::Direct);
    }

    #[test]
    fn missing_fields_never_hold_a_value() {
        let field: Field<u32> = Field::missing(Availability::PermissionDenied, Some("/x".into()));
        assert_eq!(field.value(), None);
        assert_eq!(field.availability(), Availability::PermissionDenied);
        // Asking for a "detected" missing field is downgraded, not honoured.
        let field: Field<u32> = Field::missing(Availability::Detected, None);
        assert_eq!(field.availability(), Availability::Unknown);
    }

    #[test]
    fn unavailable_is_not_zero() {
        let field: Field<u64> = Field::unsupported("no such interface");
        let json = serde_json::to_value(&field).unwrap();
        assert!(json.get("value").is_none(), "{json}");
        assert_eq!(json["availability"], "unsupported");
    }

    #[test]
    fn round_trip_and_validation_on_deserialise() {
        let field = Field::detected("Dell Inc.".to_string(), "dmi", Confidence::FirmwareReported)
            .with_note("vendor string");
        let json = serde_json::to_string(&field).unwrap();
        let back: Field<String> = serde_json::from_str(&json).unwrap();
        assert_eq!(back, field);

        for bad in [
            r#"{"availability":"detected","confidence":"direct"}"#,
            r#"{"value":5,"availability":"unavailable","confidence":"direct"}"#,
        ] {
            assert!(serde_json::from_str::<Field<u32>>(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn validate_turns_rejected_values_into_invalid() {
        let field = Field::detected(0u64, "size", Confidence::KernelReported)
            .validate(|v| *v > 0, "reported size is zero");
        assert_eq!(field.availability(), Availability::Invalid);
        assert_eq!(field.value(), None);
        assert_eq!(field.notes(), Some("reported size is zero"));
    }

    #[test]
    fn confidence_is_only_ever_lowered() {
        let field = Field::detected(1, "x", Confidence::Direct).at_most(Confidence::Inferred);
        assert_eq!(field.confidence(), Confidence::Inferred);
        let field = Field::detected(1, "x", Confidence::Inferred).at_most(Confidence::Direct);
        assert_eq!(field.confidence(), Confidence::Inferred);
        assert!(Confidence::Direct < Confidence::Inferred);
    }

    #[test]
    fn map_keeps_provenance() {
        let field =
            Field::detected(2048u64, "meminfo", Confidence::KernelReported).map(|kib| kib * 1024);
        assert_eq!(field.value(), Some(&2_097_152));
        assert_eq!(field.source(), Some("meminfo"));
    }
}
