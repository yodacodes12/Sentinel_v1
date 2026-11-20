//! Read-only access to the Linux information sources.
//!
//! All discovery code reads through [`SysRoot`]. It can only read: there is
//! no method that writes, creates, renames or deletes anything, and files are
//! only ever opened with `File::open` (read-only). Paths are relative to a
//! root directory, `/` in production and a synthetic fixture tree in tests,
//! so the same code is exercised either way.
//!
//! Some sysfs attributes identify a specific machine or device (serial
//! numbers, UUIDs). [`SysRoot`] refuses to open them at all, so they cannot
//! end up in an inventory, a cache or a log by accident.

use std::fs::{self, File};
use std::io::{BufRead, BufReader, ErrorKind, Read};
use std::ops::ControlFlow;
use std::path::{Component, Path, PathBuf};
use std::sync::Mutex;

use sentinel_inventory::Availability;

/// Largest file read in one piece. Sysfs attributes are tiny; `/proc/cpuinfo`
/// on a very large server is the biggest thing read whole.
const MAX_READ_BYTES: u64 = 4 * 1024 * 1024;

/// Attribute names that identify an individual machine, device or user.
/// They are never opened.
pub const SENSITIVE_NAMES: &[&str] = &[
    "product_serial",
    "board_serial",
    "chassis_serial",
    "product_uuid",
    "serial",
    "serial_number",
    "eppid",
    "wwid",
    "wwn",
    "uuid",
    "nguid",
    "eui",
    "subsysnqn",
    "machine-id",
    "boot_id",
    "board_asset_tag",
    "chassis_asset_tag",
    // Process private data: environment variables and memory.
    "environ",
    "mem",
    "maps",
    "smaps",
    "pagemap",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReadError {
    Missing,
    PermissionDenied,
    /// Present but empty after trimming.
    Empty,
    /// Not opened by policy (sensitive attribute or a path outside the root).
    Refused,
    TooLarge,
    Other(String),
}

impl ReadError {
    pub fn availability(&self) -> Availability {
        match self {
            ReadError::Missing => Availability::Unsupported,
            ReadError::PermissionDenied => Availability::PermissionDenied,
            ReadError::Empty => Availability::Unavailable,
            ReadError::Refused => Availability::Unavailable,
            ReadError::TooLarge | ReadError::Other(_) => Availability::Invalid,
        }
    }

    fn from_io(error: &std::io::Error) -> Self {
        match error.kind() {
            ErrorKind::NotFound => ReadError::Missing,
            ErrorKind::PermissionDenied => ReadError::PermissionDenied,
            // sysfs attributes of absent features may fail with ENODEV,
            // ENXIO or EIO; that means "no value", not "broken".
            _ if matches!(error.raw_os_error(), Some(19 | 6 | 5 | 61 | 95)) => ReadError::Missing,
            _ => ReadError::Other(error.kind().to_string()),
        }
    }
}

/// Whether a file can be opened for reading.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Access {
    Readable,
    PermissionDenied,
    Missing,
}

pub struct SysRoot {
    root: PathBuf,
    is_host: bool,
    recorder: Option<Mutex<Vec<String>>>,
}

impl SysRoot {
    /// The real machine.
    pub fn host() -> Self {
        Self {
            root: PathBuf::from("/"),
            is_host: true,
            recorder: None,
        }
    }

    /// A directory laid out like `/` (fixtures).
    pub fn at(root: impl Into<PathBuf>) -> Self {
        let root = root.into();
        let is_host = root == Path::new("/");
        Self {
            root,
            is_host,
            recorder: None,
        }
    }

    /// Record every path accessed, for tests that check what discovery
    /// touches.
    pub fn recording(mut self) -> Self {
        self.recorder = Some(Mutex::new(Vec::new()));
        self
    }

    pub fn accessed(&self) -> Vec<String> {
        self.recorder
            .as_ref()
            .map(|r| r.lock().unwrap_or_else(|p| p.into_inner()).clone())
            .unwrap_or_default()
    }

