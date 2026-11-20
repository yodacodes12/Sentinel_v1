//! Synthetic Linux system trees for discovery tests.
//!
//! Each fixture is a temporary directory laid out like `/` with just enough
//! of `/sys`, `/proc` and `/etc` to describe one kind of machine. Every value
//! is invented ("Example Corp", made-up IDs); nothing comes from a real
//! machine, so no private identifier can leak through the test suite.

use std::fs;
use std::os::unix::fs::{PermissionsExt, symlink};
use std::path::{Path, PathBuf};

pub struct HwFixture {
    dir: tempfile::TempDir,
}

impl Default for HwFixture {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone, Copy)]
pub struct CpuSpec<'a> {
    pub vendor_id: &'a str,
    pub model_name: &'a str,
    pub family: u32,
    pub model: u32,
    pub packages: u32,
    pub cores_per_package: u32,
    pub threads_per_core: u32,
    /// Intel-style hybrid: this many of the cores (per package) are
    /// efficiency cores with one thread each.
    pub efficiency_cores: u32,
}

impl HwFixture {
    pub fn new() -> Self {
        Self {
            dir: tempfile::Builder::new()
                .prefix("sntl-hw")
                .tempdir()
                .expect("temp dir"),
        }
    }

    pub fn root(&self) -> &Path {
        self.dir.path()
    }

    fn path(&self, rel: &str) -> PathBuf {
        self.dir.path().join(rel.trim_start_matches('/'))
    }

    pub fn file(&self, rel: &str, content: impl AsRef<[u8]>) -> &Self {
        let path = self.path(rel);
        fs::create_dir_all(path.parent().expect("parent")).unwrap();
        fs::write(path, content).unwrap();
        self
    }

    pub fn dir(&self, rel: &str) -> &Self {
        fs::create_dir_all(self.path(rel)).unwrap();
        self
    }

    /// Symlink `rel` -> `target` (target relative to `rel`'s directory).
    pub fn link(&self, rel: &str, target: &str) -> &Self {
        let path = self.path(rel);
        fs::create_dir_all(path.parent().expect("parent")).unwrap();
        symlink(target, path).unwrap();
        self
    }

    pub fn chmod(&self, rel: &str, mode: u32) -> &Self {
        fs::set_permissions(self.path(rel), fs::Permissions::from_mode(mode)).unwrap();
        self
    }

    fn up(rel_dir: &str) -> String {
        "../".repeat(rel_dir.split('/').filter(|c| !c.is_empty()).count())
    }

    // ------------------------------------------------------------- system

    pub fn dmi(&self, fields: &[(&str, &str)]) -> &Self {
        for (name, value) in fields {
            self.file(&format!("sys/class/dmi/id/{name}"), format!("{value}\n"));
        }
        self
    }

    pub fn os_release(&self, rel: &str, text: &str) -> &Self {
        self.file(rel, text)
    }

    pub fn kernel(&self, release: &str, arch: &str) -> &Self {
        self.file("proc/sys/kernel/osrelease", format!("{release}\n"))
            .file(
                "proc/sys/kernel/version",
                "#1 SMP PREEMPT_DYNAMIC fixture\n",
            )
            .file("proc/sys/kernel/arch", format!("{arch}\n"))
    }

    pub fn uefi(&self) -> &Self {
        self.dir("sys/firmware/efi")
    }

    // ---------------------------------------------------------------- cpu

