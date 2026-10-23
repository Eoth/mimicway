// The configuration (services and groups), kept in memory and in one YAML file.
//
// Reads take a snapshot: an Arc of the current configuration, so a reader never copies it nor waits for a writer.
// A change copies the configuration, modifies the copy and swaps it in at once; writing it to disk (temporary file,
// then rename) is left to a background task (write-behind, see WriterHandle), so a request never waits on disk I/O.
//
// Backups: before the file is overwritten, the previous content is copied to {data_dir}/backups/ and only the
// newest BACKUP_MAX_COUNT copies are kept (5 by default; the reference Kubernetes volume is small). One copy per burst
// of changes: a change backs up only when the last backup is older than BACKUP_MIN_INTERVAL_SECS (60 by default), so
// that an action sending several requests (an import merged service by service sent eight) does not push every older
// copy out of the few kept. A change that leaves the configuration as it is writes nothing, and a copy identical to
// the newest one is not made again. Replacing the whole configuration (restore, import, reset) always backs up first.
// The copy is made under the write lock and BEFORE the new content is queued for writing (prepare_and_backup), after
// the writes already queued have reached the file: a change is never accepted without its backup, which is the whole
// point of the rollback, and the copy holds the last state, not an older one still on disk.
//
// A full reset first copies the configuration to backups/protected/ (backup_before_reset). Rotation only looks at
// files directly in backups/, so later changes can never push that copy out. It expires after 30 days
// (PROTECTED_BACKUP_MAX_AGE_MS), checked at each ordinary write: a write 30 days after the reset is taken as the
// sign that nobody depends on the old state any more.
use crate::models::MockConfig;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};
use tokio::sync::{RwLock, mpsc, oneshot};

static BACKUP_SEQ: AtomicU64 = AtomicU64::new(0);

const PROTECTED_BACKUP_MAX_AGE_MS: u128 = 30 * 24 * 60 * 60 * 1000;

/// Rotated backups kept in `backups/` when `BACKUP_MAX_COUNT` is unset.
const DEFAULT_BACKUP_MAX_COUNT: usize = 5;

/// Seconds between two backups of ordinary changes when `BACKUP_MIN_INTERVAL_SECS` is unset. A minute holds the
/// requests of one action, however many it sends, and with five backups kept, at least five minutes of editing.
const DEFAULT_BACKUP_MIN_INTERVAL_SECS: u64 = 60;

/// When a write backs up the file it overwrites.
#[derive(Clone, Copy, PartialEq)]
enum BackupWhen {
    /// An ordinary change: once per burst (`backup_min_interval`).
    OncePerBurst,
    /// The whole configuration replaced (restore, import, reset): every time.
    Always,
}

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

    /// Whether the last write failed, so that the file may hold less than memory: a change that leaves the
    /// configuration as it is still writes then, which is how saving again retries after a disk error.
    fn last_write_failed(&self) -> bool {
        let last_ok = self.stats.last_write_ok_ms.load(Ordering::Relaxed);
        self.stats
            .last_error
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .as_ref()
            .is_some_and(|e| e.at_ms >= last_ok)
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
    /// The data directory as resolved on disk when the store opened it: what every backup operation checks its paths
    /// against (`BackupDirs`).
    root: PathBuf,
    writer: WriterHandle,
    backup_min_interval: Duration,
    /// When this process last backed up the file; None until the first change, which always backs up.
    last_backup: Arc<std::sync::Mutex<Option<Instant>>>,
}

impl MockStore {
    /// A store whose configuration file is `path`, starting empty without reading the disk (tests; the server uses
    /// `load_or_init`). The directory of `path` has to exist: it is the root the backups are confined to.
    pub fn new(path: PathBuf) -> Result<Self, StoreError> {
        let root = Self::resolve_root(&path)?;
        Ok(Self {
            config: Arc::new(RwLock::new(Arc::new(MockConfig::empty()))),
            path,
            root,
            writer: WriterHandle::spawn(),
            backup_min_interval: Self::backup_min_interval_in(crate::settings::env),
            last_backup: Arc::default(),
        })
    }

