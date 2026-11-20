//! Microarchitecture names from CPUID family and model.
//!
//! This is a lookup table, so the result is always *inferred*: a model
//! number Sentinel does not know returns `None` rather than a guess, and the
//! table covers common client parts only.

/// `(vendor_id, family, model) -> name`.
pub fn lookup(vendor_id: &str, family: u32, model: u32) -> Option<&'static str> {
    match vendor_id {
        "AuthenticAMD" | "HygonGenuine" => amd(family, model),
        "GenuineIntel" => intel(family, model),
        _ => None,
    }
}

fn amd(family: u32, model: u32) -> Option<&'static str> {
    Some(match (family, model) {
        (0x15, _) => "Bulldozer family",
        (0x16, _) => "Jaguar/Puma",
        (0x17, 0x00..=0x0f) => "Zen",
        (0x17, 0x10..=0x2f) => "Zen / Zen+",
        (0x17, 0x30..=0x3f | 0x47 | 0x60..=0x7f | 0x84..=0x87 | 0x90..=0xaf) => "Zen 2",
        (0x19, 0x00..=0x0f | 0x20..=0x5f) => "Zen 3",
        (0x19, 0x10..=0x1f | 0x60..=0x7f | 0xa0..=0xaf) => "Zen 4",
        (0x1a, _) => "Zen 5",
        _ => return None,
    })
}

fn intel(family: u32, model: u32) -> Option<&'static str> {
    if family != 6 {
        return None;
    }
    Some(match model {
        0x3c | 0x3f | 0x45 | 0x46 => "Haswell",
        0x3d | 0x47 | 0x4f | 0x56 => "Broadwell",
        0x4e | 0x5e | 0x55 => "Skylake",
        0x8e | 0x9e => "Kaby Lake / Coffee Lake",
        0xa5 | 0xa6 => "Comet Lake",
        0x7d | 0x7e | 0x6a | 0x6c => "Ice Lake",
        0x8c | 0x8d => "Tiger Lake",
        0xa7 => "Rocket Lake",
        0x97 | 0x9a | 0xbe => "Alder Lake",
        0xb7 | 0xba | 0xbf => "Raptor Lake",
        0xaa | 0xac => "Meteor Lake",
        0xbd => "Lunar Lake",
        0xc5 | 0xc6 => "Arrow Lake",
        0x5c | 0x5f => "Goldmont",
        0x7a => "Goldmont Plus",
        0x86 | 0x96 | 0x9c => "Tremont",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_parts() {
        // Ryzen 3 7320U (Mendocino): family 0x17, model 0xa0.
        assert_eq!(lookup("AuthenticAMD", 23, 160), Some("Zen 2"));
        // Ryzen 7 5700U (Lucienne): family 0x17, model 0x68.
        assert_eq!(lookup("AuthenticAMD", 23, 0x68), Some("Zen 2"));
        assert_eq!(lookup("AuthenticAMD", 25, 0x50), Some("Zen 3"));
        assert_eq!(lookup("AuthenticAMD", 25, 0x74), Some("Zen 4"));
        assert_eq!(lookup("GenuineIntel", 6, 0x9a), Some("Alder Lake"));
        assert_eq!(lookup("GenuineIntel", 6, 0x8c), Some("Tiger Lake"));
    }

    #[test]
    fn unknown_parts_are_not_guessed() {
        assert_eq!(lookup("GenuineIntel", 6, 0x01), None);
        assert_eq!(lookup("GenuineIntel", 15, 0x04), None);
        assert_eq!(lookup("AuthenticAMD", 0x10, 0x04), None);
        assert_eq!(lookup("SomethingElse", 6, 0x9a), None);
    }
}