    pub fn cpus(&self, spec: CpuSpec) -> &Self {
        let mut cpuinfo = String::new();
        let mut cpu = 0u32;
        let mut performance = Vec::new();
        let mut efficiency = Vec::new();
        for package in 0..spec.packages {
            for core in 0..spec.cores_per_package {
                let is_e = core >= spec.cores_per_package - spec.efficiency_cores;
                let threads = if is_e { 1 } else { spec.threads_per_core };
                let siblings: Vec<u32> = (cpu..cpu + threads).collect();
                let list = if threads == 1 {
                    format!("{cpu}")
                } else {
                    format!("{}-{}", cpu, cpu + threads - 1)
                };
                for thread in siblings {
                    let base = format!("sys/devices/system/cpu/cpu{thread}");
                    self.file(
                        &format!("{base}/topology/physical_package_id"),
                        format!("{package}\n"),
                    )
                    .file(
                        &format!("{base}/topology/core_id"),
                        format!("{}\n", core * 4),
                    )
                    .file(&format!("{base}/topology/die_id"), "0\n")
                    .file(
                        &format!("{base}/topology/core_cpus_list"),
                        format!("{list}\n"),
                    )
                    .file(
                        &format!("{base}/cpufreq/cpuinfo_max_freq"),
                        if is_e { "3400000\n" } else { "4800000\n" },
                    )
                    .file(&format!("{base}/cpufreq/cpuinfo_min_freq"), "400000\n");
                    for (index, (level, kind, size, shared)) in [
                        (1, "Data", "48K", list.clone()),
                        (1, "Instruction", "32K", list.clone()),
                        (2, "Unified", "1280K", list.clone()),
                        (
                            3,
                            "Unified",
                            "12288K",
                            format!("{}-{}", package * 1000, package * 1000 + 999),
                        ),
                    ]
                    .into_iter()
                    .enumerate()
                    {
                        let c = format!("{base}/cache/index{index}");
                        self.file(&format!("{c}/level"), format!("{level}\n"))
                            .file(&format!("{c}/type"), format!("{kind}\n"))
                            .file(&format!("{c}/size"), format!("{size}\n"))
                            .file(&format!("{c}/shared_cpu_list"), format!("{shared}\n"));
                    }
                    if is_e {
                        efficiency.push(thread)
                    } else {
                        performance.push(thread)
                    }
                    cpuinfo.push_str(&format!(
                        "processor\t: {thread}\nvendor_id\t: {}\ncpu family\t: {}\nmodel\t\t: {}\n\
                         model name\t: {}\nstepping\t: 2\nphysical id\t: {package}\n\
                         flags\t\t: fpu vme sse sse2 avx2\n\n",
                        spec.vendor_id, spec.family, spec.model, spec.model_name
                    ));
                }
                cpu += threads;
            }
        }
        self.file("sys/devices/system/cpu/online", format!("0-{}\n", cpu - 1))
            .file("sys/devices/system/cpu/present", format!("0-{}\n", cpu - 1))
            .file("proc/cpuinfo", cpuinfo);
        if spec.efficiency_cores > 0 {
            let fmt = |v: &[u32]| v.iter().map(u32::to_string).collect::<Vec<_>>().join(",");
            self.file(
                "sys/devices/cpu_core/cpus",
                format!("{}\n", fmt(&performance)),
            )
            .file(
                "sys/devices/cpu_atom/cpus",
                format!("{}\n", fmt(&efficiency)),
            );
        }
        self
    }

    // ------------------------------------------------------------- memory

    pub fn meminfo(&self, total_kib: u64, available_kib: u64, swap_kib: u64) -> &Self {
        self.file(
            "proc/meminfo",
            format!(
                "MemTotal:       {total_kib} kB\nMemFree:        {} kB\nMemAvailable:   {available_kib} kB\n\
                 Cached:         {} kB\nSwapTotal:      {swap_kib} kB\nSwapFree:       {swap_kib} kB\n",
                available_kib / 2,
                available_kib / 4
            ),
        )
    }

    pub fn memory_blocks(&self, block_bytes: u64, online: u32) -> &Self {
        self.file(
            "sys/devices/system/memory/block_size_bytes",
            format!("{block_bytes:x}\n"),
        );
        for i in 0..online {
            self.file(
                &format!("sys/devices/system/memory/memory{i}/online"),
                "1\n",
            );
        }
        self
    }

    /// SMBIOS type 17 records. `None` size means an empty slot.
    pub fn memory_modules(&self, sizes_mib: &[Option<u16>], memory_type: u8, speed: u16) -> &Self {
        for (i, size) in sizes_mib.iter().enumerate() {
            let mut raw = vec![0u8; 0x22];
            raw[0] = 17;
            raw[1] = 0x22;
            raw[0x0c..0x0e].copy_from_slice(&size.unwrap_or(0).to_le_bytes());
            raw[0x0e] = 0x0d;
            raw[0x10] = 1;
            raw[0x12] = memory_type;
            raw[0x15..0x17].copy_from_slice(&speed.to_le_bytes());
            raw[0x17] = 2;
            raw[0x18] = 3;
            raw[0x1a] = 4;
            raw[0x20..0x22].copy_from_slice(&speed.to_le_bytes());
            raw.extend_from_slice(
                format!("DIMM {i}\0ExampleMem\0FIXTURE-SERIAL-{i}\0EX-{i}\0\0").as_bytes(),
            );
            self.file(&format!("sys/firmware/dmi/entries/17-{i}/raw"), raw);
        }
        self
    }

