//! Lookups in the `pci.ids` / `usb.ids` databases shipped by the
//! distribution (hwdata).
//!
//! The databases are a megabyte or more. They are streamed once per scan,
//! only the IDs actually present on the machine are collected, and reading
//! stops at the class section. Names found here are labelled
//! [`Confidence::Database`](sentinel_inventory::Confidence::Database): they
//! are community-maintained and can be generic or outdated.

use std::collections::{BTreeMap, BTreeSet};
use std::ops::ControlFlow;

#[derive(Debug, Default, Clone)]
pub struct IdQuery {
    pub vendors: BTreeSet<u16>,
    pub devices: BTreeSet<(u16, u16)>,
    /// `(vendor, device, subsystem vendor, subsystem device)`.
    pub subsystems: BTreeSet<(u16, u16, u16, u16)>,
}

impl IdQuery {
    pub fn add(&mut self, vendor: u16, device: u16) {
        self.vendors.insert(vendor);
        self.devices.insert((vendor, device));
    }

    pub fn add_subsystem(&mut self, vendor: u16, device: u16, sub_vendor: u16, sub_device: u16) {
        self.add(vendor, device);
        self.vendors.insert(sub_vendor);
        self.subsystems
            .insert((vendor, device, sub_vendor, sub_device));
    }

    pub fn is_empty(&self) -> bool {
        self.vendors.is_empty()
    }
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct IdNames {
    pub vendors: BTreeMap<u16, String>,
    pub devices: BTreeMap<(u16, u16), String>,
    pub subsystems: BTreeMap<(u16, u16, u16, u16), String>,
}

/// Incremental parser fed one line at a time.
pub struct IdParser<'q> {
    query: &'q IdQuery,
    names: IdNames,
    vendor: Option<u16>,
    device: Option<u16>,
}

impl<'q> IdParser<'q> {
    pub fn new(query: &'q IdQuery) -> Self {
        Self {
            query,
            names: IdNames::default(),
            vendor: None,
            device: None,
        }
    }

    pub fn line(&mut self, line: &str) -> ControlFlow<()> {
        if line.is_empty() || line.starts_with('#') {
            return ControlFlow::Continue(());
        }
        if let Some(rest) = line.strip_prefix("\t\t") {
            // Subsystem: "ssss dddd  name" (pci.ids only).
            if let (Some(vendor), Some(device)) = (self.vendor, self.device)
                && let Some((ids, name)) = rest.split_once("  ")
                && let Some((sv, sd)) = ids.split_once(' ')
                && let (Some(sv), Some(sd)) = (hex4(sv), hex4(sd))
                && self.query.subsystems.contains(&(vendor, device, sv, sd))
            {
                self.names
                    .subsystems
                    .insert((vendor, device, sv, sd), name.trim().to_string());
            }
            return ControlFlow::Continue(());
        }
        if let Some(rest) = line.strip_prefix('\t') {
            self.device = None;
            if let Some(vendor) = self.vendor
                && let Some((id, name)) = rest.split_once("  ")
                && let Some(device) = hex4(id)
            {
                self.device = Some(device);
                if self.query.devices.contains(&(vendor, device)) {
                    self.names
                        .devices
                        .insert((vendor, device), name.trim().to_string());
                }
            }
            return ControlFlow::Continue(());
        }
        // A top-level line: a vendor, or the start of the class lists.
        self.device = None;
        match line
            .split_once("  ")
            .and_then(|(id, name)| Some((hex4(id)?, name)))
        {
            Some((vendor, name)) => {
                if self.query.vendors.contains(&vendor) {
                    self.vendor = Some(vendor);
                    self.names.vendors.insert(vendor, name.trim().to_string());
                } else {
                    // Devices of a vendor we do not need are skipped cheaply.
                    self.vendor = if self.query.devices.iter().any(|(v, _)| *v == vendor) {
                        Some(vendor)
                    } else {
                        None
                    };
                }
                ControlFlow::Continue(())
            }
            None => ControlFlow::Break(()),
        }
    }

    pub fn finish(self) -> IdNames {
        self.names
    }
}

fn hex4(text: &str) -> Option<u16> {
    if text.len() == 4 {
        u16::from_str_radix(text, 16).ok()
    } else {
        None
    }
}

/// Convenience for tests and small inputs.
pub fn lookup(text: &str, query: &IdQuery) -> IdNames {
    let mut parser = IdParser::new(query);
    for line in text.lines() {
        if parser.line(line).is_break() {
            break;
        }
    }
    parser.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    const PCI_IDS: &str = "# comment\n\
        1002  Advanced Micro Devices, Inc. [AMD/ATI]\n\
        \t1506  Mendocino [Radeon 610M]\n\
        \t\t1028 0c3e  Example subsystem\n\
        \t1640  Rembrandt Radeon High Definition Audio Controller\n\
        1022  Advanced Micro Devices, Inc. [AMD]\n\
        \t14b5  Family 17h-19h PCIe Root Complex\n\
        10de  NVIDIA Corporation\n\
        \t2520  GA106M [GeForce RTX 3060 Mobile / Max-Q]\n\
        C 00  Unclassified device\n\
        1002  This must never be read\n";

    #[test]
    fn finds_only_requested_ids_and_stops_at_classes() {
        let mut query = IdQuery::default();
        query.add_subsystem(0x1002, 0x1506, 0x1028, 0x0c3e);
        query.add(0x1022, 0x14b5);
        query.add(0x10de, 0xffff); // unknown device of a known vendor
        let names = lookup(PCI_IDS, &query);

        assert_eq!(
            names.vendors[&0x1002],
            "Advanced Micro Devices, Inc. [AMD/ATI]"
        );
        assert_eq!(names.devices[&(0x1002, 0x1506)], "Mendocino [Radeon 610M]");
        assert_eq!(
            names.subsystems[&(0x1002, 0x1506, 0x1028, 0x0c3e)],
            "Example subsystem"
        );
        assert_eq!(
            names.devices[&(0x1022, 0x14b5)],
            "Family 17h-19h PCIe Root Complex"
        );
        assert_eq!(names.vendors[&0x10de], "NVIDIA Corporation");
        assert!(!names.devices.contains_key(&(0x10de, 0xffff)));
        assert!(
            !names.devices.contains_key(&(0x1002, 0x1640)),
            "not requested"
        );
    }

    #[test]
    fn usb_ids_format() {
        let usb = "1d6b  Linux Foundation\n\t0002  2.0 root hub\n\t0003  3.0 root hub\n\
                   05e3  Genesys Logic, Inc.\n\t0610  Hub\n\t\t00  interface line\n\
                   C 00  (Defined at Interface level)\n";
        let mut query = IdQuery::default();
        query.add(0x1d6b, 0x0003);
        query.add(0x05e3, 0x0610);
        let names = lookup(usb, &query);
        assert_eq!(names.devices[&(0x1d6b, 0x0003)], "3.0 root hub");
        assert_eq!(names.devices[&(0x05e3, 0x0610)], "Hub");
        assert_eq!(names.vendors[&0x05e3], "Genesys Logic, Inc.");
    }

    #[test]
    fn malformed_lines_are_ignored() {
        let mut query = IdQuery::default();
        query.add(0x1234, 0x5678);
        let names = lookup("12345  Too long\n\tzzzz  bad\n", &query);
        assert!(names.devices.is_empty());
    }
}
