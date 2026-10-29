mod collector;
mod demo;
pub mod details;
pub mod ports;
pub mod power_mode;
mod sockets;
pub use collector::{send_signal, spawn_collector};
pub use demo::{SCENARIOS, demo};
pub use power_mode::PowerMode;
use serde::Serialize;
use std::sync::Arc;

#[derive(Clone, Debug, Serialize, PartialEq, Default)]
pub enum Avail<T> {
    Ok(T),
    Estimated(T),
    NoHardware(String),
    Unsupported(String),
    Permission(String),
    Failed(String),
    Stale {
        value: T,
        age_s: u16,
    },
    #[default]
    Acquiring,
}
impl<T> Avail<T> {
    pub fn value(&self) -> Option<&T> {
        match self {
            Self::Ok(v) | Self::Estimated(v) | Self::Stale { value: v, .. } => Some(v),
            _ => None,
        }
    }
    pub fn reason(&self) -> &str {
        match self {
            Self::Ok(_) => "measured",
            Self::Estimated(_) => "estimated",
            Self::NoHardware(s) | Self::Unsupported(s) | Self::Permission(s) | Self::Failed(s) => s,
            Self::Stale { .. } => "stale",
            Self::Acquiring => "acquiring",
        }
    }
    pub fn label(&self) -> &str {
        match self {
            Self::Ok(_) => "",
            Self::Estimated(_) => "≈",
            Self::NoHardware(_) => "NO HARDWARE",
            Self::Unsupported(_) => "N/A",
            Self::Permission(_) => "PERMISSION",
            Self::Failed(_) => "READ FAILED",
            Self::Stale { .. } => "STALE",
            Self::Acquiring => "ACQUIRING",
        }
    }
}
#[derive(Clone, Debug, Default, Serialize)]
pub struct Machine {
    pub chip: String,
    pub model: String,
    pub gpu_cores: usize,
    pub memory: u64,
    pub cpu_count: usize,
}
#[derive(Clone, Debug, Serialize)]
pub struct Core {
    pub id: usize,
    pub die: usize,
    pub active: f32,
    pub scaled: f32,
    pub mhz: u32,
}
#[derive(Clone, Debug, Serialize)]
pub struct Tier {
    pub label: String,
    pub mhz: u32,
    pub max_mhz: u32,
    pub freqs: Vec<u32>,
    pub active: f32,
    pub scaled: f32,
    pub cores: Vec<Core>,
}
#[derive(Clone, Debug, Serialize)]
pub struct Fan {
    pub name: String,
    pub rpm: u32,
    pub max: Option<u32>,
}
#[derive(Clone, Debug, Serialize)]
pub struct Battery {
    pub percent: f32,
    pub charging: bool,
    pub ac: bool,
    pub watts: Avail<f32>,
    pub health: Avail<f32>,
    pub cycles: Avail<u32>,
    pub minutes: Avail<u32>,
}
#[derive(Clone, Debug, Default, Serialize)]
pub struct Flow {
    pub read: f64,
    pub write: f64,
    pub packets_in: f64,
    pub packets_out: f64,
    pub errors: u64,
    pub name: String,
    pub total_read: u64,
    pub total_write: u64,
    pub addresses: Vec<String>,
}
#[derive(Clone, Debug, Default, Serialize)]
pub struct Volume {
    pub name: String,
    pub mount: String,
    pub used: u64,
    pub total: u64,
    pub external: bool,
    pub read_only: bool,
}
#[derive(Clone, Debug, Default, Serialize)]
pub struct Proc {
    pub pid: u32,
    pub start: u64,
    pub parent: u32,
    pub name: Arc<str>,
    pub command: Arc<str>,
    pub user: Arc<str>,
    pub cpu: f32,
    pub memory: u64,
    pub footprint: Avail<u64>,
    pub wakeups: Avail<f32>,
    pub read: f64,
    pub write: f64,
    pub runtime: u64,
    pub cpu_time: u64,
    pub total_read: u64,
    pub total_write: u64,
    pub threads: Avail<u32>,
    pub connections: Avail<u32>,
    pub status: Arc<str>,
}
impl Proc {
    pub fn key(&self) -> (u32, u64) {
        (self.pid, self.start)
    }
}
#[derive(Clone, Debug, Default, Serialize)]
pub struct Sample {
    pub at: f64,
    pub energy_wh: f64,
    pub machine: Arc<Machine>,
    pub cpu: f32,
    pub power_mode: Avail<PowerMode>,
    pub cpu_split: Avail<[f32; 3]>,
    pub load: [f64; 3],
    pub tiers: Vec<Tier>,
    pub gpu: Avail<f32>,
    pub gpu_mhz: Avail<u32>,
    pub gpu_scaled: Avail<f32>,
    pub gpu_freqs: Vec<u32>,
    pub power: [Avail<f32>; 5],
    pub system_power: Avail<f32>,
    pub cpu_temp: Avail<f32>,
    pub gpu_temp: Avail<f32>,
    pub thermal: Avail<String>,
    pub fans: Avail<Vec<Fan>>,
    pub memory_used: u64,
    pub memory: Avail<[u64; 5]>,
    pub pressure: Avail<String>,
    pub swap: u64,
    pub pressure_percent: Avail<f32>,
    pub compressed_original: Avail<u64>,
    pub vm_totals: Avail<[u64; 5]>,
    pub vm_rates: Avail<[f64; 5]>,
    pub disk: Avail<Flow>,
    pub disk_capacity: Avail<(u64, u64)>,
    pub network: Avail<Flow>,
    pub interfaces: Vec<Flow>,
    pub volumes: Vec<Volume>,
    pub connection_states: Avail<Vec<(String, u32)>>,
    pub battery: Avail<Battery>,
    pub ports: ports::Ports,
    pub ports_at: f64,
    pub processes: Arc<Vec<Proc>>,
    pub process_at: f64,
    pub uptime: u64,
    pub collector_ms: f64,
    pub error: Option<String>,
}
impl Sample {
    pub fn total_power(&self) -> Avail<f32> {
        match (
            self.power[0].value(),
            self.power[1].value(),
            self.power[2].value(),
        ) {
            (Some(a), Some(b), Some(c)) => Avail::Ok(a + b + c),
            _ => Avail::Unsupported("CPU + GPU + ANE power unavailable".into()),
        }
    }
    pub fn energy(&self) -> f32 {
        self.cpu
            .max(*self.gpu.value().unwrap_or(&0.))
            .max(self.total_power().value().copied().unwrap_or(0.) / 40.)
            .clamp(0., 1.)
    }
    pub fn core_count(&self) -> usize {
        self.tiers
            .iter()
            .map(|t| t.cores.len())
            .sum::<usize>()
            .max(1)
    }
    pub fn memory_ratio(&self) -> f32 {
        self.memory_used as f32 / self.machine.memory.max(1) as f32
    }
}
pub fn normalize_system_power(watts: f32) -> Avail<f32> {
    if watts.is_finite() && watts > 0. {
        Avail::Ok(watts)
    } else {
        Avail::Failed("SMC PSTR unreadable".into())
    }
}
pub fn allocate_cells<const N: usize>(bytes: [u64; N], cells: usize) -> [usize; N] {
    let total: u128 = bytes.iter().map(|&v| v as u128).sum();
    let mut out = [0; N];
    if total == 0 {
        if N > 0 {
            out[N - 1] = cells;
        }
        return out;
    }
    let mut rem: Vec<_> = bytes
        .iter()
        .enumerate()
        .map(|(i, &v)| {
            let n = v as u128 * cells as u128;
            out[i] = (n / total) as usize;
            (i, n % total)
        })
        .collect();
    rem.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    let left = cells - out.iter().sum::<usize>();
    for (i, _) in rem.into_iter().take(left) {
        out[i] += 1;
    }
    out
}
