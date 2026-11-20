//! Data only: system manufacturers Sentinel recognises, how firmware spells
//! them, and the domains they publish documentation on. Used to normalise
//! names, to recognise a document's publisher (source trust) and to notice a
//! document naming a different manufacturer (contradicting evidence).
//!
//! A vendor that is not listed is still usable: its name is kept as reported,
//! and documents can still match it by model; only domain-based trust is
//! unavailable for it.

pub struct Vendor {
    pub key: &'static str,
    pub canonical: &'static str,
    /// Normalised spellings (lower case, punctuation and company suffixes
    /// removed), matched as whole leading words.
    pub aliases: &'static [&'static str],
    /// Registrable domains; subdomains count (`dl.dell.com`).
    pub domains: &'static [&'static str],
}

pub const VENDORS: &[Vendor] = &[
    Vendor {
        key: "dell",
        canonical: "Dell",
        aliases: &["dell", "alienware"],
        domains: &["dell.com", "delltechnologies.com"],
    },
    Vendor {
        key: "lenovo",
        canonical: "Lenovo",
        aliases: &["lenovo"],
        domains: &["lenovo.com", "lenovo.com.cn"],
    },
    Vendor {
        key: "hp",
        canonical: "HP",
        aliases: &["hp", "hewlett packard", "hewlettpackard"],
        domains: &["hp.com", "hpe.com"],
    },
    Vendor {
        key: "asus",
        canonical: "ASUS",
        aliases: &["asus", "asustek"],
        domains: &["asus.com"],
    },
    Vendor {
        key: "acer",
        canonical: "Acer",
        aliases: &["acer"],
        domains: &["acer.com"],
    },
    Vendor {
        key: "msi",
        canonical: "MSI",
        aliases: &["msi", "micro star"],
        domains: &["msi.com"],
    },
    Vendor {
        key: "gigabyte",
        canonical: "Gigabyte",
        aliases: &["gigabyte"],
        domains: &["gigabyte.com"],
    },
    Vendor {
        key: "apple",
        canonical: "Apple",
        aliases: &["apple"],
        domains: &["apple.com"],
    },
    Vendor {
        key: "samsung",
        canonical: "Samsung",
        aliases: &["samsung"],
        domains: &["samsung.com"],
    },
    Vendor {
        key: "microsoft",
        canonical: "Microsoft",
        aliases: &["microsoft"],
        domains: &["microsoft.com"],
    },
    Vendor {
        key: "toshiba",
        canonical: "Toshiba / Dynabook",
        aliases: &["toshiba", "dynabook"],
        domains: &["toshiba.com", "dynabook.com"],
    },
    Vendor {
        key: "framework",
        canonical: "Framework",
        aliases: &["framework"],
        domains: &["frame.work"],
    },
    Vendor {
        key: "razer",
        canonical: "Razer",
        aliases: &["razer"],
        domains: &["razer.com"],
    },
    Vendor {
        key: "system76",
        canonical: "System76",
        aliases: &["system76"],
        domains: &["system76.com"],
    },
    Vendor {
        key: "tuxedo",
        canonical: "TUXEDO",
        aliases: &["tuxedo"],
        domains: &["tuxedocomputers.com"],
    },
    Vendor {
        key: "huawei",
        canonical: "Huawei",
        aliases: &["huawei"],
        domains: &["huawei.com"],
    },
    Vendor {
        key: "fujitsu",
        canonical: "Fujitsu",
        aliases: &["fujitsu"],
        domains: &["fujitsu.com"],
    },
    Vendor {
        key: "panasonic",
        canonical: "Panasonic",
        aliases: &["panasonic"],
        domains: &["panasonic.com", "panasonic.net"],
    },
    Vendor {
        key: "clevo",
        canonical: "Clevo",
        aliases: &["clevo"],
        domains: &["clevo.com.tw"],
    },
    Vendor {
        key: "asrock",
        canonical: "ASRock",
        aliases: &["asrock"],
        domains: &["asrock.com"],
    },
    Vendor {
        key: "supermicro",
        canonical: "Supermicro",
        aliases: &["supermicro", "super micro"],
        domains: &["supermicro.com"],
    },
    Vendor {
        key: "google",
        canonical: "Google",
        aliases: &["google"],
        domains: &["google.com", "chromium.org"],
    },
];

/// Original design manufacturers. Their names appear on schematics of
/// boards sold under other brands, so they are context, never a conflict.
pub const BOARD_DESIGNERS: &[&str] = &[
    "compal",
    "quanta",
    "wistron",
    "pegatron",
    "inventec",
    "foxconn",
    "lcfc",
    "flextronics",
    "bitland",
    "tongfang",
];

