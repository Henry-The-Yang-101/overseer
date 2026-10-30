use super::{
    reducer::Reducer,
    tail::{self, Tailer},
    transport::Receiver,
    *,
};
use std::{
    fs::File,
    io::{Read, Seek, SeekFrom},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{
        Arc, RwLock,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, SyncSender},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

#[derive(Clone, Debug)]
pub struct Options {
    pub codex_homes: Vec<PathBuf>,
    pub socket: Option<PathBuf>,
    pub poll_interval: Duration,
    pub stale_after: Duration,
    pub cloud: bool,
    pub cloud_executable: PathBuf,
    pub cloud_home: PathBuf,
    pub cloud_interval: Duration,
}

impl Default for Options {
    fn default() -> Self {
        let home = agent_home(Provider::Codex);
        Self {
            codex_homes: vec![home.clone()],
            socket: Some(super::transport::default_socket()),
            poll_interval: Duration::from_millis(500),
            stale_after: Duration::from_secs(300),
            cloud: false,
            cloud_executable: "codex".into(),
            cloud_home: home,
            cloud_interval: Duration::from_secs(30),
        }
    }
}

/// Owns the background workers. Dropping it stops polling, reaps an in-flight cloud
/// command, and releases its socket. latest() is an O(1) immutable snapshot read.
pub struct Monitor {
    snapshot: Arc<RwLock<Arc<Snapshot>>>,
    stop: Arc<AtomicBool>,
    ingress: SyncSender<Event>,
    workers: Vec<JoinHandle<()>>,
}

impl Monitor {
    pub fn start(mut options: Options) -> anyhow::Result<Self> {
        anyhow::ensure!(
            options.codex_homes.len() <= 16,
            "At most 16 Codex homes can be monitored"
        );
        for home in &mut options.codex_homes {
            *home = canonical_home(home)?;
        }
        options.codex_homes.sort();
        options.codex_homes.dedup();
        options.cloud_home = canonical_home(&options.cloud_home)?;
        options.poll_interval = options
            .poll_interval
            .clamp(Duration::from_millis(50), Duration::from_secs(30));
        options.cloud_interval = options.cloud_interval.max(Duration::from_secs(10));
        let mut receiver = options.socket.as_deref().map(Receiver::bind).transpose()?;
        let snapshot = Arc::new(RwLock::new(Arc::new(Snapshot::default())));
        let stop = Arc::new(AtomicBool::new(false));
        let (ingress, input) = mpsc::sync_channel(512);
        let (cloud_tx, cloud_rx) = mpsc::sync_channel(2);
        let mut workers = Vec::new();
        if options.cloud {
            let opts = options.clone();
            let stopping = stop.clone();
            workers.push(
                thread::Builder::new()
                    .name("overseer-agent-cloud".into())
                    .spawn(move || {
                        while !stopping.load(Ordering::Relaxed) {
                            let result = poll_cloud(&opts, &stopping);
                            if cloud_tx.try_send(result).is_err()
                                && stopping.load(Ordering::Relaxed)
                            {
                                break;
                            }
                            let until = Instant::now() + opts.cloud_interval;
                            while Instant::now() < until && !stopping.load(Ordering::Relaxed) {
                                thread::park_timeout(Duration::from_millis(100));
                            }
                        }
                    })?,
            );
        }
        let state = snapshot.clone();
        let stopping = stop.clone();
        let spawn = thread::Builder::new()
            .name("overseer-agents".into())
            .spawn(move || {
                let mut reducer = Reducer::default();
                let mut health = Health::default();
                let mut tailer = Tailer::new(options.codex_homes);
                let mut discovery = Instant::now() - Duration::from_secs(10);
                let mut polling = Instant::now() - options.poll_interval;
                let mut publishing = Instant::now() - Duration::from_secs(1);
                let mut generation = 0;
                while !stopping.load(Ordering::Relaxed) {
                    let mut events: Vec<_> = input.try_iter().take(512).collect();
                    if let Some(receiver) = &mut receiver {
                        events.extend(receiver.poll(&mut health));
                    }
                    if discovery.elapsed() >= Duration::from_secs(10) {
                        events.extend(tailer.discover(&mut health));
                        discovery = Instant::now();
                    }
                    if polling.elapsed() >= options.poll_interval {
                        events.extend(tailer.poll(&mut health));
                        polling = Instant::now();
                    }
                    for result in cloud_rx.try_iter() {
                        match result {
                            Ok((cloud_events, truncated)) => {
                                health.last_cloud_poll_ms = Some(now_ms());
                                health.cloud_truncated = truncated;
                                events.extend(cloud_events);
                            }
                            Err(message) => tail::error(&mut health, message),
                        }
                    }
                    let changed = !events.is_empty();
                    for event in events {
                        reducer.apply(event, now_ms());
                    }
                    if changed || publishing.elapsed() >= Duration::from_secs(1) {
                        let mut next = reducer.snapshot(
                            now_ms(),
                            options.stale_after.as_millis() as u64,
                            health.clone(),
                        );
                        generation += 1;
                        next.revision = generation;
                        if let Ok(mut current) = state.write() {
                            *current = Arc::new(next);
                        }
                        publishing = Instant::now();
                    }
                    thread::park_timeout(Duration::from_millis(50));
                }
            });
        match spawn {
            Ok(worker) => workers.push(worker),
            Err(e) => {
                stop.store(true, Ordering::Relaxed);
                for w in workers {
                    w.thread().unpark();
                    let _ = w.join();
                }
                return Err(e.into());
            }
        }
        Ok(Self {
            snapshot,
            stop,
            ingress,
            workers,
        })
    }

    pub fn latest(&self) -> Arc<Snapshot> {
        self.snapshot
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    /// Bounded ingress for a future app-server transport or another typed adapter.
    pub fn try_ingest(&self, event: Event) -> Result<(), Box<mpsc::TrySendError<Event>>> {
        self.ingress.try_send(event).map_err(Box::new)
    }
}

impl Drop for Monitor {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        for worker in self.workers.drain(..) {
            worker.thread().unpark();
            let _ = worker.join();
        }
    }
}

pub fn agent_home(provider: Provider) -> PathBuf {
    let (var, fallback) = match provider {
        Provider::Codex => ("CODEX_HOME", ".codex"),
        Provider::Claude => ("CLAUDE_CONFIG_DIR", ".claude"),
    };
    std::env::var_os(var)
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(fallback)
        })
}

