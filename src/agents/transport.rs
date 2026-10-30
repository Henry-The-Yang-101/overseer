use super::*;
use std::{
    io::{Read, Write},
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

const FRAME_LIMIT: usize = 64 * 1024;
pub(crate) const INPUT_LIMIT: u64 = 1024 * 1024;

#[derive(Serialize, Deserialize)]
struct Frame {
    version: u8,
    events: Vec<Event>,
}

pub fn default_socket() -> PathBuf {
    #[cfg(unix)]
    {
        PathBuf::from(format!("/tmp/overseer-{}/agents.sock", unsafe {
            libc::geteuid()
        }))
    }
    #[cfg(not(unix))]
    {
        std::env::temp_dir().join("overseer/agents.sock")
    }
}

/// Best effort, bounded by the CLI's process watchdog, with no output to the agent.
pub fn send(path: &Path, events: Vec<Event>) {
    #[cfg(unix)]
    {
        use std::os::unix::net::UnixStream;
        if events.is_empty() {
            return;
        }
        let Ok(mut bytes) = serde_json::to_vec(&Frame { version: 1, events }) else {
            return;
        };
        if bytes.len() >= FRAME_LIMIT {
            return;
        }
        bytes.push(b'\n');
        if let Ok(mut stream) = UnixStream::connect(path) {
            if !same_user(&stream) {
                return;
            }
            let _ = stream.set_write_timeout(Some(Duration::from_millis(100)));
            let _ = stream.write_all(&bytes);
        }
    }
    #[cfg(not(unix))]
    {
        let _ = (path, events);
    }
}

#[cfg(unix)]
struct Client {
    stream: std::os::unix::net::UnixStream,
    bytes: Vec<u8>,
    since: Instant,
}

#[cfg(unix)]
pub(crate) struct Receiver {
    listener: std::os::unix::net::UnixListener,
    clients: Vec<Client>,
    path: PathBuf,
    identity: (u64, u64),
    _lock: std::fs::File,
}

#[cfg(unix)]
impl Receiver {
    pub fn bind(path: &Path) -> anyhow::Result<Self> {
        use std::os::{
            fd::AsRawFd,
            unix::{
                fs::{DirBuilderExt, FileTypeExt, MetadataExt, OpenOptionsExt},
                net::UnixListener,
            },
        };
        anyhow::ensure!(path.is_absolute(), "Agent socket path must be absolute");
        let parent = path
            .parent()
            .ok_or_else(|| anyhow::anyhow!("Agent socket needs a parent directory"))?;
        if !parent.exists() {
            std::fs::DirBuilder::new()
                .recursive(true)
                .mode(0o700)
                .create(parent)?;
        }
        let meta = std::fs::symlink_metadata(parent)?;
        let uid = unsafe { libc::geteuid() };
        anyhow::ensure!(
            meta.is_dir() && meta.uid() == uid && meta.mode() & 0o077 == 0,
            "Agent socket directory must be owned by you and private (mode 700): {}",
            parent.display()
        );
        let lock = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW)
            .open(path.with_extension("lock"))?;
        anyhow::ensure!(
            unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0,
            "Another agent monitor already owns {}",
            path.display()
        );
        match std::fs::symlink_metadata(path) {
            Ok(m) => {
                anyhow::ensure!(
                    m.file_type().is_socket() && m.uid() == uid,
                    "Refusing to replace a non-owned socket path"
                );
                std::fs::remove_file(path)?;
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e.into()),
        }
        let listener = UnixListener::bind(path)?;
        listener.set_nonblocking(true)?;
        let m = std::fs::symlink_metadata(path)?;
        Ok(Self {
            listener,
            clients: Vec::new(),
            path: path.into(),
            identity: (m.dev(), m.ino()),
            _lock: lock,
        })
    }

    pub fn poll(&mut self, health: &mut Health) -> Vec<Event> {
        for _ in 0..16 {
            match self.listener.accept() {
                Ok((stream, _)) if self.clients.len() < 32 && same_user(&stream) => {
                    if stream.set_nonblocking(true).is_ok() {
                        self.clients.push(Client {
                            stream,
                            bytes: Vec::new(),
                            since: Instant::now(),
                        });
                    }
                }
                Ok(_) => {}
                Err(_) => break,
            }
        }
        let mut events = Vec::new();
        self.clients.retain_mut(|client| {
            let mut buf = [0; 4096];
            for _ in 0..16 {
                match client.stream.read(&mut buf) {
                    Ok(0) => {
                        health.malformed_records += 1;
                        return false;
                    }
                    Ok(n) => {
                        client.bytes.extend_from_slice(&buf[..n]);
                        if client.bytes.len() > FRAME_LIMIT {
                            health.malformed_records += 1;
                            return false;
                        }
                        if let Some(end) = client.bytes.iter().position(|b| *b == b'\n') {
                            match serde_json::from_slice::<Frame>(&client.bytes[..end]) {
                                Ok(frame) if frame.version == 1 && frame.events.len() <= 16 => {
                                    for mut event in frame.events {
                                        // Only this receiver assigns live observation time and transport.
                                        event.transport = Transport::Hook;
                                        event.replayed = false;
                                        event.at_ms = now_ms();
                                        events.push(event);
                                    }
                                }
                                _ => health.malformed_records += 1,
                            }
                            return false;
                        }
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
                    Err(_) => return false,
                }
            }
            client.since.elapsed() < Duration::from_secs(1)
        });
        events
    }
}

#[cfg(unix)]
impl Drop for Receiver {
    fn drop(&mut self) {
        use std::os::unix::fs::MetadataExt;
        if std::fs::symlink_metadata(&self.path).is_ok_and(|m| (m.dev(), m.ino()) == self.identity)
        {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}

#[cfg(unix)]
fn same_user(stream: &std::os::unix::net::UnixStream) -> bool {
    use std::os::fd::AsRawFd;
    #[cfg(any(target_os = "macos", target_os = "freebsd", target_os = "openbsd"))]
    {
        let (mut uid, mut gid) = (0, 0);
        unsafe {
            libc::getpeereid(stream.as_raw_fd(), &mut uid, &mut gid) == 0 && uid == libc::geteuid()
        }
    }
    #[cfg(target_os = "linux")]
    {
        let mut cred: libc::ucred = unsafe { std::mem::zeroed() };
        let mut len = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
        unsafe {
            libc::getsockopt(
                stream.as_raw_fd(),
                libc::SOL_SOCKET,
                libc::SO_PEERCRED,
                &mut cred as *mut _ as *mut _,
                &mut len,
            ) == 0
                && cred.uid == libc::geteuid()
        }
    }
    #[cfg(not(any(
        target_os = "macos",
        target_os = "freebsd",
        target_os = "openbsd",
        target_os = "linux"
    )))]
    {
        false
    }
}

#[cfg(not(unix))]
pub(crate) struct Receiver;
#[cfg(not(unix))]
impl Receiver {
    pub fn bind(_: &Path) -> anyhow::Result<Self> {
        anyhow::bail!(
            "Agent hooks currently require Unix sockets; use --no-hooks for log collection"
        )
    }
    pub fn poll(&mut self, _: &mut Health) -> Vec<Event> {
        Vec::new()
    }
}
