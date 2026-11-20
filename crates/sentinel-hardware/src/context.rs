//! State shared by the modules of one scan: the source root, the scan time,
//! cancellation, and ID-database names looked up once per scan.

use std::ops::ControlFlow;
use std::sync::Arc;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

use sentinel_inventory::{ModuleId, ModuleOutcome, ModuleReport};

use crate::parsers::ids::{IdNames, IdParser, IdQuery};
use crate::parsers::{hex_u16, hex_u32};
use crate::source::SysRoot;

/// Why a scan stopped early.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stopped {
    Cancelled,
    TimedOut,
}

/// Cooperative cancellation, checked between modules and inside loops.
#[derive(Debug, Clone, Default)]
pub struct CancelToken {
    flag: Arc<AtomicBool>,
    deadline: Option<Instant>,
}

impl CancelToken {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_deadline(mut self, deadline: Instant) -> Self {
        self.deadline = Some(deadline);
        self
    }

    pub fn cancel(&self) {
        self.flag.store(true, Ordering::SeqCst);
    }

    pub fn is_cancelled(&self) -> bool {
        self.flag.load(Ordering::SeqCst)
    }

    pub fn check(&self) -> Result<(), Stopped> {
        if self.is_cancelled() {
            return Err(Stopped::Cancelled);
        }
        if self.deadline.is_some_and(|d| Instant::now() >= d) {
            return Err(Stopped::TimedOut);
        }
        Ok(())
    }
}

pub struct DiscoveryContext {
    pub root: SysRoot,
    /// Unix seconds at scan start; every record is stamped with it.
    pub now: u64,
    pub cancel: CancelToken,
    pci_names: OnceLock<Option<IdNames>>,
    usb_names: OnceLock<Option<IdNames>>,
}

pub const PCI_DEVICES: &str = "sys/bus/pci/devices";
pub const USB_DEVICES: &str = "sys/bus/usb/devices";

const PCI_IDS: &[&str] = &[
    "usr/share/hwdata/pci.ids",
    "usr/share/misc/pci.ids",
    "usr/share/pci.ids",
];
const USB_IDS: &[&str] = &[
    "usr/share/hwdata/usb.ids",
    "usr/share/misc/usb.ids",
    "usr/share/usb.ids",
];

impl DiscoveryContext {
    pub fn new(root: SysRoot, now: u64, cancel: CancelToken) -> Self {
        Self {
            root,
            now,
            cancel,
            pci_names: OnceLock::new(),
            usb_names: OnceLock::new(),
        }
    }

    pub fn check(&self) -> Result<(), Stopped> {
        self.cancel.check()
    }

    /// Names for every PCI device present, from `pci.ids`. `None` when the
    /// database is not installed.
    pub fn pci_names(&self) -> Option<&IdNames> {
        self.pci_names
            .get_or_init(|| {
                let mut query = IdQuery::default();
                for address in self.root.list(PCI_DEVICES).unwrap_or_default() {
                    let read = |name: &str| {
                        self.root
                            .read_trimmed(&format!("{PCI_DEVICES}/{address}/{name}"))
                            .ok()
                            .and_then(|v| hex_u16(&v))
                    };
                    if let (Some(v), Some(d)) = (read("vendor"), read("device")) {
                        match (read("subsystem_vendor"), read("subsystem_device")) {
                            (Some(sv), Some(sd)) if sv != 0 => query.add_subsystem(v, d, sv, sd),
                            _ => query.add(v, d),
                        }
                    }
                }
                self.stream_ids(PCI_IDS, &query)
            })
            .as_ref()
    }

    /// Names for every USB device present, from `usb.ids`.
    pub fn usb_names(&self) -> Option<&IdNames> {
        self.usb_names
            .get_or_init(|| {
                let mut query = IdQuery::default();
                for name in self.root.list(USB_DEVICES).unwrap_or_default() {
                    if name.contains(':') {
                        continue;
                    }
                    let read = |attr: &str| {
                        self.root
                            .read_trimmed(&format!("{USB_DEVICES}/{name}/{attr}"))
                            .ok()
                            .and_then(|v| hex_u32(&v))
                            .and_then(|v| u16::try_from(v).ok())
                    };
                    if let (Some(v), Some(p)) = (read("idVendor"), read("idProduct")) {
                        query.add(v, p);
                    }
                }
                self.stream_ids(USB_IDS, &query)
            })
            .as_ref()
    }