    /// The same store, backing up ordinary changes at most once per `interval` (tests: 0 backs up every change).
    #[cfg(test)]
    fn with_backup_min_interval(mut self, interval: Duration) -> Self {
        self.backup_min_interval = interval;
        self
    }

    /// The directory of a configuration file, resolved on disk (symbolic links followed).
    fn resolve_root(config_file: &Path) -> Result<PathBuf, StoreError> {
        let dir = Self::parent_of(config_file)?;
        dir.canonicalize().map_err(|e| {
            StoreError::Io(format!(
                "cannot resolve the data directory {}: {e}",
                dir.display()
            ))
        })
    }

    fn parent_of(config_file: &Path) -> Result<&Path, StoreError> {
        config_file
            .parent()
            .ok_or_else(|| StoreError::Io("config path has no parent directory".into()))
    }

    pub fn data_path() -> PathBuf {
        Self::data_path_in(crate::settings::env)
    }

    fn data_path_in(lookup: impl Fn(&str) -> Option<String>) -> PathBuf {
        lookup("DATA_PATH")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("./data"))
    }

    pub fn config_file(data_dir: &Path) -> PathBuf {
        data_dir.join("mock-config.yaml")
    }

    pub fn backup_max_count() -> usize {
        Self::backup_max_count_in(crate::settings::env)
    }

    fn backup_max_count_in(lookup: impl Fn(&str) -> Option<String>) -> usize {
        crate::settings::number(lookup, "BACKUP_MAX_COUNT", DEFAULT_BACKUP_MAX_COUNT)
    }

    fn backup_min_interval_in(lookup: impl Fn(&str) -> Option<String>) -> Duration {
        Duration::from_secs(crate::settings::number(
            lookup,
            "BACKUP_MIN_INTERVAL_SECS",
            DEFAULT_BACKUP_MIN_INTERVAL_SECS,
        ))
    }

    pub async fn load_or_init(data_dir: &Path) -> Result<Self, StoreError> {
        let file = Self::config_file(data_dir);
        std::fs::create_dir_all(data_dir).map_err(|e| StoreError::Io(e.to_string()))?;

        // The first write is synchronous on purpose: it happens before the server takes any traffic, so no request waits
        // on it, and the file must exist as soon as load_or_init returns, without a flush().
        let config = if file.exists() {
            let content =
                std::fs::read_to_string(&file).map_err(|e| StoreError::Io(e.to_string()))?;
            Self::parse(&content)?
        } else {
            let empty = MockConfig::empty();
            let yaml =
                serde_yaml::to_string(&empty).map_err(|e| StoreError::Yaml(e.to_string()))?;
            Self::write_to_disk(&file, &yaml)?;
            empty
        };

        tracing::info!(path = %file.display(), services = config.services.len(), "config loaded");

        let root = Self::resolve_root(&file)?;
        Ok(Self {
            config: Arc::new(RwLock::new(Arc::new(config))),
            path: file,
            root,
            writer: WriterHandle::spawn(),
            backup_min_interval: Self::backup_min_interval_in(crate::settings::env),
            last_backup: Arc::default(),
        })
    }

    /// A configuration read from a file of the store (the configuration itself or a backup). Such a file may come from a
    /// version that saved two services in one slot: the extra ones get a free name, logged, rather than a refusal to
    /// start or to restore over a configuration nobody can then fix from the interface.
    fn parse(content: &str) -> Result<MockConfig, StoreError> {
        let mut config: MockConfig =
            serde_yaml::from_str(content).map_err(|e| StoreError::Yaml(e.to_string()))?;
        for (group, old, new) in config.rename_duplicate_services() {
            tracing::warn!(group = group.as_deref().unwrap_or(""), service = %old, renamed_to = %new, "two services had the same name in the same group: the second one is renamed");
        }
        Ok(config)
    }

    pub async fn snapshot(&self) -> Arc<MockConfig> {
        self.config.read().await.clone()
    }

    /// Applies the change in memory at once and leaves the disk write to the background task. The write lock is held
    /// from the backup until the job is queued, so concurrent changes reach the single-consumer queue in the order they
    /// were applied, and the disk sees them in that order. Replacing the whole configuration always backs up first;
    /// replacing it with itself writes nothing.
    pub async fn replace(&self, config: MockConfig) -> Result<(), StoreError> {
        let mut guard = self.config.write().await;
        if **guard == config && !self.writer.last_write_failed() {
            return Ok(());
        }
        let yaml = self.prepare_and_backup(&config, BackupWhen::Always).await?;
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
        if cfg == **guard && !self.writer.last_write_failed() {
            return Ok(guard.clone());
        }
        let yaml = self
            .prepare_and_backup(&cfg, BackupWhen::OncePerBurst)
            .await?;
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
        if cfg == **guard && !self.writer.last_write_failed() {
            return Ok(Ok(guard.clone()));
        }
        let yaml = self
            .prepare_and_backup(&cfg, BackupWhen::OncePerBurst)
            .await?;
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
        let dirs = BackupDirs::open(&self.root, &self.path)?;
        let mut result = Vec::new();
        Self::collect_backups_dir(&dirs.backups, false, &mut result)?;
        Self::collect_backups_dir(&dirs.protected, true, &mut result)?;

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
        let dirs = BackupDirs::open(&self.root, &self.path)?;
        let candidate = dirs.backups.join(filename);
        let protected_candidate = dirs.protected.join(filename);

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
        let config = Self::parse(&content)?;

        tracing::info!(path = %source.display(), "config restore from backup");
        self.replace(config).await
    }

    /// The protected backup taken before a full reset. Called explicitly before `replace(MockConfig::empty())` by the
    /// reset handler; ordinary writes never create a protected backup.
    pub async fn backup_before_reset(&self) -> Result<(), StoreError> {
        let dirs = BackupDirs::open(&self.root, &self.path)?;
        let Some(file) = dirs.config_file else {
            return Ok(());
        };

        let dest = dirs
            .protected
            .join(format!("pre-reset-{}.yaml", Self::now_ms()));
        std::fs::copy(&file, &dest).map_err(|e| StoreError::Io(e.to_string()))?;
        tracing::info!(path = %dest.display(), "pre-reset backup created (protected, 30j)");
        Ok(())
    }

    /// The part of a write done before it is queued: serializes to YAML, purges expired protected backups, then backs
    /// up the current file when `when` says so. Runs under the write lock (see the top of this file: the backup always
    /// comes first). Before copying, it waits for the writes already queued: without that, a change made a moment
    /// before a restore was still only in memory, and the restore's backup held the state before it.
    async fn prepare_and_backup(
        &self,
        config: &MockConfig,
        when: BackupWhen,
    ) -> Result<String, StoreError> {
        let yaml = serde_yaml::to_string(config).map_err(|e| StoreError::Yaml(e.to_string()))?;

        let dirs = BackupDirs::open(&self.root, &self.path)?;
        Self::purge_expired_protected_backups(&dirs.protected)?;
        if let Some(file) = &dirs.config_file
            && self.backup_due(when)
        {
            self.writer.flush().await;
            Self::backup_before_overwrite(file, &dirs.backups)?;
            *self.last_backup.lock().unwrap_or_else(|e| e.into_inner()) = Some(Instant::now());
        }

        Ok(yaml)
    }

    fn backup_due(&self, when: BackupWhen) -> bool {
        let last = *self.last_backup.lock().unwrap_or_else(|e| e.into_inner());
        when == BackupWhen::Always || last.is_none_or(|at| at.elapsed() >= self.backup_min_interval)
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
    fn purge_expired_protected_backups(protected_dir: &Path) -> Result<(), StoreError> {
        let now = Self::now_ms();
        for entry in std::fs::read_dir(protected_dir).map_err(|e| StoreError::Io(e.to_string()))? {
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

    /// Copies the current configuration file (`file`, resolved by `BackupDirs`) into `backups_dir` before it is
    /// overwritten, unless the newest copy already holds the same bytes, then deletes the oldest copies beyond
    /// `backup_max_count()`.
    fn backup_before_overwrite(file: &Path, backups_dir: &Path) -> Result<(), StoreError> {
        let mut files = Self::backup_files(backups_dir)?;
        if let Some(newest) = files.last()
            && Self::same_bytes(file, newest)
        {
            return Ok(());
        }
        let seq = BACKUP_SEQ.fetch_add(1, Ordering::Relaxed);
        let backup = backups_dir.join(format!("mock-config-{}-{:06}.yaml", Self::now_ms(), seq));
        std::fs::copy(file, &backup).map_err(|e| StoreError::Io(e.to_string()))?;
        files.push(backup);

        Self::rotate_backups(&files)
    }

    /// The files directly in `backups_dir`, oldest first: names hold a fixed-width timestamp and sequence, so sorting
    /// them as text sorts them by date.
    fn backup_files(backups_dir: &Path) -> Result<Vec<PathBuf>, StoreError> {
        let mut files: Vec<PathBuf> = std::fs::read_dir(backups_dir)
            .map_err(|e| StoreError::Io(e.to_string()))?
            .filter_map(|entry| entry.ok())
            .filter(|entry| entry.file_type().is_ok_and(|t| t.is_file()))
            .map(|entry| entry.path())
            .collect();
        files.sort();
        Ok(files)
    }

    /// Whether two files hold the same bytes. The sizes are compared first, so that two different configurations, the
    /// usual case, cost no read; a file that cannot be read counts as different, and the copy is made.
    fn same_bytes(a: &Path, b: &Path) -> bool {
        let len = |p: &Path| std::fs::metadata(p).map(|m| m.len()).ok();
        match (len(a), len(b)) {
            (Some(x), Some(y)) if x == y => {}
            _ => return false,
        }
        matches!((std::fs::read(a), std::fs::read(b)), (Ok(x), Ok(y)) if x == y)
    }

    /// Deletes the oldest of `files` (oldest first) beyond `backup_max_count()`.
    fn rotate_backups(files: &[PathBuf]) -> Result<(), StoreError> {
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

/// Where a backup operation reads, copies and deletes: the data directory, its `backups/` and `backups/protected/`
/// (created when missing) and the configuration file, all resolved on disk (`canonicalize`, which follows symbolic
/// links) and checked to be the directory the store opened or under it. On a shared volume, another process can write
/// to the data directory: a symbolic link planted at the place of `backups/` would otherwise send the copies, and the
/// deletions of the rotation and of the purge, wherever it points. Each operation resolves them again, so that a link
/// planted after the start is refused too; a write happens at the pace of configuration changes, and the few system
/// calls this costs are nothing next to the copy it precedes.
struct BackupDirs {
    backups: PathBuf,
    protected: PathBuf,
    /// None when the configuration file does not exist yet: nothing to back up.
    config_file: Option<PathBuf>,
}

impl BackupDirs {
    fn open(root: &Path, config_path: &Path) -> Result<Self, StoreError> {
        let data = Self::resolved_under(root, MockStore::parent_of(config_path)?)?;
        let backups = Self::created_under(&data, data.join("backups"))?;
        let protected = Self::created_under(&data, backups.join("protected"))?;
        // A file that cannot be resolved does not exist yet (or cannot be read, which the write that follows reports).
        let config_file = match config_path.canonicalize() {
            Ok(resolved) => Some(Self::under(&data, config_path, resolved)?),
            Err(_) => None,
        };
        Ok(Self {
            backups,
            protected,
            config_file,
        })
    }

    /// `dir`, created when missing, resolved and checked to be under `root`.
    fn created_under(root: &Path, dir: PathBuf) -> Result<PathBuf, StoreError> {
        std::fs::create_dir_all(&dir).map_err(|e| StoreError::Io(e.to_string()))?;
        Self::resolved_under(root, &dir)
    }

    /// `path` resolved on disk and checked to be `root` or under it.
    fn resolved_under(root: &Path, path: &Path) -> Result<PathBuf, StoreError> {
        let resolved = path
            .canonicalize()
            .map_err(|e| StoreError::Io(format!("cannot resolve {}: {e}", path.display())))?;
        Self::under(root, path, resolved)
    }

    /// `resolved`, what `path` resolves to, when it is `root` or under it.
    fn under(root: &Path, path: &Path, resolved: PathBuf) -> Result<PathBuf, StoreError> {
        if resolved.starts_with(root) {
            Ok(resolved)
        } else {
            Err(StoreError::Io(format!(
                "{} resolves to {}, outside the data directory {}: a symbolic link there is refused",
                path.display(),
                resolved.display(),
                root.display()
            )))
        }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{WsdlMode, *};
    use crate::settings::vars;

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
                    script: None,
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
    async fn load_refuses_code_in_a_removed_script_block_and_leaves_the_file_as_it_was() {
        let dir = temp_dir();
        let file = MockStore::config_file(&dir);
        let mut yaml = serde_yaml::to_string(&sample_config()).unwrap();
        yaml = yaml.replace(
            "  script: null\n",
            "  script: null\n    pre_script: '1 + 1'\n",
        );
        assert!(yaml.contains("pre_script"), "{yaml}");
        std::fs::write(&file, &yaml).unwrap();

        let Err(error) = MockStore::load_or_init(&dir).await else {
            panic!("a configuration whose code would be dropped must not load");
        };
        assert!(error.to_string().contains("pre_script"), "{error}");
        assert_eq!(std::fs::read_to_string(&file).unwrap(), yaml);
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

    /// The files directly in `backups/`, by name (oldest first), with their content.
    fn backup_contents(dir: &Path) -> Vec<(String, String)> {
        let Ok(entries) = std::fs::read_dir(dir.join("backups")) else {
            return Vec::new();
        };
        let mut files: Vec<_> = entries
            .map(|e| e.unwrap().path())
            .filter(|p| p.is_file())
            .collect();
        files.sort();
        files
            .into_iter()
            .map(|p| {
                let name = p.file_name().unwrap().to_string_lossy().into_owned();
                (name, std::fs::read_to_string(&p).unwrap())
            })
            .collect()
    }

    /// A data directory whose configuration file holds `sample_config()`, and a store loaded from it.
    async fn store_on_sample(dir: &Path) -> (MockStore, String) {
        let yaml = serde_yaml::to_string(&sample_config()).unwrap();
        std::fs::write(MockStore::config_file(dir), &yaml).unwrap();
        (MockStore::load_or_init(dir).await.unwrap(), yaml)
    }

    fn renamed(name: &str) -> impl FnOnce(&mut MockConfig) {
        let name = name.to_string();
        move |cfg| cfg.services[0].name = name
    }

    #[tokio::test]
    async fn a_burst_of_changes_shares_one_backup_taken_before_the_first() {
        // An import merged service by service sent one request each: eight backups within 30 ms, which pushed every
        // older one out of the five kept.
        let dir = temp_dir();
        let (store, before) = store_on_sample(&dir).await;
        for i in 0..8 {
            store.update(renamed(&format!("svc-{i}"))).await.unwrap();
        }
        store.flush().await;

        let backups = backup_contents(&dir);
        assert_eq!(backups.len(), 1, "{backups:?}");
        assert_eq!(backups[0].1, before);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn a_change_that_changes_nothing_writes_nothing() {
        let dir = temp_dir();
        let (store, before) = store_on_sample(&dir).await;
        store.update(|_| {}).await.unwrap();
        store
            .try_update(|_| Ok::<(), ()>(()))
            .await
            .unwrap()
            .unwrap();
        store.replace(sample_config()).await.unwrap();
        store.flush().await;

        assert!(backup_contents(&dir).is_empty());
        assert_eq!(
            std::fs::read_to_string(MockStore::config_file(&dir)).unwrap(),
            before
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn a_backup_identical_to_the_newest_one_is_not_written_again() {
        // A burst that ends where it started leaves the file as the backup taken before it.
        let dir = temp_dir();
        let (store, before) = store_on_sample(&dir).await;
        store.update(renamed("elsewhere")).await.unwrap();
        store.update(renamed("svc-a")).await.unwrap();
        let mut replaced = sample_config();
        replaced.services[0].name = "replaced".into();
        store.replace(replaced).await.unwrap();
        store.flush().await;

        let backups = backup_contents(&dir);
        assert_eq!(backups.len(), 1, "{backups:?}");
        assert_eq!(backups[0].1, before);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn replacing_the_whole_configuration_backs_up_the_last_state_even_within_a_burst() {
        // Restore, import and reset replace everything: the state they overwrite is always kept, including a change
        // the background writer has not put on disk yet.
        let dir = temp_dir();
        let (store, before) = store_on_sample(&dir).await;
        store.update(renamed("just-changed")).await.unwrap();
        store.replace(MockConfig::empty()).await.unwrap();
        store.flush().await;

        let backups = backup_contents(&dir);
        assert_eq!(backups.len(), 2, "{backups:?}");
        assert_eq!(backups[0].1, before);
        assert!(backups[1].1.contains("just-changed"), "{}", backups[1].1);
        std::fs::remove_dir_all(&dir).ok();
    }

    // The rotation tests rely on the default count: tests never change the process environment (see crate::settings).
    #[tokio::test]
    async fn backup_rotation_keeps_max_n() {
        let dir = temp_dir();

        let store = MockStore::load_or_init(&dir).await.unwrap();
        for i in 0..DEFAULT_BACKUP_MAX_COUNT + 3 {
            let mut cfg = sample_config();
            cfg.services[0].name = format!("svc-{i}");
            store.replace(cfg).await.unwrap();
        }

        let backups_dir = dir.join("backups");
        let count = std::fs::read_dir(&backups_dir)
            .unwrap()
            .filter(|entry| entry.as_ref().unwrap().path().is_file())
            .count();
        assert_eq!(
            count, DEFAULT_BACKUP_MAX_COUNT,
            "backups should be capped at BACKUP_MAX_COUNT"
        );

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
        let dir = temp_dir();

        let store = MockStore::load_or_init(&dir).await.unwrap();
        store.replace(sample_config()).await.unwrap();
        store.backup_before_reset().await.unwrap();
        store.replace(MockConfig::empty()).await.unwrap();

        // Many writes after the reset, well beyond BACKUP_MAX_COUNT: the protected pre-reset backup must survive all
        // of them.
        for i in 0..DEFAULT_BACKUP_MAX_COUNT + 6 {
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
            normal_count, DEFAULT_BACKUP_MAX_COUNT,
            "normal backups still capped at BACKUP_MAX_COUNT"
        );

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
        assert_eq!(MockStore::backup_max_count_in(vars(&[])), 5);
    }

    #[test]
    fn backup_max_count_from_env() {
        let lookup = vars(&[("BACKUP_MAX_COUNT", "12")]);
        assert_eq!(MockStore::backup_max_count_in(lookup), 12);
    }

    #[test]
    fn backup_min_interval_defaults_to_a_minute_and_reads_the_environment() {
        assert_eq!(
            MockStore::backup_min_interval_in(vars(&[])),
            Duration::from_secs(60)
        );
        let lookup = vars(&[("BACKUP_MIN_INTERVAL_SECS", "0")]);
        assert_eq!(MockStore::backup_min_interval_in(lookup), Duration::ZERO);
    }

    #[tokio::test]
    async fn without_an_interval_every_change_backs_up_as_before() {
        let dir = temp_dir();
        let (store, _) = store_on_sample(&dir).await;
        let store = store.with_backup_min_interval(Duration::ZERO);
        for i in 0..3 {
            store.update(renamed(&format!("svc-{i}"))).await.unwrap();
        }
        store.flush().await;
        assert_eq!(backup_contents(&dir).len(), 3);
        std::fs::remove_dir_all(&dir).ok();
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
        store.replace(MockConfig::empty()).await.unwrap();
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

    fn duplicated_config_yaml() -> String {
        let mut config = sample_config();
        config.services.push(config.services[0].clone());
        serde_yaml::to_string(&config).unwrap()
    }

    #[tokio::test]
    async fn a_file_with_two_services_in_one_slot_loads_with_the_second_renamed() {
        let dir = temp_dir();
        std::fs::write(MockStore::config_file(&dir), duplicated_config_yaml()).unwrap();
        let store = MockStore::load_or_init(&dir).await.unwrap();
        let names: Vec<_> = store
            .snapshot()
            .await
            .services
            .iter()
            .map(|s| s.name.clone())
            .collect();
        assert_eq!(names, vec!["svc-a", "svc-a-2"]);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn a_backup_with_two_services_in_one_slot_restores_with_the_second_renamed() {
        let dir = temp_dir();
        let store = MockStore::load_or_init(&dir).await.unwrap();
        std::fs::create_dir_all(dir.join("backups")).unwrap();
        std::fs::write(
            dir.join("backups").join("mock-config-1-0.yaml"),
            duplicated_config_yaml(),
        )
        .unwrap();
        store
            .restore_from_backup("mock-config-1-0.yaml")
            .await
            .unwrap();
        let names: Vec<_> = store
            .snapshot()
            .await
            .services
            .iter()
            .map(|s| s.name.clone())
            .collect();
        assert_eq!(names, vec!["svc-a", "svc-a-2"]);
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
        let p = MockStore::data_path_in(vars(&[]));
        assert_eq!(p, PathBuf::from("./data"));
    }

    #[test]
    fn data_path_from_env() {
        let p = MockStore::data_path_in(vars(&[("DATA_PATH", "/mnt/pvc/mimicway")]));
        assert_eq!(p, PathBuf::from("/mnt/pvc/mimicway"));
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

    /// A failed disk write is logged and reported by writer_status(), and the background task keeps running: a later
    /// change still gets written. The failure here: a directory sits where the temporary file is written.
    #[tokio::test]
    async fn write_error_is_reported_and_task_keeps_running() {
        let dir = temp_dir();
        let path = MockStore::config_file(&dir);
        let store = MockStore::new(path.clone()).unwrap();
        let blocker = dir.join(".mock-config.yaml.tmp");
        std::fs::create_dir(&blocker).unwrap();

        store.replace(sample_config()).await.unwrap();
        store.flush().await;

        let status = store.writer_status();
        assert!(
            status.last_error.is_some(),
            "a write that cannot create its temporary file must be reported as an error"
        );
        assert!(status.last_write_ok_ms.is_none());

        std::fs::remove_dir(&blocker).unwrap();
        store.replace(sample_config()).await.unwrap();
        store.flush().await;

        let status2 = store.writer_status();
        assert!(
            status2.last_write_ok_ms.is_some(),
            "task must keep processing jobs after a prior write error"
        );
        assert!(path.exists());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn new_requires_an_existing_data_directory() {
        let dir = temp_dir();
        let err = MockStore::new(dir.join("missing").join("mock-config.yaml"))
            .err()
            .expect("a missing data directory must be refused");
        assert!(matches!(err, StoreError::Io(_)), "{err}");
        std::fs::remove_dir_all(&dir).ok();
    }

    /// On a shared volume, another process can plant a symbolic link at the place of `backups/`: the copies, and the
    /// deletions of the rotation, would follow it. Creating a symbolic link needs a privilege on Windows: Unix only.
    #[cfg(unix)]
    #[tokio::test]
    async fn backup_refuses_a_backups_directory_linked_outside_the_data_directory() {
        let dir = temp_dir();
        let elsewhere = temp_dir();
        let store = MockStore::load_or_init(&dir).await.unwrap();
        store.replace(sample_config()).await.unwrap();
        store.flush().await;

        let backups_dir = dir.join("backups");
        std::fs::remove_dir_all(&backups_dir).unwrap();
        std::os::unix::fs::symlink(&elsewhere, &backups_dir).unwrap();

        let err = store.replace(MockConfig::empty()).await.unwrap_err();
        assert!(
            err.to_string().contains("outside the data directory"),
            "{err}"
        );
        assert_eq!(
            std::fs::read_dir(&elsewhere).unwrap().count(),
            0,
            "nothing may be written through the link"
        );
        assert_eq!(
            store.snapshot().await.services.len(),
            1,
            "a refused change leaves the configuration as it was"
        );
        std::fs::remove_dir_all(&dir).ok();
        std::fs::remove_dir_all(&elsewhere).ok();
    }
}
