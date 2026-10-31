//! Opt-in macOS Now Playing listener. Nothing starts until `Listener::start` is called.
//! Requires `media-control` (https://github.com/ungive/media-control) on PATH.
use serde::{Deserialize, Serialize};
use std::{
    ffi::OsStr,
    io::{self, BufRead, BufReader, Read},
    process::{Child, Command, Stdio},
    sync::{Arc, Mutex},
    thread::{self, JoinHandle},
};

/// Fields are player-reported; elapsed time is anchored at `timestamp`, not receipt time.
/// A session may have no title, artist, duration, or known playback state.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Metadata {
    pub bundle_identifier: String,
    pub title: Option<String>,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub playing: Option<bool>,
    pub duration: Option<f64>,
    pub elapsed_time: Option<f64>,
    pub playback_rate: Option<f64>,
    pub timestamp: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "status", content = "data", rename_all = "snake_case")]
pub enum State {
    Waiting,
    Idle,
    Active(Metadata),
    Failed(String),
}

/// Retains only the latest state, without storing listening history or audio.
pub struct Listener {
    child: Child,
    reader: Option<JoinHandle<()>>,
    state: Arc<Mutex<State>>,
}

impl Listener {
    pub fn start() -> io::Result<Self> {
        Self::start_with("media-control")
    }

    pub fn start_with(program: impl AsRef<OsStr>) -> io::Result<Self> {
        if !cfg!(target_os = "macos") {
            return Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "Now Playing currently requires macOS",
            ));
        }
        let mut command = Command::new(program);
        // Full snapshots avoid carrying an old player's metadata into a new session.
        // Missing titles are useful for browser audio and untagged local files.
        command.args([
            "stream",
            "--no-diff",
            "--no-artwork",
            "--allow-missing-title",
        ]);
        Self::spawn(command)
    }

    fn spawn(mut command: Command) -> io::Result<Self> {
        let mut child = command
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()?;
        let stdout = child.stdout.take().expect("piped stdout");
        let state = Arc::new(Mutex::new(State::Waiting));
        let latest = state.clone();
        let reader = thread::Builder::new()
            .name("now-playing".into())
            .spawn(move || {
                let mut input = BufReader::new(stdout);
                let mut line = Vec::new();
                loop {
                    line.clear();
                    // Artwork is disabled; cap each record so a bad helper cannot
                    // grow memory indefinitely. There is no unbounded event queue.
                    const LIMIT: usize = 64 * 1024;
                    let next = match input
                        .by_ref()
                        .take((LIMIT + 1) as u64)
                        .read_until(b'\n', &mut line)
                    {
                        Ok(0) => Err("Now Playing helper disconnected".into()),
                        Ok(_) if line.len() > LIMIT => {
                            Err("Now Playing record exceeded 64 KiB".into())
                        }
                        Ok(_) => parse(&line),
                        Err(error) => Err(format!("Could not read Now Playing stream: {error}")),
                    };
                    let failed = next.is_err();
                    *latest.lock().unwrap() = next.unwrap_or_else(State::Failed);
                    if failed {
                        break;
                    }
                }
            });
        match reader {
            Ok(reader) => Ok(Self {
                child,
                reader: Some(reader),
                state,
            }),
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                Err(error)
            }
        }
    }

    /// Idle is a valid empty OS session; Failed means metadata is unavailable.
    pub fn snapshot(&self) -> State {
        self.state.lock().unwrap().clone()
    }
}

impl Drop for Listener {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        if let Some(reader) = self.reader.take() {
            let _ = reader.join();
        }
    }
}

fn parse(line: &[u8]) -> Result<State, String> {
    #[derive(Deserialize)]
    struct Event {
        #[serde(rename = "type")]
        kind: String,
        diff: bool,
        payload: serde_json::Map<String, serde_json::Value>,
    }
    let event: Event =
        serde_json::from_slice(line).map_err(|e| format!("Invalid Now Playing event: {e}"))?;
    if event.kind != "data" || event.diff {
        return Err("Expected a full Now Playing data snapshot (--no-diff)".into());
    }
    if event.payload.is_empty() {
        return Ok(State::Idle);
    }
    let metadata: Metadata = serde_json::from_value(event.payload.into())
        .map_err(|e| format!("Invalid Now Playing metadata: {e}"))?;
    if metadata.bundle_identifier.is_empty() {
        return Err("Now Playing session has an empty bundle identifier".into());
    }
    Ok(State::Active(metadata))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn full_snapshots_handle_sparse_sessions_and_reject_diffs() {
        let first = parse(
            br#"{
            "type":"data", "diff":false,
            "payload":{
                "bundleIdentifier":"com.spotify.client", "playing":true,
                "title":"Song", "artist":"Artist", "duration":120,
                "elapsedTime":15, "timestamp":"2026-10-30T12:00:00Z"
            }
        }"#,
        )
        .unwrap();
        let State::Active(metadata) = first else {
            panic!("expected active session")
        };
        assert_eq!(metadata.artist.as_deref(), Some("Artist"));
        assert_eq!(metadata.elapsed_time, Some(15.));
        let State::Active(next) = parse(
            br#"{
            "type":"data", "diff":false,
            "payload":{"bundleIdentifier":"com.apple.QuickTimePlayerX"}
        }"#,
        )
        .unwrap() else {
            panic!("expected sparse session")
        };
        assert_eq!(next.title, None);
        assert_eq!(next.artist, None);
        assert_eq!(next.playing, None);
        assert_eq!(
            parse(br#"{"type":"data","diff":false,"payload":{}}"#).unwrap(),
            State::Idle
        );
        for invalid in [
            br#"{"type":"data","diff":true,"payload":{"playing":false}}"#.as_slice(),
            br#"{"type":"data","diff":false,"payload":{"title":"no source"}}"#,
            br#"{"type":"data","diff":false,"payload":null}"#,
            b"not json",
        ] {
            assert!(parse(invalid).is_err());
        }
    }

    #[cfg(unix)]
    #[test]
    fn listener_reports_disconnect_and_reaps_helper_on_drop() {
        use std::time::{Duration, Instant};
        let mut command = Command::new("/bin/sh");
        command.args([
            "-c",
            "printf '%s\\n' '{\"type\":\"data\",\"diff\":false,\"payload\":{}}'",
        ]);
        let listener = Listener::spawn(command).unwrap();
        let deadline = Instant::now() + Duration::from_secs(3);
        while !matches!(listener.snapshot(), State::Failed(_)) && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(5));
        }
        assert!(matches!(listener.snapshot(), State::Failed(_)));
        drop(listener);

        let mut command = Command::new("/bin/sh");
        command.args(["-c", "exec sleep 30"]);
        let listener = Listener::spawn(command).unwrap();
        let pid = listener.child.id() as libc::pid_t;
        drop(listener);
        let mut status = 0;
        // SAFETY: status is a valid output pointer; waitpid does not dereference pid.
        assert_eq!(
            unsafe { libc::waitpid(pid, &mut status, libc::WNOHANG) },
            -1
        );
        assert_eq!(
            io::Error::last_os_error().raw_os_error(),
            Some(libc::ECHILD)
        );
    }
}
