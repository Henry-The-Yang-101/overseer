use serde::Deserialize;
use std::collections::VecDeque;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Tab {
    #[default]
    Wifi,
    Bluetooth,
    Display,
    Sound,
    NowPlaying,
}
impl Tab {
    pub const ALL: [Self; 5] = [
        Self::Wifi,
        Self::Bluetooth,
        Self::Display,
        Self::Sound,
        Self::NowPlaying,
    ];
    pub fn index(self) -> usize {
        Self::ALL.iter().position(|v| *v == self).unwrap()
    }
    pub fn name(self) -> &'static str {
        match self {
            Self::Wifi => "Wi-Fi",
            Self::Bluetooth => "Bluetooth",
            Self::Display => "Display",
            Self::Sound => "Sound",
            Self::NowPlaying => "Now Playing",
        }
    }
    pub fn parse(s: &str) -> Self {
        match s {
            "bluetooth" => Self::Bluetooth,
            "display" => Self::Display,
            "sound" => Self::Sound,
            "now-playing" => Self::NowPlaying,
            _ => Self::default(),
        }
    }
    pub fn step(self, n: i32) -> Self {
        Self::ALL[(self.index() as i32 + n).rem_euclid(5) as usize]
    }
    pub fn hints(self) -> &'static str {
        match self {
            Self::Wifi => "s speed test / cancel   Tab tabs   ? help",
            Self::Bluetooth => "↑↓ devices   Tab tabs   r refresh   ? help",
            Self::Display => "↑↓ displays   Tab tabs   r refresh   ? help",
            Self::Sound => "↑↓ device   i/o input/output   Enter select   ± volume   m mute",
            Self::NowPlaying => "j prev   k play/pause   l next   ←→ seek   ± volume   Tab tabs",
        }
    }
}
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub struct Wifi {
    pub powered: bool,
    pub connected: bool,
    pub interface: String,
    pub status: String,
    pub ssid: Option<String>,
    pub bssid: Option<String>,
    pub country: Option<String>,
    pub mac: Option<String>,
    pub rssi: Option<i32>,
    pub noise: Option<i32>,
    pub tx_mbps: Option<f64>,
    pub channel: Option<u32>,
    pub band: Option<String>,
    pub width: Option<u32>,
    pub phy: Option<String>,
    pub security: Option<String>,
    pub ipv4: Vec<String>,
    pub ipv6: Vec<String>,
    pub gateway: Option<String>,
    pub dns: Vec<String>,
    pub primary_interface: Option<String>,
}
#[derive(Clone, Debug, Default)]
pub struct Bluetooth {
    pub powered: Option<bool>,
    pub devices: Vec<Device>,
}
#[derive(Clone, Debug, Default)]
pub struct Device {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub connected: bool,
    pub battery: Option<u8>,
    pub left: Option<u8>,
    pub right: Option<u8>,
    pub case: Option<u8>,
    pub firmware: Option<String>,
    pub case_firmware: Option<String>,
    pub vendor: Option<String>,
    pub product: Option<String>,
    pub rssi: Option<i32>,
    pub services: Option<String>,
}
impl Device {
    pub fn model(&self) -> &'static str {
        let n = self.name.to_lowercase();
        if n.contains("airpods") {
            "airpods-pro"
        } else if n.contains("buds") {
            "buds-fe"
        } else if n.contains("1000xm5") {
            "sony-xm5"
        } else if n.contains("mx master") {
            "mx-master-3s"
        } else if n.contains("aula") {
            "aula-f99"
        } else if n.contains("keychron") {
            "keychron-k8"
        } else if n.contains("ipad") {
            "ipad-pro"
        } else if n.contains("iphone") {
            "iphone"
        } else if self.kind.to_lowercase().contains("keyboard") {
            "keychron-k8"
        } else if n.contains("mouse") || self.kind.to_lowercase().contains("mouse") {
            "mouse"
        } else if self.kind.to_lowercase().contains("head") {
            "sony-xm5"
        } else {
            "generic"
        }
    }
}
pub fn bluetooth_json(bytes: &[u8]) -> Result<Bluetooth, String> {
    let value: serde_json::Value = serde_json::from_slice(bytes).map_err(|e| e.to_string())?;
    let root = value["SPBluetoothDataType"]
        .as_array()
        .and_then(|v| v.first())
        .ok_or("Bluetooth inventory unavailable")?;
    let powered = match root["controller_properties"]["controller_state"].as_str() {
        Some("attrib_on") => Some(true),
        Some("attrib_off") => Some(false),
        _ => None,
    };
    let mut devices = vec![];
    for (key, connected) in [("device_connected", true), ("device_not_connected", false)] {
        for entry in root[key].as_array().into_iter().flatten() {
            for (name, d) in entry.as_object().into_iter().flatten() {
                let s = |key: &str| d[key].as_str().map(str::to_owned);
                let pct = |key: &str| {
                    d[key]
                        .as_u64()
                        .and_then(|v| u8::try_from(v).ok())
                        .or_else(|| d[key].as_str()?.trim_end_matches('%').parse::<u8>().ok())
                        .filter(|v| *v <= 100)
                };
                let Some(id) = s("device_address") else {
                    continue;
                };
                devices.push(Device {
                    id,
                    name: name.trim().into(),
                    connected: connected && powered != Some(false),
                    kind: s("device_minorType").unwrap_or_else(|| "Device".into()),
                    battery: pct("device_batteryLevel").or_else(|| pct("device_batteryLevelMain")),
                    left: pct("device_batteryLevelLeft"),
                    right: pct("device_batteryLevelRight"),
                    case: pct("device_batteryLevelCase"),
                    firmware: s("device_firmwareVersion"),
                    case_firmware: s("device_caseVersion"),
                    vendor: s("device_vendorID"),
                    product: s("device_productID"),
                    rssi: d["device_rssi"]
                        .as_i64()
                        .map(|v| v as i32)
                        .or_else(|| s("device_rssi")?.parse().ok()),
                    services: s("device_services"),
                });
            }
        }
    }
    // Stable ordering preserves navigation when a connection changes.
    let mut seen = std::collections::HashSet::new();
    devices.retain(|d| seen.insert(d.id.clone()));
    devices.sort_by_key(|d| (d.name.to_lowercase(), d.id.clone()));
    Ok(Bluetooth { powered, devices })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn radio_measurements_preserve_missing_values_and_loss() {
        let bt = bluetooth_json(br#"{"SPBluetoothDataType":[{
          "controller_properties":{"controller_state":"attrib_on"},
          "device_connected":[{"MX Master 3S":{"device_address":"mouse","device_batteryLevel":"105%"}}],
          "device_not_connected":[
            {"AirPods Pro":{"device_address":"pods","device_batteryLevelLeft":"0%","device_batteryLevelRight":"83%","device_batteryLevelCase":55}},
            {"Old mouse name":{"device_address":"mouse","device_batteryLevel":"20%"}}
          ]}]}"#).unwrap();
        assert_eq!(bt.powered, Some(true));
        assert_eq!(bt.devices.len(), 2);
        assert!(!bt.devices[0].connected);
        assert_eq!(
            (bt.devices[0].left, bt.devices[0].right, bt.devices[0].case),
            (Some(0), Some(83), Some(55))
        );
        assert!(bt.devices[1].connected);
        assert_eq!(bt.devices[1].battery, None);
        let mut latency = Latency::default();
        assert_eq!(latency.stats().loss, None);
        for (i, ms) in [Some(10.), Some(14.), None, Some(30.)]
            .into_iter()
            .enumerate()
        {
            latency.push(Ping {
                at: i as f64,
                ms,
                loaded: false,
            });
        }
        let stats = latency.stats();
        assert_eq!(
            (stats.mean, stats.jitter, stats.loss),
            (Some(18.), Some(4.), Some(25.))
        );
        let speed = speed_json(br#"{"dl_throughput":120000000,"ul_throughput":0,"base_rtt":14.5}"#)
            .unwrap();
        assert_eq!(
            (speed.down, speed.up, speed.idle_ms, speed.responsiveness),
            (Some(120.), Some(0.), Some(14.5), None)
        );
        assert!(speed_json(br#"{"error":"offline"}"#).is_err());
        assert!(speed_json(b"{}").is_err());
    }
}
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub struct Display {
    pub id: u32,
    pub name: String,
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    pub pixel_width: u32,
    pub pixel_height: u32,
    pub scale: f64,
    pub builtin: bool,
    pub main: bool,
    pub rotation: f64,
    pub mirrored: bool,
    pub mm_width: f64,
    pub mm_height: f64,
    pub refresh: Option<f64>,
    pub max_refresh: u32,
    pub brightness: Option<f32>,
    pub color_space: String,
    pub edr: f64,
    pub vendor: Option<u32>,
    pub product: Option<u32>,
}
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub struct Sound {
    pub devices: Vec<AudioDevice>,
    pub sources: Vec<AudioSource>,
}
impl Sound {
    pub fn output(&self) -> Option<&AudioDevice> {
        self.devices.iter().find(|d| d.default_output)
    }
}
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub struct AudioDevice {
    pub id: u32,
    pub uid: String,
    pub name: String,
    pub inputs: u32,
    pub outputs: u32,
    pub default_input: bool,
    pub default_output: bool,
    pub can_volume: bool,
    pub can_mute: bool,
    pub manufacturer: Option<String>,
    pub sample_rate: Option<f64>,
    pub volume: Option<f32>,
    pub muted: Option<bool>,
}
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub struct AudioSource {
    pub pid: u32,
    pub bundle: String,
    pub name: String,
}
#[derive(Clone, Debug)]
pub struct Ping {
    pub at: f64,
    pub ms: Option<f64>,
    pub loaded: bool,
}
#[derive(Default, Debug)]
pub struct Latency {
    pub samples: VecDeque<Ping>,
}
#[derive(Default, Debug)]
pub struct LatencyStats {
    pub mean: Option<f64>,
    pub jitter: Option<f64>,
    pub loss: Option<f64>,
    pub min: Option<f64>,
    pub max: Option<f64>,
    pub received: usize,
}
impl Latency {
    pub fn push(&mut self, value: Ping) {
        self.samples.push_back(value);
        while self.samples.len() > 120 {
            self.samples.pop_front();
        }
    }
    pub fn stats(&self) -> LatencyStats {
        let values: Vec<_> = self
            .samples
            .iter()
            .filter_map(|p| p.ms)
            .filter(|v| v.is_finite() && *v >= 0.)
            .collect();
        let differences: Vec<_> = self
            .samples
            .iter()
            .zip(self.samples.iter().skip(1))
            .filter_map(|(a, b)| Some((b.ms? - a.ms?).abs()))
            .collect();
        LatencyStats {
            mean: (!values.is_empty()).then(|| values.iter().sum::<f64>() / values.len() as f64),
            jitter: (!differences.is_empty())
                .then(|| differences.iter().sum::<f64>() / differences.len() as f64),
            loss: (!self.samples.is_empty()).then(|| {
                100. * (self.samples.len() - values.len()) as f64 / self.samples.len() as f64
            }),
            min: values.iter().copied().reduce(f64::min),
            max: values.iter().copied().reduce(f64::max),
            received: values.len(),
        }
    }
}
#[derive(Clone, Debug, Default)]
pub struct SpeedResult {
    pub down: Option<f64>,
    pub up: Option<f64>,
    pub idle_ms: Option<f64>,
    pub responsiveness: Option<f64>,
    pub interface: Option<String>,
}
pub fn speed_json(bytes: &[u8]) -> Result<SpeedResult, String> {
    let v: serde_json::Value =
        serde_json::from_slice(bytes).map_err(|e| format!("Speed test output: {e}"))?;
    if let Some(error) = v
        .get("error")
        .filter(|v| !v.is_null() && v.as_str() != Some(""))
    {
        return Err(format!("Speed test: {error}"));
    }
    let number = |key: &str| v[key].as_f64().filter(|n| n.is_finite() && *n >= 0.);
    let result = SpeedResult {
        down: number("dl_throughput").map(|n| n / 1e6),
        up: number("ul_throughput").map(|n| n / 1e6),
        idle_ms: number("base_rtt"),
        responsiveness: number("responsiveness"),
        interface: v["interface_name"].as_str().map(str::to_owned),
    };
    if result.down.is_none() && result.up.is_none() {
        return Err("No throughput measured; check the connection and retry".into());
    }
    Ok(result)
}
