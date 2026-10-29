use super::*;
pub const SCENARIOS: &[&str] = &[
    "balanced",
    "all-idle",
    "one-core-hot",
    "gpu-burst",
    "memory-pressure",
    "fanless",
    "no-battery",
    "missing-sensors",
    "three-tier-cpu",
];
pub fn demo(scenario: &str, t: f64) -> Sample {
    let wave = |phase: f64| ((t * 0.13 + phase).sin() * 0.5 + 0.5) as f32;
    let idle = scenario == "all-idle";
    let mut tiers = Vec::new();
    let labels: Vec<(&str, usize, u32)> = if scenario == "three-tier-cpu" {
        vec![("P", 4, 3800), ("S", 6, 4800), ("E", 4, 2800)]
    } else {
        vec![("P", 8, 4512), ("E", 4, 2592)]
    };
    for (ti, (label, count, max)) in labels.iter().enumerate() {
        let cores: Vec<_> = (0..*count)
            .map(|i| {
                let active = if idle {
                    0.005 + wave(i as f64) * 0.012
                } else if scenario == "one-core-hot" {
                    if i == 2 && ti == 0 { 0.98 } else { 0.015 }
                } else {
                    (0.07 + wave(i as f64 * 0.81 + ti as f64) * 0.78)
                        * (if ti == 0 { 1. } else { 0.55 })
                };
                Core {
                    id: i,
                    die: 0,
                    active,
                    scaled: active * 0.82,
                    mhz: (972. + (*max as f32 - 972.) * active) as u32,
                }
            })
            .collect();
        let active = cores.iter().map(|c| c.active).sum::<f32>() / *count as f32;
        tiers.push(Tier {
            label: (*label).into(),
            mhz: (972. + (*max as f32 - 972.) * active) as u32,
            max_mhz: *max,
            freqs: vec![972, 1512, 2028, 2592, *max],
            active,
            scaled: active * 0.82,
            cores,
        });
    }
    let cpu = tiers
        .iter()
        .flat_map(|t| &t.cores)
        .map(|c| c.active)
        .sum::<f32>()
        / tiers.iter().map(|t| t.cores.len()).sum::<usize>() as f32;
    let gpu = if idle {
        0.
    } else if scenario == "gpu-burst" {
        0.55 + wave(2.) * 0.44
    } else {
        wave(2.) * 0.42
    };
    let total = 24 * 1024u64.pow(3);
    let mem = if scenario == "memory-pressure" {
        [3, 16, 4, 0, 1]
    } else {
        [3, 9, 2, 5, 5]
    }
    .map(|v| v * 1024u64.pow(3));
    let fans = if idle {
        0
    } else {
        (1200. + cpu * 2700.) as u32
    };
    let mut s = Sample {
        at: t,
        machine: Arc::new(Machine {
            chip: "Apple M4 Pro".into(),
            model: "Mac16,8".into(),
            gpu_cores: 16,
            memory: total,
            cpu_count: tiers.iter().map(|t| t.cores.len()).sum(),
        }),
        cpu,
        cpu_split: Avail::Ok([cpu * 0.78, cpu * 0.22, 1. - cpu]),
        load: [cpu as f64 * 12., 3.18, 2.76],
        tiers,
        gpu: Avail::Ok(gpu),
        gpu_scaled: Avail::Ok(gpu * 0.76),
        gpu_freqs: vec![338, 720, 1020, 1240, 1400, 1580],
        gpu_mhz: Avail::Ok((338. + gpu * 1000.) as u32),
        power: [
            Avail::Ok(0.3 + cpu * 22.),
            Avail::Ok(gpu * 13.),
            Avail::Ok(if idle { 0. } else { wave(4.) * 0.18 }),
            Avail::Ok(0.42),
            Avail::Ok(0.12),
        ],
        system_power: Avail::Ok(8. + cpu * 22. + gpu * 13.),
        cpu_temp: Avail::Ok(36. + cpu * 42.),
        gpu_temp: Avail::Ok(34. + gpu * 40.),
        thermal: Avail::Ok("NOMINAL".into()),
        fans: Avail::Ok(vec![
            Fan {
                name: "fan0".into(),
                rpm: fans,
                max: Some(5700),
            },
            Fan {
                name: "fan1".into(),
                rpm: fans.saturating_sub(12),
                max: Some(5700),
            },
        ]),
        memory_used: mem[0] + mem[1] + mem[2],
        memory: Avail::Ok(mem),
        pressure: Avail::Ok(
            if scenario == "memory-pressure" {
                "CRITICAL"
            } else {
                "NORMAL"
            }
            .into(),
        ),
        swap: if scenario == "memory-pressure" {
            3 * 1024u64.pow(3)
        } else {
            0
        },
        disk: Avail::Ok(Flow {
            read: if idle {
                0.
            } else {
                wave(3.) as f64 * 230_000_000.
            },
            write: if idle {
                0.
            } else {
                wave(8.) as f64 * 32_000_000.
            },
            name: "Macintosh HD".into(),
            ..Flow::default()
        }),
        disk_capacity: Avail::Ok((412_000_000_000, 994_000_000_000)),
        network: Avail::Ok(Flow {
            read: if idle {
                0.
            } else {
                wave(7.) as f64 * 9_000_000.
            },
            write: if idle {
                0.
            } else {
                wave(5.) as f64 * 1_600_000.
            },
            packets_in: wave(7.) as f64 * 6400.,
            packets_out: wave(5.) as f64 * 1100.,
            name: "en0".into(),
            errors: 0,
            total_read: 18_200_000_000,
            total_write: 2_100_000_000,
            addresses: vec!["192.0.2.12".into()],
        }),
        battery: Avail::Ok(Battery {
            percent: 82.,
            charging: true,
            ac: true,
            watts: Avail::Ok(18.4),
            health: Avail::Ok(97.),
            cycles: Avail::Ok(112),
            minutes: Avail::Ok(48),
        }),
        process_at: t,
        uptime: 273_528,
        collector_ms: 1000.,
        ..Sample::default()
    };
    s.pressure_percent = Avail::Estimated(if scenario == "memory-pressure" {
        92.
    } else {
        39.
    });
    s.compressed_original = Avail::Ok(mem[2] * 3);
    s.vm_rates = Avail::Ok([125000., 4096., 0., 0., 384.]);
    s.interfaces = vec![s.network.value().unwrap().clone()];
    s.volumes = vec![Volume {
        name: "Macintosh HD".into(),
        mount: "/".into(),
        used: 412_000_000_000,
        total: 994_000_000_000,
        external: false,
        read_only: true,
    }];
    s.connection_states = Avail::Ok(vec![
        ("ESTABLISHED".into(), 142),
        ("LISTEN".into(), 21),
        ("CLOSE_WAIT".into(), 2),
    ]);
    s.processes = Arc::new(
        [
            "WindowServer",
            "rustc",
            "Safari",
            "Code Helper",
            "overseer",
            "kernel_task",
            "mds_stores",
            "Music",
            "python3",
            "Finder",
            "Terminal",
            "launchd",
            "coreaudiod",
            "bird",
            "cloudd",
            "photolibraryd",
            "node",
            "com.apple.WebKit",
            "Spotlight",
            "Slack",
        ]
        .iter()
        .enumerate()
        .map(|(i, name)| Proc {
            pid: if i == 11 { 1 } else { 400 + i as u32 * 137 },
            start: 1700000000 + i as u64,
            parent: if i > 6 { 537 } else { 1 },
            name: Arc::from(*name),
            command: Arc::from(format!("/Applications/{name}")),
            user: Arc::from(if i == 0 || i == 5 || i == 11 {
                "root"
            } else {
                "demo"
            }),
            cpu: if idle {
                wave(i as f64) * 0.4
            } else {
                wave(i as f64) * 160. / (i + 1) as f32
            },
            memory: (((20 - i) * 87) as u64) * 1024 * 1024,
            read: if idle {
                0.
            } else {
                wave(i as f64 + 3.) as f64 * 23_000_000. / (i + 1) as f64
            },
            write: if idle {
                0.
            } else {
                wave(i as f64 + 8.) as f64 * 3_200_000. / (i + 1) as f64
            },
            total_read: 1_000_000_000 / (i + 1) as u64,
            total_write: 200_000_000 / (i + 1) as u64,
            cpu_time: (t * 1000. + i as f64 * 10000.) as u64,
            threads: Avail::Ok(6 + i as u32),
            connections: Avail::Ok(if i % 3 == 0 { (20 - i) as u32 } else { 0 }),
            runtime: 1200 + i as u64 * 42,
            status: Arc::from("Running"),
        })
        .collect(),
    );
    if scenario == "fanless" {
        s.fans = Avail::NoHardware("passive cooling".into());
        Arc::make_mut(&mut s.machine).model = "MacBook Air (demo)".into();
    }
    if scenario == "no-battery" {
        s.battery = Avail::NoHardware("AC powered".into());
        Arc::make_mut(&mut s.machine).model = "Mac mini (demo)".into();
    }
    if scenario == "three-tier-cpu" {
        Arc::make_mut(&mut s.machine).model = "Generic demo".into();
        Arc::make_mut(&mut s.machine).chip = "Three-tier CPU".into();
    }
    if scenario == "missing-sensors" {
        s.gpu = Avail::Unsupported("GPU residency not reported".into());
        s.gpu_mhz = Avail::Unsupported("GPU clock not reported".into());
        s.power = std::array::from_fn(|_| Avail::Unsupported("power counter not reported".into()));
        s.cpu_temp = Avail::Failed("CPU temperature read failed".into());
        s.gpu_temp = Avail::Unsupported("GPU temperature unavailable".into());
        s.system_power = normalize_system_power(0.);
        s.fans = Avail::Failed("SMC fan read failed".into());
    }
    s
}
