//! Per-process CPU and memory from `/proc/<pid>/stat`, owner from
//! `/proc/<pid>/status`, program name from the head of `cmdline`.
//!
//! Never read: `environ`, memory maps, open files, or arguments beyond the
//! program name (the command line is read only up to 256 bytes and cut at
//! the first argument). Processes are only observed; there is no way to
//! signal, renice or otherwise affect them.

use std::collections::BTreeMap;

use sentinel_hardware::SysRoot;
use sentinel_hardware::source::ReadError;

use super::{RunReport, SampleTime};
use crate::counters::{
    command_name, parse_passwd, parse_pid_stat, parse_proc_stat, parse_status_uid,
};
use crate::model::{ProcessList, ProcessSample, ProcessState, Reading, round1};

pub struct ProcessCollector {
    clock_ticks: u64,
    page_size: u64,
    /// `(pid, start_ticks)` -> cpu ticks. Keyed by start time so a reused
    /// pid is never mistaken for the old process.
    prev: BTreeMap<(u32, u64), u64>,
    prev_t: Option<f64>,
    users: Option<BTreeMap<u32, String>>,
}

impl ProcessCollector {
    pub fn new(clock_ticks: u64, page_size: u64) -> Self {
        Self {
            clock_ticks: clock_ticks.max(1),
            page_size,
            prev: BTreeMap::new(),
            prev_t: None,
            users: None,
        }
    }

    pub fn for_host() -> Self {
        Self::new(
            rustix::param::clock_ticks_per_second(),
            rustix::param::page_size() as u64,
        )
    }

    pub fn sample(&mut self, root: &SysRoot, t: SampleTime) -> (Option<ProcessList>, RunReport) {
        let Ok(entries) = root.list("proc") else {
            return (None, RunReport::failing("/proc is not readable"));
        };
        if self.users.is_none() {
            self.users = Some(
                root.read("etc/passwd")
                    .map(|p| parse_passwd(&p))
                    .unwrap_or_default(),
            );
        }
        let boot_time = root
            .read("proc/stat")
            .ok()
            .and_then(|s| parse_proc_stat(&s).boot_time);
        let mem_total = root.read("proc/meminfo").ok().and_then(|m| {
            sentinel_hardware::parsers::meminfo(&m)
                .get("MemTotal")
                .copied()
        });
        let elapsed = self.prev_t.map(|p| t.mono_s - p).filter(|e| *e > 0.0);

        let mut next = BTreeMap::new();
        let mut processes = Vec::new();
        let (mut inaccessible, mut running) = (0u32, 0u32);
        for entry in entries
            .iter()
            .filter(|e| e.bytes().all(|b| b.is_ascii_digit()))
        {
            let stat = match root.read(&format!("proc/{entry}/stat")) {
                Ok(text) => match parse_pid_stat(&text) {
                    Some(s) => s,
                    None => continue,
                },
                // The process exited between listing and reading: normal.
                Err(ReadError::Missing) => continue,
                Err(_) => {
                    inaccessible += 1;
                    continue;
                }
            };
            next.insert((stat.pid, stat.start_ticks), stat.cpu_ticks);
            let cpu_percent = match (elapsed, self.prev.get(&(stat.pid, stat.start_ticks))) {
                (Some(dt), Some(prev)) if stat.cpu_ticks >= *prev => Reading::calculated(round1(
                    (stat.cpu_ticks - prev) as f64 / self.clock_ticks as f64 / dt * 100.0,
                )),
                _ => Reading::pending(),
            };
            let rss = stat.rss_pages * self.page_size;
            let uid = root
                .read(&format!("proc/{entry}/status"))
                .ok()
                .and_then(|s| parse_status_uid(&s));
            let command = root
                .read_head(&format!("proc/{entry}/cmdline"), 256)
                .ok()
                .and_then(|c| command_name(&c));
            let state = match stat.state {
                'R' => ProcessState::Running,
                'S' => ProcessState::Sleeping,
                'D' => ProcessState::DiskSleep,
                'T' | 't' => ProcessState::Stopped,
                'Z' => ProcessState::Zombie,
                'I' => ProcessState::Idle,
                _ => ProcessState::Other,
            };
            if state == ProcessState::Running {
                running += 1;
            }
            processes.push(ProcessSample {
                pid: stat.pid,
                user: uid.and_then(|u| self.users.as_ref().and_then(|m| m.get(&u).cloned())),
                uid,
                name: stat.comm,
                command,
                state,
                cpu_percent,
                rss_bytes: rss,
                virtual_bytes: stat.vsize_bytes,
                memory_percent: match mem_total {
                    Some(total) if total > 0 => {
                        Reading::calculated(round1(rss as f64 / total as f64 * 100.0))
                    }
                    _ => Reading::missing(crate::model::ReadingState::Unsupported),
                },
                threads: stat.threads,
                started_at: boot_time.map(|b| b + stat.start_ticks / self.clock_ticks),
            });
        }
        self.prev = next;
        self.prev_t = Some(t.mono_s);
        let report = if inaccessible > 0 {
            RunReport::partial(format!(
                "{inaccessible} process(es) could not be read by this user"
            ))
        } else {
            RunReport::ok()
        };
        let list = ProcessList {
            measured_at: t.wall_ms,
            interval_ms: t.interval_ms,
            total: processes.len() as u32 + inaccessible,
            running,
            inaccessible,
            processes,
        };
        (Some(list), report)
    }
}
