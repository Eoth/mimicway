// The configuration (services and groups), kept in memory and in one YAML file.
//
// Reads take a snapshot: an Arc of the current configuration, so a reader never copies it nor waits for a writer.
// A change copies the configuration, modifies the copy and swaps it in at once; writing it to disk (temporary file,
// then rename) is left to a background task (write-behind, see WriterHandle), so a request never waits on disk I/O.
//
// Backups: before the file is overwritten, the previous content is copied to {data_dir}/backups/ and only the
// newest BACKUP_MAX_COUNT copies are kept (5 by default; the reference Kubernetes volume is small). It happens as
// part of the change itself, with no timer. The copy is made synchronously, under the write lock and BEFORE the new
// content is queued for writing (prepare_and_backup): a change is never accepted without its backup, which is the
// whole point of the rollback.
//
// A full reset first copies the configuration to backups/protected/ (backup_before_reset). Rotation only looks at
// files directly in backups/, so later changes can never push that copy out. It expires after 30 days
// (PROTECTED_BACKUP_MAX_AGE_MS), checked at each ordinary write: a write 30 days after the reset is taken as the
// sign that nobody depends on the old state any more.
use crate::models::MockConfig;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use tokio::sync::{RwLock, mpsc, oneshot};

static BACKUP_SEQ: AtomicU64 = AtomicU64::new(0);

const PROTECTED_BACKUP_MAX_AGE_MS: u128 = 30 * 24 * 60 * 60 * 1000;

// The write queue is bounded on purpose: write-behind exists so that requests stop waiting on disk I/O, not to let
// a queue grow without limit during a burst. Each change is one job (no batching), and 64 jobs absorb realistic
// bursts; beyond that, the change waits for a free slot (backpressure) rather than losing writes or growing memory.
// A fixed safety valve, not a setting.
const WRITE_QUEUE_CAPACITY: usize = 64;

/// A job for the background writer.
/// - `Write`: serialized YAML to write to `path`.
/// - `Barrier`: writes nothing; answers through its oneshot once every job queued before it is done. Used by
///   `flush()` (deterministic tests, draining on shutdown), never by the normal change path.
enum WriteJob {
    Write { path: PathBuf, yaml: String },
    Barrier(oneshot::Sender<()>),
}

#[derive(Default)]
struct WriterStats {
    last_write_ok_ms: AtomicU64,
    last_error: std::sync::RwLock<Option<WriterError>>,
}

/// The last write error of the background task, reported by GET /api/health so that a disk problem shows up in
/// monitoring.
#[derive(Debug, Clone, serde::Serialize)]
pub struct WriterError {
    pub message: String,
    pub at_ms: u64,
}

/// State of the background writer, reported by GET /api/health.
#[derive(Debug, Clone, serde::Serialize)]
pub struct WriteQueueStatus {
    pub pending: usize,
    pub capacity: usize,
    pub last_write_ok_ms: Option<u64>,
    pub last_error: Option<WriterError>,
}

/// The sending side of the background writer: each `MockStore` holds a clone (sender and shared stats), the task
/// holds the receiver (see `WriterHandle::spawn`).
#[derive(Clone)]
struct WriterHandle {
    tx: mpsc::Sender<WriteJob>,
    stats: Arc<WriterStats>,
}

impl WriterHandle {
    fn spawn() -> Self {
        let (tx, rx) = mpsc::channel(WRITE_QUEUE_CAPACITY);
        let stats = Arc::new(WriterStats::default());
        tokio::spawn(Self::run(rx, stats.clone()));
        Self { tx, stats }
    }

    /// The background task: takes jobs until the channel closes, waiting on `recv()` with no polling. One write per
    /// change rather than batching: batching would write less often but leave more changes only in memory in case of a
    /// crash, and the goal is to keep requests off the disk, not to save writes.
    ///
    /// A failed write is logged and kept in `stats.last_error`, and the task goes on: the request already succeeded
    /// (the change is in memory), and one transient error must not stop every later write.
    async fn run(mut rx: mpsc::Receiver<WriteJob>, stats: Arc<WriterStats>) {
        while let Some(job) = rx.recv().await {
            match job {
                WriteJob::Write { path, yaml } => match MockStore::write_to_disk(&path, &yaml) {
                    Ok(()) => {
                        stats
                            .last_write_ok_ms
                            .store(MockStore::now_ms() as u64, Ordering::Relaxed);
                    }
                    Err(e) => {
                        tracing::error!(
                            path = %path.display(),
                            error = %e,
                            "write-behind: writing the configuration to disk failed; memory is ahead of the disk, and changes may be lost if the process stops before the next successful write"
                        );
                        *stats.last_error.write().unwrap() = Some(WriterError {
                            message: e.to_string(),
                            at_ms: MockStore::now_ms() as u64,
                        });
                    }
                },
                WriteJob::Barrier(done) => {
                    let _ = done.send(());
                }
            }
        }
        tracing::warn!("write-behind: the writer task stopped (channel closed)");
    }

    /// Queues a write. When the queue is full, `send().await` waits for a free slot: the only case where a change waits
    /// on the writer, by design. If the task is gone (which should not happen), the error is logged rather than turned
    /// into a panic: the change stays applied in memory.
    async fn send_write(&self, path: PathBuf, yaml: String) {
        if self.tx.send(WriteJob::Write { path, yaml }).await.is_err() {
            tracing::error!(
                "write-behind: the writer task is gone; the change is applied in memory only, not written to disk"
            );
        }
    }

