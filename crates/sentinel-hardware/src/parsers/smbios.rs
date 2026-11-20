//! SMBIOS type 17 (Memory Device) records, as found in
//! `/sys/firmware/dmi/entries/17-*/raw`. Those files are readable only by
//! root on most systems; the parser exists so modules are reported when the
//! tables are readable (and is tested on synthetic records). The serial
//! number string is never extracted.

use sentinel_inventory::MemoryModule;

fn word(raw: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes([*raw.get(at)?, *raw.get(at + 1)?]))
}

fn dword(raw: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes([
        *raw.get(at)?,
        *raw.get(at + 1)?,
        *raw.get(at + 2)?,
        *raw.get(at + 3)?,
    ]))
}

/// Strings after the formatted area, 1-indexed.
fn strings(raw: &[u8], formatted_len: usize) -> Vec<String> {
    raw.get(formatted_len..)
        .unwrap_or_default()
        .split(|b| *b == 0)
        .take_while(|s| !s.is_empty())
        .map(|s| String::from_utf8_lossy(s).trim().to_string())
        .collect()
}

fn memory_type(code: u8) -> Option<&'static str> {
    Some(match code {
        0x12 => "DDR",
        0x13 => "DDR2",
        0x18 => "DDR3",
        0x1a => "DDR4",
        0x1b => "LPDDR",
        0x1c => "LPDDR2",
        0x1d => "LPDDR3",
        0x1e => "LPDDR4",
        0x22 => "DDR5",
        0x23 => "LPDDR5",
        _ => return None,
    })
}

fn form_factor(code: u8) -> Option<&'static str> {
    Some(match code {
        0x09 => "DIMM",
        0x0d => "SODIMM",
        0x0f => "FB-DIMM",
        0x10 => "Die",
        0x11 => "CAMM",
        _ => return None,
    })
}

/// Parse one type 17 record. `None` if it is not a type 17 record.
pub fn memory_device(raw: &[u8]) -> Option<MemoryModule> {
    if *raw.first()? != 17 {
        return None;
    }
    let len = usize::from(*raw.get(1)?);
    if len < 0x15 || raw.len() < len {
        return None;
    }
    let texts = strings(raw, len);
    let string = |at: usize| -> Option<String> {
        let index = usize::from(*raw.get(at).filter(|_| at < len)?);
        let value = texts.get(index.checked_sub(1)?)?.clone();
        (!crate::fields::is_placeholder(&value)).then_some(value)
    };
    let size_bytes = match word(raw, 0x0c)? {
        0 | 0xffff => None,
        0x7fff => dword(raw, 0x1c)
            .filter(|_| len >= 0x20)
            .map(|mib| u64::from(mib & 0x7fff_ffff) * 1024 * 1024),
        size if size & 0x8000 != 0 => Some(u64::from(size & 0x7fff) * 1024),
        size => Some(u64::from(size) * 1024 * 1024),
    };
    let speed = |at: usize| word(raw, at).filter(|s| at + 1 < len && *s != 0 && *s != 0xffff);
    Some(MemoryModule {
        locator: string(0x10),
        bank_locator: string(0x11),
        populated: size_bytes.is_some(),
        size_bytes,
        memory_type: raw
            .get(0x12)
            .and_then(|c| memory_type(*c))
            .map(str::to_string),
        form_factor: raw
            .get(0x0e)
            .and_then(|c| form_factor(*c))
            .map(str::to_string),
        speed_mts: speed(0x15).map(u32::from),
        configured_speed_mts: speed(0x20).map(u32::from),
        manufacturer: string(0x17),
        part_number: string(0x1a),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A synthetic 8 GiB DDR4 SODIMM record (no real identifiers).
    pub fn record(size_mib: u16) -> Vec<u8> {
        let mut raw = vec![0u8; 0x22];
        raw[0] = 17;
        raw[1] = 0x22;
        raw[0x0c..0x0e].copy_from_slice(&size_mib.to_le_bytes());
        raw[0x0e] = 0x0d;
        raw[0x10] = 1;
        raw[0x11] = 2;
        raw[0x12] = 0x1a;
        raw[0x15..0x17].copy_from_slice(&3200u16.to_le_bytes());
        raw[0x17] = 3;
        raw[0x18] = 4; // serial: must never be extracted
        raw[0x1a] = 5;
        raw[0x20..0x22].copy_from_slice(&3200u16.to_le_bytes());
        raw.extend_from_slice(b"DIMM A\0BANK 0\0ExampleMem\0SERIAL999\0EX-8G-3200\0\0");
        raw
    }

    #[test]
    fn parses_a_populated_module_without_its_serial() {
        let module = memory_device(&record(8192)).unwrap();
        assert_eq!(module.size_bytes, Some(8 * 1024 * 1024 * 1024));
        assert_eq!(module.memory_type.as_deref(), Some("DDR4"));
        assert_eq!(module.form_factor.as_deref(), Some("SODIMM"));
        assert_eq!(module.speed_mts, Some(3200));
        assert_eq!(module.locator.as_deref(), Some("DIMM A"));
        assert_eq!(module.manufacturer.as_deref(), Some("ExampleMem"));
        assert_eq!(module.part_number.as_deref(), Some("EX-8G-3200"));
        assert!(!format!("{module:?}").contains("SERIAL999"));
    }

    #[test]
    fn empty_slots_and_bad_records() {
        let module = memory_device(&record(0)).unwrap();
        assert!(!module.populated);
        assert_eq!(module.size_bytes, None);
        assert_eq!(memory_device(&[16, 4, 0, 0]), None);
        assert_eq!(memory_device(&[17, 0x22, 0]), None);
    }
}