    pub fn is_host(&self) -> bool {
        self.is_host
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// The path as the user would know it on the machine (`/sys/...`).
    pub fn display(&self, rel: &str) -> String {
        format!("/{}", rel.trim_start_matches('/'))
    }

    /// Relative form of an absolute host path, for sources outside the
    /// fixed locations (for example the user's data directory).
    pub fn rel_of(abs: &Path) -> Option<String> {
        abs.strip_prefix("/")
            .ok()
            .and_then(Path::to_str)
            .map(str::to_string)
    }

    fn resolve(&self, rel: &str) -> Result<PathBuf, ReadError> {
        let rel = rel.trim_start_matches('/');
        let path = Path::new(rel);
        // No parent components, no device nodes, no sensitive attributes.
        if path
            .components()
            .any(|c| !matches!(c, Component::Normal(_)))
            || rel.starts_with("dev/")
            || rel == "dev"
        {
            return Err(ReadError::Refused);
        }
        if let Some(name) = path.file_name().and_then(|n| n.to_str())
            && SENSITIVE_NAMES.contains(&name)
        {
            return Err(ReadError::Refused);
        }
        if let Some(recorder) = &self.recorder {
            recorder
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .push(rel.to_string());
        }
        Ok(self.root.join(rel))
    }

    /// Read a whole (small) file as text.
    pub fn read(&self, rel: &str) -> Result<String, ReadError> {
        let path = self.resolve(rel)?;
        let file = File::open(&path).map_err(|e| ReadError::from_io(&e))?;
        let mut text = String::new();
        let read = file
            .take(MAX_READ_BYTES + 1)
            .read_to_string(&mut text)
            .map_err(|e| ReadError::from_io(&e))?;
        if read as u64 > MAX_READ_BYTES {
            return Err(ReadError::TooLarge);
        }
        Ok(text)
    }

    /// Read a file and trim it; an empty value is [`ReadError::Empty`].
    pub fn read_trimmed(&self, rel: &str) -> Result<String, ReadError> {
        let text = self.read(rel)?;
        let trimmed = text.trim_matches(|c: char| c.is_whitespace() || c == '\0');
        if trimmed.is_empty() {
            Err(ReadError::Empty)
        } else {
            Ok(trimmed.to_string())
        }
    }

    /// Read raw bytes (binary firmware tables).
    pub fn read_bytes(&self, rel: &str) -> Result<Vec<u8>, ReadError> {
        let path = self.resolve(rel)?;
        let file = File::open(&path).map_err(|e| ReadError::from_io(&e))?;
        let mut bytes = Vec::new();
        file.take(MAX_READ_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(|e| ReadError::from_io(&e))?;
        if bytes.len() as u64 > MAX_READ_BYTES {
            return Err(ReadError::TooLarge);
        }
        Ok(bytes)
    }

    /// Read at most `max` bytes from the start of a file. Used for process
    /// command lines, so that arguments beyond the program name are mostly
    /// never read at all.
    pub fn read_head(&self, rel: &str, max: usize) -> Result<Vec<u8>, ReadError> {
        let path = self.resolve(rel)?;
        let file = File::open(&path).map_err(|e| ReadError::from_io(&e))?;
        let mut bytes = Vec::with_capacity(max);
        file.take(max as u64)
            .read_to_end(&mut bytes)
            .map_err(|e| ReadError::from_io(&e))?;
        Ok(bytes)
    }

    /// Stream a large text file line by line, stopping when `visit` says so.
    /// Used for ID databases and package databases so they are never held
    /// in memory whole.
    pub fn for_each_line(
        &self,
        rel: &str,
        mut visit: impl FnMut(&str) -> ControlFlow<()>,
    ) -> Result<(), ReadError> {
        let path = self.resolve(rel)?;
        let file = File::open(&path).map_err(|e| ReadError::from_io(&e))?;
        let mut reader = BufReader::new(file);
        let mut line = Vec::new();
        loop {
            line.clear();
            let read = reader
                .read_until(b'\n', &mut line)
                .map_err(|e| ReadError::from_io(&e))?;
            if read == 0 {
                return Ok(());
            }
            // Package databases can contain the odd non-UTF-8 byte.
            let text = String::from_utf8_lossy(&line);
            if visit(text.trim_end_matches(['\n', '\r'])).is_break() {
                return Ok(());
            }
        }
    }

    /// Names in a directory, sorted. Hidden entries are included.
    pub fn list(&self, rel: &str) -> Result<Vec<String>, ReadError> {
        let path = self.resolve(rel)?;
        let entries = fs::read_dir(&path).map_err(|e| ReadError::from_io(&e))?;
        let mut names: Vec<String> = entries
            .flatten()
            .filter_map(|entry| entry.file_name().into_string().ok())
            .collect();
        names.sort_by(|a, b| natural_cmp(a, b));
        Ok(names)
    }

    pub fn exists(&self, rel: &str) -> bool {
        self.resolve(rel).map(|p| p.exists()).unwrap_or(false)
    }

    pub fn is_dir(&self, rel: &str) -> bool {
        self.resolve(rel).map(|p| p.is_dir()).unwrap_or(false)
    }

    pub fn is_file(&self, rel: &str) -> bool {
        self.resolve(rel).map(|p| p.is_file()).unwrap_or(false)
    }

    /// Whether a regular file is executable by someone (mode bits only; the
    /// file is not run).
    pub fn is_executable_file(&self, rel: &str) -> bool {
        use std::os::unix::fs::PermissionsExt;
        self.resolve(rel)
            .and_then(|p| fs::metadata(p).map_err(|e| ReadError::from_io(&e)))
            .map(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
            .unwrap_or(false)
    }

    /// Whether the current user may open a file for reading. The file is
    /// opened and closed again; nothing is read from it.
    pub fn access(&self, rel: &str) -> Access {
        match self.resolve(rel).map(File::open) {
            Ok(Ok(_)) => Access::Readable,
            Ok(Err(e)) if e.kind() == ErrorKind::PermissionDenied => Access::PermissionDenied,
            _ => Access::Missing,
        }
    }

    /// Final component of a symlink's target, e.g. the driver name behind
    /// `.../driver`.
    pub fn link_name(&self, rel: &str) -> Option<String> {
        let path = self.resolve(rel).ok()?;
        let target = fs::read_link(path).ok()?;
        target.file_name()?.to_str().map(str::to_string)
    }

    /// Fully resolved location of `rel`, relative to the root.
    pub fn canonical(&self, rel: &str) -> Option<String> {
        let path = self.resolve(rel).ok()?;
        let resolved = fs::canonicalize(path).ok()?;
        let root = fs::canonicalize(&self.root).ok()?;
        resolved
            .strip_prefix(&root)
            .ok()
            .and_then(Path::to_str)
            .map(str::to_string)
    }
}

/// Order `cpu2` before `cpu10` and `hwmon9` before `hwmon10`.
pub fn natural_cmp(a: &str, b: &str) -> std::cmp::Ordering {
    let split = |s: &str| {
        let digits = s.len() - s.trim_end_matches(|c: char| c.is_ascii_digit()).len();
        let (head, tail) = s.split_at(s.len() - digits);
        (head.to_string(), tail.parse::<u64>().ok())
    };
    let (ha, na) = split(a);
    let (hb, nb) = split(b);
    ha.cmp(&hb).then(na.cmp(&nb)).then(a.cmp(b))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tree() -> (tempfile::TempDir, SysRoot) {
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path().join("sys/class/dmi/id");
        fs::create_dir_all(&base).unwrap();
        fs::write(base.join("sys_vendor"), "Example Corp\n").unwrap();
        fs::write(base.join("product_serial"), "SECRET123\n").unwrap();
        fs::write(base.join("empty"), "  \n").unwrap();
        let root = SysRoot::at(dir.path()).recording();
        (dir, root)
    }

    #[test]
    fn reads_and_trims() {
        let (_dir, root) = tree();
        assert_eq!(
            root.read_trimmed("sys/class/dmi/id/sys_vendor").unwrap(),
            "Example Corp"
        );
        assert_eq!(
            root.read_trimmed("sys/class/dmi/id/empty"),
            Err(ReadError::Empty)
        );
        assert_eq!(
            root.read_trimmed("sys/class/dmi/id/nope"),
            Err(ReadError::Missing)
        );
        assert_eq!(root.display("sys/class/dmi/id/x"), "/sys/class/dmi/id/x");
    }

    #[test]
    fn sensitive_attributes_are_never_opened() {
        let (_dir, root) = tree();
        assert_eq!(
            root.read("sys/class/dmi/id/product_serial"),
            Err(ReadError::Refused)
        );
        assert_eq!(
            root.access("sys/class/dmi/id/product_serial"),
            Access::Missing
        );
        assert!(
            !root
                .accessed()
                .iter()
                .any(|p| p.ends_with("product_serial")),
            "a refused path is not even recorded as accessed"
        );
    }

    #[test]
    fn escaping_the_root_and_device_nodes_are_refused() {
        let (_dir, root) = tree();
        assert_eq!(root.read("sys/../../etc/passwd"), Err(ReadError::Refused));
        assert_eq!(root.read("dev/nvme0n1"), Err(ReadError::Refused));
        assert_eq!(root.read("proc/1/environ"), Err(ReadError::Refused));
        assert_eq!(
            root.read_head("proc/1/environ", 10),
            Err(ReadError::Refused)
        );
        assert_eq!(root.list("dev"), Err(ReadError::Refused));
    }

    #[test]
    fn listing_is_naturally_sorted() {
        let dir = tempfile::tempdir().unwrap();
        for name in ["cpu10", "cpu2", "cpu1", "cpufreq"] {
            fs::create_dir_all(dir.path().join("cpus").join(name)).unwrap();
        }
        let root = SysRoot::at(dir.path());
        assert_eq!(
            root.list("cpus").unwrap(),
            ["cpu1", "cpu2", "cpu10", "cpufreq"]
        );
    }

    #[test]
    fn streaming_stops_early() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("big"), "a\nb\nc\nd\n").unwrap();
        let root = SysRoot::at(dir.path());
        let mut seen = Vec::new();
        root.for_each_line("big", |line| {
            seen.push(line.to_string());
            if line == "b" {
                ControlFlow::Break(())
            } else {
                ControlFlow::Continue(())
            }
        })
        .unwrap();
        assert_eq!(seen, ["a", "b"]);
    }

    #[test]
    fn links_resolve_inside_the_root() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join("sys/bus/pci/drivers/amdgpu")).unwrap();
        fs::create_dir_all(dir.path().join("sys/devices/pci0000:00/0000:03:00.0")).unwrap();
        std::os::unix::fs::symlink(
            "../../../bus/pci/drivers/amdgpu",
            dir.path()
                .join("sys/devices/pci0000:00/0000:03:00.0/driver"),
        )
        .unwrap();
        let root = SysRoot::at(dir.path());
        assert_eq!(
            root.link_name("sys/devices/pci0000:00/0000:03:00.0/driver"),
            Some("amdgpu".into())
        );
        assert_eq!(
            root.canonical("sys/devices/pci0000:00/0000:03:00.0/driver"),
            Some("sys/bus/pci/drivers/amdgpu".into())
        );
    }

    #[test]
    fn permission_errors_map_to_permission_denied() {
        assert_eq!(
            ReadError::PermissionDenied.availability(),
            Availability::PermissionDenied
        );
        assert_eq!(ReadError::Missing.availability(), Availability::Unsupported);
    }
}
