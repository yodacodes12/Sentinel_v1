//! A local hardware fingerprint.
//!
//! It answers one question: "is this the same hardware as the inventory I
//! cached?" It is built only from non-sensitive model identifiers
//! (manufacturer, product, board name and revision, CPU model and the sorted
//! PCI vendor:device IDs). Serial numbers, UUIDs and MAC addresses are never
//! inputs.
//!
//! Limitations, by construction:
//! - Two machines of the same model and configuration share a fingerprint.
//!   It identifies a hardware *profile*, not a unique machine.
//! - Adding or removing a PCI device (for example a Thunderbolt dock with a
//!   PCI bridge) changes it. USB devices are deliberately not included.
//! - A firmware update that changes DMI strings changes it.
//! - It is a 64-bit FNV-1a hash: stable and cheap, not a secret and not
//!   collision-resistant against deliberate attack. It is never sent anywhere.

use serde::{Deserialize, Serialize};

pub const FINGERPRINT_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HardwareFingerprint {
    pub version: u32,
    /// 16 hex digits.
    pub value: String,
    /// Names of the components that went in (not their values), so the UI
    /// can say what the fingerprint is based on.
    pub inputs: Vec<String>,
}

/// Inputs as `(name, value)`; `None` values are recorded as absent so a
/// missing field does not collide with an empty one.
pub fn fingerprint(inputs: &[(&str, Option<&str>)], pci_ids: &[(u16, u16)]) -> HardwareFingerprint {
    let mut canonical = format!("v{FINGERPRINT_VERSION}");
    let mut names = Vec::new();
    for (name, value) in inputs {
        canonical.push('\u{1f}');
        canonical.push_str(name);
        canonical.push('=');
        match value {
            Some(v) => {
                canonical.push_str(v.trim());
                names.push((*name).to_string());
            }
            None => canonical.push('\u{0}'),
        }
    }
    let mut ids: Vec<(u16, u16)> = pci_ids.to_vec();
    ids.sort_unstable();
    ids.dedup();
    canonical.push_str("\u{1f}pci=");
    for (vendor, device) in &ids {
        canonical.push_str(&format!("{vendor:04x}:{device:04x},"));
    }
    if !ids.is_empty() {
        names.push("pci_device_ids".into());
    }
    HardwareFingerprint {
        version: FINGERPRINT_VERSION,
        value: format!("{:016x}", fnv1a64(canonical.as_bytes())),
        inputs: names,
    }
}

fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fnv_matches_reference_vectors() {
        assert_eq!(fnv1a64(b""), 0xcbf29ce484222325);
        assert_eq!(fnv1a64(b"a"), 0xaf63dc4c8601ec8c);
        assert_eq!(fnv1a64(b"foobar"), 0x85944171f73967e8);
    }

    #[test]
    fn stable_and_order_independent_for_pci_ids() {
        let inputs = [
            ("manufacturer", Some("Example")),
            ("product", Some("Book 14")),
        ];
        let a = fingerprint(&inputs, &[(0x1022, 0x1234), (0x8086, 0x0001)]);
        let b = fingerprint(
            &inputs,
            &[(0x8086, 0x0001), (0x1022, 0x1234), (0x1022, 0x1234)],
        );
        assert_eq!(a, b);
        assert_eq!(a.value.len(), 16);
        assert_eq!(a.inputs, ["manufacturer", "product", "pci_device_ids"]);
    }

    #[test]
    fn missing_differs_from_empty_and_values_matter() {
        let missing = fingerprint(&[("board", None)], &[]);
        let empty = fingerprint(&[("board", Some(""))], &[]);
        let other = fingerprint(&[("board", Some("X1"))], &[]);
        assert_ne!(missing.value, empty.value);
        assert_ne!(empty.value, other.value);
        assert!(missing.inputs.is_empty());
    }
}