    // ---------------------------------------------------------------- pci

    /// A PCI function at `sys/devices/pci0000:00/<chain...>/<address>` with a
    /// `sys/bus/pci/devices/<address>` link.
    #[allow(clippy::too_many_arguments)]
    pub fn pci(
        &self,
        chain: &[&str],
        address: &str,
        class: u32,
        vendor: u16,
        device: u16,
        subsystem: (u16, u16),
        driver: Option<&str>,
    ) -> String {
        let mut dir = String::from("sys/devices/pci0000:00");
        for hop in chain {
            dir.push('/');
            dir.push_str(hop);
        }
        dir.push('/');
        dir.push_str(address);
        self.file(&format!("{dir}/class"), format!("0x{class:06x}\n"))
            .file(&format!("{dir}/vendor"), format!("0x{vendor:04x}\n"))
            .file(&format!("{dir}/device"), format!("0x{device:04x}\n"))
            .file(
                &format!("{dir}/subsystem_vendor"),
                format!("0x{:04x}\n", subsystem.0),
            )
            .file(
                &format!("{dir}/subsystem_device"),
                format!("0x{:04x}\n", subsystem.1),
            )
            .file(&format!("{dir}/revision"), "0x01\n");
        if let Some(driver) = driver {
            self.dir(&format!("sys/bus/pci/drivers/{driver}"));
            self.link(
                &format!("{dir}/driver"),
                &format!("{}bus/pci/drivers/{driver}", &Self::up(&dir)[3..]),
            );
        }
        self.link(
            &format!("sys/bus/pci/devices/{address}"),
            &format!("../../../{}", dir.trim_start_matches("sys/")),
        );
        dir
    }

    pub fn pci_ids(&self, text: &str) -> &Self {
        self.file("usr/share/hwdata/pci.ids", text)
    }

    pub fn usb_ids(&self, text: &str) -> &Self {
        self.file("usr/share/hwdata/usb.ids", text)
    }

    // ------------------------------------------------------------ storage

    /// A block device under `device_dir` (relative to `sys/devices`), with
    /// a `sys/block/<name>` link.
    #[allow(clippy::too_many_arguments)]
    pub fn block(
        &self,
        device_dir: &str,
        name: &str,
        sectors: u64,
        rotational: bool,
        removable: bool,
        model: Option<&str>,
        partitions: &[(u32, u64)],
    ) -> &Self {
        let dir = format!("sys/devices/{device_dir}/{name}");
        self.file(&format!("{dir}/size"), format!("{sectors}\n"))
            .file(
                &format!("{dir}/removable"),
                format!("{}\n", u8::from(removable)),
            )
            .file(&format!("{dir}/ro"), "0\n")
            .file(
                &format!("{dir}/queue/rotational"),
                format!("{}\n", u8::from(rotational)),
            )
            .file(&format!("{dir}/queue/logical_block_size"), "512\n")
            .file(
                &format!("{dir}/queue/physical_block_size"),
                if rotational { "4096\n" } else { "512\n" },
            )
            .file(&format!("{dir}/device/serial"), "FIXTURE-DISK-SERIAL\n");
        if let Some(model) = model {
            self.file(&format!("{dir}/device/model"), format!("{model}\n"));
        }
        for (number, part_sectors) in partitions {
            let part = if name.ends_with(|c: char| c.is_ascii_digit()) {
                format!("{name}p{number}")
            } else {
                format!("{name}{number}")
            };
            self.file(&format!("{dir}/{part}/partition"), format!("{number}\n"))
                .file(&format!("{dir}/{part}/start"), "2048\n")
                .file(&format!("{dir}/{part}/size"), format!("{part_sectors}\n"))
                .file(&format!("{dir}/{part}/ro"), "0\n");
        }
        self.link(
            &format!("sys/block/{name}"),
            &format!("../devices/{device_dir}/{name}"),
        )
    }

    // ------------------------------------------------------------- power

