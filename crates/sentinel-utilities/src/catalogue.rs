//! Sentinel's built-in catalogue of well-known Linux system utilities.
//!
//! This is reference data used to recognise and categorise what is found on
//! the machine. Names here are never executed: the strings are matched
//! against desktop-file IDs, package names and file names. A utility in this
//! list that is not found is shown as "not installed", and Sentinel never
//! offers to install it.

use sentinel_inventory::{UtilityCategory as C, UtilityInterface as I};

pub struct Known {
    pub id: &'static str,
    pub name: &'static str,
    pub category: C,
    pub interface: I,
    pub description: &'static str,
    pub executables: &'static [&'static str],
    pub desktop_ids: &'static [&'static str],
    /// Package names across dpkg, RPM, pacman, Flatpak and Snap.
    pub packages: &'static [&'static str],
}

macro_rules! known {
    ($id:literal, $name:literal, $cat:ident, $iface:ident, $desc:literal,
     [$($exe:literal),*], [$($desk:literal),*], [$($pkg:literal),*]) => {
        Known {
            id: $id, name: $name, category: C::$cat, interface: I::$iface, description: $desc,
            executables: &[$($exe),*], desktop_ids: &[$($desk),*], packages: &[$($pkg),*],
        }
    };
}

pub const KNOWN: &[Known] = &[
    // System monitoring
    known!(
        "htop",
        "htop",
        SystemMonitoring,
        Terminal,
        "Interactive process and resource viewer for the terminal.",
        ["htop"],
        ["htop.desktop"],
        ["htop"]
    ),
    known!(
        "btop",
        "btop",
        SystemMonitoring,
        Terminal,
        "Resource monitor for the terminal.",
        ["btop"],
        ["btop.desktop"],
        ["btop"]
    ),
    known!(
        "gnome-system-monitor",
        "GNOME System Monitor",
        SystemMonitoring,
        Graphical,
        "View processes and resource usage.",
        ["gnome-system-monitor"],
        [
            "org.gnome.SystemMonitor.desktop",
            "gnome-system-monitor.desktop"
        ],
        ["gnome-system-monitor"]
    ),
    known!(
        "plasma-systemmonitor",
        "KDE System Monitor",
        SystemMonitoring,
        Graphical,
        "KDE Plasma's resource and process monitor.",
        ["plasma-systemmonitor"],
        ["org.kde.plasma-systemmonitor.desktop"],
        ["plasma-systemmonitor"]
    ),
    known!(
        "resources",
        "Resources",
        SystemMonitoring,
        Graphical,
        "Monitor system resources and processes.",
        ["resources"],
        ["net.nokyan.Resources.desktop"],
        ["resources", "net.nokyan.Resources"]
    ),
    known!(
        "mission-center",
        "Mission Center",
        SystemMonitoring,
        Graphical,
        "Monitor CPU, memory, disk, network and GPU usage.",
        ["missioncenter"],
        ["io.missioncenter.MissionCenter.desktop"],
        ["mission-center", "io.missioncenter.MissionCenter"]
    ),
    known!(
        "nvtop",
        "nvtop",
        SystemMonitoring,
        Terminal,
        "GPU process monitor for the terminal.",
        ["nvtop"],
        ["nvtop.desktop"],
        ["nvtop"]
    ),
    // Process management
    known!(
        "top",
        "top",
        ProcessManagement,
        Terminal,
        "Classic terminal process viewer.",
        ["top"],
        [],
        ["procps", "procps-ng"]
    ),
    // Hardware information
    known!(
        "hardinfo",
        "HardInfo",
        HardwareInformation,
        Graphical,
        "System profiler and benchmark tool.",
        ["hardinfo", "hardinfo2"],
        ["hardinfo.desktop", "hardinfo2.desktop"],
        ["hardinfo", "hardinfo2"]
    ),
    known!(
        "cpu-x",
        "CPU-X",
        HardwareInformation,
        Graphical,
        "Detailed processor, motherboard and memory information.",
        ["cpu-x"],
        [
            "cpu-x.desktop",
            "io.github.thetumultuousunicornofdarkness.cpu-x.desktop"
        ],
        ["cpu-x"]
    ),
    known!(
        "inxi",
        "inxi",
        HardwareInformation,
        CommandLine,
        "Command-line system information script.",
        ["inxi"],
        [],
        ["inxi"]
    ),
    known!(
        "lshw",
        "lshw",
        HardwareInformation,
        CommandLine,
        "Lists hardware configuration.",
        ["lshw"],
        ["lshw-gtk.desktop"],
        ["lshw"]
    ),
    known!(
        "hwinfo",
        "hwinfo",
        HardwareInformation,
        CommandLine,
        "Hardware probing tool.",
        ["hwinfo"],
        [],
        ["hwinfo"]
    ),
    known!(
        "lscpu",
        "lscpu",
        HardwareInformation,
        CommandLine,
        "Displays CPU architecture information.",
        ["lscpu"],
        [],
        ["util-linux"]
    ),
    known!(
        "lspci",
        "lspci",
        HardwareInformation,
        CommandLine,
        "Lists PCI devices.",
        ["lspci"],
        [],
        ["pciutils"]
    ),
    known!(
        "lsusb",
        "lsusb",
        HardwareInformation,
        CommandLine,
        "Lists USB devices.",
        ["lsusb"],
        [],
        ["usbutils"]
    ),
    known!(
        "dmidecode",
        "dmidecode",
        HardwareInformation,
        CommandLine,
        "Reads firmware DMI tables (needs administrator rights).",
        ["dmidecode"],
        [],
        ["dmidecode"]
    ),
    // Sensors
    known!(
        "lm-sensors",
        "lm-sensors",
        SensorsAndTemperatures,
        CommandLine,
        "Reads temperature, voltage and fan sensors.",
        ["sensors"],
        [],
        ["lm-sensors", "lm_sensors"]
    ),
    known!(
        "psensor",
        "Psensor",
        SensorsAndTemperatures,
        Graphical,
        "Graphical temperature monitor.",
        ["psensor"],
        ["psensor.desktop"],
        ["psensor"]
    ),
    // Disks
    known!(
        "gnome-disks",
        "GNOME Disks",
        DiskManagement,
        Graphical,
        "Manage disks and partitions, view SMART data.",
        ["gnome-disks"],
        ["org.gnome.DiskUtility.desktop"],
        ["gnome-disk-utility"]
    ),
    known!(
        "gparted",
        "GParted",
        DiskManagement,
        Graphical,
        "Partition editor.",
        ["gparted"],
        ["gparted.desktop", "org.gnome.gparted.desktop"],
        ["gparted"]
    ),
    known!(
        "partitionmanager",
        "KDE Partition Manager",
        DiskManagement,
        Graphical,
        "Partition editor for KDE.",
        ["partitionmanager"],
        ["org.kde.partitionmanager.desktop"],
        ["partitionmanager"]
    ),
    known!(
        "baobab",
        "Disk Usage Analyzer",
        DiskManagement,
        Graphical,
        "Shows what uses disk space.",
        ["baobab"],
        ["org.gnome.baobab.desktop"],
        ["baobab"]
    ),
    // Storage health
    known!(
        "smartctl",
        "smartctl",
        StorageHealth,
        CommandLine,
        "Reads drive SMART health data (smartmontools).",
        ["smartctl"],
        [],
        ["smartmontools"]
    ),
    known!(
        "nvme-cli",
        "nvme-cli",
        StorageHealth,
        CommandLine,
        "NVMe drive management and health tool.",
        ["nvme"],
        [],
        ["nvme-cli"]
    ),
    known!(
        "gsmartcontrol",
        "GSmartControl",
        StorageHealth,
        Graphical,
        "Graphical front end for SMART data.",
        ["gsmartcontrol"],
        ["gsmartcontrol.desktop"],
        ["gsmartcontrol"]
    ),
    // Battery and power
    known!(
        "upower",
        "UPower",
        BatteryAndPower,
        CommandLine,
        "Battery and power device information.",
        ["upower"],
        [],
        ["upower"]
    ),
    known!(
        "tlp",
        "TLP",
        BatteryAndPower,
        CommandLine,
        "Laptop power management tool.",
        ["tlp", "tlp-stat"],
        [],
        ["tlp"]
    ),
    known!(
        "powertop",
        "PowerTOP",
        BatteryAndPower,
        Terminal,
        "Power consumption diagnosis.",
        ["powertop"],
        [],
        ["powertop"]
    ),
    // Network
    known!(
        "nmcli",
        "NetworkManager CLI",
        NetworkManagement,
        CommandLine,
        "Command-line control for NetworkManager.",
        ["nmcli"],
        [],
        ["network-manager", "NetworkManager", "networkmanager"]
    ),
    known!(
        "nm-connection-editor",
        "Network Connections",
        NetworkManagement,
        Graphical,
        "Edit NetworkManager connections.",
        ["nm-connection-editor"],
        ["nm-connection-editor.desktop"],
        [
            "network-manager-gnome",
            "network-manager-applet",
            "nm-connection-editor"
        ]
    ),
    known!(
        "wireshark",
        "Wireshark",
        NetworkDiagnostics,
        Graphical,
        "Network protocol analyser.",
        ["wireshark"],
        ["org.wireshark.Wireshark.desktop", "wireshark.desktop"],
        ["wireshark", "wireshark-qt"]
    ),
    known!(
        "mtr",
        "mtr",
        NetworkDiagnostics,
        Terminal,
        "Combined traceroute and ping.",
        ["mtr"],
        ["mtr.desktop"],
        ["mtr", "mtr-tiny"]
    ),
    known!(
        "ping",
        "ping",
        NetworkDiagnostics,
        CommandLine,
        "Tests reachability of a host.",
        ["ping"],
        [],
        ["iputils-ping", "iputils"]
    ),
    known!(
        "traceroute",
        "traceroute",
        NetworkDiagnostics,
        CommandLine,
        "Shows the route packets take.",
        ["traceroute", "tracepath"],
        [],
        ["traceroute"]
    ),
    known!(
        "ss",
        "ss",
        NetworkDiagnostics,
        CommandLine,
        "Lists network sockets (iproute2).",
        ["ss"],
        [],
        ["iproute2"]
    ),
    // Logs
    known!(
        "journalctl",
        "journalctl",
        SystemLogs,
        CommandLine,
        "Reads the systemd journal.",
        ["journalctl"],
        [],
        ["systemd"]
    ),
    known!(
        "gnome-logs",
        "GNOME Logs",
        SystemLogs,
        Graphical,
        "Browse the system journal.",
        ["gnome-logs"],
        ["org.gnome.Logs.desktop"],
        ["gnome-logs"]
    ),
    known!(
        "ksystemlog",
        "KSystemLog",
        SystemLogs,
        Graphical,
        "KDE system log viewer.",
        ["ksystemlog"],
        ["org.kde.ksystemlog.desktop"],
        ["ksystemlog"]
    ),
    // Services
    known!(
        "systemctl",
        "systemctl",
        ServiceManagement,
        CommandLine,
        "Inspect and control systemd services.",
        ["systemctl"],
        [],
        ["systemd"]
    ),
    known!(
        "cockpit",
        "Cockpit",
        ServiceManagement,
        Graphical,
        "Web-based server administration.",
        ["cockpit-bridge"],
        [],
        ["cockpit"]
    ),
    // Package management
    known!(
        "gnome-software",
        "GNOME Software",
        PackageManagement,
        Graphical,
        "Browse and manage applications.",
        ["gnome-software"],
        ["org.gnome.Software.desktop"],
        ["gnome-software"]
    ),
    known!(
        "snap-store",
        "App Center",
        PackageManagement,
        Graphical,
        "Ubuntu's application store.",
        ["snap-store"],
        [
            "snap-store_snap-store.desktop",
            "snap-store_ubuntu-software.desktop"
        ],
        ["snap-store"]
    ),
    known!(
        "discover",
        "Discover",
        PackageManagement,
        Graphical,
        "KDE software centre.",
        ["plasma-discover"],
        ["org.kde.discover.desktop"],
        ["plasma-discover", "discover"]
    ),
    known!(
        "synaptic",
        "Synaptic",
        PackageManagement,
        Graphical,
        "Graphical package manager for APT.",
        ["synaptic"],
        ["synaptic.desktop"],
        ["synaptic"]
    ),
    known!(
        "apt",
        "APT",
        PackageManagement,
        CommandLine,
        "Debian/Ubuntu package manager.",
        ["apt"],
        [],
        ["apt"]
    ),
    known!(
        "dnf",
        "DNF",
        PackageManagement,
        CommandLine,
        "Fedora package manager.",
        ["dnf"],
        [],
        ["dnf", "dnf5"]
    ),
    known!(
        "pacman",
        "pacman",
        PackageManagement,
        CommandLine,
        "Arch Linux package manager.",
        ["pacman"],
        [],
        ["pacman"]
    ),
    known!(
        "flatpak",
        "Flatpak",
        PackageManagement,
        CommandLine,
        "Sandboxed application manager.",
        ["flatpak"],
        [],
        ["flatpak"]
    ),
    known!(
        "snap",
        "Snap",
        PackageManagement,
        CommandLine,
        "Snap package manager.",
        ["snap"],
        [],
        ["snapd"]
    ),
    // Graphics
    known!(
        "nvidia-settings",
        "NVIDIA Settings",
        GraphicsAndDrivers,
        Graphical,
        "NVIDIA driver configuration.",
        ["nvidia-settings"],
        ["nvidia-settings.desktop"],
        ["nvidia-settings"]
    ),
    known!(
        "glxinfo",
        "glxinfo",
        GraphicsAndDrivers,
        CommandLine,
        "Shows OpenGL driver information.",
        ["glxinfo"],
        [],
        ["mesa-utils", "mesa-demos"]
    ),
    known!(
        "vulkaninfo",
        "vulkaninfo",
        GraphicsAndDrivers,
        CommandLine,
        "Shows Vulkan driver information.",
        ["vulkaninfo"],
        [],
        ["vulkan-tools"]
    ),
    known!(
        "radeontop",
        "radeontop",
        GraphicsAndDrivers,
        Terminal,
        "AMD GPU utilisation viewer.",
        ["radeontop"],
        [],
        ["radeontop"]
    ),
    // Backup
    known!(
        "deja-dup",
        "Déjà Dup Backups",
        BackupAndRecovery,
        Graphical,
        "Simple encrypted backups.",
        ["deja-dup"],
        ["org.gnome.DejaDup.desktop"],
        ["deja-dup"]
    ),
    known!(
        "timeshift",
        "Timeshift",
        BackupAndRecovery,
        Graphical,
        "System snapshots.",
        ["timeshift", "timeshift-gtk"],
        ["timeshift-gtk.desktop"],
        ["timeshift"]
    ),
    known!(
        "backintime",
        "Back In Time",
        BackupAndRecovery,
        Graphical,
        "Snapshot backups of folders.",
        ["backintime", "backintime-qt"],
        ["backintime-qt.desktop"],
        ["backintime-qt", "backintime-common"]
    ),
    known!(
        "rsync",
        "rsync",
        BackupAndRecovery,
        CommandLine,
        "File synchronisation and copying.",
        ["rsync"],
        [],
        ["rsync"]
    ),
    // System configuration
    known!(
        "gnome-settings",
        "GNOME Settings",
        SystemConfiguration,
        Graphical,
        "Desktop and device settings.",
        ["gnome-control-center"],
        ["org.gnome.Settings.desktop", "gnome-control-center.desktop"],
        ["gnome-control-center"]
    ),
    known!(
        "kde-settings",
        "KDE System Settings",
        SystemConfiguration,
        Graphical,
        "Plasma desktop settings.",
        ["systemsettings"],
        ["systemsettings.desktop"],
        ["systemsettings"]
    ),
    known!(
        "gnome-tweaks",
        "GNOME Tweaks",
        SystemConfiguration,
        Graphical,
        "Advanced GNOME settings.",
        ["gnome-tweaks"],
        ["org.gnome.tweaks.desktop"],
        ["gnome-tweaks"]
    ),
    known!(
        "dconf-editor",
        "dconf Editor",
        SystemConfiguration,
        Graphical,
        "Low-level settings editor.",
        ["dconf-editor"],
        ["ca.desrt.dconf-editor.desktop"],
        ["dconf-editor"]
    ),
    known!(
        "firmware-updater",
        "Firmware Updater",
        SystemConfiguration,
        Graphical,
        "Shows and installs firmware updates (fwupd).",
        ["firmware-updater"],
        [
            "firmware-updater_firmware-updater.desktop",
            "firmware-updater_firmware-updater-app.desktop"
        ],
        ["firmware-updater"]
    ),
    known!(
        "fwupdmgr",
        "fwupdmgr",
        SystemConfiguration,
        CommandLine,
        "Firmware update manager (fwupd).",
        ["fwupdmgr"],
        [],
        ["fwupd"]
    ),
];

