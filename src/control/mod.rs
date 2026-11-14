//! Control Center is opt-in, live-only, and independent of the telemetry history.
//! Radio collectors have no mutation interface. Only explicit audio/media actions write state.
mod data;
pub mod models;
mod worker;
use crate::now_playing::{Listener, State};
pub use data::*;
use std::{
    process::Command,
    sync::Arc,
    time::{Duration, Instant},
};
use worker::{Job, native, output};

enum Snapshot {
    Wifi(Box<Wifi>),
    Bluetooth(Bluetooth),
    Displays(Vec<Display>),
    Sound(Sound),
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ControlAction {
    SpeedTest,
    Refresh,
    Volume(i8),
    Mute,
    Input,
    Output,
    Activate,
    Previous,
    TogglePlayback,
    Next,
    Seek(i16),
    SeekPercent(u16),
    Capture,
}

pub struct ControlCenter {
    pub tab: Tab,
    pub wifi: Wifi,
    pub bluetooth: Bluetooth,
    pub displays: Vec<Display>,
    pub sound: Sound,
    pub device_index: usize,
    pub display_index: usize,
    pub audio_index: usize,
    pub input: bool,
    pub device_since: Instant,
    pub latency: Latency,
    pub ping_error: Option<String>,
    pub speed: Option<SpeedResult>,
    pub speed_started: Option<Instant>,
    pub speed_error: Option<String>,
    pub now: State,
    pub artwork: Option<Arc<image::RgbImage>>,
    pub artwork_error: Option<String>,
    pub errors: [Option<String>; 5],
    pub updated: [Option<Instant>; 5],
    pub message: Option<String>,
    pub model: Option<Arc<models::Mesh>>,
    pub model_note: String,
    snapshot: Option<Job<Snapshot>>,
    snapshot_tab: Tab,
    last_snapshot: Option<Instant>,
    ping: Option<Job<Option<f64>>>,
    last_ping: Option<Instant>,
    speed_job: Option<Job<SpeedResult>>,
    action: Option<Job<Sound>>,
    media_action: Option<Job<()>>,
    listener: Option<Listener>,
    media_attempted: bool,
    last_media: Option<Instant>,
    art_source: Option<Arc<str>>,
    art_job: Option<Job<Arc<image::RgbImage>>>,
    model_job: Option<Job<Arc<models::Mesh>>>,
    model_key: String,
    started: Instant,
}
impl Default for ControlCenter {
    fn default() -> Self {
        Self {
            tab: Tab::Wifi,
            wifi: Wifi::default(),
            bluetooth: Bluetooth::default(),
            displays: vec![],
            sound: Sound::default(),
            device_index: 0,
            display_index: 0,
            audio_index: 0,
            input: false,
            device_since: Instant::now(),
            latency: Latency::default(),
            ping_error: None,
            speed: None,
            speed_started: None,
            speed_error: None,
            now: State::Waiting,
            artwork: None,
            artwork_error: None,
            errors: std::array::from_fn(|_| None),
            updated: [None; 5],
            message: None,
            model: None,
            model_note: String::new(),
            snapshot: None,
            snapshot_tab: Tab::Wifi,
            last_snapshot: None,
            ping: None,
            last_ping: None,
            speed_job: None,
            action: None,
            media_action: None,
            listener: None,
            media_attempted: false,
            last_media: None,
            art_source: None,
            art_job: None,
            model_job: None,
            model_key: String::new(),
            started: Instant::now(),
        }
    }
}
impl ControlCenter {
    pub fn change_tab(&mut self, tab: Tab) {
        if tab != self.tab {
            self.tab = tab;
            self.last_snapshot = None;
            self.message = None;
        }
    }
    pub fn selected_device(&self) -> Option<&Device> {
        self.bluetooth.devices.get(self.device_index)
    }
    pub fn audio_devices(&self) -> Vec<&AudioDevice> {
        self.sound
            .devices
            .iter()
            .filter(|d| {
                if self.input {
                    d.inputs > 0
                } else {
                    d.outputs > 0
                }
            })
            .collect()
    }
    pub fn move_selection(&mut self, n: i32) {
        let (index, len) = match self.tab {
            Tab::Bluetooth => (&mut self.device_index, self.bluetooth.devices.len()),
            Tab::Display => (&mut self.display_index, self.displays.len()),
            Tab::Sound => {
                let len = self.audio_devices().len();
                (&mut self.audio_index, len)
            }
            _ => return,
        };
        if len > 0 {
            *index = (*index as i32 + n).rem_euclid(len as i32) as usize;
            self.device_since = Instant::now();
        }
    }
    pub fn select_device(&mut self, i: usize) {
        self.device_index = i.min(self.bluetooth.devices.len().saturating_sub(1));
        self.device_since = Instant::now();
    }
    pub fn tick(&mut self, active: bool, demo: bool) {
        if !active || demo {
            self.snapshot = None;
            self.ping = None;
            self.listener = None;
            self.media_attempted = false;
            self.last_snapshot = None;
            if !active && self.speed_job.take().is_some() {
                self.speed_started = None;
                self.speed_error = Some("Cancelled on leaving Control Center".into());
            }
            if demo {
                self.action = None;
                self.media_action = None;
                self.speed_job = None;
                self.speed_started = None;
            }
            return;
        }
        if let Some(result) = self.action.as_ref().and_then(Job::poll) {
            self.action = None;
            match result {
                Ok(s) => {
                    self.sound = s;
                    self.updated[Tab::Sound.index()] = Some(Instant::now());
                    self.message = Some("Audio updated".into());
                }
                Err(e) => self.message = Some(e),
            }
            self.last_snapshot = None;
        }
        if let Some(result) = self.media_action.as_ref().and_then(Job::poll) {
            self.media_action = None;
            if let Err(e) = result {
                self.message = Some(e);
            }
        }
        if self.snapshot_tab != self.tab {
            self.snapshot = None;
            self.last_snapshot = None;
        }
        if let Some(result) = self.snapshot.as_ref().and_then(Job::poll) {
            self.snapshot = None;
            match result {
                Ok(s) => {
                    self.errors[self.tab.index()] = None;
                    self.updated[self.tab.index()] = Some(Instant::now());
                    match s {
                        Snapshot::Wifi(w) => {
                            if self.wifi.interface != w.interface
                                || self.wifi.bssid != w.bssid
                                || self.wifi.connected != w.connected
                                || self.wifi.ipv4 != w.ipv4
                            {
                                self.ping = None;
                                self.latency = Latency::default();
                                self.last_ping = None;
                                self.speed = None;
                                if self.speed_job.take().is_some() {
                                    self.speed_started = None;
                                    self.speed_error =
                                        Some("Connection changed; test cancelled".into());
                                }
                            }
                            self.wifi = *w;
                        }
                        Snapshot::Bluetooth(bt) => {
                            let old = self.selected_device().map(|d| (d.id.clone(), d.connected));
                            if let Some((id, connected)) = &old {
                                self.device_index =
                                    bt.devices.iter().position(|d| d.id == *id).unwrap_or(0);
                                if bt
                                    .devices
                                    .get(self.device_index)
                                    .is_some_and(|d| d.connected != *connected)
                                {
                                    self.device_since = Instant::now();
                                }
                            }
                            self.bluetooth = bt;
                        }
                        Snapshot::Displays(ds) => {
                            let id = self.displays.get(self.display_index).map(|d| d.id);
                            self.display_index =
                                ds.iter().position(|d| Some(d.id) == id).unwrap_or(0);
                            self.displays = ds;
                        }
                        Snapshot::Sound(s) => {
                            let id = self.audio_devices().get(self.audio_index).map(|d| d.id);
                            self.sound = s;
                            self.audio_index = self
                                .audio_devices()
                                .iter()
                                .position(|d| Some(d.id) == id)
                                .unwrap_or(0);
                        }
                    }
                }
                Err(e) => self.errors[self.tab.index()] = Some(e),
            }
        }
        let interval = if self.tab == Tab::Bluetooth { 5 } else { 3 };
        if self.snapshot.is_none()
            && self.action.is_none()
            && self
                .last_snapshot
                .is_none_or(|t| t.elapsed() >= Duration::from_secs(interval))
        {
            let tab = self.tab;
            self.snapshot_tab = tab;
            self.last_snapshot = Some(Instant::now());
            self.snapshot = Some(Job::start(move |cancel| match tab {
                Tab::Wifi => native(&["wifi".into()], cancel).map(Snapshot::Wifi),
                Tab::Display => native(&["display".into()], cancel).map(Snapshot::Displays),
                Tab::Sound | Tab::NowPlaying => {
                    native(&["sound".into()], cancel).map(Snapshot::Sound)
                }
                Tab::Bluetooth => {
                    if !cfg!(target_os = "macos") {
                        return Err("Bluetooth inventory requires macOS".into());
                    }
                    let (ok, bytes) = output(
                        Command::new("/usr/sbin/system_profiler").args([
                            "SPBluetoothDataType",
                            "-json",
                            "-timeout",
                            "4",
                        ]),
                        Duration::from_secs(6),
                        cancel,
                    )?;
                    if !ok {
                        return Err("macOS Bluetooth inventory unavailable".into());
                    }
                    bluetooth_json(&bytes).map(Snapshot::Bluetooth)
                }
            }));
        }
        if self.tab == Tab::Wifi {
            if let Some(result) = self.ping.as_ref().and_then(Job::poll) {
                self.ping = None;
                match result {
                    Ok(ms) => {
                        self.ping_error = None;
                        self.latency.push(Ping {
                            at: self.started.elapsed().as_secs_f64(),
                            ms,
                            loaded: self.speed_started.is_some(),
                        });
                    }
                    Err(e) => self.ping_error = Some(e),
                }
            }
            if self.wifi.connected
                && self.ping.is_none()
                && self
                    .last_ping
                    .is_none_or(|t| t.elapsed() >= Duration::from_secs(1))
            {
                let interface = self.wifi.interface.clone();
                self.last_ping = Some(Instant::now());
                self.ping = Some(Job::start(move |cancel| {
                    let (ok, bytes) = output(
                        Command::new("/sbin/ping").args([
                            "-n", "-c", "1", "-W", "900", "-t", "2", "-b", &interface, "1.1.1.1",
                        ]),
                        Duration::from_secs(3),
                        cancel,
                    )?;
                    let text = String::from_utf8_lossy(&bytes);
                    if let Some(time) = text
                        .split("time=")
                        .nth(1)
                        .and_then(|s| s.split_whitespace().next())
                        .and_then(|s| s.parse::<f64>().ok())
                        .filter(|v| v.is_finite() && *v >= 0.)
                    {
                        return Ok(Some(time));
                    }
                    if !ok && text.contains("packets transmitted") {
                        Ok(None)
                    } else {
                        Err("ICMP probe unavailable; no loss estimate".into())
                    }
                }));
            }
        } else {
            self.ping = None;
        }
        if let Some(result) = self.speed_job.as_ref().and_then(Job::poll) {
            self.speed_job = None;
            self.speed_started = None;
            match result {
                Ok(result) => {
                    self.speed = Some(result);
                    self.speed_error = None;
                }
                Err(e) => self.speed_error = Some(e),
            }
        }
        if self.tab == Tab::NowPlaying {
            self.tick_media();
        } else {
            self.listener = None;
            self.media_attempted = false;
        }
        if self.tab == Tab::Bluetooth {
            self.tick_model();
        }
    }
    fn tick_media(&mut self) {
        if !self.media_attempted {
            self.media_attempted = true;
            match Listener::start_with_artwork() {
                Ok(listener) => {
                    self.listener = Some(listener);
                    self.now = State::Waiting;
                }
                Err(e) => {
                    self.now = State::Failed(format!(
                        "media-control unavailable: {e}. Install with brew install media-control"
                    ))
                }
            }
        }
        if self
            .last_media
            .is_none_or(|t| t.elapsed() >= Duration::from_millis(250))
        {
            self.last_media = Some(Instant::now());
            if let Some(listener) = &self.listener {
                self.now = listener.snapshot();
            }
            let source = match &self.now {
                State::Active(m) => m.artwork_data.clone(),
                _ => None,
            };
            if source != self.art_source {
                self.art_job = None;
                self.artwork = None;
                self.artwork_error = None;
                self.art_source = source.clone();
                if let Some(source) = source {
                    self.art_job = Some(Job::start(move |_| {
                        use base64::Engine;
                        let bytes = base64::engine::general_purpose::STANDARD
                            .decode(source.as_bytes())
                            .map_err(|e| format!("Artwork: {e}"))?;
                        let mut reader = image::ImageReader::new(std::io::Cursor::new(bytes))
                            .with_guessed_format()
                            .map_err(|e| e.to_string())?;
                        let mut limits = image::Limits::default();
                        limits.max_image_width = Some(4096);
                        limits.max_image_height = Some(4096);
                        limits.max_alloc = Some(96 * 1024 * 1024);
                        reader.limits(limits);
                        let image = reader
                            .decode()
                            .map_err(|e| format!("Artwork unavailable: {e}"))?;
                        Ok(Arc::new(image.thumbnail(256, 256).to_rgb8()))
                    }));
                }
            }
        }
        if let Some(result) = self.art_job.as_ref().and_then(Job::poll) {
            self.art_job = None;
            match result {
                Ok(image) => self.artwork = Some(image),
                Err(e) => self.artwork_error = Some(e),
            }
        }
    }
    fn tick_model(&mut self) {
        let Some(device) = self.selected_device() else {
            return;
        };
        let key = device.model().to_owned();
        if key != self.model_key {
            self.model_key = key.clone();
            self.model = None;
            self.model_job = None;
            self.model_note = "Loading device geometry…".into();
            self.model_job = Some(Job::start(move |_| Ok(Arc::new(models::load(&key)))));
        }
        if let Some(result) = self.model_job.as_ref().and_then(Job::poll) {
            self.model_job = None;
            match result {
                Ok(mesh) => {
                    self.model_note = mesh.source.clone();
                    self.model = Some(mesh);
                }
                Err(e) => self.model_note = e,
            }
        }
    }
    pub fn prepare_demo_model(&mut self) {
        if let Some(device) = self.selected_device() {
            let key = device.model();
            if self.model_key != key || self.model.is_none() {
                self.model_key = key.into();
                self.model = Some(Arc::new(models::load(key)));
            }
        }
    }
    pub fn act(&mut self, action: ControlAction, demo: bool) {
        if matches!(action, ControlAction::Input | ControlAction::Output) {
            self.input = action == ControlAction::Input;
            self.audio_index = 0;
            return;
        }
        if action == ControlAction::Refresh {
            self.last_snapshot = None;
            self.media_attempted = false;
            self.listener = None;
            return;
        }
        if demo {
            self.message = Some("Demo · hardware and playback controls are disabled".into());
            return;
        }
        self.message = None;
        match action {
            ControlAction::SpeedTest => {
                if self.speed_job.take().is_some() {
                    self.speed_started = None;
                    self.speed_error = Some("Cancelled".into());
                    return;
                }
                if !self.wifi.connected || self.wifi.interface.is_empty() {
                    self.speed_error = Some("Connect to Wi-Fi before testing".into());
                    return;
                }
                self.speed = None;
                self.speed_error = None;
                self.speed_started = Some(Instant::now());
                let interface = self.wifi.interface.clone();
                self.speed_job = Some(Job::start(move |cancel| {
                    let (ok, bytes) = output(
                        Command::new("/usr/bin/networkQuality")
                            .args(["-c", "-M", "12", "-I", &interface]),
                        Duration::from_secs(20),
                        cancel,
                    )?;
                    if !ok {
                        return Err(format!(
                            "Speed test failed: {}",
                            String::from_utf8_lossy(&bytes)
                                .chars()
                                .take(180)
                                .collect::<String>()
                        ));
                    }
                    speed_json(&bytes)
                }));
            }
            ControlAction::Volume(step) => self.audio_action(vec![
                "volume".into(),
                (step.signum() as f32 * 0.05).to_string(),
            ]),
            ControlAction::Mute => self.audio_action(vec!["mute".into()]),
            ControlAction::Activate => {
                let id = self.audio_devices().get(self.audio_index).map(|d| d.id);
                if let Some(id) = id {
                    self.audio_action(vec![
                        if self.input { "input" } else { "output" }.into(),
                        id.to_string(),
                    ]);
                }
            }
            ControlAction::Previous
            | ControlAction::TogglePlayback
            | ControlAction::Next
            | ControlAction::Seek(_)
            | ControlAction::SeekPercent(_) => {
                if self.media_action.is_some() {
                    return;
                }
                let State::Active(metadata) = &self.now else {
                    self.message = Some("No system media session to control".into());
                    return;
                };
                let bundle = metadata.bundle_identifier.clone();
                let args: Vec<String> = match action {
                    ControlAction::Previous => vec!["previous-track".into()],
                    ControlAction::Next => vec!["next-track".into()],
                    ControlAction::TogglePlayback => vec!["toggle-play-pause".into()],
                    ControlAction::Seek(delta) => {
                        let (Some(position), Some(duration)) =
                            (metadata.position(), metadata.duration_seconds())
                        else {
                            self.message = Some("This stream has no seekable timeline".into());
                            return;
                        };
                        vec![
                            "seek".into(),
                            (position + delta as f64).clamp(0., duration).to_string(),
                        ]
                    }
                    ControlAction::SeekPercent(percent) => {
                        let Some(duration) = metadata.duration_seconds() else {
                            self.message = Some("No seekable timeline".into());
                            return;
                        };
                        vec![
                            "seek".into(),
                            (duration * percent.min(1000) as f64 / 1000.).to_string(),
                        ]
                    }
                    _ => unreachable!(),
                };
                self.media_action = Some(Job::start(move |cancel| {
                    let (ok, bytes) = output(
                        Command::new("media-control").args([
                            "get",
                            "--no-artwork",
                            "--allow-missing-title",
                        ]),
                        Duration::from_secs(3),
                        cancel,
                    )?;
                    let current: serde_json::Value =
                        serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
                    if !ok || current["bundleIdentifier"].as_str() != Some(&bundle) {
                        return Err("Active player changed; retry the control".into());
                    }
                    let (ok, _) = output(
                        Command::new("media-control").args(args),
                        Duration::from_secs(3),
                        cancel,
                    )?;
                    if ok {
                        Ok(())
                    } else {
                        Err("The player did not accept this control".into())
                    }
                }));
            }
            _ => {}
        }
    }
    fn audio_action(&mut self, mut args: Vec<String>) {
        if self.action.is_some() {
            self.message = Some("Audio update pending…".into());
            return;
        }
        self.snapshot = None;
        args.insert(0, "audio".into());
        self.action = Some(Job::start(move |cancel| native(&args, cancel)));
    }
    #[allow(clippy::field_reassign_with_default)]
    pub fn demo() -> Self {
        let mut c = Self::default();
        c.wifi = Wifi {
            powered: true,
            connected: true,
            interface: "en0".into(),
            status: "Connected".into(),
            ssid: Some("Studio network".into()),
            bssid: Some("02:00:00:00:00:01".into()),
            rssi: Some(-48),
            noise: Some(-94),
            tx_mbps: Some(1200.),
            channel: Some(149),
            band: Some("5 GHz".into()),
            width: Some(80),
            phy: Some("802.11ax".into()),
            security: Some("WPA3 Personal".into()),
            ipv4: vec!["192.0.2.10".into()],
            gateway: Some("192.0.2.1".into()),
            dns: vec!["1.1.1.1".into()],
            primary_interface: Some("en0".into()),
            ..Wifi::default()
        };
        for i in 0..90 {
            c.latency.push(Ping {
                at: i as f64,
                ms: if i == 37 {
                    None
                } else {
                    Some(12. + (i as f64 * 0.31).sin().abs() * 8. + if i == 58 { 35. } else { 0. })
                },
                loaded: false,
            });
        }
        c.speed = Some(SpeedResult {
            down: Some(487.4),
            up: Some(95.8),
            idle_ms: Some(16.2),
            responsiveness: Some(1243.),
            interface: Some("en0".into()),
        });
        c.bluetooth = Bluetooth {
            powered: Some(true),
            devices: [
                "AirPods Pro",
                "AULA F99 Pro",
                "Bluetooth Mouse 4.0",
                "Galaxy Buds FE",
                "iPad Pro",
                "iPhone",
                "Keychron K8 Pro",
                "MX Master 3S",
                "WH-1000XM5",
            ]
            .into_iter()
            .enumerate()
            .map(|(i, name)| Device {
                id: format!("demo-{i}"),
                name: name.into(),
                connected: [0, 6, 7].contains(&i),
                kind: "Known device".into(),
                left: (i == 0).then_some(94),
                right: (i == 0).then_some(96),
                case: (i == 0).then_some(68),
                firmware: Some("Demo firmware".into()),
                ..Device::default()
            })
            .collect(),
        };
        c.model = Some(Arc::new(models::load("airpods-pro")));
        c.model_key = "airpods-pro".into();
        c.model_note = c.model.as_ref().unwrap().source.clone();
        c.displays = vec![
            Display {
                id: 1,
                name: "Built-in Retina Display".into(),
                width: 1512.,
                height: 982.,
                pixel_width: 3024,
                pixel_height: 1964,
                scale: 2.,
                builtin: true,
                main: true,
                refresh: Some(120.),
                max_refresh: 120,
                brightness: Some(0.72),
                color_space: "Display P3".into(),
                edr: 16.,
                ..Display::default()
            },
            Display {
                id: 2,
                name: "Studio Display".into(),
                x: 1512.,
                y: -260.,
                width: 2560.,
                height: 1440.,
                pixel_width: 5120,
                pixel_height: 2880,
                scale: 2.,
                refresh: Some(60.),
                max_refresh: 60,
                brightness: Some(0.64),
                color_space: "Display P3".into(),
                ..Display::default()
            },
        ];
        c.sound.devices = vec![
            AudioDevice {
                id: 1,
                name: "MacBook Pro Speakers".into(),
                outputs: 2,
                default_output: true,
                can_volume: true,
                can_mute: true,
                sample_rate: Some(48000.),
                volume: Some(0.35),
                muted: Some(false),
                ..AudioDevice::default()
            },
            AudioDevice {
                id: 2,
                name: "AirPods Pro".into(),
                outputs: 2,
                sample_rate: Some(48000.),
                ..AudioDevice::default()
            },
            AudioDevice {
                id: 3,
                name: "MacBook Pro Microphone".into(),
                inputs: 1,
                default_input: true,
                sample_rate: Some(48000.),
                ..AudioDevice::default()
            },
        ];
        c.sound.sources = vec![
            AudioSource {
                name: "Music".into(),
                bundle: "com.apple.Music".into(),
                pid: 1,
            },
            AudioSource {
                name: "Browser".into(),
                bundle: "demo.browser".into(),
                pid: 2,
            },
        ];
        c.now = State::Active(serde_json::from_value(serde_json::json!({"bundleIdentifier":"com.apple.Music","title":"Midnight transmission","artist":"Overseer sessions","album":"Signals / Vol. 01","playing":true,"duration":245.,"elapsedTime":83.})).unwrap());
        c.artwork = Some(Arc::new(image::RgbImage::from_fn(128, 128, |x, y| {
            let dx = x as f32 - 64.;
            let dy = y as f32 - 64.;
            let wave = ((dx * dx + dy * dy).sqrt() * 0.2).sin() * 0.5 + 0.5;
            image::Rgb([
                (20. + wave * 90.) as u8,
                (50. + x as f32) as u8,
                (70. + y as f32) as u8,
            ])
        })));
        c
    }
}