/// Component vendors named in almost any document; never a conflict.
pub const COMPONENT_VENDORS: &[&str] = &[
    "intel",
    "amd",
    "nvidia",
    "realtek",
    "qualcomm",
    "mediatek",
    "broadcom",
    "micron",
    "sk hynix",
    "kioxia",
    "western digital",
];

/// Community sites (category C). Being listed says nothing about whether an
/// upload there is legitimately shared.
pub const COMMUNITY_DOMAINS: &[(&str, &str)] = &[
    ("archive.org", "Internet Archive"),
    ("ifixit.com", "iFixit"),
    ("github.com", "GitHub"),
    ("githubusercontent.com", "GitHub"),
];

const SUFFIXES: &[&str] = &[
    "inc",
    "incorporated",
    "corp",
    "corporation",
    "co",
    "company",
    "ltd",
    "limited",
    "llc",
    "gmbh",
    "ag",
    "sa",
    "srl",
    "bv",
    "plc",
    "computer",
    "computers",
    "technology",
    "technologies",
    "international",
    "electronics",
    "group",
    "holdings",
];

/// Lower case, punctuation as spaces, company suffixes dropped.
pub fn normalise_name(name: &str) -> String {
    let words: Vec<String> = name
        .to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(str::to_string)
        .collect();
    let mut kept: Vec<&str> = words.iter().map(String::as_str).collect();
    while kept.len() > 1 && kept.last().is_some_and(|w| SUFFIXES.contains(w)) {
        kept.pop();
    }
    kept.join(" ")
}

/// The vendor a manufacturer name refers to, matched on whole leading words.
pub fn vendor_for(name: &str) -> Option<&'static Vendor> {
    let norm = normalise_name(name);
    VENDORS.iter().find(|v| {
        v.aliases.iter().any(|a| {
            norm == *a
                || norm.starts_with(&format!("{a} "))
                || norm.replace(' ', "") == a.replace(' ', "")
        })
    })
}

pub fn by_key(key: &str) -> Option<&'static Vendor> {
    VENDORS.iter().find(|v| v.key == key)
}

/// True when `host` is `domain` or a subdomain of it.
pub fn host_in(host: &str, domain: &str) -> bool {
    let host = host.trim_end_matches('.').to_ascii_lowercase();
    host == domain || host.ends_with(&format!(".{domain}"))
}

/// The vendor whose official domain hosts `host`.
pub fn vendor_for_host(host: &str) -> Option<&'static Vendor> {
    VENDORS
        .iter()
        .find(|v| v.domains.iter().any(|d| host_in(host, d)))
}

pub fn community_site(host: &str) -> Option<&'static str> {
    COMMUNITY_DOMAINS
        .iter()
        .find(|(d, _)| host_in(host, d))
        .map(|(_, n)| *n)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn firmware_spellings_map_to_one_vendor() {
        for (name, key) in [
            ("Dell Inc.", "dell"),
            ("LENOVO", "lenovo"),
            ("Hewlett-Packard", "hp"),
            ("HP", "hp"),
            ("ASUSTeK COMPUTER INC.", "asus"),
            ("Micro-Star International Co., Ltd.", "msi"),
            ("Gigabyte Technology Co., Ltd.", "gigabyte"),
            ("SAMSUNG ELECTRONICS CO., LTD.", "samsung"),
            ("Framework", "framework"),
            ("Dynabook Inc.", "toshiba"),
        ] {
            assert_eq!(vendor_for(name).map(|v| v.key), Some(key), "{name}");
        }
    }

    #[test]
    fn unknown_or_partial_names_are_not_forced_onto_a_vendor() {
        assert!(vendor_for("Example Corp").is_none());
        // "HPC Systems" is not HP; "Dellwood" is not Dell.
        assert!(vendor_for("HPC Systems").is_none());
        assert!(vendor_for("Dellwood Ltd").is_none());
    }

    #[test]
    fn domains_match_whole_labels() {
        assert!(host_in("dl.dell.com", "dell.com"));
        assert!(host_in("DELL.COM.", "dell.com"));
        assert!(!host_in("notdell.com", "dell.com"));
        assert!(!host_in("dell.com.evil.example", "dell.com"));
        assert_eq!(
            vendor_for_host("download.lenovo.com").map(|v| v.key),
            Some("lenovo")
        );
        assert_eq!(
            community_site("ia800.us.archive.org"),
            Some("Internet Archive")
        );
    }
}