    /// Waits until every job queued before this call is done, successful or not: for deterministic tests and for
    /// draining the queue on shutdown (main.rs).
    async fn flush(&self) {
        let (tx, rx) = oneshot::channel();
        if self.tx.send(WriteJob::Barrier(tx)).await.is_ok() {
            let _ = rx.await;
        }
    }

    fn status(&self) -> WriteQueueStatus {
        let pending = WRITE_QUEUE_CAPACITY.saturating_sub(self.tx.capacity());
        let last_write_ok_ms = match self.stats.last_write_ok_ms.load(Ordering::Relaxed) {
            0 => None,
            v => Some(v),
        };
        let last_error = self.stats.last_error.read().unwrap().clone();
        WriteQueueStatus {
            pending,
            capacity: WRITE_QUEUE_CAPACITY,
            last_write_ok_ms,
            last_error,
        }
    }
}

#[derive(Clone)]
pub struct MockStore {
    config: Arc<RwLock<Arc<MockConfig>>>,
    path: PathBuf,
    writer: WriterHandle,
}

impl MockStore {
    pub fn new(path: PathBuf) -> Self {
        Self {
            config: Arc::new(RwLock::new(Arc::new(MockConfig::empty()))),
            path,
            writer: WriterHandle::spawn(),
        }
    }

