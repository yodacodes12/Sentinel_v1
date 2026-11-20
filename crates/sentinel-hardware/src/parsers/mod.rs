//! Pure parsers for the text and binary formats Linux exposes. They take
//! strings or bytes and never touch the filesystem, so they are tested on
//! literal inputs.

pub mod classes;
pub mod cpuinfo;
pub mod ids;
pub mod microarch;
pub mod smbios;

use std::collections::BTreeMap;

/// Parse a kernel CPU list such as `0-3,8,10-11`.
pub fn cpu_list(text: &str) -> Option<Vec<u32>> {
    let text = text.trim();
    if text.is_empty() {
        return Some(Vec::new());
    }
    let mut cpus = Vec::new();
    for part in text.split(',') {
        let part = part.trim();
        match part.split_once('-') {
            Some((start, end)) => {
                let (start, end): (u32, u32) = (start.parse().ok()?, end.parse().ok()?);
                if end < start || end - start > 65_536 {
                    return None;
                }
                cpus.extend(start..=end);
            }
            None => cpus.push(part.parse().ok()?),
        }
    }
    cpus.sort_unstable();
    cpus.dedup();
    Some(cpus)
}

/// `KEY=VALUE` lines (uevent files, os-release). Quotes around values are
/// removed; comments and malformed lines are skipped.
pub fn key_values(text: &str) -> BTreeMap<String, String> {
    text.lines()
        .filter_map(|line| {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                return None;
            }
            let (key, value) = line.split_once('=')?;
            let key = key.trim();
            if key.is_empty() || !key.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
                return None;
            }
            Some((key.to_string(), unquote(value.trim())))
        })
        .collect()
}

fn unquote(value: &str) -> String {
    let stripped = value
        .strip_prefix('"')
        .and_then(|v| v.strip_suffix('"'))
        .or_else(|| value.strip_prefix('\'').and_then(|v| v.strip_suffix('\'')));
    match stripped {
        Some(inner) => inner
            .replace("\\\"", "\"")
            .replace("\\$", "$")
            .replace("\\`", "`")
            .replace("\\\\", "\\"),
        None => value.to_string(),
    }
}

/// `/proc/meminfo`: values in bytes, keyed by field name.
pub fn meminfo(text: &str) -> BTreeMap<String, u64> {
    text.lines()
        .filter_map(|line| {
            let (key, rest) = line.split_once(':')?;
            let mut parts = rest.split_whitespace();
            let number: u64 = parts.next()?.parse().ok()?;
            let bytes = match parts.next() {
                Some("kB") => number.checked_mul(1024)?,
                None => number,
                Some(_) => return None,
            };
            Some((key.trim().to_string(), bytes))
        })
        .collect()
}

/// Cache sizes as sysfs writes them: `32K`, `4096K`, `16M`, or plain bytes.
pub fn cache_size(text: &str) -> Option<u64> {
    let text = text.trim();
    let (digits, multiplier) = match text.chars().last()? {
        'K' | 'k' => (&text[..text.len() - 1], 1024),
        'M' | 'm' => (&text[..text.len() - 1], 1024 * 1024),
        'G' | 'g' => (&text[..text.len() - 1], 1024 * 1024 * 1024),
        c if c.is_ascii_digit() => (text, 1),
        _ => return None,
    };
    digits.trim().parse::<u64>().ok()?.checked_mul(multiplier)
}

/// A hexadecimal sysfs value such as `0x1022`.
pub fn hex_u32(text: &str) -> Option<u32> {
    let text = text.trim();
    let digits = text
        .strip_prefix("0x")
        .or_else(|| text.strip_prefix("0X"))
        .unwrap_or(text);
    u32::from_str_radix(digits, 16).ok()
}

pub fn hex_u16(text: &str) -> Option<u16> {
    hex_u32(text).and_then(|v| u16::try_from(v).ok())
}

/// USB speed in Mbit/s as sysfs writes it (`1.5`, `12`, `480`, `5000`),
/// converted to kbit/s so it stays an integer.
pub fn usb_speed_kbps(text: &str) -> Option<u64> {
    let text = text.trim();
    match text.split_once('.') {
        Some((whole, fraction)) => {
            let whole: u64 = whole.parse().ok()?;
            let fraction = format!("{fraction:0<3}");
            let fraction: u64 = fraction.get(..3)?.parse().ok()?;
            Some(whole * 1000 + fraction)
        }
        None => text.parse::<u64>().ok().map(|mbps| mbps * 1000),
    }
}