    pub fn power_supply(&self, name: &str, attrs: &[(&str, &str)]) -> &Self {
        for (attr, value) in attrs {
            self.file(
                &format!("sys/class/power_supply/{name}/{attr}"),
                format!("{value}\n"),
            );
        }
        self.file(
            &format!("sys/class/power_supply/{name}/serial_number"),
            "FIXTURE-BATTERY-SERIAL\n",
        )
    }

    // ----------------------------------------------------------- network

    pub fn net(
        &self,
        name: &str,
        arp_type: u32,
        devtype: Option<&str>,
        mac: &str,
        device: Option<&str>,
        wireless: bool,
    ) -> &Self {
        let dir = format!("sys/class/net/{name}");
        self.file(&format!("{dir}/type"), format!("{arp_type}\n"))
            .file(&format!("{dir}/address"), format!("{mac}\n"))
            .file(&format!("{dir}/addr_assign_type"), "0\n")
            .file(&format!("{dir}/operstate"), "up\n")
            .file(&format!("{dir}/carrier"), "1\n")
            .file(&format!("{dir}/mtu"), "1500\n")
            .file(
                &format!("{dir}/uevent"),
                devtype
                    .map(|d| format!("DEVTYPE={d}\nINTERFACE={name}\n"))
                    .unwrap_or_else(|| format!("INTERFACE={name}\n")),
            );
        if let Some(device) = device {
            self.dir(&format!("sys/devices/{device}"));
            self.link(
                &format!("{dir}/device"),
                &format!("../../../devices/{device}"),
            );
        }
        if wireless {
            self.dir(&format!("{dir}/wireless"));
        }
        self
    }

    // ---------------------------------------------------------------- usb

    pub fn usb(
        &self,
        path: &str,
        name: &str,
        attrs: &[(&str, &str)],
        interface_class: &str,
    ) -> &Self {
        let dir = format!("sys/devices/{path}/{name}");
        for (attr, value) in attrs {
            self.file(&format!("{dir}/{attr}"), format!("{value}\n"));
        }
        self.file(&format!("{dir}/serial"), "FIXTURE-USB-SERIAL\n");
        let iface = if let Some(bus) = name.strip_prefix("usb") {
            format!("{bus}-0:1.0")
        } else {
            format!("{name}:1.0")
        };
        self.file(
            &format!("{dir}/{iface}/bInterfaceClass"),
            format!("{interface_class}\n"),
        );
        self.link(
            &format!("sys/bus/usb/devices/{name}"),
            &format!("../../../devices/{path}/{name}"),
        );
        self.link(
            &format!("sys/bus/usb/devices/{iface}"),
            &format!("../../../devices/{path}/{name}/{iface}"),
        )
    }

    pub fn usb_port(&self, path: &str, hub: &str, port: u32, connect_type: &str) -> &Self {
        let iface = if let Some(bus) = hub.strip_prefix("usb") {
            format!("{bus}-0:1.0")
        } else {
            format!("{hub}:1.0")
        };
        self.file(
            &format!("sys/devices/{path}/{hub}/{iface}/{hub}-port{port}/connect_type"),
            format!("{connect_type}\n"),
        )
    }

    // ------------------------------------------------------------ sensors

    pub fn hwmon(
        &self,
        index: u32,
        chip: &str,
        device: Option<&str>,
        inputs: &[(&str, Option<&str>)],
    ) -> &Self {
        let dir = format!("sys/class/hwmon/hwmon{index}");
        self.file(&format!("{dir}/name"), format!("{chip}\n"));
        for (input, label) in inputs {
            self.file(&format!("{dir}/{input}_input"), "42000\n");
            if let Some(label) = label {
                self.file(&format!("{dir}/{input}_label"), format!("{label}\n"));
            }
        }
        if let Some(device) = device {
            self.link(
                &format!("{dir}/device"),
                &format!("../../../devices/{device}"),
            );
        }
        self
    }

    pub fn thermal_zone(&self, index: u32, kind: &str) -> &Self {
        let dir = format!("sys/class/thermal/thermal_zone{index}");
        self.file(&format!("{dir}/type"), format!("{kind}\n"))
            .file(&format!("{dir}/temp"), "45000\n")
            .file(&format!("{dir}/trip_point_0_temp"), "95000\n")
    }
}

// ------------------------------------------------------------ scenarios

