//! Shared harness for the end-to-end suites.
//!
//! Every test gets a [`Sandbox`]: a temporary directory acting as `HOME` and
//! `XDG_RUNTIME_DIR`. The real binaries run with a cleared environment
//! pointing only at the sandbox, so no test can read or write the
//! developer's actual Sentinel data, and nothing outside the sandbox is
//! touched.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::sync::Once;
use std::time::{Duration, Instant};

use sentinel_client::Sentinel;
use sentinel_core::AppPaths;

pub mod hwfixture;

pub const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/fixtures");

/// Root of the repository.
pub fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("tests/ has a parent")
        .to_path_buf()
}

/// Directory holding the compiled binaries for the profile under test.
fn target_profile_dir() -> PathBuf {
    // <target>/<profile>/deps/<test-binary>
    let exe = std::env::current_exe().expect("test executable path");
    exe.parent()
        .and_then(Path::parent)
        .expect("test binary lives in <target>/<profile>/deps")
        .to_path_buf()
}

/// Path to a Sentinel binary, building the binaries first so the suites
/// always exercise the current sources.
pub fn bin(name: &str) -> PathBuf {
    static BUILD: Once = Once::new();
    let dir = target_profile_dir();
    BUILD.call_once(|| {
        let mut build = Command::new(env!("CARGO"));
        build.current_dir(workspace_root()).args([
            "build",
            "--quiet",
            "-p",
            "sentinel-daemon",
            "-p",
            "sentinel-cli",
        ]);
        if dir.file_name().is_some_and(|n| n == "release") {
            build.arg("--release");
        }
        let status = build.status().expect("cargo is runnable");
        assert!(status.success(), "building the Sentinel binaries failed");
    });
    let path = dir.join(name);
    assert!(path.is_file(), "{} was not built", path.display());
    path
}

pub struct Sandbox {
    dir: tempfile::TempDir,
    children: Vec<Child>,
    extra_env: Vec<(String, OsString)>,
}

impl Default for Sandbox {
    fn default() -> Self {
        Self::new()
    }
}

impl Sandbox {
    pub fn new() -> Self {
        // A short prefix keeps the socket path well under the 107-byte limit.
        let dir = tempfile::Builder::new()
            .prefix("sntl")
            .tempdir()
            .expect("temporary directory");
        std::fs::create_dir(dir.path().join("home")).unwrap();
        std::fs::create_dir(dir.path().join("run")).unwrap();
        Self {
            dir,
            children: Vec::new(),
            extra_env: Vec::new(),
        }
    }

    /// Add (or override) an environment variable for every binary the
    /// sandbox runs, and for the daemons they start.
    pub fn with_env(mut self, key: &str, value: impl Into<OsString>) -> Self {
        self.extra_env.retain(|(k, _)| k != key);
        self.extra_env.push((key.to_string(), value.into()));
        self
    }

    pub fn root(&self) -> &Path {
        self.dir.path()
    }

    pub fn home(&self) -> PathBuf {
        self.dir.path().join("home")
    }

    fn env(&self) -> Vec<(String, OsString)> {
        let mut env: Vec<(String, OsString)> = vec![
            ("HOME".into(), self.home().into_os_string()),
            (
                "XDG_RUNTIME_DIR".into(),
                self.dir.path().join("run").into_os_string(),
            ),
            ("PATH".into(), std::env::var_os("PATH").unwrap_or_default()),
            // Make sure `sentinel ui` can never open a real window in tests.
            (
                "SENTINEL_DESKTOP_BIN".into(),
                "/nonexistent/sentinel-desktop".into(),
            ),
        ];
        for (key, value) in &self.extra_env {
            env.retain(|(k, _)| k != key);
            env.push((key.clone(), value.clone()));
        }
        env
    }

    /// The same paths the sandboxed binaries resolve.
    pub fn paths(&self) -> AppPaths {
        let env = self.env();
        AppPaths::resolve(|key| {
            env.iter()
                .find(|(name, _)| name == key && key != "PATH")
                .map(|(_, value)| value.clone())
        })
        .expect("sandbox paths resolve")
    }

    /// The backend interface the desktop app uses, pointed at the sandbox.
    pub fn client(&self) -> Sentinel {
        Sentinel::new(self.paths())
    }

    fn command(&self, binary: &str) -> Command {
        let mut command = Command::new(bin(binary));
        command.env_clear().envs(self.env()).stdin(Stdio::null());
        command
    }