/// `/proc/mounts` escapes spaces and similar as octal (`\040`).
pub fn unescape_mount(path: &str) -> String {
    let bytes = path.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'\\'
            && i + 3 < bytes.len()
            && bytes[i + 1..i + 4]
                .iter()
                .all(|b| (b'0'..=b'7').contains(b))
        {
            let v = (bytes[i + 1] - b'0') * 64 + (bytes[i + 2] - b'0') * 8 + (bytes[i + 3] - b'0');
            out.push(v);
            i += 4;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// One `/proc/mounts` entry backed by a device node.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MountEntry {
    pub device: String,
    pub mount_point: String,
    pub fs_type: String,
    pub read_only: bool,
}

/// Every mount whose source is a device node (`/dev/...`), in table order.
/// Pseudo filesystems (proc, tmpfs, overlay without a device) are skipped.
pub fn mounts(text: &str) -> Vec<MountEntry> {
    text.lines()
        .filter_map(|line| {
            let f: Vec<&str> = line.split_whitespace().collect();
            if f.len() < 4 || !f[0].starts_with("/dev/") {
                return None;
            }
            Some(MountEntry {
                device: unescape_mount(f[0]),
                mount_point: unescape_mount(f[1]),
                fs_type: f[2].to_string(),
                read_only: f[3].split(',').any(|o| o == "ro"),
            })
        })
        .collect()
}

/// Device nodes with active swap, from `/proc/swaps` (swap files skipped).
pub fn swap_devices(text: &str) -> Vec<String> {
    text.lines()
        .skip(1)
        .filter_map(|line| {
            let mut f = line.split_whitespace();
            let name = f.next()?;
            (f.next()? == "partition").then(|| unescape_mount(name))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cpu_lists() {
        assert_eq!(cpu_list("0-3").unwrap(), [0, 1, 2, 3]);
        assert_eq!(cpu_list("0,2,4-5\n").unwrap(), [0, 2, 4, 5]);
        assert_eq!(cpu_list("7").unwrap(), [7]);
        assert_eq!(cpu_list("").unwrap(), Vec::<u32>::new());
        assert_eq!(cpu_list("3-1"), None);
        assert_eq!(cpu_list("a-b"), None);
        assert_eq!(cpu_list("0-4000000"), None);
    }

    #[test]
    fn key_value_files() {
        let parsed = key_values(
            "# comment\nNAME=\"Ubuntu\"\nID=ubuntu\nID_LIKE=debian\nPRETTY_NAME='Ubuntu 26.04'\n\
             bad line\n=nokey\nVERSION=\"26.04 \\\"LTS\\\"\"\n",
        );
        assert_eq!(parsed["NAME"], "Ubuntu");
        assert_eq!(parsed["ID"], "ubuntu");
        assert_eq!(parsed["PRETTY_NAME"], "Ubuntu 26.04");
        assert_eq!(parsed["VERSION"], "26.04 \"LTS\"");
        assert_eq!(parsed.len(), 5);
    }

    #[test]
    fn meminfo_values_are_bytes() {
        let parsed = meminfo(
            "MemTotal:        6777356 kB\nMemFree: 353920 kB\nHugePages_Total:       0\n\
             Broken: x kB\nOdd: 5 MB\n",
        );
        assert_eq!(parsed["MemTotal"], 6_777_356 * 1024);
        assert_eq!(parsed["HugePages_Total"], 0);
        assert!(!parsed.contains_key("Broken"));
        assert!(!parsed.contains_key("Odd"));
    }

    #[test]
    fn cache_sizes() {
        assert_eq!(cache_size("32K"), Some(32 * 1024));
        assert_eq!(cache_size("4096K\n"), Some(4 * 1024 * 1024));
        assert_eq!(cache_size("16M"), Some(16 * 1024 * 1024));
        assert_eq!(cache_size("512"), Some(512));
        assert_eq!(cache_size("big"), None);
        assert_eq!(cache_size(""), None);
    }

    #[test]
    fn hex_values() {
        assert_eq!(hex_u16("0x1022\n"), Some(0x1022));
        assert_eq!(hex_u32("0x030000"), Some(0x030000));
        assert_eq!(hex_u16("1d6b"), Some(0x1d6b));
        assert_eq!(hex_u16("0x123456"), None);
        assert_eq!(hex_u16("zz"), None);
    }

    #[test]
    fn usb_speeds() {
        assert_eq!(usb_speed_kbps("1.5"), Some(1_500));
        assert_eq!(usb_speed_kbps("12"), Some(12_000));
        assert_eq!(usb_speed_kbps("480\n"), Some(480_000));
        assert_eq!(usb_speed_kbps("10000"), Some(10_000_000));
        assert_eq!(usb_speed_kbps("fast"), None);
    }

    #[test]
    fn mount_table_keeps_every_device_mount() {
        let text = "/dev/nvme0n1p2 / ext4 rw,relatime 0 0\n\
                    proc /proc proc rw 0 0\n\
                    /dev/nvme0n1p1 /boot/efi vfat ro,relatime 0 0\n\
                    /dev/mapper/vg-home /home/My\\040Files btrfs rw 0 0\n\
                    /dev/mapper/vg-home /srv btrfs rw 0 0\n";
        let m = mounts(text);
        assert_eq!(m.len(), 4);
        assert_eq!(m[1].mount_point, "/boot/efi");
        assert!(m[1].read_only && !m[0].read_only);
        assert_eq!(m[2].mount_point, "/home/My Files");
        assert_eq!(m[3].device, "/dev/mapper/vg-home");
    }

    #[test]
    fn swap_partitions_only() {
        let text = "Filename\tType\tSize\tUsed\tPriority\n\
                    /swap.img                               file\t\t4194300\t0\t-2\n\
                    /dev/nvme0n1p3                          partition\t8388604\t0\t-3\n";
        assert_eq!(swap_devices(text), ["/dev/nvme0n1p3"]);
        assert!(swap_devices("").is_empty());
    }
}
