//! `/proc/cpuinfo`.
//!
//! Used for identification only (vendor, model, family, flags). Topology is
//! never taken from here: counting `processor` lines says nothing about
//! cores, and `core id`/`physical id` are absent on many architectures.
//! Topology comes from `/sys/devices/system/cpu/cpuN/topology`.

use std::collections::BTreeMap;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CpuInfoEntry {
    pub processor: Option<u32>,
    pub vendor_id: Option<String>,
    pub model_name: Option<String>,
    pub family: Option<u32>,
    pub model: Option<u32>,
    pub stepping: Option<u32>,
    pub physical_id: Option<u32>,
    pub flags: Vec<String>,
    /// ARM: `CPU implementer` (hex).
    pub implementer: Option<u32>,
    /// ARM: `CPU part` (hex).
    pub part: Option<u32>,
}

pub fn parse(text: &str) -> Vec<CpuInfoEntry> {
    let mut entries = Vec::new();
    let mut fields: BTreeMap<String, String> = BTreeMap::new();
    let mut flush = |fields: &mut BTreeMap<String, String>| {
        if !fields.is_empty() {
            entries.push(entry(fields));
            fields.clear();
        }
    };
    for line in text.lines() {
        if line.trim().is_empty() {
            flush(&mut fields);
            continue;
        }
        if let Some((key, value)) = line.split_once(':') {
            fields.insert(key.trim().to_string(), value.trim().to_string());
        }
    }
    flush(&mut fields);
    entries
}

fn entry(fields: &BTreeMap<String, String>) -> CpuInfoEntry {
    let get = |key: &str| fields.get(key).filter(|v| !v.is_empty()).cloned();
    let number = |key: &str| get(key).and_then(|v| v.parse::<u32>().ok());
    let hex = |key: &str| get(key).and_then(|v| super::hex_u32(&v));
    let flags = get("flags")
        .or_else(|| get("Features"))
        .map(|v| v.split_whitespace().map(str::to_string).collect())
        .unwrap_or_default();
    CpuInfoEntry {
        processor: number("processor"),
        vendor_id: get("vendor_id"),
        model_name: get("model name").or_else(|| get("Processor")),
        family: number("cpu family"),
        model: number("model"),
        stepping: number("stepping"),
        physical_id: number("physical id"),
        flags,
        implementer: hex("CPU implementer"),
        part: hex("CPU part"),
    }
}

/// Human vendor name for a CPUID vendor string.
pub fn vendor_name(vendor_id: &str) -> Option<&'static str> {
    Some(match vendor_id {
        "AuthenticAMD" => "AMD",
        "GenuineIntel" => "Intel",
        "HygonGenuine" => "Hygon",
        "CentaurHauls" => "Centaur",
        "  Shanghai  " | "Shanghai" => "Zhaoxin",
        "GenuineTMx86" | "TransmetaCPU" => "Transmeta",
        "CyrixInstead" => "Cyrix",
        "VIA VIA VIA " => "VIA",
        _ => return None,
    })
}

/// Human vendor name for an ARM implementer code.
pub fn arm_implementer_name(code: u32) -> Option<&'static str> {
    Some(match code {
        0x41 => "ARM",
        0x42 => "Broadcom",
        0x43 => "Cavium",
        0x48 => "HiSilicon",
        0x4e => "NVIDIA",
        0x51 => "Qualcomm",
        0x53 => "Samsung",
        0x61 => "Apple",
        0x6d => "Microsoft",
        0xc0 => "Ampere",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const AMD: &str = "processor\t: 0\nvendor_id\t: AuthenticAMD\ncpu family\t: 23\n\
        model\t\t: 160\nmodel name\t: AMD Ryzen 3 7320U with Radeon Graphics\nstepping\t: 0\n\
        physical id\t: 0\ncore id\t\t: 0\nflags\t\t: fpu vme sse2 avx2\n\n\
        processor\t: 1\nvendor_id\t: AuthenticAMD\ncpu family\t: 23\nmodel\t\t: 160\n\
        model name\t: AMD Ryzen 3 7320U with Radeon Graphics\nstepping\t: 0\nflags\t\t: fpu\n";

    #[test]
    fn x86_entries() {
        let entries = parse(AMD);
        assert_eq!(entries.len(), 2);
        let first = &entries[0];
        assert_eq!(first.processor, Some(0));
        assert_eq!(first.vendor_id.as_deref(), Some("AuthenticAMD"));
        assert_eq!(first.family, Some(23));
        assert_eq!(first.model, Some(160));
        assert_eq!(first.stepping, Some(0));
        assert_eq!(first.physical_id, Some(0));
        assert_eq!(first.flags, ["fpu", "vme", "sse2", "avx2"]);
        assert_eq!(
            first.model_name.as_deref(),
            Some("AMD Ryzen 3 7320U with Radeon Graphics")
        );
    }

    #[test]
    fn arm_entries_have_no_x86_fields() {
        let entries = parse(
            "processor\t: 0\nBogoMIPS\t: 48.00\nFeatures\t: fp asimd evtstrm\n\
             CPU implementer\t: 0x41\nCPU architecture: 8\nCPU part\t: 0xd08\n\n",
        );
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].implementer, Some(0x41));
        assert_eq!(entries[0].part, Some(0xd08));
        assert_eq!(entries[0].flags, ["fp", "asimd", "evtstrm"]);
        assert_eq!(entries[0].vendor_id, None);
        assert_eq!(entries[0].family, None);
        assert_eq!(arm_implementer_name(0x41), Some("ARM"));
    }

    #[test]
    fn garbage_is_tolerated() {
        assert!(parse("").is_empty());
        let entries = parse("processor : x\nmodel : not-a-number\n");
        assert_eq!(entries[0].processor, None);
        assert_eq!(entries[0].model, None);
    }

    #[test]
    fn vendors() {
        assert_eq!(vendor_name("GenuineIntel"), Some("Intel"));
        assert_eq!(vendor_name("AuthenticAMD"), Some("AMD"));
        assert_eq!(vendor_name("Nobody"), None);
    }
}
