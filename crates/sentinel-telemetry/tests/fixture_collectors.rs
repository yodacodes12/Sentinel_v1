//! Collectors run against a synthetic `/proc` and `/sys` tree, through the
//! real engine. Every expected number is worked out by hand from the
//! fixture's counters, so these tests validate the calculations without
//! depending on (or loading) the machine running them.

use std::path::Path;

use sentinel_hardware::SysRoot;
use sentinel_telemetry::collectors::SampleTime;
use sentinel_telemetry::collectors::process::ProcessCollector;
use sentinel_telemetry::collectors::storage::StorageCollector;
use sentinel_telemetry::{
    CollectorHealth, CollectorKind, Engine, GpuMemoryModel, Quality, ReadingState, RuntimeEstimate,
};

struct Fixture(tempfile::TempDir);

impl Fixture {
    fn new() -> Self {
        Self(tempfile::tempdir().unwrap())
    }
    fn write(&self, rel: &str, content: &str) -> &Self {
        let path = self.0.path().join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, content).unwrap();
        self
    }
    fn write_bytes(&self, rel: &str, content: &[u8]) {
        let path = self.0.path().join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, content).unwrap();
    }
    fn mkdir(&self, rel: &str) {
        std::fs::create_dir_all(self.0.path().join(rel)).unwrap();
    }
    fn link(&self, rel: &str, target: &str) {
        let path = self.0.path().join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::os::unix::fs::symlink(self.0.path().join(target), path).unwrap();
    }
    fn engine(&self) -> Engine {
        // Fixed clock ticks and page size; a statvfs stub so no real
        // filesystem is measured.
        let statvfs = |_: &Path| Ok((100 * GIB, 40 * GIB, 45 * GIB));
        Engine::new(
            SysRoot::at(self.0.path()),
            ProcessCollector::new(100, 4096),
            StorageCollector::new(Box::new(statvfs)),
        )
    }
}

const GIB: u64 = 1 << 30;

fn at(mono_s: f64) -> SampleTime {
    SampleTime {
        wall_ms: 1_700_000_000_000 + (mono_s * 1000.0) as u64,
        mono_s,
        interval_ms: 1000,
    }
}

fn health(engine: &Engine, kind: CollectorKind) -> CollectorHealth {
    engine
        .statuses()
        .into_iter()
        .find(|s| s.kind == kind)
        .unwrap()
        .health
}

// ------------------------------------------------------------------ CPU

fn proc_stat(total: [u64; 4], cpu0: [u64; 4], cpu1: [u64; 4]) -> String {
    // user nice system idle iowait irq softirq steal guest guest_nice
    let line = |name: &str, [user, system, idle, iowait]: [u64; 4]| {
        format!("{name} {user} 0 {system} {idle} {iowait} 0 0 0 0 0\n")
    };
    format!(
        "{}{}{}intr 0\nctxt 0\nbtime 1700000000\nprocesses 1\n",
        line("cpu ", total),
        line("cpu0", cpu0),
        line("cpu1", cpu1)
    )
}

#[test]
fn cpu_total_and_per_core_utilisation_come_from_counter_deltas() {
    let fx = Fixture::new();
    fx.write(
        "proc/stat",
        &proc_stat([200, 200, 1600, 0], [100, 100, 800, 0], [100, 100, 800, 0]),
    );
    fx.write(
        "sys/devices/system/cpu/cpu0/cpufreq/scaling_cur_freq",
        "2400000\n",
    );
    let mut engine = fx.engine();

    engine.run(CollectorKind::Cpu, at(0.0));
    let first = engine.latest().cpu.clone().unwrap();
    // One reading is not a rate: everything waits for the second.
    assert_eq!(first.total_percent.state, ReadingState::Pending);
    assert!(
        first
            .cores
            .iter()
            .all(|c| c.utilisation_percent.state == ReadingState::Pending)
    );

    // cpu0: +100 user, +50 system, +50 idle  -> 150 / 200 = 75 %
    // cpu1: +200 idle, +100 iowait           -> 0 / 300   = 0 %
    // total: +100 user, +50 system, +250 idle, +100 iowait -> 150 / 500 = 30 %
    fx.write(
        "proc/stat",
        &proc_stat(
            [300, 250, 1850, 100],
            [200, 150, 850, 0],
            [100, 100, 1000, 100],
        ),
    );
    engine.run(CollectorKind::Cpu, at(1.0));
    let cpu = engine.latest().cpu.clone().unwrap();
    assert_eq!(cpu.logical_cpus, 2);
    assert_eq!(cpu.total_percent.value, Some(30.0));
    assert_eq!(cpu.total_percent.quality, Quality::Calculated);
    assert_eq!(cpu.times.user_percent.value, Some(20.0));
    assert_eq!(cpu.times.system_percent.value, Some(10.0));
    assert_eq!(cpu.times.iowait_percent.value, Some(20.0));
    assert_eq!(cpu.cores[0].utilisation_percent.value, Some(75.0));
    assert_eq!(cpu.cores[1].utilisation_percent.value, Some(0.0));
    // Frequency only where cpufreq exposes it; nothing is filled in for cpu1.
    assert_eq!(cpu.cores[0].frequency_khz.value, Some(2_400_000));
    assert_eq!(cpu.cores[1].frequency_khz.value, None);
    assert_ne!(cpu.cores[1].frequency_khz.state, ReadingState::Ok);
    assert_eq!(health(&engine, CollectorKind::Cpu), CollectorHealth::Ok);
}

