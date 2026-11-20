//! Pure parsing and counter arithmetic. No I/O, so every calculation is
//! tested on literal kernel output.
//!
//! The kernel exposes cumulative counters. Rates and percentages are always
//! computed from the *difference* of two readings divided by the *measured*
//! elapsed time (monotonic clock). A counter that went backwards (reset,
//! rollover, device replaced) yields no value for that interval, never a
//! negative or huge one.

use std::collections::BTreeMap;

// ------------------------------------------------------------ /proc/stat

/// Jiffy counters of one `cpu` line in `/proc/stat`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CpuCounters {
    pub user: u64,
    pub nice: u64,
    pub system: u64,
    pub idle: u64,
    pub iowait: u64,
    pub irq: u64,
    pub softirq: u64,
    pub steal: u64,
}

impl CpuCounters {
    /// Sum of all accounted time. Guest time is already part of user/nice
    /// and is therefore not added again.
    pub fn total(&self) -> u64 {
        self.user
            + self.nice
            + self.system
            + self.idle
            + self.iowait
            + self.irq
            + self.softirq
            + self.steal
    }
}

/// Per-field shares of one interval, in percent.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CpuShares {
    pub busy: f64,
    pub user: f64,
    pub nice: f64,
    pub system: f64,
    pub idle: f64,
    pub iowait: f64,
    pub irq: f64,
    pub softirq: f64,
    pub steal: f64,
}

/// Utilisation between two readings. `None` if no time passed or any
/// counter decreased.
pub fn cpu_shares(prev: &CpuCounters, cur: &CpuCounters) -> Option<CpuShares> {
    let fields = [
        (prev.user, cur.user),
        (prev.nice, cur.nice),
        (prev.system, cur.system),
        (prev.idle, cur.idle),
        (prev.iowait, cur.iowait),
        (prev.irq, cur.irq),
        (prev.softirq, cur.softirq),
        (prev.steal, cur.steal),
    ];
    if fields.iter().any(|(p, c)| c < p) {
        return None;
    }
    let d: Vec<f64> = fields.iter().map(|(p, c)| (c - p) as f64).collect();
    let total: f64 = d.iter().sum();
    if total <= 0.0 {
        return None;
    }
    let pct = |v: f64| (v / total * 100.0).clamp(0.0, 100.0);
    let idle_like = d[3] + d[4];
    Some(CpuShares {
        busy: pct(total - idle_like),
        user: pct(d[0]),
        nice: pct(d[1]),
        system: pct(d[2]),
        idle: pct(d[3]),
        iowait: pct(d[4]),
        irq: pct(d[5]),
        softirq: pct(d[6]),
        steal: pct(d[7]),
    })
}

/// Parsed `/proc/stat`: the aggregate line and each `cpuN` line.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ProcStat {
    pub total: Option<CpuCounters>,
    pub per_cpu: BTreeMap<u32, CpuCounters>,
    pub boot_time: Option<u64>,
}

pub fn parse_proc_stat(text: &str) -> ProcStat {
    let mut stat = ProcStat::default();
    for line in text.lines() {
        let mut parts = line.split_whitespace();
        let Some(label) = parts.next() else { continue };
        if label == "btime" {
            stat.boot_time = parts.next().and_then(|v| v.parse().ok());
            continue;
        }
        let Some(rest) = label.strip_prefix("cpu") else {
            continue;
        };
        let values: Vec<u64> = parts.filter_map(|v| v.parse().ok()).collect();
        if values.len() < 4 {
            continue;
        }
        let get = |i: usize| values.get(i).copied().unwrap_or(0);
        let counters = CpuCounters {
            user: get(0),
            nice: get(1),
            system: get(2),
            idle: get(3),
            iowait: get(4),
            irq: get(5),
            softirq: get(6),
            steal: get(7),
        };
        if rest.is_empty() {
            stat.total = Some(counters);
        } else if let Ok(n) = rest.parse::<u32>() {
            stat.per_cpu.insert(n, counters);
        }
    }
    stat
}

// ------------------------------------------------------------ rates

