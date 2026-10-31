use super::*;
use std::{
    collections::HashMap,
    sync::mpsc::{self, Receiver},
    time::{Duration, Instant},
};
use sysinfo::{Disks, Networks, ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind, Users};

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
fn cpu_channels() -> HashMap<(String, usize, usize), String> {
    let Ok(report) =
        macmon::sources::IOReport::new(vec![("CPU Stats", Some("CPU Core Performance States"))])
    else {
        return HashMap::new();
    };
    report
        .get_sample(0)
        .filter_map(|item| {
            let name = item.channel;
            let (die, suffix) = if let Some(rest) = name.strip_prefix("DIE_") {
                let (die, suffix) = rest.split_once('_')?;
                (die.parse().ok()?, suffix)
            } else {
                (0, name.as_str())
            };
            let (label, digits) = ["E", "P", "M"].into_iter().find_map(|label| {
                suffix
                    .strip_prefix(&format!("{label}CPU"))
                    .map(|digits| (label, digits))
            })?;
            // Only match direct numeric IDs; macmon flattens newer clustered channel names.
            let id = digits.parse::<usize>().ok()?;
            Some(((label.to_owned(), die, id), name))
        })
        .collect()
}

fn m4_pro_cluster(channel: &str) -> Option<usize> {
    let suffix = channel
        .strip_prefix("ECPU")
        .or_else(|| channel.strip_prefix("PCPU"))?;
    let bytes = suffix.as_bytes();
    (bytes.len() == 3 && bytes.iter().all(u8::is_ascii_digit) && bytes[2] == b'0')
        .then(|| (bytes[0] - b'0') as usize)
}