#[test]
fn cpu_hotplug_and_counter_resets_never_produce_values() {
    let fx = Fixture::new();
    fx.write(
        "proc/stat",
        "cpu  100 0 100 800 0 0 0 0 0 0\ncpu0 100 0 100 800 0 0 0 0 0 0\n",
    );
    let mut engine = fx.engine();
    engine.run(CollectorKind::Cpu, at(0.0));
    // cpu1 comes online, and the aggregate counters go backwards.
    fx.write(
        "proc/stat",
        "cpu  50 0 50 400 0 0 0 0 0 0\ncpu0 150 0 150 900 0 0 0 0 0 0\ncpu1 5 0 5 90 0 0 0 0 0 0\n",
    );
    engine.run(CollectorKind::Cpu, at(1.0));
    let cpu = engine.latest().cpu.clone().unwrap();
    assert_eq!(
        cpu.total_percent.value, None,
        "a reset counter is not a measurement"
    );
    assert_eq!(cpu.cores[0].utilisation_percent.value, Some(50.0));
    assert_eq!(
        cpu.cores[1].utilisation_percent.state,
        ReadingState::Pending,
        "a new CPU has no previous reading"
    );
    assert_eq!(
        health(&engine, CollectorKind::Cpu),
        CollectorHealth::Partial
    );
}

// ------------------------------------------------------------ processes

fn pid_stat(
    pid: u32,
    comm: &str,
    state: char,
    cpu_ticks: u64,
    start: u64,
    rss_pages: u64,
) -> String {
    // Fields 3..=24: state ppid pgrp session tty tpgid flags minflt cminflt
    // majflt cmajflt utime stime cutime cstime priority nice threads
    // itrealvalue starttime vsize rss
    format!(
        "{pid} ({comm}) {state} 1 {pid} {pid} 0 -1 0 0 0 0 0 {cpu_ticks} 0 0 0 20 0 3 0 {start} 104857600 {rss_pages} 0\n"
    )
}

#[test]
fn process_cpu_and_memory_are_calculated_and_arguments_are_never_kept() {
    let fx = Fixture::new();
    fx.write(
        "proc/stat",
        &proc_stat([0, 0, 0, 0], [0, 0, 0, 0], [0, 0, 0, 0]),
    );
    fx.write("proc/meminfo", "MemTotal:       4096000 kB\n");
    fx.write("etc/passwd", "alice:x:1000:1000::/home/alice:/bin/bash\n");
    fx.write(
        "proc/1234/stat",
        &pid_stat(1234, "worker (x)", 'R', 1000, 5000, 102_400),
    );
    fx.write(
        "proc/1234/status",
        "Name:\tworker\nUid:\t1000\t1000\t1000\t1000\n",
    );
    fx.write_bytes(
        "proc/1234/cmdline",
        b"/usr/bin/worker\0--password=hunter2\0--token=abc\0",
    );
    fx.write("proc/1234/environ", "AWS_SECRET_ACCESS_KEY=never-read\0");
    fx.write("proc/77/stat", &pid_stat(77, "sleepy", 'S', 10, 100, 256));
    let mut engine = fx.engine();

    engine.run(CollectorKind::Processes, at(0.0));
    let first = engine.process_list().unwrap().clone();
    assert!(
        first
            .processes
            .iter()
            .all(|p| p.cpu_percent.state == ReadingState::Pending)
    );

    // 1234 used 150 ticks in 1.5 s at 100 ticks/s: 1.0 CPU-seconds per
    // second = 100 % (of one CPU). 77 used nothing.
    fx.write(
        "proc/1234/stat",
        &pid_stat(1234, "worker (x)", 'R', 1150, 5000, 102_400),
    );
    engine.run(CollectorKind::Processes, at(1.5));
    let list = engine.process_list().unwrap().clone();
    assert_eq!((list.total, list.running, list.inaccessible), (2, 1, 0));
    let worker = list.processes.iter().find(|p| p.pid == 1234).unwrap();
    assert_eq!(worker.cpu_percent.value, Some(100.0));
    // 102 400 pages x 4 KiB = 400 MiB of 4 000 MiB = 10 %.
    assert_eq!(worker.rss_bytes, 102_400 * 4096);
    assert_eq!(worker.memory_percent.value, Some(10.0));
    assert_eq!(worker.name, "worker (x)");
    assert_eq!(worker.command.as_deref(), Some("worker"));
    assert_eq!(worker.user.as_deref(), Some("alice"));
    assert_eq!(worker.threads, 3);
    assert_eq!(worker.started_at, Some(1_700_000_000 + 50));
    let sleepy = list.processes.iter().find(|p| p.pid == 77).unwrap();
    assert_eq!(sleepy.cpu_percent.value, Some(0.0));

    // Nothing from the arguments or the environment reaches the output.
    let json =
        serde_json::to_string(&list).unwrap() + &serde_json::to_string(&engine.snapshot()).unwrap();
    for secret in [
        "hunter2",
        "password",
        "token",
        "abc",
        "AWS_SECRET",
        "never-read",
    ] {
        assert!(!json.contains(secret), "{secret} leaked into telemetry");
    }
}