pub const PCI_IDS: &str = "1022  Example AMD-like Vendor\n\t15e2  Example Platform Function\n\
    1002  Example GPU Vendor A\n\t1638  Example Integrated GPU\n\t\t1234 5678  Example Laptop Subsystem\n\t73ff  Example Discrete GPU A\n\
    8086  Example Intel-like Vendor\n\t46a6  Example Intel iGPU\n\
    10de  Example GPU Vendor N\n\t25a2  Example Discrete GPU N\n\
    144d  Example SSD Vendor\n\ta808  Example NVMe Controller\n\
    C 00  Unclassified device\n";

pub const USB_IDS: &str = "1d6b  Linux Foundation\n\t0002  2.0 root hub\n\t0003  3.0 root hub\n\
    0781  Example Storage Vendor\n\t5581  Example Flash Drive\nC 00  (Defined at Interface level)\n";

const OS_UBUNTU: &str = "NAME=\"Ubuntu\"\nPRETTY_NAME=\"Ubuntu 24.04 LTS\"\nID=ubuntu\nID_LIKE=debian\nVERSION=\"24.04 LTS (Noble Numbat)\"\nVERSION_ID=\"24.04\"\n";
pub const OS_FEDORA: &str = "NAME=\"Fedora Linux\"\nPRETTY_NAME=\"Fedora Linux 41 (Workstation Edition)\"\nID=fedora\nVERSION=\"41 (Workstation Edition)\"\nVERSION_ID=41\n";
pub const OS_ARCH: &str =
    "NAME=\"Arch Linux\"\nPRETTY_NAME=\"Arch Linux\"\nID=arch\nBUILD_ID=rolling\n";

fn laptop_dmi(f: &HwFixture, vendor: &str, product: &str, board: &str) {
    f.dmi(&[
        ("sys_vendor", vendor),
        ("product_name", product),
        ("product_family", "Example Series"),
        ("product_version", "1.0"),
        ("product_sku", "EX-SKU-1"),
        ("chassis_type", "10"),
        ("board_vendor", vendor),
        ("board_name", board),
        ("board_version", "A01"),
        ("bios_vendor", "Example BIOS Co"),
        ("bios_version", "1.2.3"),
        ("bios_date", "01/02/2026"),
        ("bios_release", "1.2"),
        ("product_serial", "FIXTURE-SYSTEM-SERIAL"),
        ("product_uuid", "00000000-1111-2222-3333-444444444444"),
        ("board_serial", "FIXTURE-BOARD-SERIAL"),
    ]);
}