pub fn spawn_collector() -> Receiver<Sample> {
    let (tx, rx) = mpsc::sync_channel(2);
    std::thread::spawn(move || {
        let result = collect(&tx);
        if let Err(e) = result {
            let _ = tx.send(Sample {
                error: Some(format!("Collector stopped: {e}")),
                ..Sample::default()
            });
        }
    });
    rx
}
fn collect(tx: &mpsc::SyncSender<Sample>) -> anyhow::Result<()> {
    let mut sys = System::new_all()?;
    let mut nets = Networks::new_with_refreshed_list()?;
    let mut disks = Disks::new_with_refreshed_list()?;
    let users = Users::new_with_refreshed_list()?;
    let start = Instant::now();
    let extra_rx = extra::spawn(start);
    let mut extra = Arc::new(extra::Snapshot::default());
    let mut previous = Instant::now();
    type Metadata = (Arc<str>, Arc<str>, Arc<str>, Arc<str>);
    let mut strings: HashMap<(u32, u64), Metadata> = HashMap::new();
    let mut processes = Arc::new(Vec::new());
    let mut gpu_collector = super::gpu::Collector::default();
    let mut process_gpu = Avail::Acquiring;
    let mut process_at = 0.;
    let mut wakeups: HashMap<(u32, u64), u64> = HashMap::new();
    let mut previous_vm: Option<[u64; 5]> = None;
    let mut previous_ticks = cpu_ticks();
    let mut socket_rx = None;
    let mut socket_data: Option<sockets::Snapshot> = None;
    let mut socket_at = Instant::now() - Duration::from_secs(20);
    let mut last_process = Instant::now() - Duration::from_secs(20);
    let mut port_rx = Some(ports::request());
    let mut port_state = Avail::Acquiring;
    let mut ports_at = 0.;
    let mut last_ports = Instant::now();
    let mut battery = Avail::Acquiring;
    let mut last_battery = Instant::now() - Duration::from_secs(20);
    let mut power_mode = Avail::Acquiring;
    let mut last_power_mode = Instant::now() - Duration::from_secs(20);
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    let mut sampler = macmon::Sampler::new().ok();
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    let channels = cpu_channels();
    loop {
        let began = Instant::now();
        let mut s = Sample::default();
        #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
        if let Some(sampler) = sampler.as_mut() {
            match sampler.get_metrics(1000) {
                Ok(m) => {
                    let info = sampler.get_soc_info();
                    s.machine = Arc::new(Machine {
                        chip: info.chip_name.clone(),
                        model: info.mac_model.clone(),
                        gpu_cores: info.gpu_cores as usize,
                        memory: m.memory.ram_total,
                        cpu_count: info.cpu_tiers.iter().map(|t| t.cores as usize).sum(),
                    });
                    s.cpu = m.cpu_active_ratio;
                    s.tiers = m
                        .cpu_tiers
                        .iter()
                        .map(|t| Tier {
                            label: t.label.clone(),
                            mhz: t.freq_mhz,
                            freqs: info
                                .cpu_tiers
                                .iter()
                                .find(|i| i.label == t.label)
                                .map(|i| i.freqs.clone())
                                .unwrap_or_default(),
                            max_mhz: info
                                .cpu_tiers
                                .iter()
                                .find(|i| i.label == t.label)
                                .and_then(|i| i.freqs.last())
                                .copied()
                                .unwrap_or(1),
                            active: t.active_ratio,
                            scaled: t.scaled_ratio,
                            cores: t
                                .cores
                                .iter()
                                .map(|c| Core {
                                    id: c.core_id,
                                    die: c.die_id,
                                    channel: channels
                                        .get(&(t.label.clone(), c.die_id, c.core_id))
                                        .cloned()
                                        .unwrap_or_else(|| format!("{}CPU{}", t.label, c.core_id)),
                                    cluster: if info.chip_name == "Apple M4 Pro" {
                                        // M4 Pro channel suffix: cluster, core, thread (e.g. PCPU130).
                                        channels
                                            .get(&(t.label.clone(), c.die_id, c.core_id))
                                            .and_then(|name| m4_pro_cluster(name))
                                    } else {
                                        None
                                    },
                                    active: c.active_ratio,
                                    scaled: c.scaled_ratio,
                                    mhz: c.freq_mhz,
                                })
                                .collect(),
                        })
                        .collect();
                    s.tiers.sort_by_key(|t| std::cmp::Reverse(t.cores.len()));
                    s.gpu = Avail::Ok(m.gpu_active_ratio);
                    s.gpu_mhz = Avail::Ok(m.gpu_freq_mhz);
                    s.gpu_scaled = Avail::Ok(m.gpu_scaled_ratio);
                    s.gpu_freqs = info.gpu_freqs.clone();
                    s.power = [
                        m.cpu_power,
                        m.gpu_power,
                        m.ane_power,
                        m.ram_power,
                        m.gpu_ram_power,
                    ]
                    .map(|v| {
                        if v.is_finite() && v >= 0. {
                            Avail::Ok(v)
                        } else {
                            Avail::Failed("invalid power counter".into())
                        }
                    });
                    s.system_power = normalize_system_power(m.sys_power);
                    s.cpu_temp = m
                        .temp
                        .cpu_temp_avg
                        .map(Avail::Ok)
                        .unwrap_or(Avail::Unsupported("CPU temperature not exposed".into()));
                    s.gpu_temp = m
                        .temp
                        .gpu_temp_avg
                        .map(Avail::Ok)
                        .unwrap_or(Avail::Unsupported("GPU temperature not exposed".into()));
                    s.fans = if m.fans.is_empty() {
                        if info.chip_name.contains("Air") || info.mac_model.contains("Mac14,2") {
                            Avail::NoHardware("passive cooling".into())
                        } else {
                            Avail::Unsupported("no fan sensors reported".into())
                        }
                    } else {
                        Avail::Ok(
                            m.fans
                                .iter()
                                .map(|f| Fan {
                                    name: f.name.clone(),
                                    rpm: f.rpm,
                                    max: f.max_rpm,
                                })
                                .collect(),
                        )
                    };
                    if info.vm {
                        s.power =
                            std::array::from_fn(|_| Avail::Unsupported("virtual machine".into()));
                        s.gpu = Avail::Unsupported("virtual machine".into());
                    }
                    s.memory_used = m.memory.ram_usage;
                    s.swap = m.memory.swap_usage;
                }
                Err(e) => s.error = Some(format!("Hardware sampler: {e}")),
            }
        }
        if s.machine.chip.is_empty() {
            std::thread::sleep(Duration::from_secs(1).saturating_sub(began.elapsed()));
            sys.refresh_cpu_usage();
            sys.refresh_memory();
            s.machine = Arc::new(Machine {
                chip: sys
                    .cpus()
                    .first()
                    .map(|c| c.brand().to_owned())
                    .unwrap_or("System CPU".into()),
                model: "Generic system".into(),
                gpu_cores: 0,
                memory: sys.total_memory(),
                cpu_count: sys.cpus().len(),
            });
            s.cpu = sys.global_cpu_usage() / 100.;
            s.memory_used = sys.used_memory();
            s.swap = sys.used_swap();
            s.tiers = vec![Tier {
                label: "CPU".into(),
                mhz: 0,
                max_mhz: 1,
                freqs: vec![],
                active: s.cpu,
                scaled: s.cpu,
                cores: sys
                    .cpus()
                    .iter()
                    .enumerate()
                    .map(|(i, c)| Core {
                        id: i,
                        die: 0,
                        channel: format!("CPU{i}"),
                        cluster: None,
                        active: c.usage() / 100.,
                        scaled: c.usage() / 100.,
                        mhz: 0,
                    })
                    .collect(),
            }];
            let na = || Avail::Unsupported("hardware sensor not exposed".into());
            s.power = std::array::from_fn(|_| na());
            s.system_power = na();
            s.cpu_temp = na();
            s.gpu_temp = na();
            s.gpu = na();
            s.gpu_mhz = Avail::Unsupported("clock unavailable".into());
            s.fans = Avail::Unsupported("fan sensors unavailable".into());
        }
        s.at = start.elapsed().as_secs_f64();
        while let Ok(value) = extra_rx.try_recv() {
            extra = value;
        }
        s.extra = extra.clone();
        let elapsed = previous.elapsed().as_secs_f64().max(0.001);
        previous = Instant::now();
        let ticks = cpu_ticks();
        if let (Some(old), Some(now)) = (previous_ticks, ticks) {
            let d: [f32; 4] = std::array::from_fn(|i| now[i].wrapping_sub(old[i]) as f32);
            let total: f32 = d.iter().sum();
            if total > 0. {
                s.cpu_split = Avail::Ok([(d[0] + d[3]) / total, d[1] / total, d[2] / total]);
            }
        }
        previous_ticks = ticks;
        if let Ok(load) = System::load_average() {
            s.load = [load.one, load.five, load.fifteen];
        }
        if let Some(rx) = &socket_rx
            && let Ok(data) = std::sync::mpsc::Receiver::try_recv(rx)
        {
            socket_data = Some(data);
            socket_rx = None;
        }
        if socket_rx.is_none()
            && socket_at.elapsed() > Duration::from_secs(15)
            && !processes.is_empty()
        {
            socket_rx = Some(sockets::request(processes.clone()));
            socket_at = Instant::now();
        }
        s.connection_states = socket_data
            .as_ref()
            .map(|d| d.states.clone())
            .unwrap_or(Avail::Acquiring);
        nets.refresh(true);
        disks.refresh(true);
        // One primary active physical interface; avoid counting loopback and VPN copies twice.
        if let Some((name, n)) = nets
            .iter()
            .filter(|(n, _)| {
                n.to_string_lossy().starts_with("en")
                    || n.to_string_lossy().starts_with("eth")
                    || n.to_string_lossy().starts_with("wl")
            })
            .max_by_key(|(_, n)| n.received() + n.transmitted())
        {
            s.network = Avail::Ok(Flow {
                read: n.received() as f64 / elapsed,
                write: n.transmitted() as f64 / elapsed,
                packets_in: n.packets_received() as f64 / elapsed,
                packets_out: n.packets_transmitted() as f64 / elapsed,
                errors: n.errors_on_received() + n.errors_on_transmitted(),
                name: name.to_string_lossy().into_owned(),
                total_read: n.total_received(),
                total_write: n.total_transmitted(),
                addresses: n
                    .ip_networks()
                    .iter()
                    .map(|ip| ip.addr.to_string())
                    .collect(),
            });
        } else {
            s.network = Avail::Unsupported("no active physical interface".into());
        }
        s.interfaces = nets
            .iter()
            .map(|(name, n)| Flow {
                name: name.to_string_lossy().into_owned(),
                read: n.received() as f64 / elapsed,
                write: n.transmitted() as f64 / elapsed,
                packets_in: n.packets_received() as f64 / elapsed,
                packets_out: n.packets_transmitted() as f64 / elapsed,
                errors: n.errors_on_received() + n.errors_on_transmitted(),
                total_read: n.total_received(),
                total_write: n.total_transmitted(),
                addresses: n
                    .ip_networks()
                    .iter()
                    .map(|ip| ip.addr.to_string())
                    .collect(),
            })
            .collect();
        s.interfaces.sort_by(|a, b| {
            (b.read + b.write)
                .total_cmp(&(a.read + a.write))
                .then(a.name.cmp(&b.name))
        });
        s.volumes = disks
            .iter()
            .map(|d| Volume {
                name: clean(&d.name().to_string_lossy()),
                mount: clean(&d.mount_point().to_string_lossy()),
                used: d.total_space().saturating_sub(d.available_space()),
                total: d.total_space(),
                external: d.is_removable(),
                read_only: d.is_read_only(),
            })
            .collect();
        // Use the root volume only; APFS volumes share physical counters/capacity.
        if let Some(d) = disks
            .iter()
            .find(|d| d.mount_point() == std::path::Path::new("/"))
        {
            let u = d.usage();
            s.disk = Avail::Ok(Flow {
                read: u.read_bytes as f64 / elapsed,
                write: u.written_bytes as f64 / elapsed,
                name: d.name().to_string_lossy().into(),
                total_read: u.total_read_bytes,
                total_write: u.total_written_bytes,
                ..Flow::default()
            });
            s.disk_capacity = Avail::Ok((
                d.total_space().saturating_sub(d.available_space()),
                d.total_space(),
            ));
        } else {
            s.disk = Avail::Unsupported("root disk counters unavailable".into());
            s.disk_capacity = Avail::Unsupported("capacity unavailable".into());
        }
        if last_process.elapsed() >= Duration::from_secs(5) {
            let dt = last_process.elapsed().as_secs_f64();
            sys.refresh_processes_specifics(
                ProcessesToUpdate::All,
                true,
                ProcessRefreshKind::nothing()
                    .with_cpu()
                    .with_memory()
                    .with_disk_usage()
                    .with_cmd(UpdateKind::OnlyIfNotSet)
                    .with_exe(UpdateKind::OnlyIfNotSet)
                    .with_user(UpdateKind::OnlyIfNotSet),
            );
            let mut out = Vec::with_capacity(sys.processes().len());
            for (pid, p) in sys.processes() {
                let key = (pid.as_u32(), p.start_time());
                let meta = strings.entry(key).or_insert_with(|| {
                    (
                        Arc::from(clean(p.name().to_string_lossy().as_ref())),
                        Arc::from(clean(
                            &p.cmd()
                                .iter()
                                .map(|s| s.to_string_lossy())
                                .collect::<Vec<_>>()
                                .join(" "),
                        )),
                        Arc::from(
                            p.user_id()
                                .and_then(|uid| users.get_user_by_id(uid))
                                .map(|u| u.name())
                                .unwrap_or("unknown"),
                        ),
                        Arc::from(format!("{:?}", p.status())),
                    )
                });
                let u = p.disk_usage();
                let usage = process_usage(pid.as_u32());
                let (footprint, wakes) = if let Some((footprint, count)) = usage {
                    let rate = wakeups
                        .insert(key, count)
                        .map(|old| Avail::Ok(count.saturating_sub(old) as f32 / dt as f32))
                        .unwrap_or(Avail::Acquiring);
                    (Avail::Ok(footprint), rate)
                } else {
                    (
                        Avail::Permission("footprint restricted".into()),
                        Avail::Permission("wakeups restricted".into()),
                    )
                };
                out.push(Proc {
                    pid: pid.as_u32(),
                    start: p.start_time(),
                    parent: p.parent().map(|p| p.as_u32()).unwrap_or(0),
                    name: meta.0.clone(),
                    command: meta.1.clone(),
                    application: Arc::from(
                        p.exe()
                            .and_then(|path| path.to_str())
                            .and_then(|path| {
                                path.split_once(".app/Contents/")
                                    .map(|(app, _)| format!("{app}.app"))
                            })
                            .unwrap_or_default(),
                    ),
                    user: meta.2.clone(),
                    cpu: p.cpu_usage(),
                    gpu: Avail::Acquiring,
                    memory: p.memory(),
                    footprint,
                    wakeups: wakes,
                    read: u.read_bytes as f64 / dt,
                    write: u.written_bytes as f64 / dt,
                    runtime: p.run_time(),
                    cpu_time: p.accumulated_cpu_time(),
                    total_read: u.total_read_bytes,
                    total_write: u.total_written_bytes,
                    threads: process_threads(pid.as_u32()),
                    connections: socket_data
                        .as_ref()
                        .and_then(|d| d.counts.get(&key))
                        .copied()
                        .map(Avail::Ok)
                        .unwrap_or(Avail::Unsupported("no visible socket records".into())),
                    status: meta.3.clone(),
                });
            }
            strings.retain(|(pid, start), _| {
                sys.process(sysinfo::Pid::from_u32(*pid))
                    .is_some_and(|p| p.start_time() == *start)
            });
            wakeups.retain(|key, _| strings.contains_key(key));
            out.sort_by_key(|p| p.pid);
            process_gpu = gpu_collector.sample(&mut out);
            processes = Arc::new(out);
            process_at = s.at;
            last_process = Instant::now();
        }
        s.processes = processes.clone();
        s.process_at = process_at;
        s.process_gpu = process_gpu.clone();
        if last_battery.elapsed() >= Duration::from_secs(10) {
            battery = read_battery();
            last_battery = Instant::now();
        }
        s.battery = battery.clone();
        if last_power_mode.elapsed() >= Duration::from_secs(3) {
            power_mode = super::power_mode::read();
            last_power_mode = Instant::now();
        }
        s.power_mode = power_mode.clone();
        if let Some(rx) = &port_rx {
            match rx.try_recv() {
                Ok(state) => {
                    port_state = state;
                    ports_at = s.at;
                    port_rx = None;
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    port_state = Avail::Failed("port reader stopped".into());
                    port_rx = None;
                }
                Err(mpsc::TryRecvError::Empty) => {}
            }
        }
        if port_rx.is_none() && last_ports.elapsed() >= Duration::from_secs(3) {
            port_rx = Some(ports::request());
            last_ports = Instant::now();
        }
        s.ports = port_state.clone();
        s.ports_at = ports_at;

        s.uptime = System::uptime().unwrap_or(0);
        read_vm(&mut s);
        if let Some(now) = s.vm_totals.value() {
            if let Some(old) = previous_vm {
                s.vm_rates = Avail::Ok(std::array::from_fn(|i| {
                    now[i].saturating_sub(old[i]) as f64 / elapsed
                }));
            }
            previous_vm = Some(*now);
        }
        s.thermal = thermal();
        s.collector_ms = began.elapsed().as_secs_f64() * 1000.;
        if tx.send(s).is_err() {
            return Ok(());
        }
    }
}
#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
fn thermal() -> Avail<String> {
    use objc2_foundation::{NSProcessInfo, NSProcessInfoThermalState as T};
    let state = NSProcessInfo::processInfo().thermalState();
    Avail::Ok(
        match state {
            T::Nominal => "NOMINAL",
            T::Fair => "FAIR",
            T::Serious => "SERIOUS",
            T::Critical => "CRITICAL",
            _ => "UNKNOWN",
        }
        .into(),
    )
}
#[cfg(not(all(target_os = "macos", target_arch = "aarch64")))]
fn thermal() -> Avail<String> {
    Avail::Unsupported("thermal state unavailable".into())
}
fn read_vm(s: &mut Sample) {
    #[cfg(target_os = "macos")]
    {
        if let Ok(o) = std::process::Command::new("/usr/bin/vm_stat").output() {
            let text = String::from_utf8_lossy(&o.stdout);
            let page = text
                .lines()
                .next()
                .and_then(|l| l.split("page size of ").nth(1))
                .and_then(|p| p.split_whitespace().next())
                .and_then(|p| p.parse::<u64>().ok())
                .unwrap_or(16384);
            let count = |key: &str| {
                text.lines()
                    .find(|l| {
                        l.split(':')
                            .next()
                            .is_some_and(|name| name.trim_matches('"') == key.trim_end_matches(':'))
                    })
                    .and_then(|l| l.split(':').nth(1))
                    .and_then(|v| v.trim().trim_end_matches('.').parse::<u64>().ok())
                    .map(|n| n * page)
            };
            s.compressed_original = count("Pages stored in compressor:")
                .map(Avail::Ok)
                .unwrap_or_default();
            if let (Some(pi), Some(po), Some(si), Some(so), Some(faults)) = (
                count("Pageins:"),
                count("Pageouts:"),
                count("Swapins:"),
                count("Swapouts:"),
                count("Translation faults:"),
            ) {
                s.vm_totals = Avail::Ok([pi, po, si, so, faults / page]);
            }
            if let (Some(wired), Some(comp), Some(free)) = (
                count("Pages wired down:"),
                count("Pages occupied by compressor:"),
                count("Pages free:"),
            ) {
                let used = s.memory_used.min(s.machine.memory);
                let wired = wired.min(used);
                let comp = comp.min(used - wired);
                let free = free.min(s.machine.memory - used);
                s.memory = Avail::Estimated([
                    wired,
                    used - wired - comp,
                    comp,
                    s.machine.memory - used - free,
                    free,
                ]);
            }
        }
        s.pressure_percent = std::process::Command::new("/usr/sbin/sysctl")
            .args(["-n", "kern.memorystatus_level"])
            .output()
            .ok()
            .and_then(|o| String::from_utf8(o.stdout).ok())
            .and_then(|v| v.trim().parse::<f32>().ok())
            .map(|v| Avail::Estimated((100. - v).clamp(0., 100.)))
            .unwrap_or(Avail::Unsupported("pressure percentage unavailable".into()));
        s.pressure = std::process::Command::new("/usr/sbin/sysctl")
            .args(["-n", "kern.memorystatus_vm_pressure_level"])
            .output()
            .ok()
            .and_then(|o| String::from_utf8(o.stdout).ok())
            .and_then(|s| match s.trim() {
                "1" => Some("NORMAL"),
                "2" => Some("WARN"),
                "4" => Some("CRITICAL"),
                _ => None,
            })
            .map(|s| Avail::Ok(s.into()))
            .unwrap_or(Avail::Unsupported("memory pressure unavailable".into()));
    }
    if matches!(s.vm_totals, Avail::Acquiring) {
        s.vm_totals = Avail::Unsupported("VM counters unavailable".into());
        s.vm_rates = Avail::Unsupported("VM counters unavailable".into());
    }
    if matches!(s.memory, Avail::Acquiring) {
        s.memory = Avail::Unsupported("VM categories unavailable".into());
    }
}
#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
fn read_battery() -> Avail<Battery> {
    let Ok(o) = std::process::Command::new("/usr/sbin/ioreg")
        .args(["-a", "-r", "-c", "AppleSmartBattery"])
        .output()
    else {
        return Avail::Failed("battery query failed".into());
    };
    let Ok(v) = plist::Value::from_reader(std::io::Cursor::new(o.stdout)) else {
        return Avail::Failed("battery data unreadable".into());
    };
    let Some(d) = v
        .as_array()
        .and_then(|a| a.first())
        .and_then(|v| v.as_dictionary())
    else {
        return Avail::NoHardware("AC powered".into());
    };
    let n = |k: &str| {
        d.get(k).and_then(|v| {
            v.as_signed_integer()
                .map(|v| v as f64)
                .or_else(|| v.as_unsigned_integer().map(|v| v as f64))
        })
    };
    let b = |k: &str| d.get(k).and_then(|v| v.as_boolean()).unwrap_or(false);
    let Some(current) = n("CurrentCapacity") else {
        return Avail::Failed("battery capacity unreadable".into());
    };
    let max = n("MaxCapacity").unwrap_or(100.).max(1.);
    let watts = match (n("Voltage"), n("Amperage")) {
        (Some(v), Some(a)) => {
            let signed = if a > i64::MAX as f64 {
                (a as u64 as i64) as f64
            } else {
                a
            };
            Avail::Ok((v * signed / 1_000_000.) as f32)
        }
        _ => Avail::Unsupported("pack power unavailable".into()),
    };
    let nested = |key: &str| {
        d.get("BatteryData")
            .and_then(|v| v.as_dictionary())
            .and_then(|d| d.get(key))
            .and_then(|v| {
                v.as_signed_integer()
                    .map(|v| v as f64)
                    .or_else(|| v.as_unsigned_integer().map(|v| v as f64))
            })
    };
    let health = match (
        n("AppleRawMaxCapacity")
            .or_else(|| nested("FullChargeCapacity"))
            .or_else(|| nested("MaxCapacity")),
        n("DesignCapacity").or_else(|| nested("DesignCapacity")),
    ) {
        (Some(a), Some(b)) if b > 0. => Avail::Estimated((a / b * 100.).min(100.) as f32),
        _ => Avail::Unsupported("health unavailable".into()),
    };
    Avail::Ok(Battery {
        percent: (current / max * 100.).clamp(0., 100.) as f32,
        charging: b("IsCharging"),
        ac: b("ExternalConnected"),
        watts,
        health,
        cycles: n("CycleCount")
            .map(|n| Avail::Ok(n as u32))
            .unwrap_or(Avail::Unsupported("cycles unavailable".into())),
        minutes: n("TimeRemaining")
            .filter(|n| *n >= 0. && *n < 65535.)
            .map(|n| Avail::Estimated(n as u32))
            .unwrap_or(Avail::Unsupported("time unavailable".into())),
    })
}
#[cfg(not(all(target_os = "macos", target_arch = "aarch64")))]
fn read_battery() -> Avail<Battery> {
    Avail::Unsupported("battery collector requires Apple Silicon".into())
}