#[test]
fn reused_pids_and_vanished_or_unreadable_processes_are_handled() {
    let fx = Fixture::new();
    fx.write(
        "proc/stat",
        &proc_stat([0, 0, 0, 0], [0, 0, 0, 0], [0, 0, 0, 0]),
    );
    fx.write("proc/500/stat", &pid_stat(500, "old", 'S', 900, 1000, 10));
    fx.write(
        "proc/600/stat",
        &pid_stat(600, "exiting", 'S', 10, 1000, 10),
    );
    let mut engine = fx.engine();
    engine.run(CollectorKind::Processes, at(0.0));

    // 500 exited and a new process took its pid (different start time); 600
    // exited between the listing and the read (directory without stat); 700
    // belongs to someone else.
    fx.write("proc/500/stat", &pid_stat(500, "new", 'S', 5, 2000, 10));
    std::fs::remove_file(fx.0.path().join("proc/600/stat")).unwrap();
    fx.write("proc/700/stat", &pid_stat(700, "private", 'S', 5, 2000, 10));
    let private = fx.0.path().join("proc/700/stat");
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&private, std::fs::Permissions::from_mode(0o000)).unwrap();
    let readable_anyway = std::fs::read(&private).is_ok(); // running as root
    engine.run(CollectorKind::Processes, at(1.0));
    std::fs::set_permissions(&private, std::fs::Permissions::from_mode(0o600)).unwrap();

    let list = engine.process_list().unwrap();
    let reused = list.processes.iter().find(|p| p.pid == 500).unwrap();
    assert_eq!(reused.name, "new");
    assert_eq!(
        reused.cpu_percent.state,
        ReadingState::Pending,
        "no delta against a different process"
    );
    assert!(list.processes.iter().all(|p| p.pid != 600));
    if !readable_anyway {
        assert_eq!(list.inaccessible, 1);
        assert_eq!(
            health(&engine, CollectorKind::Processes),
            CollectorHealth::Partial
        );
    }
}

// --------------------------------------------------------------- memory

#[test]
fn memory_usage_follows_mem_available() {
    let fx = Fixture::new();
    fx.write(
        "proc/meminfo",
        "MemTotal:       8000000 kB\nMemFree:        1000000 kB\nMemAvailable:   2000000 kB\n\
         Buffers:         100000 kB\nCached:         1500000 kB\nSwapTotal:      2000000 kB\n\
         SwapFree:       1500000 kB\nShmem:           200000 kB\n",
    );
    let mut engine = fx.engine();
    engine.run(CollectorKind::Memory, at(0.0));
    let m = engine.latest().memory.clone().unwrap();
    assert_eq!(m.total_bytes.value, Some(8_000_000 * 1024));
    assert_eq!(m.available_bytes.value, Some(2_000_000 * 1024));
    // used = total - available = 6 000 000 kB = 75 %.
    assert_eq!(m.used_bytes.value, Some(6_000_000 * 1024));
    assert_eq!(m.used_percent.value, Some(75.0));
    assert_eq!(m.swap_used_bytes.value, Some(500_000 * 1024));
    // Pressure files absent: unknown, not zero.
    assert_eq!(m.pressure.some_avg10.value, None);
}

