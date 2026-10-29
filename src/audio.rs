use serde::Deserialize;
use std::{
    sync::{Arc, Mutex},
    time::Instant,
};
#[derive(Clone, Deserialize)]
pub struct Frame {
    pub status: String,
    #[serde(default)]
    pub channels: Vec<Vec<f32>>,
}
impl Frame {
    pub fn level(&self, channel: usize) -> f32 {
        let Some(samples) = self.channels.get(channel).filter(|s| !s.is_empty()) else {
            return 0.;
        };
        let rms = (samples.iter().map(|v| v * v).sum::<f32>() / samples.len() as f32).sqrt();
        (rms * 4.).clamp(0., 1.)
    }
}
impl Default for Frame {
    fn default() -> Self {
        Self {
            status: "Starting audio capture…".into(),
            channels: vec![],
        }
    }
}
#[derive(Default)]
pub struct Capture {
    child: Option<std::process::Child>,
    frame: Arc<Mutex<(Frame, Option<Instant>)>>,
    attempted: bool,
}
impl Capture {
    pub fn update(&mut self, enabled: bool) {
        if !enabled {
            self.stop();
            return;
        }
        if self.attempted {
            return;
        }
        self.attempted = true;
        if let Err(e) = self.start() {
            self.frame.lock().unwrap().0 = Frame {
                status: e,
                channels: vec![],
            };
        }
    }
    #[cfg(target_os = "macos")]
    fn start(&mut self) -> Result<(), String> {
        use std::{
            io::{BufRead, BufReader},
            os::unix::fs::PermissionsExt,
            process::{Command, Stdio},
        };
        let path = crate::config::Config::path()
            .parent()
            .unwrap()
            .join("bin/overseer-audio");
        let binary = include_bytes!(concat!(env!("OUT_DIR"), "/overseer-audio"));
        std::fs::create_dir_all(path.parent().unwrap()).map_err(|e| e.to_string())?;
        if std::fs::read(&path).ok().as_deref() != Some(binary.as_slice()) {
            let tmp = path.with_extension(format!("{}.tmp", std::process::id()));
            std::fs::write(&tmp, binary).map_err(|e| e.to_string())?;
            std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o700))
                .map_err(|e| e.to_string())?;
            std::fs::rename(tmp, &path).map_err(|e| e.to_string())?;
        }
        let mut child = Command::new(path)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| e.to_string())?;
        let stdout = child.stdout.take().unwrap();
        let state = self.frame.clone();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                if let Ok(frame) = serde_json::from_str::<Frame>(&line) {
                    if !frame.channels.is_empty()
                        && (frame.channels.len() != 2
                            || frame
                                .channels
                                .iter()
                                .any(|c| c.len() != 64 || c.iter().any(|v| !v.is_finite())))
                    {
                        continue;
                    }
                    *state.lock().unwrap() = (frame, Some(Instant::now()));
                }
            }
        });
        self.child = Some(child);
        Ok(())
    }
    #[cfg(not(target_os = "macos"))]
    fn start(&mut self) -> Result<(), String> {
        Err("System audio capture requires macOS".into())
    }
    pub fn snapshot(&self) -> Frame {
        let guard = self.frame.lock().unwrap();
        let mut frame = guard.0.clone();
        if !frame.channels.is_empty() && guard.1.is_some_and(|t| t.elapsed().as_secs() >= 2) {
            frame.channels.clear();
            frame.status = "No audio frames · check macOS audio permission".into();
        }
        frame
    }
    fn stop(&mut self) {
        if !self.attempted && self.child.is_none() {
            return;
        }
        if let Some(mut child) = self.child.take() {
            child.stdin.take();
            let start = Instant::now();
            while start.elapsed().as_millis() < 300 {
                if child.try_wait().ok().flatten().is_some() {
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            if child.try_wait().ok().flatten().is_none() {
                let _ = child.kill();
            }
            let _ = child.wait();
        }
        self.attempted = false;
        self.frame = Arc::new(Mutex::new((Frame::default(), None)));
    }
}
impl Drop for Capture {
    fn drop(&mut self) {
        self.stop();
    }
}
