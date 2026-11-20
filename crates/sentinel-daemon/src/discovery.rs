//! Coordinates hardware and utility scans.
//!
//! Scans run on one worker thread, one at a time, in request order. The
//! thread exists only while there is work and exits when the queue is
//! empty, so an idle daemon stays single-threaded and never wakes up.
//!
//! Rules:
//! - Nothing is scanned automatically; every scan is requested by a client.
//! - A job already queued or running is never queued twice.
//! - A cancelled or timed-out scan keeps the previous result and is not
//!   cached; only complete scans replace the inventory and the cache.
//! - Results loaded from the cache are labelled as cached until a scan in
//!   this session replaces them.

use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use sentinel_hardware::{CancelToken, DiscoveryContext, Event, Stopped, SysRoot};
use sentinel_inventory::cache::{self, Cacheable};
use sentinel_inventory::{
    DiscoveryJob, DiscoveryStatus, InventoryOrigin, JobProgress, JobState, ModuleOutcome,
    SystemInventory, UtilityCatalog,
};
use sentinel_storage::Storage;
use sentinel_utilities::UtilityEnv;

use crate::context::unix_now;

/// Longest a single scan may take before it is stopped.
pub const SCAN_TIME_LIMIT: Duration = Duration::from_secs(60);

pub struct Slot<T> {
    pub current: Option<Arc<T>>,
    pub origin: Option<InventoryOrigin>,
    pub progress: JobProgress,
}

impl<T> Slot<T> {
    fn new(job: DiscoveryJob) -> Self {
        Self {
            current: None,
            origin: None,
            progress: JobProgress::idle(job),
        }
    }
}

struct Shared {
    hardware: Slot<SystemInventory>,
    utilities: Slot<UtilityCatalog>,
    queue: VecDeque<DiscoveryJob>,
    running: Option<(DiscoveryJob, CancelToken)>,
    worker_active: bool,
    shutting_down: bool,
}

impl Shared {
    fn progress_mut(&mut self, job: DiscoveryJob) -> &mut JobProgress {
        match job {
            DiscoveryJob::Hardware => &mut self.hardware.progress,
            DiscoveryJob::Utilities => &mut self.utilities.progress,
        }
    }
}

/// Where scans read from and write to.
#[derive(Clone)]
pub struct DiscoveryConfig {
    /// `/` normally; a fixture directory in tests.
    pub root: PathBuf,
    pub utility_env: UtilityEnv,
    /// Database for the cache; `None` when storage is unavailable.
    pub database: Option<PathBuf>,
}

pub struct Coordinator {
    shared: Arc<Mutex<Shared>>,
    worker: Mutex<Option<JoinHandle<()>>>,
    config: DiscoveryConfig,
}

fn lock(shared: &Mutex<Shared>) -> MutexGuard<'_, Shared> {
    shared.lock().unwrap_or_else(|p| p.into_inner())
}

impl Coordinator {
    pub fn new(config: DiscoveryConfig) -> Self {
        Self {
            shared: Arc::new(Mutex::new(Shared {
                hardware: Slot::new(DiscoveryJob::Hardware),
                utilities: Slot::new(DiscoveryJob::Utilities),
                queue: VecDeque::new(),
                running: None,
                worker_active: false,
                shutting_down: false,
            })),
            worker: Mutex::new(None),
            config,
        }
    }

    /// Install cached results found at start-up.
    pub fn load_cache(&self, storage: &Storage) {
        let mut shared = lock(&self.shared);
        if let Some(inventory) = load::<SystemInventory>(storage) {
            shared.hardware.progress.last_successful_scan = Some(inventory.discovered_at);
            shared.hardware.current = Some(Arc::new(inventory));
            shared.hardware.origin = Some(InventoryOrigin::Cache);
        }
        if let Some(catalog) = load::<UtilityCatalog>(storage) {
            shared.utilities.progress.last_successful_scan = Some(catalog.discovered_at);
            shared.utilities.current = Some(Arc::new(catalog));
            shared.utilities.origin = Some(InventoryOrigin::Cache);
        }
    }

    pub fn status(&self) -> DiscoveryStatus {
        let shared = lock(&self.shared);
        DiscoveryStatus {
            hardware: shared.hardware.progress.clone(),
            utilities: shared.utilities.progress.clone(),
        }
    }

    pub fn hardware(
        &self,
    ) -> (
        Option<Arc<SystemInventory>>,
        Option<InventoryOrigin>,
        JobProgress,
    ) {
        let shared = lock(&self.shared);
        let slot = &shared.hardware;
        (slot.current.clone(), slot.origin, slot.progress.clone())
    }