// -------------------------------------------------------------- storage

#[test]
fn storage_throughput_counts_physical_disks_only() {
    let fx = Fixture::new();
    let disk = |reads, sectors_r, writes, sectors_w, io_ms| {
        format!(
            " 259 0 nvme0n1 {reads} 0 {sectors_r} 0 {writes} 0 {sectors_w} 0 0 {io_ms} 0\n\
             7 0 loop0 100 0 100000 0 0 0 0 0 0 10 0\n"
        )
    };
    fx.write("proc/diskstats", &disk(1000, 20_000, 500, 10_000, 1000));
    fx.mkdir("sys/devices/pci0000:00/nvme/nvme0n1");
    fx.link("sys/block/nvme0n1", "sys/devices/pci0000:00/nvme/nvme0n1");
    fx.mkdir("sys/devices/virtual/block/loop0");
    fx.link("sys/block/loop0", "sys/devices/virtual/block/loop0");
    fx.write(
        "proc/self/mounts",
        "/dev/nvme0n1p2 / ext4 rw,relatime 0 0\n/dev/loop0 /snap/core squashfs ro 0 0\nproc /proc proc rw 0 0\n",
    );
    let mut engine = fx.engine();
    engine.run(CollectorKind::Storage, at(0.0));
    assert_eq!(
        engine.latest().storage.as_ref().unwrap().devices[0]
            .read_bps
            .state,
        ReadingState::Pending
    );

    // In 2 s: +2048 sectors read (1 MiB), +4096 written (2 MiB), +100 reads,
    // +50 writes, +500 ms busy.
    fx.write("proc/diskstats", &disk(1100, 22_048, 550, 14_096, 1500));
    engine.run(CollectorKind::Storage, at(2.0));
    let s = engine.latest().storage.clone().unwrap();
    assert_eq!(
        s.devices.len(),
        1,
        "loop devices are virtual: {:?}",
        s.devices
    );
    let d = &s.devices[0];
    assert_eq!(d.name, "nvme0n1");
    assert_eq!(d.read_bps.value, Some(524_288.0));
    assert_eq!(d.write_bps.value, Some(1_048_576.0));
    assert_eq!(d.reads_per_s.value, Some(50.0));
    assert_eq!(d.writes_per_s.value, Some(25.0));
    assert_eq!(d.busy_percent.value, Some(25.0));

    // Filesystems: local ones only, df-style percentage from the stub
    // (used 55 GiB, available 40 GiB -> 57.9 %).
    let mounts: Vec<&str> = s
        .filesystems
        .iter()
        .map(|f| f.mount_point.as_str())
        .collect();
    assert!(mounts.contains(&"/"));
    assert!(!mounts.contains(&"/proc"));
    let root = s.filesystems.iter().find(|f| f.mount_point == "/").unwrap();
    assert_eq!(root.used_bytes.value, Some(55 * GIB));
    assert_eq!(root.used_percent.value, Some(57.9));
}

// -------------------------------------------------------------- network

#[test]
fn network_throughput_is_a_rate_and_counter_resets_are_errors() {
    let fx = Fixture::new();
    let set = |rx: u64, tx: u64| {
        for (f, v) in [
            ("rx_bytes", rx),
            ("tx_bytes", tx),
            ("rx_packets", rx / 100),
            ("tx_packets", tx / 100),
        ] {
            fx.write(
                &format!("sys/class/net/wlan0/statistics/{f}"),
                &format!("{v}\n"),
            );
        }
    };
    fx.write("sys/class/net/wlan0/type", "1\n")
        .write("sys/class/net/wlan0/operstate", "up\n");
    fx.write("sys/class/net/lo/type", "772\n");
    fx.write("sys/class/net/lo/statistics/rx_bytes", "5\n")
        .write("sys/class/net/lo/statistics/tx_bytes", "5\n");
    set(1_000_000, 200_000);
    let mut engine = fx.engine();
    engine.run(CollectorKind::Network, at(10.0));

    set(1_500_000, 300_000); // +500 kB down, +100 kB up in 0.5 s
    engine.run(CollectorKind::Network, at(10.5));
    let n = engine.latest().network.clone().unwrap();
    let wlan = n.interfaces.iter().find(|i| i.name == "wlan0").unwrap();
    assert_eq!(wlan.rx_bps.value, Some(1_000_000.0));
    assert_eq!(wlan.tx_bps.value, Some(200_000.0));
    assert_eq!(wlan.rx_packets_per_s.value, Some(10_000.0));
    assert_eq!(wlan.operational_state.as_deref(), Some("up"));
    assert!(!wlan.loopback);
    assert!(
        n.interfaces
            .iter()
            .find(|i| i.name == "lo")
            .unwrap()
            .loopback
    );

    set(10, 10); // the interface was re-created
    engine.run(CollectorKind::Network, at(11.0));
    let n = engine.latest().network.clone().unwrap();
    let wlan = n.interfaces.iter().find(|i| i.name == "wlan0").unwrap();
    assert_eq!(wlan.rx_bps.value, None);
    assert_eq!(wlan.rx_bps.state, ReadingState::Error);
}

