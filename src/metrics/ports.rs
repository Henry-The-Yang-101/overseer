use super::Avail;
use serde::Serialize;
use std::sync::{
    Arc,
    mpsc::{self, Receiver},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize)]
pub enum PortKind {
    MagSafe,
    UsbC(u8),
    Audio,
    Hdmi,
    SdCard,
}
impl PortKind {
    pub const MACBOOK: [Self; 7] = [
        Self::MagSafe,
        Self::UsbC(1),
        Self::UsbC(2),
        Self::Audio,
        Self::Hdmi,
        Self::UsbC(3),
        Self::SdCard,
    ];
    pub fn label(self) -> String {
        match self {
            Self::MagSafe => "MagSafe".into(),
            Self::UsbC(n) => format!("USB-C {n}"),
            Self::Audio => "3.5 mm".into(),
            Self::Hdmi => "HDMI".into(),
            Self::SdCard => "SD card".into(),
        }
    }
}
#[derive(Clone, Debug, Serialize)]
pub struct Port {
    pub kind: PortKind,
    pub connected: Avail<bool>,
    pub detail: String,
    pub devices: Vec<String>,
}
impl Port {
    pub fn status(&self) -> &'static str {
        match self.connected {
            Avail::Ok(true) => "CONNECTED",
            Avail::Ok(false) => "EMPTY",
            Avail::Stale { .. } => "STALE",
            _ => "UNKNOWN",
        }
    }
}
pub type Ports = Avail<Arc<Vec<Port>>>;
pub fn request() -> Receiver<Ports> {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(collect());
    });
    rx
}
#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
fn collect() -> Ports {
    let registry = match query("IOPort") {
        Ok(v) => v,
        Err(e) => return Avail::Failed(e),
    };
    let mut ports = parse(&registry);
    if let Ok(usb) = query("AppleUSBHostPort") {
        attach_usb(&usb, None, &mut ports);
    }
    let audio = query("AppleCS42L84Audio")
        .ok()
        .and_then(|v| headphone_detect(&v))
        .or_else(|| {
            query("AppleCS42L83Audio")
                .ok()
                .and_then(|v| headphone_detect(&v))
        });
    if !ports.is_empty() || audio.is_some() {
        ports.push(Port {
            kind: PortKind::Audio,
            devices: vec![],
            connected: audio.map(Avail::Ok).unwrap_or(Avail::Unsupported(
                "headphone jack sensor not exposed".into(),
            )),
            detail: audio
                .map_or("Jack detection unavailable", |v| {
                    if v {
                        "Headphones / analogue audio"
                    } else {
                        "Nothing connected"
                    }
                })
                .into(),
        });
        ports.sort_by_key(|p| {
            PortKind::MACBOOK
                .iter()
                .position(|k| *k == p.kind)
                .unwrap_or(7)
        });
        Avail::Ok(Arc::new(ports))
    } else {
        Avail::Unsupported("physical port sensors not exposed".into())
    }
}
#[cfg(not(all(target_os = "macos", target_arch = "aarch64")))]
fn collect() -> Ports {
    Avail::Unsupported("physical port detection requires Apple Silicon macOS".into())
}

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
fn query(class: &str) -> Result<plist::Value, String> {
    use std::{
        io::Read,
        process::{Command, Stdio},
        time::{Duration, Instant},
    };
    let mut child = Command::new("/usr/sbin/ioreg")
        .args(["-a", "-l", "-r", "-c", class])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| e.to_string())?;
    let stdout = child.stdout.take().ok_or("port query has no output")?;
    let reader = std::thread::spawn(move || {
        let mut data = Vec::new();
        let result = stdout.take(2_000_000).read_to_end(&mut data);
        result.map(|_| data)
    });
    let began = Instant::now();
    let success = loop {
        match child.try_wait() {
            Ok(Some(s)) => break s.success(),
            Ok(None) if began.elapsed() < Duration::from_secs(3) => {
                std::thread::sleep(Duration::from_millis(20))
            }
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                break false;
            }
        }
    };
    let bytes = reader
        .join()
        .map_err(|_| "port reader stopped")?
        .map_err(|e| e.to_string())?;
    if !success {
        return Err("port query failed or timed out".into());
    }
    plist::Value::from_reader(std::io::Cursor::new(bytes))
        .map_err(|_| "port data unreadable".into())
}
#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
fn boolean(v: &plist::Value) -> Option<bool> {
    v.as_boolean()
        .or_else(|| v.as_unsigned_integer().filter(|v| *v <= 1).map(|v| v == 1))
}
#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
fn nodes(v: &plist::Value, visit: &mut impl FnMut(&plist::Dictionary)) {
    if let Some(nodes) = v.as_array() {
        for n in nodes {
            self::nodes(n, visit);
        }
    } else if let Some(n) = v.as_dictionary() {
        visit(n);
        if let Some(children) = n.get("IORegistryEntryChildren") {
            nodes(children, visit);
        }
    }
}
#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
fn parse(v: &plist::Value) -> Vec<Port> {
    let mut ports = vec![];
    nodes(v, &mut |n| {
        if n.get("BuiltIn").and_then(boolean) == Some(false) {
            return;
        }
        let kind = match n
            .get("PortTypeDescription")
            .and_then(plist::Value::as_string)
        {
            Some("USB-C") => match n
                .get("PortNumber")
                .and_then(plist::Value::as_unsigned_integer)
                .filter(|v| *v > 0 && *v <= 255)
            {
                Some(v) => PortKind::UsbC(v as u8),
                None => return,
            },
            Some("HDMI") => PortKind::Hdmi,
            Some("SD Card") => PortKind::SdCard,
            Some("MagSafe 3") => PortKind::MagSafe,
            _ => return,
        };
        let connected = n
            .get("ConnectionActive")
            .and_then(boolean)
            .map(Avail::Ok)
            .unwrap_or(Avail::Unsupported("connection flag not exposed".into()));
        let active = n
            .get("TransportsActive")
            .and_then(plist::Value::as_array)
            .map(|v| {
                v.iter()
                    .filter_map(plist::Value::as_string)
                    .filter(|s| *s != "CC")
                    .map(|s| match s {
                        "USB2" => "USB 2",
                        "USB3" => "USB 3",
                        "CIO" => "Thunderbolt / USB4",
                        v => v,
                    })
                    .collect::<Vec<_>>()
                    .join(" · ")
            })
            .unwrap_or_default();
        let detail = match connected {
            Avail::Ok(false) => "Nothing connected".into(),
            Avail::Ok(true) if !active.is_empty() => active,
            Avail::Ok(true) => match kind {
                PortKind::MagSafe => "Charging cable attached",
                PortKind::Hdmi => "Display attached",
                PortKind::SdCard => "Card inserted",
                _ => "Cable attached",
            }
            .into(),
            _ => "Connection status unavailable".into(),
        };
        if !ports.iter().any(|p: &Port| p.kind == kind) {
            ports.push(Port {
                kind,
                devices: if connected == Avail::Ok(true) {
                    device_names(&plist::Value::Dictionary(n.clone()))
                } else {
                    vec![]
                },
                connected,
                detail,
            });
        }
    });
    ports
}
#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
fn device_names(v: &plist::Value) -> Vec<String> {
    let mut names = vec![];
    nodes(v, &mut |n| {
        for key in ["ProductName", "USB Product Name", "Product Name"] {
            if let Some(name) = n.get(key).and_then(plist::Value::as_string) {
                let name: String = name.chars().filter(|c| !c.is_control()).take(160).collect();
                if !name.trim().is_empty() && !names.contains(&name) {
                    names.push(name);
                }
            }
        }
    });
    names
}
#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
fn attach_usb(v: &plist::Value, inherited: Option<u8>, ports: &mut [Port]) {
    if let Some(list) = v.as_array() {
        for child in list {
            attach_usb(child, inherited, ports);
        }
    }
    if let Some(n) = v.as_dictionary() {
        let id = n
            .get("usb-c-port-number")
            .and_then(|v| {
                v.as_unsigned_integer()
                    .map(|n| n as u8)
                    .or_else(|| v.as_data().and_then(|d| d.first().copied()))
            })
            .or(inherited);
        if let Some(id) = id
            && let Some(port) = ports
                .iter_mut()
                .find(|p| p.kind == PortKind::UsbC(id) && p.connected == Avail::Ok(true))
        {
            for name in device_names(v) {
                if !port.devices.contains(&name) {
                    port.devices.push(name);
                }
            }
            return;
        }
        if let Some(children) = n.get("IORegistryEntryChildren") {
            attach_usb(children, id, ports);
        }
    }
}
#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
fn headphone_detect(v: &plist::Value) -> Option<bool> {
    let mut found = None;
    nodes(v, &mut |n| {
        if let Some(v) = n.get("HPDetect").and_then(boolean) {
            found = Some(v);
        }
    });
    found
}