    pub fn utilities(
        &self,
    ) -> (
        Option<Arc<UtilityCatalog>>,
        Option<InventoryOrigin>,
        JobProgress,
    ) {
        let shared = lock(&self.shared);
        let slot = &shared.utilities;
        (slot.current.clone(), slot.origin, slot.progress.clone())
    }

    pub fn root(&self) -> SysRoot {
        SysRoot::at(&self.config.root)
    }

    /// Queue a scan. Without `force` it is skipped when a result exists.
    pub fn request(&self, job: DiscoveryJob, force: bool) -> DiscoveryStatus {
        {
            let mut shared = lock(&self.shared);
            let has_result = match job {
                DiscoveryJob::Hardware => shared.hardware.current.is_some(),
                DiscoveryJob::Utilities => shared.utilities.current.is_some(),
            };
            let active = shared.progress_mut(job).state.is_active();
            if !shared.shutting_down && !active && (force || !has_result) {
                shared.queue.push_back(job);
                let progress = shared.progress_mut(job);
                progress.state = JobState::Queued;
                progress.error = None;
                tracing::info!(job = job.as_str(), force, "discovery queued");
                if !shared.worker_active {
                    shared.worker_active = true;
                    drop(shared);
                    self.start_worker();
                }
            }
        }
        self.status()
    }

    pub fn cancel(&self, job: DiscoveryJob) -> DiscoveryStatus {
        {
            let mut shared = lock(&self.shared);
            if let Some(index) = shared.queue.iter().position(|j| *j == job) {
                shared.queue.remove(index);
                let progress = shared.progress_mut(job);
                progress.state = JobState::Cancelled;
                progress.error =
                    Some("cancelled before it started; the previous result is kept".into());
                tracing::info!(job = job.as_str(), "queued discovery cancelled");
            }
            if let Some((running, token)) = &shared.running
                && *running == job
            {
                token.cancel();
                tracing::info!(job = job.as_str(), "running discovery asked to stop");
            }
        }
        self.status()
    }

    /// Stop everything and wait (bounded) for the worker to finish.
    pub fn shutdown(&self) {
        {
            let mut shared = lock(&self.shared);
            shared.shutting_down = true;
            let queued: Vec<DiscoveryJob> = shared.queue.drain(..).collect();
            for job in queued {
                shared.progress_mut(job).state = JobState::Cancelled;
            }
            if let Some((_, token)) = &shared.running {
                token.cancel();
            }
        }
        let handle = self.worker.lock().unwrap_or_else(|p| p.into_inner()).take();
        if let Some(handle) = handle {
            let deadline = Instant::now() + Duration::from_secs(5);
            while !handle.is_finished() && Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(10));
            }
            if handle.is_finished() {
                let _ = handle.join();
            } else {
                tracing::warn!("a discovery module did not stop in time; exiting without it");
            }
        }
    }

    fn start_worker(&self) {
        let mut worker = self.worker.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(old) = worker.take() {
            // The previous worker has finished (it cleared worker_active).
            let _ = old.join();
        }
        let shared = self.shared.clone();
        let config = self.config.clone();
        let spawned = std::thread::Builder::new()
            .name("sentinel-discovery".into())
            .spawn(move || worker_loop(&shared, &config));
        match spawned {
            Ok(handle) => *worker = Some(handle),
            Err(error) => {
                tracing::error!(%error, "could not start the discovery thread");
                let mut shared = lock(&self.shared);
                shared.worker_active = false;
                let queued: Vec<DiscoveryJob> = shared.queue.drain(..).collect();
                for job in queued {
                    let progress = shared.progress_mut(job);
                    progress.state = JobState::Failed;
                    progress.error = Some("the discovery thread could not be started".into());
                }
            }
        }
    }
}

fn load<T: Cacheable>(storage: &Storage) -> Option<T> {
    match storage.load_cache(T::KEY) {
        Ok(Some(row)) => match cache::decode::<T>(row.schema_version, &row.payload) {
            Ok(value) => {
                tracing::info!(key = T::KEY, "cached scan loaded");
                Some(value)
            }
            Err(problem) => {
                tracing::info!(key = T::KEY, %problem, "cached scan ignored; a new scan is needed");
                None
            }
        },
        Ok(None) => None,
        Err(error) => {
            tracing::warn!(%error, "reading the scan cache failed");
            None
        }
    }
}