// ------------------------------------------------------ thermal and fans

#[test]
fn temperatures_and_fan_speeds_are_parsed_and_malformed_values_rejected() {
    let fx = Fixture::new();
    fx.write("sys/class/hwmon/hwmon0/name", "k10temp\n");
    fx.write("sys/class/hwmon/hwmon0/temp1_input", "61250\n");
    fx.write("sys/class/hwmon/hwmon0/temp1_label", "Tctl\n");
    fx.write("sys/class/hwmon/hwmon0/temp1_crit", "100000\n");
    fx.write("sys/class/hwmon/hwmon1/name", "thinkpad\n");
    fx.write("sys/class/hwmon/hwmon1/temp1_input", "garbage\n");
    fx.write("sys/class/hwmon/hwmon1/fan1_input", "2900\n");
    fx.write("sys/class/hwmon/hwmon1/fan1_label", "CPU fan\n");
    fx.write("sys/class/hwmon/hwmon1/fan1_max", "5200\n");
    fx.write("sys/class/hwmon/hwmon1/fan2_input", "-1\n");
    fx.write("sys/class/hwmon/hwmon1/pwm1", "128\n");
    fx.write("sys/class/thermal/thermal_zone0/type", "acpitz\n");
    fx.write("sys/class/thermal/thermal_zone0/temp", "45000\n");
    fx.write(
        "sys/class/thermal/thermal_zone0/trip_point_0_type",
        "critical\n",
    );
    fx.write(
        "sys/class/thermal/thermal_zone0/trip_point_0_temp",
        "105000\n",
    );
    let mut engine = fx.engine();
    engine.run(CollectorKind::Thermal, at(0.0));
    engine.run(CollectorKind::Fans, at(0.0));

    let t = engine.latest().thermal.clone().unwrap();
    let tctl = t
        .temperatures
        .iter()
        .find(|r| r.id == "hwmon0/temp1")
        .unwrap();
    assert_eq!(tctl.value_c.value, Some(61.3));
    assert_eq!(tctl.critical_c, Some(100.0));
    let bad = t
        .temperatures
        .iter()
        .find(|r| r.id == "hwmon1/temp1")
        .unwrap();
    assert_eq!(
        (bad.value_c.value, bad.value_c.state),
        (None, ReadingState::Error)
    );
    let zone = t
        .temperatures
        .iter()
        .find(|r| r.id == "thermal_zone0")
        .unwrap();
    assert_eq!(
        (zone.value_c.value, zone.critical_c),
        (Some(45.0), Some(105.0))
    );
    assert_eq!(
        health(&engine, CollectorKind::Thermal),
        CollectorHealth::Partial
    );

    let f = engine.latest().fans.clone().unwrap();
    assert_eq!(f.fans.len(), 2, "pwm files are not fans: {:?}", f.fans);
    let fan1 = f.fans.iter().find(|r| r.id == "hwmon1/fan1").unwrap();
    assert_eq!(fan1.rpm.value, Some(2900));
    assert_eq!(fan1.max_rpm, Some(5200));
    assert_eq!(fan1.label.as_deref(), Some("CPU fan"));
    let fan2 = f.fans.iter().find(|r| r.id == "hwmon1/fan2").unwrap();
    assert_eq!(
        (fan2.rpm.value, fan2.rpm.state),
        (None, ReadingState::Error)
    );
    // The fan control file was left exactly as it was.
    assert_eq!(
        std::fs::read_to_string(fx.0.path().join("sys/class/hwmon/hwmon1/pwm1")).unwrap(),
        "128\n"
    );
}