/// AMD laptop: 8 cores / 16 threads, integrated GPU (shares its slot with
/// platform functions), NVMe, one battery, Wi-Fi, USB stick, sensors.
pub fn amd_laptop() -> HwFixture {
    let f = HwFixture::new();
    laptop_dmi(&f, "Example Corp", "Example Book 14 AMD", "EXB14-A");
    f.os_release("etc/os-release", OS_UBUNTU)
        .kernel("6.8.0-fixture", "x86_64")
        .uefi();
    f.cpus(CpuSpec {
        vendor_id: "AuthenticAMD",
        model_name: "Example AMD 8-Core Processor",
        family: 0x17,
        model: 0x68,
        packages: 1,
        cores_per_package: 8,
        threads_per_core: 2,
        efficiency_cores: 0,
    });
    f.meminfo(15_728_640, 9_000_000, 4_194_304)
        .memory_blocks(0x800_0000, 128);
    f.pci_ids(PCI_IDS).usb_ids(USB_IDS);
    f.pci(
        &["0000:00:08.1"],
        "0000:03:00.0",
        0x030000,
        0x1002,
        0x1638,
        (0x1234, 0x5678),
        Some("amdgpu"),
    );
    f.file(
        "sys/devices/pci0000:00/0000:00:08.1/0000:03:00.0/boot_vga",
        "1\n",
    )
    .file(
        "sys/devices/pci0000:00/0000:00:08.1/0000:03:00.0/mem_info_vram_total",
        "536870912\n",
    )
    .dir("sys/devices/pci0000:00/0000:00:08.1/0000:03:00.0/drm/card0")
    .dir("sys/devices/pci0000:00/0000:00:08.1/0000:03:00.0/drm/renderD128");
    f.pci(
        &["0000:00:08.1"],
        "0000:03:00.5",
        0x048000,
        0x1022,
        0x15e2,
        (0x1234, 0x5678),
        None,
    );
    f.pci(
        &["0000:00:02.4"],
        "0000:01:00.0",
        0x010802,
        0x144d,
        0xa808,
        (0x144d, 0xa801),
        Some("nvme"),
    );
    f.block(
        "pci0000:00/0000:00:02.4/0000:01:00.0/nvme/nvme0",
        "nvme0n1",
        1_000_215_216,
        false,
        false,
        Some("Example NVMe 512GB"),
        &[(1, 1_048_576), (2, 999_000_000)],
    );
    f.file(
        "sys/devices/pci0000:00/0000:00:02.4/0000:01:00.0/nvme/nvme0/nvme0n1/device/firmware_rev",
        "EXFW01\n",
    );
    f.block("virtual/block", "loop0", 0, false, false, None, &[]);
    f.block("virtual/block", "loop1", 200_000, false, false, None, &[]);
    // An encrypted root volume on the second partition, the EFI partition
    // mounted read-only, and a mount table with pseudo filesystems.
    f.block("virtual/block", "dm-0", 998_000_000, false, false, None, &[])
        .file("sys/devices/virtual/block/dm-0/dm/name", "cryptroot\n")
        .dir("sys/devices/virtual/block/dm-0/slaves/nvme0n1p2")
        .dir("sys/devices/pci0000:00/0000:00:02.4/0000:01:00.0/nvme/nvme0/nvme0n1/nvme0n1p2/holders/dm-0")
        .file(
            "proc/mounts",
            "proc /proc proc rw,nosuid 0 0\n\
             /dev/mapper/cryptroot / ext4 rw,relatime 0 0\n\
             tmpfs /run tmpfs rw 0 0\n\
             /dev/nvme0n1p1 /boot/efi vfat ro,relatime 0 0\n\
             /dev/sda1 /media/fixture/EXAMPLE\\040STICK vfat rw 0 0\n",
        )
        .file(
            "proc/swaps",
            "Filename\t\t\t\tType\t\tSize\t\tUsed\t\tPriority\n\
             /swap.img                               file\t\t2097148\t\t0\t\t-2\n",
        );
    f.power_supply(
        "BAT0",
        &[
            ("type", "Battery"),
            ("status", "Discharging"),
            ("present", "1"),
            ("capacity", "77"),
            ("energy_now", "38000000"),
            ("energy_full", "50000000"),
            ("energy_full_design", "56000000"),
            ("voltage_now", "11800000"),
            ("power_now", "7500000"),
            ("technology", "Li-ion"),
            ("manufacturer", "ExampleCell"),
            ("model_name", "EXB-BATT"),
            ("cycle_count", "120"),
            ("scope", "System"),
        ],
    );
    f.power_supply("AC", &[("type", "Mains"), ("online", "0")]);
    f.pci(
        &["0000:00:02.3"],
        "0000:02:00.0",
        0x028000,
        0x8086,
        0x2725,
        (0x8086, 0x0024),
        Some("iwlwifi"),
    );
    f.net(
        "wlan0",
        1,
        Some("wlan"),
        "02:11:22:33:44:55",
        Some("pci0000:00/0000:00:02.3/0000:02:00.0"),
        true,
    );
    f.net("lo", 772, None, "00:00:00:00:00:00", None, false);
    f.net(
        "docker0",
        1,
        Some("bridge"),
        "02:42:00:00:00:01",
        None,
        false,
    );
    // USB: root hub with a flash drive on port 1, port 2 empty.
    let hc = "pci0000:00/0000:00:08.1/0000:03:00.3";
    f.pci(
        &["0000:00:08.1"],
        "0000:03:00.3",
        0x0c0330,
        0x1022,
        0x15e0,
        (0x1234, 0x5678),
        Some("xhci_hcd"),
    );
    f.usb(
        hc,
        "usb1",
        &[
            ("busnum", "1"),
            ("devnum", "1"),
            ("idVendor", "1d6b"),
            ("idProduct", "0002"),
            ("bDeviceClass", "09"),
            ("version", " 2.00"),
            ("speed", "480"),
            ("product", "xHCI Host Controller"),
            ("maxchild", "2"),
        ],
        "09",
    )
    .usb_port(hc, "usb1", 1, "hotplug")
    .usb_port(hc, "usb1", 2, "hotplug");
    f.usb(
        &format!("{hc}/usb1"),
        "1-1",
        &[
            ("busnum", "1"),
            ("devnum", "2"),
            ("idVendor", "0781"),
            ("idProduct", "5581"),
            ("bDeviceClass", "00"),
            ("version", " 3.20"),
            ("speed", "480"),
            ("product", "Example Flash"),
            ("manufacturer", "ExampleStorage"),
            ("removable", "removable"),
        ],
        "08",
    );
    f.block(
        &format!("{hc}/usb1/1-1/1-1:1.0/host0/target0:0:0/0:0:0:0/block"),
        "sda",
        62_521_344,
        false,
        true,
        Some("Example Flash"),
        &[(1, 62_519_296)],
    );
    f.hwmon(
        0,
        "examplecpu",
        Some("pci0000:00/0000:00:18.3"),
        &[("temp1", Some("Tctl"))],
    );
    f.dir("sys/devices/pci0000:00/0000:00:18.3");
    f.hwmon(
        1,
        "nvme",
        Some("pci0000:00/0000:00:02.4/0000:01:00.0/nvme/nvme0"),
        &[("temp1", Some("Composite")), ("temp2", None)],
    );
    f.thermal_zone(0, "acpitz");
    f
}