    pub fn data_path() -> PathBuf {
        std::env::var("DATA_PATH")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("./data"))
    }

    pub fn config_file(data_dir: &Path) -> PathBuf {
        data_dir.join("mock-config.yaml")
    }

    pub fn backup_max_count() -> usize {
        std::env::var("BACKUP_MAX_COUNT")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(5)
    }

    pub async fn load_or_init(data_dir: &Path) -> Result<Self, StoreError> {
        let file = Self::config_file(data_dir);
        std::fs::create_dir_all(data_dir).map_err(|e| StoreError::Io(e.to_string()))?;

        // The first write is synchronous on purpose: it happens before the server takes any traffic, so no request waits
        // on it, and the file must exist as soon as load_or_init returns, without a flush().
        let config = if file.exists() {
            let content =
                std::fs::read_to_string(&file).map_err(|e| StoreError::Io(e.to_string()))?;
            serde_yaml::from_str(&content).map_err(|e| StoreError::Yaml(e.to_string()))?
        } else {
            let empty = MockConfig::empty();
            let yaml =
                serde_yaml::to_string(&empty).map_err(|e| StoreError::Yaml(e.to_string()))?;
            Self::write_to_disk(&file, &yaml)?;
            empty
        };

        tracing::info!(path = %file.display(), services = config.services.len(), "config loaded");

        Ok(Self {
            config: Arc::new(RwLock::new(Arc::new(config))),
            path: file,
            writer: WriterHandle::spawn(),
        })
    }

    pub async fn snapshot(&self) -> Arc<MockConfig> {
        self.config.read().await.clone()
    }

    /// Applies the change in memory at once and leaves the disk write to the background task. The write lock is held
    /// from the synchronous backup until the job is queued, so concurrent changes reach the single-consumer queue in
    /// the order they were applied, and the disk sees them in that order.
    pub async fn replace(&self, config: MockConfig) -> Result<(), StoreError> {
        let mut guard = self.config.write().await;
        let yaml = Self::prepare_and_backup(&self.path, &config)?;
        *guard = Arc::new(config);
        self.writer.send_write(self.path.clone(), yaml).await;
        Ok(())
    }

    pub async fn update<F>(&self, f: F) -> Result<Arc<MockConfig>, StoreError>
    where
        F: FnOnce(&mut MockConfig),
    {
        let mut guard = self.config.write().await;
        let mut cfg = (**guard).clone();
        f(&mut cfg);
        let yaml = Self::prepare_and_backup(&self.path, &cfg)?;
        *guard = Arc::new(cfg);
        self.writer.send_write(self.path.clone(), yaml).await;
        Ok(guard.clone())
    }

    /// Like `update`, but `f` may refuse the change by returning an error, in which case nothing is written. The
    /// check and the mutation run under the same write lock, so two concurrent requests cannot both pass a check
    /// that only one of them should pass (two services created with the same name, for instance).
    pub async fn try_update<F, E>(&self, f: F) -> Result<Result<Arc<MockConfig>, E>, StoreError>
    where
        F: FnOnce(&mut MockConfig) -> Result<(), E>,
    {
        let mut guard = self.config.write().await;
        let mut cfg = (**guard).clone();
        if let Err(refusal) = f(&mut cfg) {
            return Ok(Err(refusal));
        }
        let yaml = Self::prepare_and_backup(&self.path, &cfg)?;
        *guard = Arc::new(cfg);
        self.writer.send_write(self.path.clone(), yaml).await;
        Ok(Ok(guard.clone()))
    }

    /// Waits until every change submitted so far is written (or failed). For deterministic tests and for draining on
    /// shutdown (main.rs); request handlers never call it.
    pub async fn flush(&self) {
        self.writer.flush().await;
    }

    /// State of the background writer (queue length, last successful write, last error), for GET /api/health.
    pub fn writer_status(&self) -> WriteQueueStatus {
        self.writer.status()
    }

    /// The available backups (backups/ and backups/protected/), newest first. Only file metadata is read (name, size,
    /// modification time), never the YAML content.
    pub async fn list_backups(&self) -> Result<Vec<BackupInfo>, StoreError> {
        let parent = self
            .path
            .parent()
            .ok_or_else(|| StoreError::Io("config path has no parent directory".into()))?;

        let backups_dir = parent.join("backups");
        let mut result = Vec::new();
        Self::collect_backups_dir(&backups_dir, false, &mut result)?;
        Self::collect_backups_dir(&backups_dir.join("protected"), true, &mut result)?;

        // Several backups can share a millisecond. Within one, the sequence number of the name orders the normal ones,
        // and a protected backup is the oldest: a reset takes it before the write that makes the normal one.
        result.sort_by_key(|b| {
            std::cmp::Reverse((
                b.created_at_ms,
                !b.protected,
                Self::backup_sequence(&b.filename),
            ))
        });
        Ok(result)
    }

    /// The sequence number of a normal backup's name (`mock-config-{ts}-{seq}.yaml`), 0 for any other name.
    fn backup_sequence(filename: &str) -> u64 {
        filename
            .strip_prefix("mock-config-")
            .and_then(|rest| rest.strip_suffix(".yaml"))
            .and_then(|rest| rest.split('-').nth(1))
            .and_then(|seq| seq.parse().ok())
            .unwrap_or(0)
    }

    fn collect_backups_dir(
        dir: &Path,
        protected: bool,
        out: &mut Vec<BackupInfo>,
    ) -> Result<(), StoreError> {
        if !dir.exists() {
            return Ok(());
        }

        for entry in std::fs::read_dir(dir).map_err(|e| StoreError::Io(e.to_string()))? {
            let Ok(entry) = entry else { continue };
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            let Ok(meta) = entry.metadata() else { continue };
            let Some(filename) = path.file_name().and_then(|n| n.to_str()) else {
                continue;
            };

            let created_at_ms =
                Self::extract_backup_timestamp(&path, protected).unwrap_or_else(|| {
                    meta.modified()
                        .ok()
                        .and_then(|m| m.duration_since(std::time::UNIX_EPOCH).ok())
                        .map(|d| d.as_millis())
                        .unwrap_or(0)
                }) as u64;

            out.push(BackupInfo {
                filename: filename.to_string(),
                protected,
                size_bytes: meta.len(),
                created_at_ms,
            });
        }
        Ok(())
    }

    fn extract_backup_timestamp(path: &Path, protected: bool) -> Option<u128> {
        if protected {
            Self::extract_protected_timestamp(path)
        } else {
            path.file_stem()?
                .to_str()?
                .strip_prefix("mock-config-")?
                .split('-')
                .next()?
                .parse::<u128>()
                .ok()
        }
    }

    /// Restores the configuration from a backup in backups/ or backups/protected/. The caller has already validated
    /// `filename` (`validate_backup_filename`, no path traversal); this only looks the name up in the two directories.
    /// The restore goes through `replace()`, so the state it overwrites is backed up first, like any other change.
    pub async fn restore_from_backup(&self, filename: &str) -> Result<(), StoreError> {
        let parent = self
            .path
            .parent()
            .ok_or_else(|| StoreError::Io("config path has no parent directory".into()))?;

        let backups_dir = parent.join("backups");
        let candidate = backups_dir.join(filename);
        let protected_candidate = backups_dir.join("protected").join(filename);

        let source = if candidate.is_file() {
            candidate
        } else if protected_candidate.is_file() {
            protected_candidate
        } else {
            return Err(StoreError::NotFound(format!(
                "backup file not found: {filename}"
            )));
        };

        let content =
            std::fs::read_to_string(&source).map_err(|e| StoreError::Io(e.to_string()))?;
        let config: MockConfig =
            serde_yaml::from_str(&content).map_err(|e| StoreError::Yaml(e.to_string()))?;

        tracing::info!(path = %source.display(), "config restore from backup");
        self.replace(config).await
    }

    /// The protected backup taken before a full reset. Called explicitly before `replace(MockConfig::empty())` by the
    /// reset handler; ordinary writes never create a protected backup.
    pub async fn backup_before_reset(&self) -> Result<(), StoreError> {
        if !self.path.exists() {
            return Ok(());
        }
        let parent = self
            .path
            .parent()
            .ok_or_else(|| StoreError::Io("config path has no parent directory".into()))?;

        let protected_dir = parent.join("backups").join("protected");
        std::fs::create_dir_all(&protected_dir).map_err(|e| StoreError::Io(e.to_string()))?;

        let dest = protected_dir.join(format!("pre-reset-{}.yaml", Self::now_ms()));
        std::fs::copy(&self.path, &dest).map_err(|e| StoreError::Io(e.to_string()))?;
        tracing::info!(path = %dest.display(), "pre-reset backup created (protected, 30j)");
        Ok(())
    }

    /// The synchronous part of a write: serializes to YAML, purges expired protected backups, then backs up the current
    /// file before anything replaces it. Runs under the write lock, before the asynchronous write is queued (see the
    /// top of this file: the backup always comes first).
    fn prepare_and_backup(path: &Path, config: &MockConfig) -> Result<String, StoreError> {
        let yaml = serde_yaml::to_string(config).map_err(|e| StoreError::Yaml(e.to_string()))?;

        let parent = path
            .parent()
            .ok_or_else(|| StoreError::Io("config path has no parent directory".into()))?;

        Self::purge_expired_protected_backups(parent)?;
        Self::backup_before_overwrite(path, parent)?;

        Ok(yaml)
    }

    /// The asynchronous part: writes the serialized YAML through a temporary file and a rename. Only the background
    /// task calls it, except for the first write in load_or_init(), which is synchronous on purpose.
    fn write_to_disk(path: &Path, yaml: &str) -> Result<(), StoreError> {
        let parent = path
            .parent()
            .ok_or_else(|| StoreError::Io("config path has no parent directory".into()))?;

        let tmp_path = parent.join(".mock-config.yaml.tmp");
        std::fs::write(&tmp_path, yaml.as_bytes()).map_err(|e| StoreError::Io(e.to_string()))?;
        std::fs::rename(&tmp_path, path).map_err(|e| StoreError::Io(e.to_string()))?;

        Ok(())
    }

    /// Deletes the pre-reset backups (backups/protected/) older than PROTECTED_BACKUP_MAX_AGE_MS, at each write rather
    /// than on a timer. The age comes from the timestamp in the file name (`pre-reset-{ts}.yaml`), not from file
    /// metadata.
    ///
    /// It runs under the write lock on every change, and this directory has no count-based rotation: it can hold
    /// thousands of files (test suites reset before every test). `entry.file_type()` reuses what the directory listing
    /// already returned, whereas `entry.path().is_file()` would make one more system call per file on every change.
    fn purge_expired_protected_backups(parent: &Path) -> Result<(), StoreError> {
        let protected_dir = parent.join("backups").join("protected");
        if !protected_dir.exists() {
            return Ok(());
        }

        let now = Self::now_ms();
        for entry in std::fs::read_dir(&protected_dir).map_err(|e| StoreError::Io(e.to_string()))? {
            let Ok(entry) = entry else { continue };
            let is_file = entry.file_type().map(|t| t.is_file()).unwrap_or(false);
            if !is_file {
                continue;
            }
            let path = entry.path();
            if let Some(ts) = Self::extract_protected_timestamp(&path)
                && now.saturating_sub(ts) >= PROTECTED_BACKUP_MAX_AGE_MS
            {
                std::fs::remove_file(&path).map_err(|e| StoreError::Io(e.to_string()))?;
                tracing::info!(path = %path.display(), "expired pre-reset backup purged");
            }
        }
        Ok(())
    }

    fn extract_protected_timestamp(path: &Path) -> Option<u128> {
        path.file_stem()?
            .to_str()?
            .strip_prefix("pre-reset-")?
            .parse::<u128>()
            .ok()
    }

    /// Copies the current configuration file into a backup directory before it is overwritten, then deletes the oldest
    /// copies beyond `backup_max_count()`. Does nothing when the file does not exist yet.
    fn backup_before_overwrite(path: &Path, parent: &Path) -> Result<(), StoreError> {
        if !path.exists() {
            return Ok(());
        }

        let backups_dir = parent.join("backups");
        std::fs::create_dir_all(&backups_dir).map_err(|e| StoreError::Io(e.to_string()))?;

        let seq = BACKUP_SEQ.fetch_add(1, Ordering::Relaxed);
        let backup_name = format!("mock-config-{}-{:06}.yaml", Self::now_ms(), seq);
        std::fs::copy(path, backups_dir.join(&backup_name))
            .map_err(|e| StoreError::Io(e.to_string()))?;

        Self::rotate_backups(&backups_dir)?;
        Ok(())
    }

    fn rotate_backups(backups_dir: &Path) -> Result<(), StoreError> {
        let mut files: Vec<PathBuf> = std::fs::read_dir(backups_dir)
            .map_err(|e| StoreError::Io(e.to_string()))?
            .filter_map(|entry| entry.ok())
            .map(|entry| entry.path())
            .filter(|p| p.is_file())
            .collect();

        // Names hold a fixed-width timestamp and sequence: sorting them as text sorts them by date (newest last).
        files.sort();

        let max = Self::backup_max_count();
        if files.len() > max {
            for old in &files[..files.len() - max] {
                std::fs::remove_file(old).map_err(|e| StoreError::Io(e.to_string()))?;
            }
        }
        Ok(())
    }

    fn now_ms() -> u128 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis()
    }
}