/// Built-in list of services that relate to hardware, power, storage,
/// networking or system health: `(name prefix, category)`.
pub const RELEVANT_SERVICES: &[(&str, C)] = &[
    ("NetworkManager", C::NetworkManagement),
    ("systemd-networkd", C::NetworkManagement),
    ("systemd-resolved", C::NetworkManagement),
    ("wpa_supplicant", C::NetworkManagement),
    ("iwd", C::NetworkManagement),
    ("ModemManager", C::NetworkManagement),
    ("avahi-daemon", C::NetworkManagement),
    ("bluetooth", C::HardwareInformation),
    ("upower", C::BatteryAndPower),
    ("power-profiles-daemon", C::BatteryAndPower),
    ("tuned", C::BatteryAndPower),
    ("tlp", C::BatteryAndPower),
    ("thermald", C::SensorsAndTemperatures),
    ("lm-sensors", C::SensorsAndTemperatures),
    ("fancontrol", C::SensorsAndTemperatures),
    ("smartmontools", C::StorageHealth),
    ("smartd", C::StorageHealth),
    ("udisks2", C::DiskManagement),
    ("fstrim", C::DiskManagement),
    ("fwupd", C::SystemConfiguration),
    ("systemd-journald", C::SystemLogs),
    ("rsyslog", C::SystemLogs),
    ("systemd-udevd", C::HardwareInformation),
    ("systemd-timesyncd", C::SystemConfiguration),
    ("cups", C::SystemConfiguration),
    ("acpid", C::BatteryAndPower),
    ("irqbalance", C::SystemMonitoring),
    ("snapd", C::PackageManagement),
    ("packagekit", C::PackageManagement),
    ("unattended-upgrades", C::PackageManagement),
    ("apt-daily", C::PackageManagement),
    ("ufw", C::NetworkManagement),
    ("firewalld", C::NetworkManagement),
    ("ssh", C::NetworkManagement),
    ("sshd", C::NetworkManagement),
    ("docker", C::ServiceManagement),
    ("containerd", C::ServiceManagement),
    ("cron", C::ServiceManagement),
    ("crond", C::ServiceManagement),
    ("switcheroo-control", C::GraphicsAndDrivers),
    ("nvidia-persistenced", C::GraphicsAndDrivers),
    ("gdm", C::SystemConfiguration),
    ("sddm", C::SystemConfiguration),
    ("pipewire", C::SystemConfiguration),
    ("wireplumber", C::SystemConfiguration),
];

