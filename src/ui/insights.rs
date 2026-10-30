use super::kit::*;
use crate::{
    app::{App, Layer, Page, WINDOWS},
    metrics::{Sample, insights},
};
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::Style,
    widgets::{Clear, Widget},
};
use unicode_width::UnicodeWidthChar;

pub fn app_groups(s: &Sample, page: Page) -> Vec<insights::AppGroup<'_>> {
    let mut groups = insights::groups(s);
    let score = |g: &insights::AppGroup<'_>| match page {
        Page::Memory => g.memory as f64,
        Page::Disk => g.read + g.write,
        Page::Network => g.download + g.upload,
        _ => g.cpu as f64,
    };
    groups.sort_by(|a, b| score(b).total_cmp(&score(a)).then(a.key.cmp(&b.key)));
    groups
}
pub fn wifi(s: &Sample) -> String {
    if s.extra.stale(s.at) {
        return "Wi-Fi: stale telemetry".into();
    }
    avail(&s.extra.wifi, |v| {
        format!(
            "{} {} dBm · CH {} · {} link",
            v.interface,
            v.rssi,
            v.channel,
            v.tx_mbps.map_or("N/A".into(), |n| format!("{n:.0} Mb/s"))
        )
    })
}
pub fn disk(s: &Sample) -> String {
    if s.extra.stale(s.at) {
        return "IOPS: stale telemetry".into();
    }
    avail(&s.extra.disks, |v| {
        v.first().map_or("IOPS: no matching devices".into(), |d| {
            format!("{} IOPS R {:.0} / W {:.0}", d.name, d.reads, d.writes)
        })
    })
}
pub fn power(s: &Sample) -> String {
    if s.extra.stale(s.at) {
        return "Power details: stale telemetry".into();
    }
    format!(
        "ADAPTER {} · {} sleep assertions",
        avail(&s.extra.adapter, |v| v
            .watts
            .map_or("watts N/A".into(), |w| format!("{w:.0} W rated"))),
        avail(&s.extra.assertions, |v| v.len().to_string())
    )
}
fn signed_bytes(v: i64, a: &App) -> String {
    format!(
        "{}{}",
        if v < 0 { "−" } else { "+" },
        bytes(v.unsigned_abs(), &a.cfg.units)
    )
}
pub fn growth(p: &crate::metrics::Proc, s: &Sample, a: &App, seconds: f64) -> String {
    insights::memory_growth(&a.history, s, p, seconds).map_or("N/A".into(), |n| signed_bytes(n, a))
}
fn details(a: &App, s: &Sample) -> Vec<String> {
    let mut lines = vec![format!(
        "{} · extended sample age {:.1}s",
        a.page.label().to_uppercase(),
        (s.at - s.extra.at).max(0.)
    )];
    if matches!(a.page, Page::Network | Page::Disk | Page::Energy) && s.extra.stale(s.at) {
        lines.push("Extended telemetry is stale; reopen after the collector recovers.".into());
        return lines;
    }
    match a.page {
        Page::Network => {
            lines.push(format!("WI-FI  {}", wifi(s)));
            if let Some(w) = s.extra.wifi.value() {
                lines.push(format!(
                    "Noise {} · SNR {}",
                    w.noise.map_or("N/A".into(), |n| format!("{n} dBm")),
                    w.noise
                        .map_or("N/A".into(), |n| format!("{} dB", w.rssi - n))
                ));
                lines.push(
                    "Link rate is the radio transmit rate, not measured internet speed.".into(),
                );
            } else {
                lines.push(s.extra.wifi.reason().into());
            }
            lines.push("".into());
            lines.push("PRIMARY ROUTES".into());
            if let Some(routes) = s.extra.routes.value() {
                if routes.is_empty() {
                    lines.push("No primary route reported.".into());
                }
                for route in routes {
                    lines.push(format!(
                        "{} · {} · gateway {}",
                        route.family, route.interface, route.gateway
                    ));
                }
            } else {
                lines.push(s.extra.routes.reason().into());
            }
            let tunnels: Vec<_> = s
                .interfaces
                .iter()
                .filter(|f| {
                    ["utun", "tun", "tap", "ppp", "ipsec", "wg"]
                        .iter()
                        .any(|p| f.name.starts_with(p))
                })
                .collect();
            lines.push("".into());
            lines.push("TUNNEL INTERFACES · presence does not prove VPN routing".into());
            if tunnels.is_empty() {
                lines.push("No tunnel interfaces reported.".into());
            }
            for f in tunnels {
                lines.push(format!(
                    "{} · RX {} · TX {}",
                    f.name,
                    rate(f.read, &a.cfg.units, true),
                    rate(f.write, &a.cfg.units, true)
                ));
            }
            lines.push("".into());
            lines.push(format!("PROCESS TRAFFIC · {}", s.extra.traffic.reason()));
            lines.push(
                "Rates use differences between ~5s nettop samples; loopback is excluded.".into(),
            );
            lines.push(
                "New processes need two samples. Missing or reset counters are unavailable.".into(),
            );
            lines.push(
                "Process totals and physical-interface totals have different accounting.".into(),
            );
        }
        Page::Disk => {
            lines.push("DEVICE OPERATIONS · ~5s sample · storage-driver counters".into());
            if let Some(disks) = s.extra.disks.value() {
                if disks.is_empty() {
                    lines.push("No matching devices yet; new devices need two samples.".into());
                }
                for disk in disks {
                    lines.push(format!(
                        "{} · READ {:.0} IOPS · WRITE {:.0} IOPS",
                        disk.name, disk.reads, disk.writes
                    ));
                    lines.push(format!(
                        "  Average operation: read {} · write {}",
                        disk.read_size
                            .map_or("— (idle)".into(), |n| bytes(n as u64, &a.cfg.units)),
                        disk.write_size
                            .map_or("— (idle)".into(), |n| bytes(n as u64, &a.cfg.units))
                    ));
                }
            } else {
                lines.push(s.extra.disks.reason().into());
            }
            lines.push("Each device is listed once; mounted disk images may also appear.".into());
            lines.push("The main throughput graph still uses the root volume.".into());
            lines.push("".into());
            lines.push("VOLUMES · APFS volumes can share capacity".into());
            for v in &s.volumes {
                lines.push(format!(
                    "{} · {} · {} free of {}",
                    v.name,
                    v.mount,
                    bytes(v.total.saturating_sub(v.used), &a.cfg.units),
                    bytes(v.total, &a.cfg.units)
                ));
            }
        }
        Page::Energy => {
            lines.push(power(s));
            if let Some(adapter) = s.extra.adapter.value() {
                if let Some(ma) = adapter.milliamps {
                    lines.push(format!("Adapter reported current capability: {ma:.0} mA"));
                }
            } else {
                lines.push(s.extra.adapter.reason().into());
            }
            lines.push(
                "Adapter capability is separate from measured system and battery power.".into(),
            );
            lines.push("".into());
            lines.push("SLEEP BLOCKERS · active system/display sleep assertions".into());
            if let Some(assertions) = s.extra.assertions.value() {
                if assertions.is_empty() {
                    lines.push("No active sleep-blocking assertions reported.".into());
                }
                for v in assertions {
                    let name = s
                        .processes
                        .iter()
                        .find(|p| p.pid == v.pid)
                        .map_or("process unavailable", |p| &p.name);
                    lines.push(format!("{name} · PID {} · {}", v.pid, v.kind));
                    lines.push(format!("  {}", v.reason));
                }
            } else {
                lines.push(s.extra.assertions.reason().into());
            }
            lines.push("Assertions can be intentional, for example while playing audio.".into());
        }
        Page::Memory => {
            lines.push(
                "FOOTPRINT CHANGE · retained ~15s process history; N/A until enough history".into(),
            );
            if let Some(p) = a
                .selected_process
                .and_then(|key| s.processes.iter().find(|p| p.key() == key))
            {
                lines.push(format!("{} · PID {}", p.name, p.pid));
                for minutes in [1, 5, 15] {
                    lines.push(format!(
                        "  {minutes:>2}m change: {}",
                        growth(p, s, a, minutes as f64 * 60.)
                    ));
                }
            } else {
                lines.push(
                    "Select a task before opening this panel for its 1/5/15m changes.".into(),
                );
            }
            lines.push("".into());
            let span = if s.process_at - a.history.front().map_or(s.process_at, |h| h.process_at)
                >= 300.
            {
                300.
            } else {
                60.
            };
            lines.push(format!(
                "LARGEST GROWTH · last {:.0}m · increases do not prove a leak",
                span / 60.
            ));
            let mut changes: Vec<_> = s
                .processes
                .iter()
                .filter_map(|p| insights::memory_growth(&a.history, s, p, span).map(|n| (p, n)))
                .collect();
            changes.sort_by_key(|(_, n)| std::cmp::Reverse(*n));
            if changes.is_empty() {
                lines.push("Not enough retained process history yet.".into());
            }
            for (p, n) in changes.into_iter().take(12) {
                lines.push(format!(
                    "{:>12}  {} ({})",
                    signed_bytes(n, a),
                    p.name,
                    p.pid
                ));
            }
        }
        Page::Cpu | Page::Gpu => {
            lines.push("History: time-weighted averages, peaks, and threshold durations.".into());
            lines.push("Apps: expandable application totals, sorted by this resource.".into());
            if a.page == Page::Gpu {
                lines.push("Per-app GPU counters remain unavailable; Apps sorts by CPU.".into());
            }
            lines.push(
                "Use ← / → to switch tabs. The snapshot stays pinned while this panel is open."
                    .into(),
            );
        }
    }
    lines
}
fn history(a: &App, s: &Sample) -> Vec<String> {
    let span = WINDOWS[a.window] as f64;
    let mut lines = vec![
        format!(
            "HISTORY · {:.0}m window ending at this snapshot · [ ] change window",
            span / 60.
        ),
        "Time-weighted samples; missing data and gaps longer than 3s are excluded.".into(),
        "".into(),
    ];
    type Metric = (&'static str, fn(&Sample) -> Option<f64>, f64, &'static str);
    let metrics: [Metric; 6] = [
        (
            "CPU",
            |s| (!s.tiers.is_empty()).then_some(s.cpu as f64 * 100.),
            80.,
            "%",
        ),
        ("GPU", |s| s.gpu.value().map(|v| *v as f64 * 100.), 80., "%"),
        (
            "Memory used",
            |s| (s.machine.memory > 0).then_some(s.memory_ratio() as f64 * 100.),
            90.,
            "%",
        ),
        (
            "System power",
            |s| s.system_power.value().map(|v| *v as f64),
            30.,
            "W",
        ),
        (
            "Disk R+W",
            |s| s.disk.value().map(|f| (f.read + f.write) / 1_048_576.),
            100.,
            "MiB/s",
        ),
        (
            "Network RX+TX",
            |s| s.network.value().map(|f| (f.read + f.write) / 1_048_576.),
            10.,
            "MiB/s",
        ),
    ];
    for (name, value, threshold, unit) in metrics {
        let sum = insights::summary(&a.history, s.at, span, value, threshold);
        if sum.coverage == 0. {
            lines.push(format!("{name}: unavailable / insufficient history"));
            continue;
        }
        lines.push(format!(
            "{name}: avg {:.1} {unit} · peak {:.1} {unit}",
            sum.average, sum.peak
        ));
        lines.push(format!(
            "  ≥ {threshold:.0} {unit} for {} · {} measured",
            duration(sum.above),
            duration(sum.coverage)
        ));
    }
    let pressure = insights::summary(
        &a.history,
        s.at,
        span,
        |s| {
            s.pressure
                .value()
                .map(|v| if v == "NORMAL" { 0. } else { 1. })
        },
        1.,
    );
    if pressure.coverage > 0. {
        lines.push(format!(
            "Elevated memory pressure: {} of {} measured",
            duration(pressure.above),
            duration(pressure.coverage)
        ));
    }
    lines.push("".into());
    lines.push("EVENTS · also marked on the Systems timeline".into());
    let events: Vec<_> = a
        .system_events
        .iter()
        .filter(|e| e.at <= s.at && e.at >= s.at - span)
        .collect();
    if events.is_empty() {
        lines.push("No recorded state changes in this window.".into());
    }
    for e in events.iter().rev() {
        lines.push(format!("−{}  {}", duration(s.at - e.at), e.text));
    }
    lines
}
fn wrapped(lines: Vec<String>, width: u16) -> Vec<String> {
    let mut result = vec![];
    for line in lines {
        let mut current = String::new();
        let mut used = 0;
        for c in line.chars().filter(|c| !c.is_control()) {
            let w = c.width().unwrap_or(0);
            if used + w > width as usize && !current.is_empty() {
                result.push(std::mem::take(&mut current));
                used = 0;
            }
            current.push(c);
            used += w;
        }
        result.push(current);
    }
    result
}
pub fn modal(b: &mut Buffer, rect: Rect, a: &mut App) {
    let Some(Layer::SystemsInfo {
        tab,
        mut scroll,
        mut index,
        expanded,
        sample,
    }) = a.layers.last().cloned()
    else {
        return;
    };
    let t = &a.theme;
    Clear.render(rect, b);
    let r = frame(b, rect, "SYSTEMS DETAILS", "Esc close", true, t);
    line(
        b,
        r,
        0,
        [
            "[Stats]    History    Apps",
            "Stats    [History]    Apps",
            "Stats    History    [Apps]",
        ][tab],
        t.accent,
    );
    line(
        b,
        r,
        1,
        format!(
            "Snapshot at +{} · close / reopen to refresh",
            duration(sample.at)
        ),
        t.muted,
    );
    let body = Rect::new(r.x, r.y + 3, r.width, r.height.saturating_sub(4));
    let mut selected_line = None;
    let lines = if tab == 2 {
        let groups = app_groups(&sample, a.page);
        index = index.min(groups.len().saturating_sub(1));
        let mut lines = vec![
            "CPU is summed core %. RSS can double-count shared memory.".into(),
            "Network coverage counts measured processes; missing totals stay N/A.".into(),
            "".into(),
        ];
        for (i, g) in groups.iter().enumerate() {
            if i == index {
                selected_line = Some(lines.len());
            }
            lines.push(format!(
                "{} {} {} · {} tasks",
                if i == index { "›" } else { " " },
                if expanded.contains(&g.key) {
                    "▾"
                } else {
                    "▸"
                },
                g.name,
                g.processes.len()
            ));
            lines.push(format!(
                "    CPU {:.1}% · RSS {} · DISK {}",
                g.cpu,
                bytes(g.memory, &a.cfg.units),
                rate(g.read + g.write, &a.cfg.units, false)
            ));
            lines.push(if g.traffic_count == 0 {
                "    NET N/A".into()
            } else {
                format!(
                    "    RX {} · TX {} · coverage {}/{}",
                    rate(g.download, &a.cfg.units, true),
                    rate(g.upload, &a.cfg.units, true),
                    g.traffic_count,
                    g.processes.len()
                )
            });
            if expanded.contains(&g.key) {
                for p in &g.processes {
                    lines.push(format!(
                        "      {} ({}) · {:.1}% CPU · {} RSS",
                        p.name,
                        p.pid,
                        p.cpu,
                        bytes(p.memory, &a.cfg.units)
                    ));
                }
            }
        }
        lines
    } else {
        wrapped(
            if tab == 0 {
                details(a, &sample)
            } else {
                history(a, &sample)
            },
            body.width,
        )
    };
    if let Some(selected) = selected_line
        && scroll == usize::MAX
    {
        scroll = selected.saturating_sub(3);
    }
    scroll = scroll.min(lines.len().saturating_sub(body.height as usize));
    for (i, text0) in lines
        .iter()
        .enumerate()
        .skip(scroll)
        .take(body.height as usize)
    {
        let rr = row(body, (i - scroll) as u16, 1);
        if Some(i) == selected_line {
            b.set_style(rr, Style::new().bg(t.substrate));
        }
        line(
            b,
            rr,
            0,
            text0,
            if Some(i) == selected_line {
                t.accent
            } else {
                t.text
            },
        );
    }
    line(
        b,
        r,
        r.height.saturating_sub(1),
        format!(
            "←→ tabs · ↑↓ {} · {} / {}",
            if tab == 2 {
                "apps · Enter expand · PgUp/Dn scroll"
            } else {
                "scroll"
            },
            (scroll + 1).min(lines.len()),
            lines.len()
        ),
        t.muted,
    );
    if let Some(Layer::SystemsInfo {
        scroll: dest,
        index: idx,
        ..
    }) = a.layers.last_mut()
    {
        *dest = scroll;
        *idx = index;
    }
}