/// A backup file as `GET /api/config/backups` lists it: metadata only, never the YAML content.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct BackupInfo {
    pub filename: String,
    pub protected: bool,
    pub size_bytes: u64,
    pub created_at_ms: u64,
}

#[derive(Debug, Clone)]
pub enum StoreError {
    Io(String),
    Yaml(String),
    NotFound(String),
}

impl std::fmt::Display for StoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StoreError::Io(msg) => write!(f, "IO error: {msg}"),
            StoreError::Yaml(msg) => write!(f, "YAML error: {msg}"),
            StoreError::NotFound(msg) => write!(f, "Not found: {msg}"),
        }
    }
}

impl std::error::Error for StoreError {}

// ENV_MUTEX guards are held across awaits on purpose: they serialize the tests that mutate process-wide
// environment variables, and each #[tokio::test] owns its runtime, so holding one cannot deadlock.
#[cfg(test)]
#[allow(clippy::await_holding_lock)]
mod tests {
    use super::*;
    use crate::models::{WsdlMode, *};

    // Process-wide env vars (BACKUP_MAX_COUNT, DATA_PATH) are mutated by
    // several tests below; cargo test runs test fns in parallel OS threads,
    // so without serialization one test's set_var/remove_var can leak into
    // another's assertion window (pre-existing flakiness, unrelated to
    // write-behind). Any test touching these env vars must hold this lock
    // for its whole body.
    static ENV_MUTEX: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn temp_dir() -> PathBuf {
        let dir = crate::server::test_support::temp_data_dir("test");
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn sample_config() -> MockConfig {
        MockConfig {
            services: vec![Service {
                name: "svc-a".into(),

                listen_path: "/svc-a/*".into(),
                real_target_url: "http://svc-a:8080".into(),
                is_mocked: true,
                rewrite_directory_urls: false,
                group_name: None,
                wsdl_mode: WsdlMode::default(),
                rules: vec![Rule {
                    name: "default".into(),
                    method: "GET".into(),
                    sub_path: None,
                    action: RuleAction::default(),
                    pre_script: None,
                    script: None,
                    post_script: None,
                    response_mode: None,
                    conditions: ConditionGroup::default(),
                    response: MockResponse {
                        status: 200,
                        headers: vec![],
                        body: vec![BodyFragment::Literal { value: "ok".into() }],
                        chaos: None,
                    },
                }],
            }],
            groups: vec![],
        }
    }

    #[tokio::test]
    async fn load_creates_empty_config_if_missing() {
        let dir = temp_dir();
        let store = MockStore::load_or_init(&dir).await.unwrap();
        let config = store.snapshot().await;
        assert!(config.services.is_empty());
        assert!(MockStore::config_file(&dir).exists());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn load_reads_existing_config() {
        let dir = temp_dir();
        let file = MockStore::config_file(&dir);
        let cfg = sample_config();
        let yaml = serde_yaml::to_string(&cfg).unwrap();
        std::fs::write(&file, &yaml).unwrap();

        let store = MockStore::load_or_init(&dir).await.unwrap();
        let loaded = store.snapshot().await;
        assert_eq!(loaded.services.len(), 1);
        assert_eq!(loaded.services[0].name, "svc-a");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn replace_persists_to_disk() {
        let dir = temp_dir();
        let store = MockStore::load_or_init(&dir).await.unwrap();

        let cfg = sample_config();
        store.replace(cfg.clone()).await.unwrap();
        // write-behind: wait for the queued disk write before reading the file.
        store.flush().await;

        let on_disk: MockConfig =
            serde_yaml::from_str(&std::fs::read_to_string(MockStore::config_file(&dir)).unwrap())
                .unwrap();
        assert_eq!(on_disk.services.len(), 1);

        let in_mem = store.snapshot().await;
        assert_eq!(*in_mem, on_disk);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn update_modifies_in_place() {
        let dir = temp_dir();
        let store = MockStore::load_or_init(&dir).await.unwrap();
        store.replace(sample_config()).await.unwrap();

        let updated = store
            .update(|cfg| {
                cfg.services[0].is_mocked = false;
                cfg.services.push(Service {
                    name: "svc-b".into(),

                    listen_path: "/svc-b/*".into(),
                    real_target_url: "http://svc-b:9090".into(),
                    is_mocked: false,
                    rewrite_directory_urls: false,
                    group_name: None,
                    wsdl_mode: WsdlMode::default(),
                    rules: vec![],
                });
            })
            .await
            .unwrap();

        assert_eq!(updated.services.len(), 2);
        assert!(!updated.services[0].is_mocked);
        assert_eq!(updated.services[1].name, "svc-b");

        // write-behind: wait for the queued disk write before reading the file.
        store.flush().await;
        let on_disk: MockConfig =
            serde_yaml::from_str(&std::fs::read_to_string(MockStore::config_file(&dir)).unwrap())
                .unwrap();
        assert_eq!(on_disk, *updated);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn atomic_write_no_partial_file() {
        let dir = temp_dir();
        let store = MockStore::load_or_init(&dir).await.unwrap();
        store.replace(sample_config()).await.unwrap();
        store.flush().await;

        let tmp_path = dir.join(".mock-config.yaml.tmp");
        assert!(
            !tmp_path.exists(),
            "temp file should be cleaned up after rename"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn backup_created_on_write() {
        let dir = temp_dir();
        let store = MockStore::load_or_init(&dir).await.unwrap();
        // load_or_init writes the initial empty config; no prior file to back up yet.
        store.replace(sample_config()).await.unwrap();

        let backups_dir = dir.join("backups");
        let count = std::fs::read_dir(&backups_dir).unwrap().count();
        assert!(
            count >= 1,
            "expected at least one backup after overwriting an existing config"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn backup_rotation_keeps_max_n() {
        let _guard = ENV_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
        let dir = temp_dir();
        unsafe { std::env::set_var("BACKUP_MAX_COUNT", "3") };

        let store = MockStore::load_or_init(&dir).await.unwrap();
        for i in 0..6 {
            let mut cfg = sample_config();
            cfg.services[0].name = format!("svc-{i}");
            store.replace(cfg).await.unwrap();
        }

        let backups_dir = dir.join("backups");
        let count = std::fs::read_dir(&backups_dir).unwrap().count();
        assert_eq!(count, 3, "backups should be capped at BACKUP_MAX_COUNT");

        unsafe { std::env::remove_var("BACKUP_MAX_COUNT") };
        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn protected_backup_created_before_reset() {
        let dir = temp_dir();
        let store = MockStore::load_or_init(&dir).await.unwrap();
        store.replace(sample_config()).await.unwrap();

        store.backup_before_reset().await.unwrap();

        let protected_dir = dir.join("backups").join("protected");
        let count = std::fs::read_dir(&protected_dir).unwrap().count();
        assert_eq!(count, 1, "expected exactly one pre-reset backup");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn protected_backup_exempt_from_normal_rotation() {
        let _guard = ENV_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
        let dir = temp_dir();
        unsafe { std::env::set_var("BACKUP_MAX_COUNT", "2") };

        let store = MockStore::load_or_init(&dir).await.unwrap();
        store.replace(sample_config()).await.unwrap();
        store.backup_before_reset().await.unwrap();
        store.replace(MockConfig::empty()).await.unwrap();

        // Many writes after the reset, well beyond BACKUP_MAX_COUNT=2 —
        // the protected pre-reset backup must survive all of them.
        for i in 0..8 {
            let mut cfg = sample_config();
            cfg.services[0].name = format!("svc-{i}");
            store.replace(cfg).await.unwrap();
        }

        let protected_dir = dir.join("backups").join("protected");
        let count = std::fs::read_dir(&protected_dir).unwrap().count();
        assert_eq!(
            count, 1,
            "pre-reset backup must not be rotated away by normal quota"
        );

        let backups_dir = dir.join("backups");
        let normal_count = std::fs::read_dir(&backups_dir)
            .unwrap()
            .filter(|e| e.as_ref().unwrap().path().is_file())
            .count();
        assert_eq!(
            normal_count, 2,
            "normal backups still capped at BACKUP_MAX_COUNT"
        );

        unsafe { std::env::remove_var("BACKUP_MAX_COUNT") };
        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn protected_backup_purged_after_one_month_on_next_write() {
        let dir = temp_dir();
        let store = MockStore::load_or_init(&dir).await.unwrap();
        store.replace(sample_config()).await.unwrap();

        let protected_dir = dir.join("backups").join("protected");
        std::fs::create_dir_all(&protected_dir).unwrap();
        let old_ts = MockStore::now_ms() - PROTECTED_BACKUP_MAX_AGE_MS - 1_000;
        std::fs::write(
            protected_dir.join(format!("pre-reset-{old_ts}.yaml")),
            b"services: []\ngroups: []\n",
        )
        .unwrap();

        // Any subsequent write is the "activity 1 month later" signal.
        store.replace(MockConfig::empty()).await.unwrap();

        let count = std::fs::read_dir(&protected_dir).unwrap().count();
        assert_eq!(
            count, 0,
            "pre-reset backup older than 30 days should be purged"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn protected_backup_kept_if_not_yet_expired() {
        let dir = temp_dir();
        let store = MockStore::load_or_init(&dir).await.unwrap();
        store.replace(sample_config()).await.unwrap();

        let protected_dir = dir.join("backups").join("protected");
        std::fs::create_dir_all(&protected_dir).unwrap();
        let recent_ts = MockStore::now_ms() - 1_000;
        std::fs::write(
            protected_dir.join(format!("pre-reset-{recent_ts}.yaml")),
            b"services: []\ngroups: []\n",
        )
        .unwrap();

        store.replace(MockConfig::empty()).await.unwrap();

        let count = std::fs::read_dir(&protected_dir).unwrap().count();
        assert_eq!(count, 1, "pre-reset backup under 30 days old must be kept");
        std::fs::remove_dir_all(&dir).ok();
    }

    /// backups/protected/ has no count-based rotation: only the age-based purge, run under the write lock on every
    /// change, bounds it, and it can grow to thousands of files. The purge reads the file type from the directory
    /// listing so that this stays cheap. The test checks correctness at that scale rather than timing (unreliable in
    /// CI): among 1,501 entries, only the expired one is deleted.
    #[tokio::test]
    async fn purge_expired_protected_backups_correct_with_many_entries() {
        let dir = temp_dir();
        let store = MockStore::load_or_init(&dir).await.unwrap();
        store.replace(sample_config()).await.unwrap();

        let protected_dir = dir.join("backups").join("protected");
        std::fs::create_dir_all(&protected_dir).unwrap();

        // now_ms() is read once before the loop: read at each iteration, the clock could move during the 1,500 writes and
        // two iterations could produce the same file name, leaving fewer than 1,500 files.
        let base_now = MockStore::now_ms();
        for i in 0..1500 {
            let fresh_ts = base_now - 1_000 - i;
            std::fs::write(
                protected_dir.join(format!("pre-reset-{fresh_ts}.yaml")),
                b"services: []\ngroups: []\n",
            )
            .unwrap();
        }
        let old_ts = MockStore::now_ms() - PROTECTED_BACKUP_MAX_AGE_MS - 1_000;
        std::fs::write(
            protected_dir.join(format!("pre-reset-{old_ts}.yaml")),
            b"services: []\ngroups: []\n",
        )
        .unwrap();

        store.replace(MockConfig::empty()).await.unwrap();

        let remaining = std::fs::read_dir(&protected_dir).unwrap().count();
        assert_eq!(
            remaining, 1500,
            "only the single expired entry should be purged out of 1501"
        );
        assert!(
            !protected_dir
                .join(format!("pre-reset-{old_ts}.yaml"))
                .exists(),
            "the expired entry itself must be gone"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn backup_max_count_default() {
        let _guard = ENV_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
        unsafe { std::env::remove_var("BACKUP_MAX_COUNT") };
        assert_eq!(MockStore::backup_max_count(), 5);
    }

    #[test]
    fn backup_max_count_from_env() {
        let _guard = ENV_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
        unsafe { std::env::set_var("BACKUP_MAX_COUNT", "12") };
        assert_eq!(MockStore::backup_max_count(), 12);
        unsafe { std::env::remove_var("BACKUP_MAX_COUNT") };
    }

    #[tokio::test]
    async fn replace_empty_config() {
        let dir = temp_dir();
        let store = MockStore::load_or_init(&dir).await.unwrap();
        store.replace(sample_config()).await.unwrap();
        store.replace(MockConfig::empty()).await.unwrap();

        let snapshot = store.snapshot().await;
        assert!(snapshot.services.is_empty());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn list_backups_empty_dir() {
        let dir = temp_dir();
        let store = MockStore::load_or_init(&dir).await.unwrap();
        // load_or_init only writes the initial config, no prior file to back up.
        let backups = store.list_backups().await.unwrap();
        assert!(backups.is_empty());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn list_backups_includes_normal_and_protected() {
        let dir = temp_dir();
        let store = MockStore::load_or_init(&dir).await.unwrap();
        store.replace(sample_config()).await.unwrap();
        store.replace(sample_config()).await.unwrap();
        store.backup_before_reset().await.unwrap();

        let backups = store.list_backups().await.unwrap();
        assert_eq!(backups.iter().filter(|b| !b.protected).count(), 2);
        assert_eq!(backups.iter().filter(|b| b.protected).count(), 1);
        assert!(backups.iter().all(|b| b.size_bytes > 0));
        assert!(backups.iter().all(|b| b.created_at_ms > 0));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn list_backups_sorted_most_recent_first() {
        let dir = temp_dir();
        let store = MockStore::load_or_init(&dir).await.unwrap();
        for i in 0..3 {
            let mut cfg = sample_config();
            cfg.services[0].name = format!("svc-{i}");
            store.replace(cfg).await.unwrap();
        }

        let backups = store.list_backups().await.unwrap();
        assert!(backups.len() >= 2);
        for pair in backups.windows(2) {
            assert!(pair[0].created_at_ms >= pair[1].created_at_ms);
        }
        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn list_backups_orders_the_backups_of_one_millisecond_newest_first() {
        // A reset takes a protected backup then a normal one, and a fast client can make several changes, within the
        // same millisecond: the order cannot come from the timestamp alone, nor from the order the directory lists.
        let dir = temp_dir();
        let store = MockStore::load_or_init(&dir).await.unwrap();
        let backups_dir = dir.join("backups");
        std::fs::create_dir_all(backups_dir.join("protected")).unwrap();
        std::fs::write(
            backups_dir.join("protected/pre-reset-1700000000000.yaml"),
            "",
        )
        .unwrap();
        for seq in [3, 1, 5, 2, 4] {
            std::fs::write(
                backups_dir.join(format!("mock-config-1700000000000-{seq:06}.yaml")),
                "",
            )
            .unwrap();
        }

        let names: Vec<String> = store
            .list_backups()
            .await
            .unwrap()
            .into_iter()
            .map(|b| b.filename)
            .collect();
        assert_eq!(
            names,
            [
                "mock-config-1700000000000-000005.yaml",
                "mock-config-1700000000000-000004.yaml",
                "mock-config-1700000000000-000003.yaml",
                "mock-config-1700000000000-000002.yaml",
                "mock-config-1700000000000-000001.yaml",
                "pre-reset-1700000000000.yaml",
            ]
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn restore_from_backup_replaces_config() {
        let dir = temp_dir();
        let store = MockStore::load_or_init(&dir).await.unwrap();
        store.replace(sample_config()).await.unwrap();
        // write-behind: flush so "svc-a" actually lands on disk before the
        // next replace() backs up "whatever is currently on disk" — without
        // this, the backup below could capture the stale bootstrap content.
        store.flush().await;

        let mut other = sample_config();
        other.services[0].name = "restored-svc".into();
        store.replace(other).await.unwrap();

        let backups = store.list_backups().await.unwrap();
        let first_backup = backups
            .iter()
            .find(|b| !b.protected)
            .expect("expected at least one normal backup");

        store
            .restore_from_backup(&first_backup.filename)
            .await
            .unwrap();

        let restored = store.snapshot().await;
        assert_eq!(restored.services[0].name, "svc-a");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn restore_from_backup_finds_protected_file() {
        let dir = temp_dir();
        let store = MockStore::load_or_init(&dir).await.unwrap();
        store.replace(sample_config()).await.unwrap();
        // write-behind: flush so "svc-a" actually lands on disk before
        // backup_before_reset() copies "whatever is currently on disk" into
        // backups/protected/ — otherwise it would copy the stale bootstrap
        // content instead.
        store.flush().await;
        store.backup_before_reset().await.unwrap();
        store.replace(MockConfig::empty()).await.unwrap();

        let backups = store.list_backups().await.unwrap();
        let protected = backups
            .iter()
            .find(|b| b.protected)
            .expect("expected a protected backup");

        store
            .restore_from_backup(&protected.filename)
            .await
            .unwrap();

        let restored = store.snapshot().await;
        assert_eq!(restored.services.len(), 1);
        assert_eq!(restored.services[0].name, "svc-a");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn restore_from_backup_unknown_filename_errors() {
        let dir = temp_dir();
        let store = MockStore::load_or_init(&dir).await.unwrap();
        store.replace(sample_config()).await.unwrap();

        let err = store
            .restore_from_backup("mock-config-9999999999999-000042.yaml")
            .await
            .unwrap_err();
        assert!(matches!(err, StoreError::NotFound(_)));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn restore_from_backup_creates_safety_backup_of_current_state_first() {
        // This test counts the rotated backups of backups/: it holds ENV_MUTEX, or another test setting BACKUP_MAX_COUNT
        // at the same time could cap the count and make the before/after comparison fail now and then.
        let _guard = ENV_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
        unsafe { std::env::remove_var("BACKUP_MAX_COUNT") };
        let dir = temp_dir();
        let store = MockStore::load_or_init(&dir).await.unwrap();
        store.replace(sample_config()).await.unwrap();

        let mut other = sample_config();
        other.services[0].name = "before-restore".into();
        store.replace(other).await.unwrap();

        let backups_before = store.list_backups().await.unwrap();
        let target = backups_before
            .iter()
            .find(|b| !b.protected)
            .unwrap()
            .filename
            .clone();

        store.restore_from_backup(&target).await.unwrap();

        // restore_from_backup() -> replace() -> prepare_and_backup() must have backed
        // up "before-restore" (the state overwritten by the restore) before queuing
        // the restored content for write-behind — never lose the pre-restore state.
        let backups_after = store.list_backups().await.unwrap();
        assert!(
            backups_after.iter().filter(|b| !b.protected).count()
                > backups_before.iter().filter(|b| !b.protected).count()
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn data_path_default() {
        let _guard = ENV_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
        unsafe { std::env::remove_var("DATA_PATH") };
        let p = MockStore::data_path();
        assert_eq!(p, PathBuf::from("./data"));
    }

    #[test]
    fn data_path_from_env() {
        let _guard = ENV_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
        unsafe { std::env::set_var("DATA_PATH", "/mnt/pvc/mimicway") };
        let p = MockStore::data_path();
        assert_eq!(p, PathBuf::from("/mnt/pvc/mimicway"));
        unsafe { std::env::remove_var("DATA_PATH") };
    }

    #[tokio::test]
    async fn concurrent_reads_dont_block() {
        let dir = temp_dir();
        let store = MockStore::load_or_init(&dir).await.unwrap();
        store.replace(sample_config()).await.unwrap();

        let s1 = store.clone();
        let s2 = store.clone();
        let (r1, r2) = tokio::join!(s1.snapshot(), s2.snapshot());
        assert_eq!(r1, r2);
        std::fs::remove_dir_all(&dir).ok();
    }

    // --------------- Write-behind ---------------

    #[tokio::test]
    async fn write_behind_flush_persists_to_disk() {
        let dir = temp_dir();
        let store = MockStore::load_or_init(&dir).await.unwrap();

        store.replace(sample_config()).await.unwrap();
        store.flush().await;

        let on_disk: MockConfig =
            serde_yaml::from_str(&std::fs::read_to_string(MockStore::config_file(&dir)).unwrap())
                .unwrap();
        assert_eq!(on_disk.services.len(), 1);
        let status = store.writer_status();
        assert!(status.last_write_ok_ms.is_some());
        assert!(status.last_error.is_none());
        std::fs::remove_dir_all(&dir).ok();
    }

    /// The backup of the overwritten state is on disk right after replace(), without flush(): prepare_and_backup()
    /// writes it synchronously before the new content is even queued. Only the new content itself (mock-config.yaml)
    /// is written in the background.
    #[tokio::test]
    async fn backup_is_synchronous_before_write_is_queued() {
        let dir = temp_dir();
        let store = MockStore::load_or_init(&dir).await.unwrap();

        store.replace(sample_config()).await.unwrap();
        // No flush() on purpose: the previous state's backup must already be on disk whatever the background task is doing.
        let backups = store.list_backups().await.unwrap();
        assert_eq!(
            backups.iter().filter(|b| !b.protected).count(),
            1,
            "backup must be created synchronously, before the async write is even queued"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn concurrent_updates_preserve_order_and_match_disk() {
        let dir = temp_dir();
        let store = MockStore::load_or_init(&dir).await.unwrap();
        store.replace(sample_config()).await.unwrap();

        let mut handles = Vec::new();
        for i in 0..10 {
            let s = store.clone();
            handles.push(tokio::spawn(async move {
                s.update(move |cfg| {
                    cfg.services.push(Service {
                        name: format!("concurrent-{i}"),
                        listen_path: format!("/concurrent-{i}/*"),
                        real_target_url: "http://x:8080".into(),
                        is_mocked: true,
                        rewrite_directory_urls: false,
                        group_name: None,
                        wsdl_mode: WsdlMode::default(),
                        rules: vec![],
                    });
                })
                .await
                .unwrap();
            }));
        }
        for h in handles {
            h.await.unwrap();
        }
        store.flush().await;

        let on_disk: MockConfig =
            serde_yaml::from_str(&std::fs::read_to_string(MockStore::config_file(&dir)).unwrap())
                .unwrap();
        let in_mem = store.snapshot().await;
        assert_eq!(*in_mem, on_disk, "disk must match memory once flushed");
        assert_eq!(
            on_disk.services.len(),
            11,
            "the original service + all 10 concurrent updates"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn write_queue_status_reports_bounded_capacity() {
        let dir = temp_dir();
        let store = MockStore::load_or_init(&dir).await.unwrap();

        let status = store.writer_status();
        assert_eq!(status.capacity, WRITE_QUEUE_CAPACITY);

        for i in 0..20 {
            let mut cfg = sample_config();
            cfg.services[0].name = format!("svc-{i}");
            store.replace(cfg).await.unwrap();
            assert!(store.writer_status().pending <= WRITE_QUEUE_CAPACITY);
        }
        store.flush().await;
        std::fs::remove_dir_all(&dir).ok();
    }

    /// A failed disk write (invalid path) is logged and reported by writer_status(), and the background task keeps
    /// running: a later change to a valid path still gets written.
    #[tokio::test]
    async fn write_error_is_reported_and_task_keeps_running() {
        let dir = temp_dir();
        let bogus_path = dir.join("missing-subdir").join("mock-config.yaml");
        let store = MockStore::new(bogus_path.clone());

        store.replace(sample_config()).await.unwrap();
        store.flush().await;

        let status = store.writer_status();
        assert!(
            status.last_error.is_some(),
            "write to a missing directory must be reported as an error"
        );
        assert!(status.last_write_ok_ms.is_none());

        std::fs::create_dir_all(bogus_path.parent().unwrap()).unwrap();
        store.replace(sample_config()).await.unwrap();
        store.flush().await;

        let status2 = store.writer_status();
        assert!(
            status2.last_write_ok_ms.is_some(),
            "task must keep processing jobs after a prior write error"
        );
        assert!(bogus_path.exists());
        std::fs::remove_dir_all(&dir).ok();
    }
}