#[test]
fn a_thermal_zone_is_not_counted_twice() {
    let fx = Fixture::new();
    let zone = "sys/devices/virtual/thermal/thermal_zone0";
    fx.write(&format!("{zone}/type"), "acpitz\n");
    fx.write(&format!("{zone}/temp"), "45000\n");
    fx.write(&format!("{zone}/hwmon1/name"), "acpitz\n");
    fx.write(&format!("{zone}/hwmon1/temp1_input"), "45000\n");
    fx.link("sys/class/thermal/thermal_zone0", zone);
    fx.link("sys/class/hwmon/hwmon1", &format!("{zone}/hwmon1"));
    let mut engine = fx.engine();
    engine.run(CollectorKind::Thermal, at(0.0));
    let t = engine.latest().thermal.clone().unwrap();
    let ids: Vec<&str> = t.temperatures.iter().map(|r| r.id.as_str()).collect();
    assert_eq!(ids, ["thermal_zone0"]);
}

#[test]
fn no_sensors_means_unsupported_not_zero() {
    let fx = Fixture::new();
    let mut engine = fx.engine();
    for kind in [
        CollectorKind::Thermal,
        CollectorKind::Fans,
        CollectorKind::Gpu,
        CollectorKind::Battery,
    ] {
        engine.run(kind, at(0.0));
        assert_eq!(
            health(&engine, kind),
            CollectorHealth::Unsupported,
            "{kind:?}"
        );
    }
    let s = engine.latest();
    assert!(s.thermal.as_ref().is_none_or(|t| t.temperatures.is_empty()));
    assert!(s.fans.as_ref().is_none_or(|f| f.fans.is_empty()));
    assert!(s.gpu.as_ref().is_none_or(|g| g.gpus.is_empty()));
    assert!(s.battery.is_none());
}

// ------------------------------------------------------------------ GPU

#[test]
fn gpu_metrics_are_read_where_the_driver_exposes_them() {
    let fx = Fixture::new();
    let amd = "sys/bus/pci/devices/0000:03:00.0";
    fx.write(&format!("{amd}/class"), "0x030000\n")
        .write(&format!("{amd}/vendor"), "0x1002\n");
    fx.mkdir("sys/bus/pci/drivers/amdgpu");
    fx.link(&format!("{amd}/driver"), "sys/bus/pci/drivers/amdgpu");
    fx.write(&format!("{amd}/gpu_busy_percent"), "37\n");
    fx.write(&format!("{amd}/mem_info_vram_used"), "1073741824\n");
    fx.write(&format!("{amd}/mem_info_vram_total"), "8589934592\n");
    fx.write(&format!("{amd}/hwmon/hwmon5/temp1_input"), "54000\n");
    fx.write(&format!("{amd}/hwmon/hwmon5/power1_average"), "15250000\n");
    fx.write(&format!("{amd}/hwmon/hwmon5/freq1_input"), "800000000\n");

    let nv = "sys/bus/pci/devices/0000:01:00.0";
    fx.write(&format!("{nv}/class"), "0x030200\n")
        .write(&format!("{nv}/vendor"), "0x10de\n");
    fx.mkdir("sys/bus/pci/drivers/nvidia");
    fx.link(&format!("{nv}/driver"), "sys/bus/pci/drivers/nvidia");

    let usb = "sys/bus/pci/devices/0000:00:14.0";
    fx.write(&format!("{usb}/class"), "0x0c0330\n")
        .write(&format!("{usb}/vendor"), "0x8086\n");

    let mut engine = fx.engine();
    engine.run(CollectorKind::Gpu, at(0.0));
    let g = engine.latest().gpu.clone().unwrap();
    assert_eq!(g.gpus.len(), 2, "only display-class devices are GPUs");

    let a = g
        .gpus
        .iter()
        .find(|x| x.pci_address.ends_with("03:00.0"))
        .unwrap();
    assert_eq!(a.driver.as_deref(), Some("amdgpu"));
    assert_eq!(a.memory_model, GpuMemoryModel::Dedicated);
    assert_eq!(a.busy_percent.value, Some(37.0));
    assert_eq!(a.vram_used_bytes.value, Some(GIB));
    assert_eq!(a.vram_total_bytes.value, Some(8 * GIB));
    assert_eq!(a.temperature_c.value, Some(54.0));
    assert_eq!(a.power_w.value, Some(15.3));
    assert_eq!(a.core_clock_mhz.value, Some(800));
    assert!(a.note.is_none());

    // NVIDIA: honestly unsupported, with the reason; nothing invented.
    let n = g
        .gpus
        .iter()
        .find(|x| x.pci_address.ends_with("01:00.0"))
        .unwrap();
    assert_eq!(n.busy_percent.value, None);
    assert_eq!(n.temperature_c.value, None);
    assert!(
        n.note
            .as_deref()
            .unwrap()
            .contains("does not run nvidia-smi")
    );
    assert_eq!(
        health(&engine, CollectorKind::Gpu),
        CollectorHealth::Partial
    );
}