/// Intel laptop with hybrid cores (4 P-cores with 2 threads, 8 E-cores),
/// an integrated GPU on the root bus and a dedicated NVIDIA GPU, SATA SSD,
/// two batteries.
pub fn intel_hybrid_laptop() -> HwFixture {
    let f = HwFixture::new();
    laptop_dmi(&f, "Example Corp", "Example Pro 16 Intel", "EXP16-I");
    f.os_release("usr/lib/os-release", OS_FEDORA)
        .kernel("6.11.0-fixture", "x86_64")
        .uefi();
    f.cpus(CpuSpec {
        vendor_id: "GenuineIntel",
        model_name: "Example Intel Hybrid Processor",
        family: 6,
        model: 0x9a,
        packages: 1,
        cores_per_package: 12,
        threads_per_core: 2,
        efficiency_cores: 8,
    });
    f.meminfo(32_000_000, 20_000_000, 0)
        .memory_blocks(0x800_0000, 250);
    f.memory_modules(&[Some(16384), Some(16384)], 0x22, 4800);
    f.pci_ids(PCI_IDS);
    f.pci(
        &[],
        "0000:00:02.0",
        0x030000,
        0x8086,
        0x46a6,
        (0x1234, 0x0001),
        Some("i915"),
    );
    f.file("sys/devices/pci0000:00/0000:00:02.0/boot_vga", "1\n");
    f.pci(
        &["0000:00:01.0"],
        "0000:01:00.0",
        0x030200,
        0x10de,
        0x25a2,
        (0x1234, 0x0002),
        Some("nvidia"),
    );
    f.file(
        "sys/devices/pci0000:00/0000:00:01.0/0000:01:00.0/boot_vga",
        "0\n",
    );
    f.pci(
        &[],
        "0000:00:17.0",
        0x010601,
        0x8086,
        0x43d3,
        (0x1234, 0x0003),
        Some("ahci"),
    );
    f.block(
        "pci0000:00/0000:00:17.0/ata1/host0/target0:0:0/0:0:0:0/block",
        "sda",
        1_953_525_168,
        false,
        false,
        Some("Example SATA SSD 1TB"),
        &[(1, 1_000_000), (2, 1_900_000_000)],
    );
    let battery = |name: &str| {
        f.power_supply(
            name,
            &[
                ("type", "Battery"),
                ("status", "Not charging"),
                ("present", "1"),
                ("capacity", "95"),
                ("charge_now", "3800000"),
                ("charge_full", "4000000"),
                ("charge_full_design", "4200000"),
                ("voltage_now", "12000000"),
                ("current_now", "0"),
                ("technology", "Li-poly"),
            ],
        );
    };
    battery("BAT0");
    battery("BAT1");
    f.net(
        "enp3s0",
        1,
        None,
        "02:aa:bb:cc:dd:ee",
        Some("pci0000:00/0000:00:1c.0/0000:03:00.0"),
        false,
    );
    f.file("sys/class/net/enp3s0/speed", "1000\n");
    f
}