#[cfg(all(test, target_os = "macos", target_arch = "aarch64"))]
mod tests {
    use super::*;
    #[test]
    fn missing_flags_stay_unknown_and_usb_identity_is_preserved() {
        let xml=br#"<?xml version="1.0"?><plist version="1.0"><array>
        <dict><key>PortTypeDescription</key><string>USB-C</string><key>PortNumber</key><integer>3</integer><key>ConnectionActive</key><true/><key>TransportsActive</key><array><string>CC</string><string>DisplayPort</string></array><key>IORegistryEntryChildren</key><array><dict><key>ProductName</key><string>Test display</string></dict></array></dict>
        <dict><key>PortTypeDescription</key><string>HDMI</string><key>ConnectionActive</key><false/></dict>
        <dict><key>PortTypeDescription</key><string>MagSafe 3</string><key>ConnectionCount</key><integer>42</integer></dict>
        </array></plist>"#;
        let v = plist::Value::from_reader(std::io::Cursor::new(xml)).unwrap();
        let ports = parse(&v);
        assert_eq!(ports[0].kind, PortKind::UsbC(3));
        assert_eq!(ports[0].connected, Avail::Ok(true));
        assert_eq!(ports[0].detail, "DisplayPort");
        assert_eq!(ports[0].devices, vec!["Test display"]);
        assert_eq!(ports[1].status(), "EMPTY");
        assert_eq!(ports[2].status(), "UNKNOWN");
    }
}