/// Per-second rate of a cumulative counter. `None` when it decreased or
/// the interval is not positive.
pub fn rate(prev: u64, cur: u64, elapsed_s: f64) -> Option<f64> {
    if cur < prev || elapsed_s <= 0.0 || !elapsed_s.is_finite() {
        return None;
    }
    Some((cur - prev) as f64 / elapsed_s)
}

// ---------------------------------------------------------- pressure

/// One line of `/proc/pressure/*`: `(avg10, avg60)`.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct PressureLine {
    pub avg10: f64,
    pub avg60: f64,
}

/// `(some, full)`; `full` is absent for CPU on older kernels.
pub fn parse_pressure(text: &str) -> (Option<PressureLine>, Option<PressureLine>) {
    let mut some = None;
    let mut full = None;
    for line in text.lines() {
        let mut parts = line.split_whitespace();
        let kind = parts.next();
        let mut entry = PressureLine::default();
        let mut seen = 0;
        for part in parts {
            if let Some((k, v)) = part.split_once('=') {
                match (k, v.parse::<f64>()) {
                    ("avg10", Ok(v)) => {
                        entry.avg10 = v;
                        seen += 1;
                    }
                    ("avg60", Ok(v)) => {
                        entry.avg60 = v;
                        seen += 1;
                    }
                    _ => {}
                }
            }
        }
        if seen < 2 {
            continue;
        }
        match kind {
            Some("some") => some = Some(entry),
            Some("full") => full = Some(entry),
            _ => {}
        }
    }
    (some, full)
}

// ---------------------------------------------------------- loadavg

/// `/proc/loadavg`: `(load1, load5, load15, runnable, total)`.
pub fn parse_loadavg(text: &str) -> Option<(f64, f64, f64, u32, u32)> {
    let mut parts = text.split_whitespace();
    let l1 = parts.next()?.parse().ok()?;
    let l5 = parts.next()?.parse().ok()?;
    let l15 = parts.next()?.parse().ok()?;
    let (run, total) = parts.next()?.split_once('/')?;
    Some((l1, l5, l15, run.parse().ok()?, total.parse().ok()?))
}

// ---------------------------------------------------------- vmstat

pub fn parse_vmstat(text: &str) -> BTreeMap<String, u64> {
    text.lines()
        .filter_map(|l| {
            let (k, v) = l.split_once(' ')?;
            Some((k.to_string(), v.trim().parse().ok()?))
        })
        .collect()
}

// --------------------------------------------------------- diskstats

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct DiskCounters {
    pub reads: u64,
    pub sectors_read: u64,
    pub writes: u64,
    pub sectors_written: u64,
    /// Milliseconds spent with I/O in flight.
    pub io_ms: u64,
}

/// `/proc/diskstats`, keyed by device name. Sectors are always 512 bytes
/// here, whatever the device's real sector size.
pub fn parse_diskstats(text: &str) -> BTreeMap<String, DiskCounters> {
    text.lines()
        .filter_map(|line| {
            let f: Vec<&str> = line.split_whitespace().collect();
            if f.len() < 14 {
                return None;
            }
            let n = |i: usize| f[i].parse::<u64>().ok();
            Some((
                f[2].to_string(),
                DiskCounters {
                    reads: n(3)?,
                    sectors_read: n(5)?,
                    writes: n(7)?,
                    sectors_written: n(9)?,
                    io_ms: n(12)?,
                },
            ))
        })
        .collect()
}

// ------------------------------------------------------ /proc/<pid>/stat

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PidStat {
    pub pid: u32,
    pub comm: String,
    pub state: char,
    /// utime + stime in clock ticks.
    pub cpu_ticks: u64,
    pub threads: u32,
    /// Start time in clock ticks after boot.
    pub start_ticks: u64,
    pub vsize_bytes: u64,
    pub rss_pages: u64,
}

/// Parse `/proc/<pid>/stat`. The command name is in parentheses and may
/// itself contain spaces and parentheses, so fields are counted from the
/// last `)`.
pub fn parse_pid_stat(text: &str) -> Option<PidStat> {
    let open = text.find('(')?;
    let close = text.rfind(')')?;
    let pid = text[..open].trim().parse().ok()?;
    let comm = text[open + 1..close].to_string();
    let rest: Vec<&str> = text[close + 1..].split_whitespace().collect();
    // rest[0] is field 3 (state); field n is rest[n - 3].
    let field = |n: usize| rest.get(n - 3).and_then(|v| v.parse::<u64>().ok());
    Some(PidStat {
        pid,
        comm,
        state: rest.first()?.chars().next()?,
        cpu_ticks: field(14)? + field(15)?,
        threads: field(20)? as u32,
        start_ticks: field(22)?,
        vsize_bytes: field(23)?,
        rss_pages: field(24)?,
    })
}

