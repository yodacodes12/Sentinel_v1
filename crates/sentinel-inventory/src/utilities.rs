//! The catalogue of system utilities and services installed on the machine.
//!
//! Nothing here is an instruction. A desktop entry's `Exec` line is never
//! stored: launching goes through the desktop environment's own launcher
//! with the entry's file path, and only when the user asks.

use serde::{Deserialize, Serialize};

use crate::discovery::ModuleReport;

pub const UTILITY_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UtilityCatalog {
    pub schema_version: u32,
    pub discovered_at: u64,
    pub duration_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub root_override: Option<String>,
    pub utilities: Vec<SystemUtility>,
    pub services: Vec<ServiceInfo>,
    pub package_managers: Vec<PackageManagerStatus>,
    pub discovery_status: Vec<ModuleReport>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UtilityCategory {
    SystemMonitoring,
    HardwareInformation,
    DiskManagement,
    StorageHealth,
    BatteryAndPower,
    SensorsAndTemperatures,
    NetworkManagement,
    NetworkDiagnostics,
    ProcessManagement,
    SystemLogs,
    ServiceManagement,
    PackageManagement,
    GraphicsAndDrivers,
    BackupAndRecovery,
    SystemConfiguration,
}

impl UtilityCategory {
    pub const ALL: [UtilityCategory; 15] = [
        UtilityCategory::SystemMonitoring,
        UtilityCategory::HardwareInformation,
        UtilityCategory::DiskManagement,
        UtilityCategory::StorageHealth,
        UtilityCategory::BatteryAndPower,
        UtilityCategory::SensorsAndTemperatures,
        UtilityCategory::NetworkManagement,
        UtilityCategory::NetworkDiagnostics,
        UtilityCategory::ProcessManagement,
        UtilityCategory::SystemLogs,
        UtilityCategory::ServiceManagement,
        UtilityCategory::PackageManagement,
        UtilityCategory::GraphicsAndDrivers,
        UtilityCategory::BackupAndRecovery,
        UtilityCategory::SystemConfiguration,
    ];

    pub fn label(self) -> &'static str {
        match self {
            UtilityCategory::SystemMonitoring => "System monitoring",
            UtilityCategory::HardwareInformation => "Hardware information",
            UtilityCategory::DiskManagement => "Disk management",
            UtilityCategory::StorageHealth => "Storage health",
            UtilityCategory::BatteryAndPower => "Battery and power",
            UtilityCategory::SensorsAndTemperatures => "Sensors and temperatures",
            UtilityCategory::NetworkManagement => "Network management",
            UtilityCategory::NetworkDiagnostics => "Network diagnostics",
            UtilityCategory::ProcessManagement => "Process management",
            UtilityCategory::SystemLogs => "System logs",
            UtilityCategory::ServiceManagement => "Service management",
            UtilityCategory::PackageManagement => "Package management",
            UtilityCategory::GraphicsAndDrivers => "Graphics and drivers",
            UtilityCategory::BackupAndRecovery => "Backup and recovery",
            UtilityCategory::SystemConfiguration => "System configuration",
        }
    }

    pub fn parse(text: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|c| {
            let json = serde_json::to_value(c).ok();
            json.as_ref().and_then(|v| v.as_str()) == Some(text)
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UtilityInterface {
    Graphical,
    /// A full-screen terminal program (it has a desktop entry with
    /// `Terminal=true`, or is known to be one).
    Terminal,
    CommandLine,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiscoverySource {
    /// Found as an XDG `.desktop` file.
    DesktopEntry,
    /// Found in an installed-package database.
    PackageDatabase,
    /// Found as an executable file in a `PATH` directory. Never run.
    Executable,
    /// Known to Sentinel's built-in catalogue but not found on this machine.
    Catalogue,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PackageManager {
    Dpkg,
    Rpm,
    Pacman,
    Flatpak,
    Snap,
}

impl PackageManager {
    pub fn label(self) -> &'static str {
        match self {
            PackageManager::Dpkg => "dpkg (Debian/Ubuntu)",
            PackageManager::Rpm => "RPM (Fedora/openSUSE)",
            PackageManager::Pacman => "pacman (Arch)",
            PackageManager::Flatpak => "Flatpak",
            PackageManager::Snap => "Snap",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PackageRef {
    pub manager: PackageManager,
    pub name: String,
    pub version: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DesktopEntryRef {
    /// Desktop file ID, e.g. `org.gnome.DiskUtility.desktop`.
    pub id: String,
    /// Absolute path of the `.desktop` file.
    pub path: String,
    pub scope: InstallScope,
    pub terminal: bool,
    /// The entry declares itself hidden from menus (`NoDisplay`).
    pub no_display: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InstallScope {
    /// Installed for all users.
    System,
    /// Installed in the user's own data directory.
    User,
    Flatpak,
    Snap,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SystemUtility {
    /// Stable identifier, e.g. `htop` or `desktop:org.gnome.DiskUtility`.
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub category: UtilityCategory,
    pub interface: UtilityInterface,
    pub installed: bool,
    pub version: Option<String>,
    /// Absolute path of the executable found on `PATH`, for information.
    pub executable: Option<String>,
    pub desktop_entry: Option<DesktopEntryRef>,
    pub package: Option<PackageRef>,
    pub icon: Option<String>,
    pub sources: Vec<DiscoverySource>,
    /// Whether Sentinel can open it for the user (through the desktop
    /// environment). Command-line tools are never run by Sentinel.
    pub launch_available: bool,
    pub launch_note: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ServiceScope {
    System,
    User,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServiceInfo {
    /// Unit name, e.g. `NetworkManager.service`.
    pub name: String,
    pub description: Option<String>,
    pub scope: ServiceScope,
    pub load_state: Option<String>,
    pub active_state: Option<String>,
    pub sub_state: Option<String>,
    /// Unit-file state from systemd: `enabled`, `disabled`, `static`, ...
    pub enabled_state: Option<String>,
    pub unit_file_path: Option<String>,
    /// Set when the service relates to hardware, power, storage, network or
    /// system health (Sentinel's built-in list).
    pub relevant_category: Option<UtilityCategory>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PackageManagerStatus {
    pub manager: PackageManager,
    pub database_found: bool,
    /// Number of installed packages the database lists, when it was read.
    pub installed_packages: Option<u32>,
    pub note: Option<String>,
}

/// Utility IDs are short and made of safe characters; anything else is
/// rejected before it is looked up.
pub fn is_valid_utility_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 160
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_' | ':' | '+'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn categories_parse_from_their_serialised_names() {
        for category in UtilityCategory::ALL {
            let text = serde_json::to_value(category).unwrap();
            assert_eq!(
                UtilityCategory::parse(text.as_str().unwrap()),
                Some(category)
            );
        }
        assert_eq!(UtilityCategory::parse("bogus"), None);
    }
}