// -------------------------------------------------------------- battery

fn battery(fx: &Fixture, status: &str, energy_now_uwh: u64, power_uw: u64) {
    let b = "sys/class/power_supply/BAT0";
    fx.write(&format!("{b}/type"), "Battery\n");
    fx.write(&format!("{b}/status"), &format!("{status}\n"));
    fx.write(
        &format!("{b}/capacity"),
        &format!("{}\n", energy_now_uwh * 100 / 50_000_000),
    );
    fx.write(&format!("{b}/energy_now"), &format!("{energy_now_uwh}\n"));
    fx.write(&format!("{b}/energy_full"), "50000000\n");
    fx.write(&format!("{b}/energy_full_design"), "57000000\n");
    fx.write(&format!("{b}/power_now"), &format!("{power_uw}\n"));
    fx.write(&format!("{b}/voltage_now"), "12000000\n");
    fx.write(&format!("{b}/cycle_count"), "0\n");
}

/// A battery that reports charge (Ah, A) rather than energy, like the
/// development laptop's: the runtime is charge / current, and the average
/// draw is reported in watts (current x voltage), never as amperes.
#[test]
fn charge_reporting_battery_reports_its_draw_in_watts() {
    let fx = Fixture::new();
    let b = "sys/class/power_supply/BAT1";
    fx.write(&format!("{b}/type"), "Battery\n");
    fx.write(&format!("{b}/status"), "Discharging\n");
    fx.write(&format!("{b}/charge_full"), "2988000\n");
    fx.write(&format!("{b}/charge_full_design"), "3645000\n");
    fx.write(&format!("{b}/current_now"), "522000\n");
    fx.write(&format!("{b}/voltage_now"), "10552000\n");
    let mut engine = fx.engine();
    let mut charge = 1_000_000u64;
    for i in 0..=12 {
        fx.write(&format!("{b}/charge_now"), &format!("{charge}\n"));
        fx.write(
            &format!("{b}/capacity"),
            &format!("{}\n", charge * 100 / 2_988_000),
        );
        engine.run(CollectorKind::Battery, at(i as f64 * 30.0));
        charge -= 522_000 / 120; // 0.522 A for 30 s
    }
    let bat = engine.latest().battery.clone().unwrap().batteries.remove(0);
    assert_eq!(bat.energy_now_wh.state, ReadingState::Unsupported);
    assert_eq!(bat.power_w.value, Some(5.51));
    assert_eq!(bat.power_w.quality, Quality::Calculated);
    let RuntimeEstimate::Estimated {
        seconds,
        average_power_w,
        ..
    } = bat.runtime
    else {
        panic!("steady discharge gives an estimate: {:?}", bat.runtime);
    };
    let expected = (charge + 522_000 / 120) as f64 / 522_000.0 * 3600.0;
    assert!(
        (seconds as f64 - expected).abs() <= 60.0,
        "{seconds} vs {expected:.0}"
    );
    assert_eq!(average_power_w, Some(5.51), "watts, not 0.52 A");

    // Without a voltage the draw cannot be expressed in watts: none is given.
    std::fs::remove_file(fx.0.path().join(format!("{b}/voltage_now"))).unwrap();
    engine.run(CollectorKind::Battery, at(400.0));
    let bat = engine.latest().battery.clone().unwrap().batteries.remove(0);
    if let RuntimeEstimate::Estimated {
        average_power_w, ..
    } = bat.runtime
    {
        assert_eq!(average_power_w, None);
    }
}