    fn stream_ids(&self, candidates: &[&str], query: &IdQuery) -> Option<IdNames> {
        let path = candidates.iter().find(|p| self.root.is_file(p))?;
        if query.is_empty() {
            return Some(IdNames::default());
        }
        let mut parser = IdParser::new(query);
        self.root
            .for_each_line(path, |line| {
                if self.cancel.is_cancelled() {
                    return ControlFlow::Break(());
                }
                parser.line(line)
            })
            .ok()?;
        Some(parser.finish())
    }

    pub fn pci_ids_source(&self) -> String {
        PCI_IDS
            .iter()
            .find(|p| self.root.is_file(p))
            .map(|p| self.root.display(p))
            .unwrap_or_else(|| "pci.ids".into())
    }

    pub fn usb_ids_source(&self) -> String {
        USB_IDS
            .iter()
            .find(|p| self.root.is_file(p))
            .map(|p| self.root.display(p))
            .unwrap_or_else(|| "usb.ids".into())
    }
}

/// What a module produced, before the manager stamps time and duration.
#[derive(Debug, Clone, PartialEq)]
pub struct ModuleRun<T> {
    pub value: T,
    pub outcome: ModuleOutcome,
    pub items: u32,
    pub notes: Vec<String>,
}

impl<T> ModuleRun<T> {
    pub fn new(value: T, outcome: ModuleOutcome, items: usize, notes: Vec<String>) -> Self {
        Self {
            value,
            outcome,
            items: u32::try_from(items).unwrap_or(u32::MAX),
            notes,
        }
    }

    pub fn report(&self, module: ModuleId, duration_ms: u64, finished_at: u64) -> ModuleReport {
        ModuleReport {
            module,
            outcome: self.outcome,
            items: self.items,
            duration_ms,
            finished_at,
            notes: self.notes.clone(),
        }
    }
}

/// `Completed` when nothing expected was missing, `Partial` otherwise.
pub fn outcome_for(missing: usize) -> ModuleOutcome {
    if missing == 0 {
        ModuleOutcome::Completed
    } else {
        ModuleOutcome::Partial
    }
}

/// Extract the last PCI address (`0000:03:00.0`) from a sysfs device path.
pub fn last_pci_address(path: &str) -> Option<String> {
    path.split('/')
        .rev()
        .find(|c| is_pci_address(c))
        .map(str::to_string)
}

pub fn is_pci_address(text: &str) -> bool {
    let b = text.as_bytes();
    b.len() == 12
        && b[4] == b':'
        && b[7] == b':'
        && b[10] == b'.'
        && text
            .chars()
            .enumerate()
            .all(|(i, c)| matches!(i, 4 | 7 | 10) || c.is_ascii_hexdigit())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cancellation_and_deadline() {
        let token = CancelToken::new();
        assert_eq!(token.check(), Ok(()));
        let clone = token.clone();
        clone.cancel();
        assert_eq!(token.check(), Err(Stopped::Cancelled));

        let expired = CancelToken::new().with_deadline(Instant::now());
        assert_eq!(expired.check(), Err(Stopped::TimedOut));
    }

    #[test]
    fn pci_addresses() {
        assert!(is_pci_address("0000:03:00.0"));
        assert!(!is_pci_address("pci0000:00"));
        assert!(!is_pci_address("0000:03:00"));
        assert_eq!(
            last_pci_address("sys/devices/pci0000:00/0000:00:02.1/0000:01:00.0/nvme/nvme0"),
            Some("0000:01:00.0".into())
        );
        assert_eq!(last_pci_address("sys/devices/virtual/block/loop0"), None);
    }
}