fn worker_loop(shared: &Arc<Mutex<Shared>>, config: &DiscoveryConfig) {
    loop {
        let (job, token) = {
            let mut guard = lock(shared);
            let Some(job) = guard.queue.pop_front().filter(|_| !guard.shutting_down) else {
                guard.worker_active = false;
                guard.running = None;
                return;
            };
            let token = CancelToken::new().with_deadline(Instant::now() + SCAN_TIME_LIMIT);
            guard.running = Some((job, token.clone()));
            let progress = guard.progress_mut(job);
            progress.state = JobState::Running;
            progress.started_at = Some(unix_now());
            progress.finished_at = None;
            progress.current_module = None;
            progress.modules.clear();
            progress.error = None;
            progress.cache_saved = None;
            (job, token)
        };
        tracing::info!(job = job.as_str(), "discovery started");
        let started = Instant::now();

        let mut on_event = |event: Event| {
            let mut guard = lock(shared);
            let progress = guard.progress_mut(job);
            match event {
                Event::ModuleStarted(module) => progress.current_module = Some(module),
                Event::ModuleFinished(report) => {
                    progress.current_module = None;
                    progress.modules.push(report);
                }
            }
        };
        let root = SysRoot::at(&config.root);
        let now = unix_now();
        let stopped = match job {
            DiscoveryJob::Hardware => {
                let ctx = DiscoveryContext::new(root, now, token);
                let outcome = sentinel_hardware::discover(&ctx, &mut on_event);
                finish(shared, config, job, outcome.stopped, outcome.inventory)
            }
            DiscoveryJob::Utilities => {
                let scan = sentinel_utilities::discover(
                    &root,
                    &config.utility_env,
                    now,
                    &token,
                    &mut on_event,
                );
                finish(shared, config, job, scan.stopped, scan.catalog)
            }
        };
        let modules = lock(shared).progress_mut(job).modules.clone();
        let count = |o: ModuleOutcome| modules.iter().filter(|m| m.outcome == o).count();
        tracing::info!(
            job = job.as_str(),
            duration_ms = started.elapsed().as_millis() as u64,
            completed = count(ModuleOutcome::Completed),
            partial = count(ModuleOutcome::Partial),
            unavailable = count(ModuleOutcome::Unavailable),
            failed = count(ModuleOutcome::Failed),
            stopped = ?stopped,
            "discovery finished"
        );
    }
}

trait Stored: Cacheable + Send + Sync + 'static {
    fn slot(shared: &mut Shared) -> &mut Slot<Self>
    where
        Self: Sized;
}

impl Stored for SystemInventory {
    fn slot(shared: &mut Shared) -> &mut Slot<Self> {
        &mut shared.hardware
    }
}

impl Stored for UtilityCatalog {
    fn slot(shared: &mut Shared) -> &mut Slot<Self> {
        &mut shared.utilities
    }
}

