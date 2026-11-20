//! Disk I/O from `/proc/diskstats` deltas; filesystem usage from
//! `/proc/self/mounts` and `statvfs`. I/O activity is not a health measure.
//! Nothing is mounted, unmounted or opened for writing.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use sentinel_hardware::SysRoot;

use super::{RunReport, SampleTime};
use crate::counters::{DiskCounters, parse_diskstats, rate};
use crate::model::{DiskIo, FilesystemUsage, Reading, ReadingState, StorageReport, round1};

/// `(total, available-to-user, free)` bytes of a mounted filesystem.
pub type FsStatFn = Box<dyn Fn(&Path) -> std::io::Result<(u64, u64, u64)> + Send>;

/// Real local filesystems. Pseudo, network and snap/squash mounts are
/// skipped: `statvfs` on a network mount can block.
const LOCAL_FS: &[&str] = &[
    "ext2", "ext3", "ext4", "xfs", "btrfs", "f2fs", "vfat", "exfat", "ntfs", "ntfs3", "fuseblk",
    "zfs", "jfs", "reiserfs", "nilfs2", "bcachefs",
];

pub struct StorageCollector {
    prev: Option<(f64, BTreeMap<String, DiskCounters>)>,
    statvfs: FsStatFn,
}

/// Host `statvfs` (a read-only system call).
pub fn host_statvfs(path: &Path) -> std::io::Result<(u64, u64, u64)> {
    let s = rustix::fs::statvfs(path).map_err(std::io::Error::from)?;
    Ok((
        s.f_blocks * s.f_frsize,
        s.f_bavail * s.f_frsize,
        s.f_bfree * s.f_frsize,
    ))
}

pub use sentinel_hardware::parsers::unescape_mount;

/// `(device, mount point, fs type, read-only)` of local filesystems,
/// one per device.
pub fn parse_mounts(text: &str) -> Vec<(String, String, String, bool)> {
    let mut seen = BTreeSet::new();
    text.lines()
        .filter_map(|l| {
            let f: Vec<&str> = l.split_whitespace().collect();
            if f.len() < 4 || !f[0].starts_with("/dev/") || !LOCAL_FS.contains(&f[2]) {
                return None;
            }
            let ro = f[3].split(',').any(|o| o == "ro");
            Some((f[0].to_string(), unescape_mount(f[1]), f[2].to_string(), ro))
        })
        .filter(|(dev, ..)| seen.insert(dev.clone()))
        .collect()
}

impl StorageCollector {
    pub fn new(statvfs: FsStatFn) -> Self {
        Self {
            prev: None,
            statvfs,
        }
    }

    pub fn for_host() -> Self {
        Self::new(Box::new(host_statvfs))
    }

    pub fn sample(&mut self, root: &SysRoot, t: SampleTime) -> (Option<StorageReport>, RunReport) {
        let stats = match root.read("proc/diskstats") {
            Ok(s) => parse_diskstats(&s),
            Err(e) => {
                return (
                    None,
                    RunReport::failing(format!("/proc/diskstats unreadable: {e:?}")),
                );
            }
        };
        // Physical disks only: devices in /sys/block that are not virtual.
        let physical: Vec<String> = root
            .list("sys/block")
            .unwrap_or_default()
            .into_iter()
            .filter(|d| {
                !root
                    .canonical(&format!("sys/block/{d}"))
                    .unwrap_or_default()
                    .contains("/virtual/")
            })
            .collect();
        let elapsed = self.prev.as_ref().map(|(pt, _)| t.mono_s - pt);
        let mut devices = Vec::new();
        for name in &physical {
            let Some(cur) = stats.get(name) else { continue };
            let prev = self.prev.as_ref().and_then(|(_, m)| m.get(name));
            let r = |p: fn(&DiskCounters) -> u64, scale: f64| match (prev, elapsed) {
                (Some(prev), Some(dt)) => match rate(p(prev), p(cur), dt) {
                    Some(v) => Reading::calculated(round1(v * scale)),
                    None => Reading::missing(ReadingState::Error),
                },
                _ => Reading::pending(),
            };
            let busy = match (prev, elapsed) {
                (Some(prev), Some(dt)) => match rate(prev.io_ms, cur.io_ms, dt) {
                    Some(ms_per_s) => Reading::calculated(round1((ms_per_s / 10.0).min(100.0))),
                    None => Reading::missing(ReadingState::Error),
                },
                _ => Reading::pending(),
            };
            devices.push(DiskIo {
                name: name.clone(),
                read_bps: r(|c| c.sectors_read, 512.0),
                write_bps: r(|c| c.sectors_written, 512.0),
                reads_per_s: r(|c| c.reads, 1.0),
                writes_per_s: r(|c| c.writes, 1.0),
                busy_percent: busy,
                read_total_bytes: cur.sectors_read * 512,
                written_total_bytes: cur.sectors_written * 512,
            });
        }
        self.prev = Some((t.mono_s, stats));

        let mounts = root
            .read("proc/self/mounts")
            .map(|m| parse_mounts(&m))
            .unwrap_or_default();
        let mut filesystems = Vec::new();
        let mut failed = 0;
        for (device, mount_point, fs_type, read_only) in mounts {
            let path = root.root().join(mount_point.trim_start_matches('/'));
            let (total, available, used, pct) = match (self.statvfs)(&path) {
                Ok((total, avail, free)) if total > 0 => {
                    let used = total.saturating_sub(free);
                    // Like df: used / (used + available to users).
                    let pct = round1(used as f64 / (used + avail).max(1) as f64 * 100.0);
                    (
                        Reading::direct(total),
                        Reading::direct(avail),
                        Reading::calculated(used),
                        Reading::calculated(pct),
                    )
                }
                Ok(_) => {
                    let m = || Reading::missing(ReadingState::Unavailable);
                    (m(), m(), m(), Reading::missing(ReadingState::Unavailable))
                }
                Err(e) => {
                    failed += 1;
                    let state = if e.kind() == std::io::ErrorKind::PermissionDenied {
                        ReadingState::PermissionDenied
                    } else {
                        ReadingState::Error
                    };
                    (
                        Reading::missing(state),
                        Reading::missing(state),
                        Reading::missing(state),
                        Reading::missing(state),
                    )
                }
            };
            filesystems.push(FilesystemUsage {
                mount_point,
                device,
                fs_type,
                total_bytes: total,
                used_bytes: used,
                available_bytes: available,
                used_percent: pct,
                read_only,
            });
        }
        let report = if failed > 0 {
            RunReport::partial(format!("{failed} filesystem(s) could not be measured"))
        } else {
            RunReport::ok()
        };
        (
            Some(StorageReport {
                measured_at: t.wall_ms,
                interval_ms: t.interval_ms,
                devices,
                filesystems,
            }),
            report,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mounts_are_filtered_and_unescaped() {
        let m = parse_mounts(
            "/dev/nvme0n1p2 / ext4 rw,relatime 0 0\nproc /proc proc rw 0 0\n\
             /dev/nvme0n1p1 /boot/efi vfat rw 0 0\n/dev/loop1 /snap/core squashfs ro 0 0\n\
             /dev/sdb1 /media/u/My\\040Drive exfat ro,nosuid 0 0\n/dev/nvme0n1p2 /home ext4 rw 0 0\n\
             server:/x /mnt nfs4 rw 0 0\n",
        );
        let points: Vec<&str> = m.iter().map(|x| x.1.as_str()).collect();
        assert_eq!(points, ["/", "/boot/efi", "/media/u/My Drive"]);
        assert!(m[2].3, "read-only flag");
    }
}