#[test]
fn battery_health_runtime_and_temperature_are_reported_only_with_evidence() {
    let fx = Fixture::new();
    fx.write("sys/class/power_supply/AC/type", "Mains\n");
    fx.write("sys/class/power_supply/AC/online", "0\n");
    battery(&fx, "Discharging", 40_000_000, 10_000_000);
    let mut engine = fx.engine();
    engine.run(CollectorKind::Battery, at(0.0));
    let report = engine.latest().battery.clone().unwrap();
    let b = &report.batteries[0];
    assert_eq!(report.on_ac.value, Some(false));
    assert_eq!(b.percent.value, Some(80.0));
    assert_eq!(b.energy_now_wh.value, Some(40.0));
    assert_eq!(b.power_w.value, Some(10.0));
    // 50 Wh of 57 Wh design capacity.
    assert_eq!(b.health_percent.value, Some(87.7));
    assert_eq!(b.health_percent.quality, Quality::Estimated);
    // "0 cycles" is firmware for "not counted".
    assert_eq!(b.cycle_count.value, None);
    // No temp file: no temperature, and nothing borrowed from other sensors.
    assert_eq!(b.temperature_c.value, None);
    // One sample is not enough to estimate a runtime.
    assert!(
        matches!(b.runtime, RuntimeEstimate::Calculating { .. }),
        "{:?}",
        b.runtime
    );

    // A steady 10 W discharge over several minutes gives an estimate near
    // energy / power.
    let mut energy = 40_000_000u64;
    for i in 1..=20 {
        energy -= 10_000_000 / 120; // 10 W for 30 s = 83 333 µWh
        battery(&fx, "Discharging", energy, 10_000_000);
        engine.run(CollectorKind::Battery, at(i as f64 * 30.0));
    }
    let b = engine.latest().battery.clone().unwrap().batteries.remove(0);
    let RuntimeEstimate::Estimated {
        seconds,
        average_power_w,
        ..
    } = b.runtime
    else {
        panic!("steady discharge gives an estimate: {:?}", b.runtime);
    };
    let expected = energy as f64 / 10_000_000.0 * 3600.0;
    assert!(
        (seconds as f64 - expected).abs() <= 120.0,
        "{seconds} s vs {expected:.0} s"
    );
    assert!((average_power_w.unwrap() - 10.0).abs() < 0.1);

    // Plugging in clears the estimate immediately.
    battery(&fx, "Charging", energy, 20_000_000);
    fx.write("sys/class/power_supply/AC/online", "1\n");
    engine.run(CollectorKind::Battery, at(700.0));
    let report = engine.latest().battery.clone().unwrap();
    assert_eq!(report.on_ac.value, Some(true));
    assert_eq!(report.batteries[0].runtime, RuntimeEstimate::NotDischarging);

    // A battery that reports its own temperature (decidegrees).
    fx.write("sys/class/power_supply/BAT0/temp", "312\n");
    engine.run(CollectorKind::Battery, at(701.0));
    assert_eq!(
        engine.latest().battery.as_ref().unwrap().batteries[0]
            .temperature_c
            .value,
        Some(31.2)
    );
}

// --------------------------------------------------- failure isolation

#[test]
fn a_failing_collector_does_not_stop_the_others() {
    let fx = Fixture::new();
    // No /proc/stat and a malformed loadavg; memory and network are fine.
    fx.write(
        "proc/meminfo",
        "MemTotal:       1000 kB\nMemAvailable:    500 kB\n",
    );
    fx.write("proc/loadavg", "not numbers\n");
    fx.write("sys/class/net/eth0/statistics/rx_bytes", "1\n");
    fx.write("sys/class/net/eth0/statistics/tx_bytes", "1\n");
    let mut engine = fx.engine();
    for kind in CollectorKind::ALL {
        engine.run(kind, at(0.0));
    }
    assert_eq!(
        health(&engine, CollectorKind::Cpu),
        CollectorHealth::Failing
    );
    assert_eq!(
        health(&engine, CollectorKind::SystemLoad),
        CollectorHealth::Failing
    );
    assert_eq!(
        health(&engine, CollectorKind::Storage),
        CollectorHealth::Failing
    );
    assert_eq!(health(&engine, CollectorKind::Memory), CollectorHealth::Ok);
    assert_eq!(health(&engine, CollectorKind::Network), CollectorHealth::Ok);
    let s = engine.latest();
    assert!(
        s.cpu.is_none(),
        "a failed collector leaves its section empty, never zeroed"
    );
    let load = s.load.as_ref().unwrap();
    assert_eq!(
        (load.load1.value, load.load1.state),
        (None, ReadingState::Error)
    );
    assert!(s.memory.is_some() && s.network.is_some());
    let cpu = engine
        .statuses()
        .into_iter()
        .find(|c| c.kind == CollectorKind::Cpu)
        .unwrap();
    assert_eq!(cpu.consecutive_failures, 1);
    assert!(cpu.note.unwrap().contains("/proc/stat"));
    assert!(cpu.last_sample_at.is_none());

    // The source comes back: the collector recovers on its own.
    fx.write(
        "proc/stat",
        &proc_stat([1, 1, 1, 0], [1, 1, 1, 0], [0, 0, 0, 0]),
    );
    engine.run(CollectorKind::Cpu, at(1.0));
    assert_eq!(health(&engine, CollectorKind::Cpu), CollectorHealth::Ok);
}