fn finish<T: Stored>(
    shared: &Arc<Mutex<Shared>>,
    config: &DiscoveryConfig,
    job: DiscoveryJob,
    stopped: Option<Stopped>,
    result: T,
) -> Option<Stopped> {
    let now = unix_now();
    if let Some(stop) = stopped {
        let mut guard = lock(shared);
        guard.running = None;
        let progress = guard.progress_mut(job);
        progress.finished_at = Some(now);
        progress.current_module = None;
        let (state, message) = match stop {
            Stopped::Cancelled => (
                JobState::Cancelled,
                "the scan was cancelled; the previous result is kept",
            ),
            Stopped::TimedOut => (
                JobState::TimedOut,
                "the scan took too long and was stopped; the previous result is kept",
            ),
        };
        progress.state = state;
        progress.error = Some(message.into());
        return Some(stop);
    }

    // Write the cache before publishing, outside the lock.
    let saved = config.database.as_ref().map(|path| {
        Storage::open(path)
            .and_then(|mut storage| {
                storage.save_cache(
                    T::KEY,
                    T::SCHEMA_VERSION,
                    result.discovered_at(),
                    now,
                    &cache::encode(&result),
                )
            })
            .map_err(|error| tracing::warn!(%error, key = T::KEY, "could not cache the scan"))
            .is_ok()
    });

    let mut guard = lock(shared);
    guard.running = None;
    let slot = T::slot(&mut guard);
    slot.progress.last_successful_scan = Some(result.discovered_at());
    slot.current = Some(Arc::new(result));
    slot.origin = Some(InventoryOrigin::ThisSession);
    slot.progress.state = JobState::Completed;
    slot.progress.finished_at = Some(now);
    slot.progress.current_module = None;
    slot.progress.cache_saved = Some(saved.unwrap_or(false));
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wait_idle(coordinator: &Coordinator) {
        let deadline = Instant::now() + Duration::from_secs(30);
        while DiscoveryJob::ALL
            .iter()
            .any(|j| coordinator.status().get(*j).state.is_active())
        {
            assert!(Instant::now() < deadline, "discovery did not finish");
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    fn fixture() -> (tempfile::TempDir, Coordinator) {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("root");
        let dmi = root.join("sys/class/dmi/id");
        std::fs::create_dir_all(&dmi).unwrap();
        std::fs::write(dmi.join("sys_vendor"), "Example Corp\n").unwrap();
        std::fs::write(dmi.join("product_name"), "Fixture 14\n").unwrap();
        let coordinator = Coordinator::new(DiscoveryConfig {
            root,
            utility_env: UtilityEnv::default(),
            database: Some(dir.path().join("sentinel.db")),
        });
        (dir, coordinator)
    }

    #[test]
    fn nothing_runs_until_asked() {
        let (_dir, coordinator) = fixture();
        assert_eq!(coordinator.status().hardware.state, JobState::Idle);
        assert!(coordinator.hardware().0.is_none());
    }

    #[test]
    fn scan_completes_caches_and_reloads_as_cached() {
        let (dir, coordinator) = fixture();
        coordinator.request(DiscoveryJob::Hardware, false);
        wait_idle(&coordinator);
        let (inventory, origin, progress) = coordinator.hardware();
        assert_eq!(progress.state, JobState::Completed);
        assert_eq!(origin, Some(InventoryOrigin::ThisSession));
        assert_eq!(progress.cache_saved, Some(true));
        assert_eq!(progress.modules.len(), 12);
        let inventory = inventory.unwrap();
        assert_eq!(
            inventory
                .system
                .as_ref()
                .unwrap()
                .product_name
                .value()
                .map(String::as_str),
            Some("Fixture 14")
        );

        // A non-forced request with a result present does nothing.
        coordinator.request(DiscoveryJob::Hardware, false);
        assert_eq!(coordinator.status().hardware.state, JobState::Completed);

        // A new daemon sees it as cached, not live.
        let again = Coordinator::new(DiscoveryConfig {
            root: dir.path().join("root"),
            utility_env: UtilityEnv::default(),
            database: Some(dir.path().join("sentinel.db")),
        });
        again.load_cache(&Storage::open(&dir.path().join("sentinel.db")).unwrap());
        let (cached, origin, progress) = again.hardware();
        assert_eq!(origin, Some(InventoryOrigin::Cache));
        assert_eq!(cached.unwrap().discovered_at, inventory.discovered_at);
        assert_eq!(progress.state, JobState::Idle);
        assert_eq!(progress.last_successful_scan, Some(inventory.discovered_at));
    }

    #[test]
    fn jobs_run_one_at_a_time_and_are_not_duplicated() {
        let (_dir, coordinator) = fixture();
        coordinator.request(DiscoveryJob::Utilities, true);
        coordinator.request(DiscoveryJob::Utilities, true);
        coordinator.request(DiscoveryJob::Hardware, true);
        wait_idle(&coordinator);
        let status = coordinator.status();
        assert_eq!(status.utilities.state, JobState::Completed);
        assert_eq!(status.hardware.state, JobState::Completed);
        assert_eq!(status.utilities.modules.len(), 4);
    }

    #[test]
    fn cancelling_a_queued_job_keeps_the_previous_result() {
        let (_dir, coordinator) = fixture();
        coordinator.request(DiscoveryJob::Hardware, true);
        wait_idle(&coordinator);
        let first = coordinator.hardware().0.unwrap();
        // Queue utilities then hardware; cancel hardware while it waits.
        {
            let mut shared = lock(&coordinator.shared);
            shared.queue.push_back(DiscoveryJob::Hardware);
            shared.hardware.progress.state = JobState::Queued;
        }
        coordinator.cancel(DiscoveryJob::Hardware);
        let (current, origin, progress) = coordinator.hardware();
        assert_eq!(progress.state, JobState::Cancelled);
        assert_eq!(origin, Some(InventoryOrigin::ThisSession));
        assert!(Arc::ptr_eq(&current.unwrap(), &first));
    }

    #[test]
    fn shutdown_refuses_new_work() {
        let (_dir, coordinator) = fixture();
        coordinator.shutdown();
        coordinator.request(DiscoveryJob::Hardware, true);
        assert_eq!(coordinator.status().hardware.state, JobState::Idle);
    }

    #[test]
    fn without_storage_results_are_kept_in_memory_only() {
        let dir = tempfile::tempdir().unwrap();
        let coordinator = Coordinator::new(DiscoveryConfig {
            root: dir.path().to_path_buf(),
            utility_env: UtilityEnv::default(),
            database: None,
        });
        coordinator.request(DiscoveryJob::Hardware, true);
        wait_idle(&coordinator);
        let (inventory, _, progress) = coordinator.hardware();
        assert!(inventory.is_some());
        assert_eq!(progress.cache_saved, Some(false));
    }
}
