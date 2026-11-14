use std::{
    io::{Read, Seek},
    process::{Command, Stdio},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread,
    time::{Duration, Instant},
};

/// One bounded job per subsystem. Cancellation kills and reaps its subprocess.
pub struct Job<T> {
    rx: mpsc::Receiver<Result<T, String>>,
    cancel: Arc<AtomicBool>,
    thread: Option<thread::JoinHandle<()>>,
}
impl<T: Send + 'static> Job<T> {
    pub fn start(f: impl FnOnce(&AtomicBool) -> Result<T, String> + Send + 'static) -> Self {
        let (tx, rx) = mpsc::sync_channel(1);
        let cancel = Arc::new(AtomicBool::new(false));
        let flag = cancel.clone();
        let handle = thread::spawn(move || {
            let _ = tx.send(f(&flag));
        });
        Self {
            rx,
            cancel,
            thread: Some(handle),
        }
    }
    pub fn poll(&self) -> Option<Result<T, String>> {
        match self.rx.try_recv() {
            Ok(v) => Some(v),
            Err(mpsc::TryRecvError::Empty) => None,
            Err(_) => Some(Err("Background collector stopped".into())),
        }
    }
}
impl<T> Drop for Job<T> {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
        // Native jobs poll at 20 ms, and image decoding is bounded. Reap before returning.
        if let Some(handle) = self.thread.take() {
            let _ = handle.join();
        }
    }
}
pub fn output(
    command: &mut Command,
    timeout: Duration,
    cancel: &AtomicBool,
) -> Result<(bool, Vec<u8>), String> {
    let mut out = tempfile::tempfile().map_err(|e| e.to_string())?;
    let mut err = tempfile::tempfile().map_err(|e| e.to_string())?;
    let mut child = command
        .stdin(Stdio::null())
        .stdout(out.try_clone().map_err(|e| e.to_string())?)
        .stderr(err.try_clone().map_err(|e| e.to_string())?)
        .spawn()
        .map_err(|e| e.to_string())?;
    let start = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Err(e) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(e.to_string());
            }
            _ => {}
        }
        let too_large = out.metadata().map_or(true, |m| m.len() > 8 * 1024 * 1024)
            || err.metadata().map_or(true, |m| m.len() > 1024 * 1024);
        if cancel.load(Ordering::Relaxed) || start.elapsed() > timeout || too_large {
            let _ = child.kill();
            let _ = child.wait();
            return Err(if cancel.load(Ordering::Relaxed) {
                "Cancelled"
            } else if too_large {
                "Collector output exceeded limit"
            } else {
                "Timed out"
            }
            .into());
        }
        thread::sleep(Duration::from_millis(20));
    };
    out.rewind().map_err(|e| e.to_string())?;
    let mut bytes = vec![];
    out.take(8 * 1024 * 1024)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if !status.success() && bytes.is_empty() {
        err.rewind().map_err(|e| e.to_string())?;
        err.take(4096)
            .read_to_end(&mut bytes)
            .map_err(|e| e.to_string())?;
    }
    Ok((status.success(), bytes))
}
#[cfg(target_os = "macos")]
fn helper() -> Result<std::path::PathBuf, String> {
    use std::{os::unix::fs::PermissionsExt, sync::OnceLock};
    static PATH: OnceLock<Result<std::path::PathBuf, String>> = OnceLock::new();
    PATH.get_or_init(|| {
        let binary = include_bytes!(concat!(env!("OUT_DIR"), "/overseer-control"));
        let dir = crate::config::Config::path().parent().unwrap().join("bin");
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let path = dir.join("overseer-control");
        if std::fs::read(&path).ok().as_deref() != Some(binary.as_slice()) {
            let mut temp = tempfile::NamedTempFile::new_in(&dir).map_err(|e| e.to_string())?;
            std::io::Write::write_all(&mut temp, binary).map_err(|e| e.to_string())?;
            temp.as_file()
                .set_permissions(std::fs::Permissions::from_mode(0o700))
                .map_err(|e| e.to_string())?;
            temp.persist(&path).map_err(|e| e.to_string())?;
        }
        Ok(path)
    })
    .clone()
}
pub fn native<T: serde::de::DeserializeOwned>(
    args: &[String],
    cancel: &AtomicBool,
) -> Result<T, String> {
    #[cfg(target_os = "macos")]
    {
        let (ok, bytes) = output(
            Command::new(helper()?).args(args),
            Duration::from_secs(6),
            cancel,
        )?;
        let v: serde_json::Value = serde_json::from_slice(&bytes)
            .map_err(|e| format!("System metadata unavailable: {e}"))?;
        if !ok || v.get("error").is_some() {
            return Err(v["error"]
                .as_str()
                .unwrap_or("System request failed")
                .into());
        }
        serde_json::from_value(v).map_err(|e| e.to_string())
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (args, cancel);
        Err("Control Center requires macOS".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cancellation_reaps_child_and_does_not_wait_for_command_timeout() {
        let job = Job::start(|cancel| {
            output(
                Command::new("/bin/sleep").arg("30"),
                Duration::from_secs(40),
                cancel,
            )
        });
        std::thread::sleep(Duration::from_millis(50));
        let start = Instant::now();
        drop(job);
        assert!(start.elapsed() < Duration::from_secs(2));
        let cancelled = AtomicBool::new(true);
        assert_eq!(
            output(
                Command::new("/bin/sleep").arg("30"),
                Duration::from_secs(40),
                &cancelled
            )
            .unwrap_err(),
            "Cancelled"
        );
    }
}