pub fn service_category(unit: &str) -> Option<C> {
    let stem = unit.strip_suffix(".service").unwrap_or(unit);
    RELEVANT_SERVICES
        .iter()
        .find(|(prefix, _)| {
            stem == *prefix
                || stem
                    .strip_prefix(prefix)
                    .is_some_and(|rest| rest.starts_with(['-', '@', '.']))
        })
        .map(|(_, c)| *c)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_unique_and_every_category_is_represented() {
        let ids: std::collections::BTreeSet<_> = KNOWN.iter().map(|k| k.id).collect();
        assert_eq!(ids.len(), KNOWN.len());
        for category in C::ALL {
            assert!(KNOWN.iter().any(|k| k.category == category), "{category:?}");
        }
    }

    #[test]
    fn relevant_services() {
        assert_eq!(
            service_category("NetworkManager.service"),
            Some(C::NetworkManagement)
        );
        assert_eq!(
            service_category("NetworkManager-wait-online.service"),
            Some(C::NetworkManagement)
        );
        assert_eq!(service_category("upower.service"), Some(C::BatteryAndPower));
        assert_eq!(service_category("sshd.service"), Some(C::NetworkManagement));
        assert_eq!(service_category("sshfoo.service"), None);
        assert_eq!(service_category("accounts-daemon.service"), None);
    }
}
