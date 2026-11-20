//! Discovery jobs, their progress and the outcome of each module.

use serde::{Deserialize, Serialize};

/// The two kinds of scan the daemon coordinates.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiscoveryJob {
    Hardware,
    Utilities,
}

impl DiscoveryJob {
    pub const ALL: [DiscoveryJob; 2] = [DiscoveryJob::Hardware, DiscoveryJob::Utilities];

    pub fn as_str(self) -> &'static str {
        match self {
            DiscoveryJob::Hardware => "hardware",
            DiscoveryJob::Utilities => "utilities",
        }
    }

    pub fn modules(self) -> &'static [ModuleId] {
        match self {
            DiscoveryJob::Hardware => &ModuleId::HARDWARE,
            DiscoveryJob::Utilities => &ModuleId::UTILITIES,
        }
    }
}

/// Every discovery module, in the order it runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModuleId {
    SystemIdentity,
    Firmware,
    Cpu,
    Memory,
    Motherboard,
    Gpu,
    Storage,
    Battery,
    Network,
    Usb,
    Pci,
    Sensors,
    DesktopEntries,
    Packages,
    Executables,
    Services,
}

impl ModuleId {
    pub const HARDWARE: [ModuleId; 12] = [
        ModuleId::SystemIdentity,
        ModuleId::Firmware,
        ModuleId::Cpu,
        ModuleId::Memory,
        ModuleId::Motherboard,
        ModuleId::Gpu,
        ModuleId::Storage,
        ModuleId::Battery,
        ModuleId::Network,
        ModuleId::Usb,
        ModuleId::Pci,
        ModuleId::Sensors,
    ];

    pub const UTILITIES: [ModuleId; 4] = [
        ModuleId::DesktopEntries,
        ModuleId::Packages,
        ModuleId::Executables,
        ModuleId::Services,
    ];

    pub fn label(self) -> &'static str {
        match self {
            ModuleId::SystemIdentity => "System identity",
            ModuleId::Firmware => "Firmware",
            ModuleId::Cpu => "Processor",
            ModuleId::Memory => "Memory",
            ModuleId::Motherboard => "Motherboard",
            ModuleId::Gpu => "Graphics",
            ModuleId::Storage => "Storage",
            ModuleId::Battery => "Battery and power",
            ModuleId::Network => "Network",
            ModuleId::Usb => "USB",
            ModuleId::Pci => "PCI",
            ModuleId::Sensors => "Sensors",
            ModuleId::DesktopEntries => "Desktop applications",
            ModuleId::Packages => "Package databases",
            ModuleId::Executables => "Executables on PATH",
            ModuleId::Services => "System services",
        }
    }
}

/// How one module ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModuleOutcome {
    /// Everything the module looks for was read.
    Completed,
    /// The module produced results, but some values were unavailable.
    Partial,
    /// The information source does not exist or is not readable here.
    Unavailable,
    /// The module hit an unexpected error. Other modules still ran.
    Failed,
    /// The scan was cancelled or timed out before this module finished.
    Cancelled,
}

impl ModuleOutcome {
    pub fn produced_results(self) -> bool {
        matches!(self, ModuleOutcome::Completed | ModuleOutcome::Partial)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModuleReport {
    pub module: ModuleId,
    pub outcome: ModuleOutcome,
    /// Number of records produced (devices, utilities, ...).
    pub items: u32,
    pub duration_ms: u64,
    /// Unix seconds.
    pub finished_at: u64,
    /// Plain-language notes about what was unavailable and why. Never
    /// contains serial numbers, MAC addresses or file contents.
    pub notes: Vec<String>,
}

/// Where the inventory being shown came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InventoryOrigin {
    /// Scanned by the running daemon since it started.
    ThisSession,
    /// Loaded from Sentinel's local cache: an earlier scan, not live.
    Cache,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JobState {
    /// Nothing has run since the daemon started.
    Idle,
    Queued,
    Running,
    Completed,
    Cancelled,
    TimedOut,
    Failed,
}

impl JobState {
    pub fn is_active(self) -> bool {
        matches!(self, JobState::Queued | JobState::Running)
    }
}

/// Live progress of one discovery job.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JobProgress {
    pub job: DiscoveryJob,
    pub state: JobState,
    pub started_at: Option<u64>,
    pub finished_at: Option<u64>,
    pub current_module: Option<ModuleId>,
    /// Reports of modules finished so far in the current or last run.
    pub modules: Vec<ModuleReport>,
    /// When a scan of this kind last completed (from this session or cache).
    pub last_successful_scan: Option<u64>,
    /// Why the last run did not complete.
    pub error: Option<String>,
    /// False when the result could not be written to the local cache.
    pub cache_saved: Option<bool>,
}

impl JobProgress {
    pub fn idle(job: DiscoveryJob) -> Self {
        Self {
            job,
            state: JobState::Idle,
            started_at: None,
            finished_at: None,
            current_module: None,
            modules: Vec::new(),
            last_successful_scan: None,
            error: None,
            cache_saved: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiscoveryStatus {
    pub hardware: JobProgress,
    pub utilities: JobProgress,
}

impl DiscoveryStatus {
    pub fn get(&self, job: DiscoveryJob) -> &JobProgress {
        match job {
            DiscoveryJob::Hardware => &self.hardware,
            DiscoveryJob::Utilities => &self.utilities,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn module_lists_cover_every_module_once() {
        let mut all: Vec<ModuleId> = ModuleId::HARDWARE.to_vec();
        all.extend(ModuleId::UTILITIES);
        let unique: std::collections::HashSet<_> = all.iter().collect();
        assert_eq!(unique.len(), all.len());
        assert_eq!(all.len(), 16);
        // System identity runs first, as the plan requires.
        assert_eq!(ModuleId::HARDWARE[0], ModuleId::SystemIdentity);
    }

    #[test]
    fn serialised_names_are_snake_case() {
        assert_eq!(
            serde_json::to_string(&ModuleId::SystemIdentity).unwrap(),
            "\"system_identity\""
        );
        assert_eq!(
            serde_json::to_string(&InventoryOrigin::ThisSession).unwrap(),
            "\"this_session\""
        );
        assert_eq!(
            serde_json::to_string(&JobState::TimedOut).unwrap(),
            "\"timed_out\""
        );
    }

    #[test]
    fn only_completed_and_partial_count_as_results() {
        assert!(ModuleOutcome::Completed.produced_results());
        assert!(ModuleOutcome::Partial.produced_results());
        for outcome in [
            ModuleOutcome::Unavailable,
            ModuleOutcome::Failed,
            ModuleOutcome::Cancelled,
        ] {
            assert!(!outcome.produced_results());
        }
    }
}