/// Revalidate process identity immediately before signalling; never elevate privileges.
pub fn send_signal(p: &Proc, force: bool) -> anyhow::Result<()> {
    anyhow::ensure!(
        p.pid > 1 && p.pid != std::process::id() && p.name.as_ref() != "kernel_task",
        "This process is protected"
    );
    let mut s = System::new()?;
    let pid = sysinfo::Pid::from_u32(p.pid);
    s.refresh_processes(ProcessesToUpdate::Some(&[pid]), true);
    let live = s
        .process(pid)
        .ok_or_else(|| anyhow::anyhow!("Process has exited"))?;
    anyhow::ensure!(
        live.start_time() == p.start,
        "PID was reused; no signal sent"
    );
    let signal = if force {
        sysinfo::Signal::Kill
    } else {
        sysinfo::Signal::Term
    };
    anyhow::ensure!(
        live.kill_with(signal) == Some(true),
        "Signal failed: permission denied or process exited"
    );
    Ok(())
}

fn clean(s: &str) -> String {
    s.chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect()
}

#[cfg(target_os = "macos")]
#[allow(deprecated)]
fn cpu_ticks() -> Option<[u32; 4]> {
    unsafe extern "C" {
        fn mach_port_deallocate(task: u32, name: u32) -> i32;
    }
    let mut info = libc::host_cpu_load_info { cpu_ticks: [0; 4] };
    let mut count = libc::HOST_CPU_LOAD_INFO_COUNT;
    // SAFETY: the exact Mach output struct and word count are supplied; the host port is released.
    unsafe {
        let host = libc::mach_host_self();
        let result = libc::host_statistics(
            host,
            libc::HOST_CPU_LOAD_INFO,
            (&mut info as *mut libc::host_cpu_load_info).cast(),
            &mut count,
        );
        #[allow(deprecated)]
        mach_port_deallocate(libc::mach_task_self(), host);
        (result == 0).then_some(info.cpu_ticks)
    }
}
#[cfg(not(target_os = "macos"))]
fn cpu_ticks() -> Option<[u32; 4]> {
    None
}
#[cfg(target_os = "macos")]
fn process_threads(pid: u32) -> Avail<u32> {
    // SAFETY: proc_pidinfo writes at most the size of this initialized C struct.
    unsafe {
        let mut info: libc::proc_taskinfo = std::mem::zeroed();
        let size = std::mem::size_of_val(&info) as i32;
        if libc::proc_pidinfo(
            pid as i32,
            libc::PROC_PIDTASKINFO,
            0,
            (&mut info as *mut libc::proc_taskinfo).cast(),
            size,
        ) == size
        {
            Avail::Ok(info.pti_threadnum.max(0) as u32)
        } else {
            Avail::Permission("task info restricted".into())
        }
    }
}
#[cfg(not(target_os = "macos"))]
fn process_threads(_: u32) -> Avail<u32> {
    Avail::Unsupported("thread count unavailable".into())
}