/// Desktop without a battery: SATA HDD, missing and placeholder DMI fields,
/// a sensor readable only by root, Arch Linux, no PCI ID database.
pub fn desktop_missing_dmi() -> HwFixture {
    let f = HwFixture::new();
    f.dmi(&[
        ("sys_vendor", "To Be Filled By O.E.M."),
        ("product_name", "System Product Name"),
        ("chassis_type", "3"),
        ("board_vendor", "Example Boards Inc."),
        ("board_name", "EX-B650"),
        ("board_version", "Default string"),
        ("bios_vendor", "Example BIOS Co"),
        ("bios_version", "0805"),
    ]);
    f.os_release("etc/os-release", OS_ARCH)
        .kernel("6.12.1-arch1-1", "x86_64");
    f.dir("sys/firmware");
    f.cpus(CpuSpec {
        vendor_id: "AuthenticAMD",
        model_name: "Example AMD 16-Core Processor",
        family: 0x19,
        model: 0x61,
        packages: 1,
        cores_per_package: 16,
        threads_per_core: 2,
        efficiency_cores: 0,
    });
    f.meminfo(65_000_000, 60_000_000, 8_000_000);
    f.memory_modules(
        &[
            Some(16384),
            None,
            Some(16384),
            None,
            Some(16384),
            None,
            Some(16384),
            None,
        ],
        0x22,
        6000,
    );
    f.pci(
        &["0000:00:01.1"],
        "0000:01:00.0",
        0x030000,
        0x1002,
        0x73ff,
        (0x1002, 0x0e3a),
        Some("amdgpu"),
    );
    f.pci(
        &["0000:00:01.1"],
        "0000:01:00.1",
        0x040300,
        0x1002,
        0xab28,
        (0x1002, 0xab28),
        Some("snd_hda_intel"),
    );
    f.file(
        "sys/devices/pci0000:00/0000:00:01.1/0000:01:00.0/mem_info_vram_total",
        "8573157376\n",
    );
    f.pci(
        &[],
        "0000:00:11.0",
        0x010601,
        0x1022,
        0x7901,
        (0x1022, 0x7901),
        Some("ahci"),
    );
    f.block(
        "pci0000:00/0000:00:11.0/ata2/host1/target1:0:0/1:0:0:0/block",
        "sdb",
        7_814_037_168,
        true,
        false,
        Some("Example HDD 4TB"),
        &[(1, 7_814_035_000)],
    );
    f.dir("sys/class/power_supply");
    f.net(
        "eno1",
        1,
        None,
        "02:00:00:00:00:10",
        Some("pci0000:00/0000:00:1f.6"),
        false,
    );
    f.hwmon(
        0,
        "exampleio",
        Some("platform/exampleio.656"),
        &[
            ("fan1", Some("CPU fan")),
            ("in0", None),
            ("temp1", Some("SYSTIN")),
        ],
    );
    f.chmod("sys/class/hwmon/hwmon0/temp1_input", 0o000);
    f
}

/// An ARM single-board computer: no DMI, no PCI, no cpufreq, minimal sysfs.
pub fn arm_board() -> HwFixture {
    let f = HwFixture::new();
    f.os_release("etc/os-release", "NAME=\"Debian GNU/Linux\"\nPRETTY_NAME=\"Debian GNU/Linux 12 (bookworm)\"\nID=debian\nVERSION_ID=\"12\"\n")
        .kernel("6.6.0-fixture", "aarch64");
    for cpu in 0..4 {
        let base = format!("sys/devices/system/cpu/cpu{cpu}");
        f.file(&format!("{base}/topology/physical_package_id"), "0\n")
            .file(&format!("{base}/topology/core_id"), format!("{cpu}\n"))
            .file(
                &format!("{base}/topology/core_cpus_list"),
                format!("{cpu}\n"),
            );
    }
    f.file("sys/devices/system/cpu/online", "0-3\n");
    f.file("proc/cpuinfo", (0..4).map(|i| format!("processor\t: {i}\nFeatures\t: fp asimd\nCPU implementer\t: 0x41\nCPU part\t: 0xd08\n\n")).collect::<String>());
    f.meminfo(4_000_000, 3_000_000, 0);
    f.block(
        "platform/fe340000.mmc/mmc_host/mmc0/mmc0:0001/block",
        "mmcblk0",
        62_333_952,
        false,
        false,
        None,
        &[(1, 524_288), (2, 61_800_000)],
    );
    f
}
