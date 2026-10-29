use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
#[derive(Default)]
pub struct Config {
    pub ui: Ui,
    pub units: Units,
    pub teardown: Teardown,
    pub timeline: Timeline,
    pub processes: Processes,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Ui {
    pub theme: String,
    pub default_view: String,
    pub motion: String,
    pub fps: u32,
    pub glyphs: String,
    pub transparent_bg: bool,
    pub show_boot: bool,
    pub mouse: bool,
    pub onboarding_seen: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Units {
    pub bytes: String,
    pub temp: String,
    pub net: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Teardown {
    pub orientation: String,
    pub labels: bool,
    pub highlight_active: bool,
    pub cells: Vec<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Timeline {
    pub window: String,
    pub retention: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Processes {
    pub sort: String,
    pub cpu_mode: String,
}
impl Default for Ui {
    fn default() -> Self {
        Self {
            theme: "holo".into(),
            default_view: "board".into(),
            motion: "full".into(),
            fps: 30,
            glyphs: "unicode".into(),
            transparent_bg: false,
            show_boot: true,
            mouse: true,
            onboarding_seen: false,
        }
    }
}
impl Default for Units {
    fn default() -> Self {
        Self {
            bytes: "binary".into(),
            temp: "c".into(),
            net: "bytes".into(),
        }
    }
}
impl Default for Teardown {
    fn default() -> Self {
        Self {
            orientation: "underside".into(),
            labels: true,
            highlight_active: true,
            cells: ["cpu", "gpu", "memory", "power", "thermal", "battery"]
                .map(String::from)
                .to_vec(),
        }
    }
}
impl Default for Timeline {
    fn default() -> Self {
        Self {
            window: "5m".into(),
            retention: "1h".into(),
        }
    }
}
impl Default for Processes {
    fn default() -> Self {
        Self {
            sort: "cpu".into(),
            cpu_mode: "per-core".into(),
        }
    }
}
impl Config {
    pub fn path() -> PathBuf {
        std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".config")
            })
            .join("overseer/config.toml")
    }
    pub fn load() -> anyhow::Result<Self> {
        let path = Self::path();
        let mut c: Self = if path.exists() {
            toml::from_str(&std::fs::read_to_string(path)?)?
        } else {
            Self::default()
        };
        c.ui.fps = c.ui.fps.clamp(4, 60);
        anyhow::ensure!(
            ["binary", "si"].contains(&c.units.bytes.as_str()),
            "units.bytes must be binary or si"
        );
        anyhow::ensure!(
            ["c", "f"].contains(&c.units.temp.as_str()),
            "units.temp must be c or f"
        );
        anyhow::ensure!(
            ["bytes", "bits"].contains(&c.units.net.as_str()),
            "units.net must be bytes or bits"
        );
        anyhow::ensure!(
            c.teardown.cells.len() == 6,
            "teardown.cells needs six assignments"
        );
        Ok(c)
    }
    pub fn save(&self) -> anyhow::Result<()> {
        let path = Self::path();
        std::fs::create_dir_all(path.parent().unwrap())?;
        let temp = path.with_extension(format!("{}.tmp", std::process::id()));
        std::fs::write(&temp, toml::to_string_pretty(self)?)?;
        std::fs::rename(temp, path)?;
        Ok(())
    }
}