/// The real UID from `/proc/<pid>/status`.
pub fn parse_status_uid(text: &str) -> Option<u32> {
    text.lines()
        .find_map(|l| l.strip_prefix("Uid:"))
        .and_then(|v| v.split_whitespace().next())
        .and_then(|v| v.parse().ok())
}

/// Program name from `/proc/<pid>/cmdline`: the first argument with any
/// directories removed. Further arguments are discarded unread.
pub fn command_name(cmdline: &[u8]) -> Option<String> {
    let first = cmdline.split(|b| *b == 0).next()?;
    if first.is_empty() {
        return None;
    }
    let text = String::from_utf8_lossy(first);
    let mut head: &str = &text;
    // Programs that rewrite argv[0] to a title ("sshd: user@pts/0"): keep
    // the part before the title.
    if let Some((before, _)) = head.split_once(": ") {
        head = before;
    }
    // Some programs (Chromium-based browsers among them) rewrite argv[0]
    // into the whole command line. Always cut at the first space: a program
    // path containing spaces is shortened, but no argument can leak.
    head = head.split_whitespace().next().unwrap_or(head);
    let base = head
        .rsplit('/')
        .next()
        .unwrap_or(head)
        .trim_end_matches(':');
    (!base.is_empty()).then(|| base.chars().take(64).collect())
}