    /// Run `sentinel <args>` to completion.
    pub fn cli(&self, args: &[&str]) -> CliResult {
        let output = self
            .command("sentinel")
            .args(args)
            .output()
            .expect("sentinel runs");
        CliResult::from(output)
    }

    /// Run `sentineld` directly as a child of the test, so the test owns the
    /// process and may terminate it to simulate a crash.
    pub fn spawn_daemon(&mut self) -> u32 {
        let child = self
            .command("sentineld")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("sentineld runs");
        let pid = child.id();
        self.children.push(child);
        self.wait_until("daemon ready", || {
            self.client().probe().ok().flatten().is_some()
        });
        pid
    }

    /// Run `sentineld` to completion and return its exit code and stderr.
    pub fn run_daemon_to_exit(&self) -> CliResult {
        let output = self.command("sentineld").output().expect("sentineld runs");
        CliResult::from(output)
    }

    /// Hand back a daemon started with [`Sandbox::spawn_daemon`].
    pub fn take_daemon(&mut self, pid: u32) -> Child {
        let index = self
            .children
            .iter()
            .position(|child| child.id() == pid)
            .expect("daemon was spawned by this sandbox");
        self.children.remove(index)
    }

    pub fn wait_until(&self, what: &str, condition: impl Fn() -> bool) {
        let deadline = Instant::now() + Duration::from_secs(10);
        while !condition() {
            assert!(Instant::now() < deadline, "timed out waiting for: {what}");
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    /// Every file, socket and symlink under the sandbox, relative to it.
    pub fn files(&self) -> Vec<String> {
        fn walk(dir: &Path, root: &Path, out: &mut Vec<String>) {
            for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
                let path = entry.path();
                let is_dir = entry.file_type().map(|t| t.is_dir()).unwrap_or(false);
                if is_dir {
                    walk(&path, root, out);
                } else {
                    out.push(path.strip_prefix(root).unwrap().display().to_string());
                }
            }
        }
        let mut out = Vec::new();
        walk(self.root(), self.root(), &mut out);
        out.sort();
        out
    }
}

impl Drop for Sandbox {
    fn drop(&mut self) {
        // Stop anything the test left running: politely over IPC first, and
        // forcibly only for processes this sandbox itself spawned.
        let _ = self.client().stop();
        for child in &mut self.children {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

#[derive(Debug)]
pub struct CliResult {
    pub code: i32,
    pub stdout: String,
    pub stderr: String,
}

impl From<Output> for CliResult {
    fn from(output: Output) -> Self {
        Self {
            code: output.status.code().unwrap_or(-1),
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        }
    }
}

impl CliResult {
    #[track_caller]
    pub fn expect_code(self, code: i32) -> Self {
        assert_eq!(
            self.code, code,
            "unexpected exit code\nstdout: {}\nstderr: {}",
            self.stdout, self.stderr
        );
        self
    }

    #[track_caller]
    pub fn expect_stdout(self, needle: &str) -> Self {
        assert!(
            self.stdout.contains(needle),
            "stdout lacks {needle:?}:\n{}",
            self.stdout
        );
        self
    }

    #[track_caller]
    pub fn expect_stderr(self, needle: &str) -> Self {
        assert!(
            self.stderr.contains(needle),
            "stderr lacks {needle:?}:\n{}",
            self.stderr
        );
        self
    }

    pub fn json(&self) -> serde_json::Value {
        serde_json::from_str(&self.stdout)
            .unwrap_or_else(|e| panic!("stdout is not JSON ({e}):\n{}", self.stdout))
    }
}

/// Fields of `/proc/<pid>/status` as `(key, value)` pairs.
pub fn proc_status(pid: u32) -> Vec<(String, String)> {
    std::fs::read_to_string(format!("/proc/{pid}/status"))
        .unwrap_or_else(|e| panic!("reading /proc/{pid}/status: {e}"))
        .lines()
        .filter_map(|line| line.split_once(':'))
        .map(|(key, value)| (key.to_string(), value.trim().to_string()))
        .collect()
}

pub fn proc_field(pid: u32, key: &str) -> String {
    proc_status(pid)
        .into_iter()
        .find(|(name, _)| name == key)
        .map(|(_, value)| value)
        .unwrap_or_else(|| panic!("/proc/{pid}/status has no {key}"))
}