/// macOS exposes attachments, but not a portable active-input port identifier.
/// Infer only when AC is present and exactly one known power-capable port is attached.
pub fn power_input(s: &super::Sample) -> Avail<PortKind> {
    let Some(battery) = s.battery.value() else {
        return Avail::Unsupported("power source unavailable".into());
    };
    if !battery.ac {
        return Avail::NoHardware("running on battery".into());
    }
    let Some(ports) = s.ports.value().filter(|_| s.at - s.ports_at <= 10.) else {
        return Avail::Acquiring;
    };
    if s.machine.model != "Mac16,8"
        || PortKind::MACBOOK
            .iter()
            .any(|kind| !ports.iter().any(|p| p.kind == *kind))
    {
        return Avail::Unsupported("complete port map unavailable".into());
    }
    let capable: Vec<_> = ports
        .iter()
        .filter(|p| matches!(p.kind, PortKind::MagSafe | PortKind::UsbC(_)))
        .collect();
    if capable.iter().any(|p| !matches!(p.connected, Avail::Ok(_))) {
        return Avail::Unsupported("power input unknown".into());
    }
    let mut connected = capable.iter().filter(|p| p.connected == Avail::Ok(true));
    match (connected.next(), connected.next()) {
        (Some(p), None) => Avail::Estimated(p.kind),
        _ => Avail::Unsupported("multiple possible power inputs".into()),
    }
}