/// `/etc/passwd` -> uid -> name.
pub fn parse_passwd(text: &str) -> BTreeMap<u32, String> {
    text.lines()
        .filter_map(|l| {
            let mut f = l.split(':');
            let name = f.next()?;
            let _ = f.next();
            let uid = f.next()?.parse().ok()?;
            Some((uid, name.to_string()))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const STAT_A: &str = "cpu  100 0 50 800 50 0 0 0 0 0\ncpu0 50 0 25 400 25 0 0 0 0 0\n\
                          cpu1 50 0 25 400 25 0 0 0 0 0\nintr 1 2\nbtime 1700000000\n";
    const STAT_B: &str = "cpu  200 0 100 1550 50 0 0 100 0 0\ncpu0 150 0 75 400 25 0 0 0 0 0\n\
                          cpu1 50 0 25 1150 25 0 0 100 0 0\n";

    #[test]
    fn proc_stat_parsing() {
        let a = parse_proc_stat(STAT_A);
        assert_eq!(a.total.unwrap().total(), 1000);
        assert_eq!(a.per_cpu.len(), 2);
        assert_eq!(a.boot_time, Some(1_700_000_000));
    }

    #[test]
    fn utilisation_from_counter_deltas() {
        let (a, b) = (parse_proc_stat(STAT_A), parse_proc_stat(STAT_B));
        // Total: +100 user, +50 system, +750 idle, +100 steal = 1000 jiffies.
        let s = cpu_shares(&a.total.unwrap(), &b.total.unwrap()).unwrap();
        assert!((s.busy - 25.0).abs() < 1e-9, "{s:?}");
        assert!((s.user - 10.0).abs() < 1e-9);
        assert!((s.system - 5.0).abs() < 1e-9);
        assert!((s.idle - 75.0).abs() < 1e-9);
        assert!((s.steal - 10.0).abs() < 1e-9);
        // cpu0 fully busy, cpu1 mostly idle with steal.
        let c0 = cpu_shares(&a.per_cpu[&0], &b.per_cpu[&0]).unwrap();
        assert!((c0.busy - 100.0).abs() < 1e-9);
        let c1 = cpu_shares(&a.per_cpu[&1], &b.per_cpu[&1]).unwrap();
        assert!((c1.busy - 100.0 / 850.0 * 100.0).abs() < 1e-9);
    }

    #[test]
    fn counter_resets_and_empty_intervals_yield_nothing() {
        let a = parse_proc_stat(STAT_A).total.unwrap();
        let b = parse_proc_stat(STAT_B).total.unwrap();
        assert!(cpu_shares(&b, &a).is_none(), "decrease");
        assert!(cpu_shares(&a, &a).is_none(), "no time passed");
        assert_eq!(rate(10, 5, 1.0), None);
        assert_eq!(rate(10, 20, 0.0), None);
        assert_eq!(rate(10, 20, f64::NAN), None);
        assert_eq!(rate(1_000, 3_000, 2.0), Some(1_000.0));
    }

    #[test]
    fn pressure_and_loadavg() {
        let (some, full) = parse_pressure(
            "some avg10=1.50 avg60=0.75 avg300=0.10 total=1\nfull avg10=0.25 avg60=0.10 avg300=0.0 total=2\n",
        );
        assert_eq!(some.unwrap().avg10, 1.5);
        assert_eq!(full.unwrap().avg60, 0.10);
        assert_eq!(parse_pressure("garbage").0, None);
        assert_eq!(
            parse_loadavg("1.51 2.03 1.53 2/1090 168216\n"),
            Some((1.51, 2.03, 1.53, 2, 1090))
        );
        assert_eq!(parse_loadavg("x"), None);
    }

    #[test]
    fn diskstats() {
        let d = parse_diskstats(
            " 259       0 nvme0n1 249419 81583 15420020 93118 189296 434930 24680866 720752 0 97301 820877 0 0 0 0\n\
               7       0 loop0 14 0 34 0 0 0 0 0 0 0 0 0 0 0 0\nshort line\n",
        );
        let n = d["nvme0n1"];
        assert_eq!(
            (
                n.reads,
                n.sectors_read,
                n.writes,
                n.sectors_written,
                n.io_ms
            ),
            (249_419, 15_420_020, 189_296, 24_680_866, 97_301)
        );
        assert!(d.contains_key("loop0"));
        assert_eq!(d.len(), 2);
    }

    #[test]
    fn pid_stat_with_awkward_names() {
        let s = parse_pid_stat("1234 (Web Content (x)) S 1 2 3 0 -1 4194304 537 0 0 0 150 50 0 0 20 0 7 0 824867 18214912 1819 18446744073709551615 1 2 3").unwrap();
        assert_eq!(s.pid, 1234);
        assert_eq!(s.comm, "Web Content (x)");
        assert_eq!(s.state, 'S');
        assert_eq!(s.cpu_ticks, 200);
        assert_eq!(s.threads, 7);
        assert_eq!(s.start_ticks, 824_867);
        assert_eq!(s.vsize_bytes, 18_214_912);
        assert_eq!(s.rss_pages, 1_819);
        assert!(parse_pid_stat("garbage").is_none());
        assert!(parse_pid_stat("12 (x) S 1").is_none());
    }

    #[test]
    fn command_names_never_include_arguments() {
        assert_eq!(
            command_name(b"/usr/bin/python3\0script.py\0--password=hunter2\0"),
            Some("python3".into())
        );
        assert_eq!(command_name(b"sshd: alice@pts/0\0"), Some("sshd".into()));
        assert_eq!(command_name(b""), None);
        assert_eq!(command_name(b"\0"), None);
        assert!(
            !command_name(b"tool\0--token=abc\0")
                .unwrap()
                .contains("abc")
        );
        assert_eq!(
            command_name(
                b"/opt/brave.com/brave/brave --type=renderer --crashpad-handler-pid=5995\0"
            ),
            Some("brave".into())
        );
        assert_eq!(
            command_name(b"python3 -m http.server\0"),
            Some("python3".into())
        );
    }

    #[test]
    fn uid_and_passwd() {
        assert_eq!(
            parse_status_uid("Name:\tx\nUid:\t1000\t1000\t1000\t1000\n"),
            Some(1000)
        );
        let users = parse_passwd(
            "root:x:0:0:root:/root:/bin/bash\nalice:x:1000:1000::/home/alice:/bin/sh\nbad\n",
        );
        assert_eq!(users[&0], "root");
        assert_eq!(users[&1000], "alice");
    }

    #[test]
    fn vmstat() {
        let v = parse_vmstat("pswpin 83764\npswpout 329004\nbad\n");
        assert_eq!(v["pswpout"], 329_004);
    }
}