pub fn canonical_home(path: &Path) -> anyhow::Result<PathBuf> {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()?.join(path)
    };
    Ok(std::fs::canonicalize(&absolute).unwrap_or(absolute))
}

fn poll_cloud(options: &Options, stop: &AtomicBool) -> Result<(Vec<Event>, bool), String> {
    let mut events = Vec::new();
    let mut cursor = None;
    let mut seen_cursors = std::collections::HashSet::new();
    for _ in 0..10 {
        let value = cloud_page(options, cursor.as_deref(), stop)
            .map_err(|e| format!("Cloud polling unavailable: {e}"))?;
        if !value["tasks"].is_array() {
            return Err("Cloud listing did not contain a tasks array".into());
        }
        events.extend(decode::cloud(
            &options.cloud_home.to_string_lossy(),
            &value,
            now_ms(),
        ));
        cursor = text(&value, "cursor").filter(|s| !s.is_empty());
        if cursor.is_none() {
            return Ok((events, false));
        }
        if !seen_cursors.insert(cursor.clone()) {
            return Ok((events, true));
        }
    }
    Ok((events, true))
}

fn cloud_page(
    options: &Options,
    cursor: Option<&str>,
    stop: &AtomicBool,
) -> anyhow::Result<serde_json::Value> {
    // The upstream CLI creates error.log in its cwd. Isolate and remove it, too.
    let directory = tempfile::tempdir()?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o700))?;
    }
    let mut output = tempfile::tempfile()?;
    let mut command = Command::new(&options.cloud_executable);
    command
        .args(["cloud", "list", "--json", "--limit", "20"])
        .env("CODEX_HOME", &options.cloud_home)
        .current_dir(directory.path())
        .stdin(Stdio::null())
        .stdout(Stdio::from(output.try_clone()?))
        .stderr(Stdio::null());
    if let Some(cursor) = cursor {
        command.args(["--cursor", cursor]);
    }
    let mut child = command
        .spawn()
        .map_err(|_| anyhow::anyhow!("could not start Codex CLI"))?;
    let started = Instant::now();
    let result = (|| -> anyhow::Result<()> {
        loop {
            if stop.load(Ordering::Relaxed)
                || started.elapsed() > Duration::from_secs(10)
                || output.metadata()?.len() > 1024 * 1024
            {
                anyhow::bail!("request cancelled or exceeded its time/output limit");
            }
            if let Some(status) = child.try_wait()? {
                anyhow::ensure!(
                    status.success(),
                    "Codex CLI exited unsuccessfully ({status})"
                );
                return Ok(());
            }
            thread::sleep(Duration::from_millis(50));
        }
    })();
    if let Err(error) = result {
        let _ = child.kill();
        let _ = child.wait();
        return Err(error);
    }
    output.seek(SeekFrom::Start(0))?;
    read_cloud_json(output)
}

fn read_cloud_json(output: File) -> anyhow::Result<serde_json::Value> {
    let mut bytes = Vec::new();
    output.take(1024 * 1024 + 1).read_to_end(&mut bytes)?;
    anyhow::ensure!(bytes.len() <= 1024 * 1024, "cloud output too large");
    serde_json::from_slice(&bytes).map_err(|_| anyhow::anyhow!("invalid cloud JSON"))
}