#[cfg(target_os = "macos")]
fn process_usage(pid: u32) -> Option<(u64, u64)> {
    // SAFETY: RUSAGE_INFO_V2 matches the initialized struct and writable buffer size.
    unsafe {
        let mut info: libc::rusage_info_v2 = std::mem::zeroed();
        let result = libc::proc_pid_rusage(
            pid as i32,
            libc::RUSAGE_INFO_V2,
            (&mut info as *mut libc::rusage_info_v2).cast(),
        );
        (result == 0).then_some((info.ri_phys_footprint, info.ri_interrupt_wkups))
    }
}
#[cfg(not(target_os = "macos"))]
fn process_usage(_: u32) -> Option<(u64, u64)> {
    None
}

#[cfg(test)]
mod topology_tests {
    use super::*;
    #[test]
    fn m4_pro_channels_preserve_cluster_boundaries() {
        for (name, expected) in [
            ("ECPU000", Some(0)),
            ("ECPU030", Some(0)),
            ("PCPU000", Some(0)),
            ("PCPU030", Some(0)),
            ("PCPU100", Some(1)),
            ("PCPU130", Some(1)),
            ("PCPU1_CPU0", None),
            ("PCPU0", None),
            ("PCPUxyz", None),
        ] {
            assert_eq!(m4_pro_cluster(name), expected);
        }
        let s = demo("balanced", 10.);
        let clusters = s.cpu_clusters();
        assert_eq!(
            clusters
                .iter()
                .map(|c| (c.tier.label.as_str(), c.id, c.cores.len()))
                .collect::<Vec<_>>(),
            [("P", Some(0), 4), ("P", Some(1), 4), ("E", Some(0), 4)]
        );
        assert_eq!(clusters[1].cores[0].channel, "PCPU100");
    }
}
