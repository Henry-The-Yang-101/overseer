use super::{
    kit::*,
    theme::{Theme, blend},
};
use crate::{
    app::{App, Part},
    metrics::Sample,
    motion::{Mood, face},
};
use ratatui::{buffer::Buffer, layout::Rect};
pub fn tint(p: Part, t: &Theme) -> ratatui::style::Color {
    match p {
        Part::Cpu | Part::Soc => t.cpu,
        Part::Gpu => t.gpu,
        Part::Ane => t.ane,
        Part::Memory => t.memory,
        Part::Disk => t.disk,
        Part::Network => t.net,
        Part::Battery | Part::Cell(_) => t.battery,
        _ => t.accent,
    }
}
pub fn readout(p: Part, s: &Sample, a: &App) -> String {
    match p {
        Part::Cpu | Part::Soc => percent(s.cpu),
        Part::Gpu => avail(&s.gpu, |v| percent(*v)),
        Part::Ane => watts(&s.power[2]),
        Part::Memory => format!(
            "{} / {}",
            bytes(s.memory_used, &a.cfg.units),
            bytes(s.machine.memory, &a.cfg.units)
        ),
        Part::Power => format!("Σ {}", watts(&s.total_power())),
        Part::Thermal => s
            .thermal
            .value()
            .cloned()
            .unwrap_or(s.thermal.label().into()),
        Part::Battery => avail(&s.battery, |b| {
            format!(
                "{:.0}% · {}",
                b.percent,
                if b.charging {
                    "charging"
                } else if b.ac {
                    "AC"
                } else {
                    "on battery"
                }
            )
        }),
        Part::Fan0 | Part::Fan1 => s
            .fans
            .value()
            .and_then(|f| f.get(if p == Part::Fan0 { 0 } else { 1 }))
            .map_or(s.fans.label().into(), |f| format!("{} rpm", f.rpm)),
        Part::Disk => s
            .disk
            .value()
            .map_or(s.disk.label().into(), |f| f.name.clone()),
        Part::Network => s
            .network
            .value()
            .map_or(s.network.label().into(), |f| f.name.clone()),
        Part::Cell(_) => String::new(),
    }
}
pub fn panel(b: &mut Buffer, r: Rect, p: Part, a: &mut App) {
    if r.is_empty() {
        return;
    }
    let s = a.sample();
    let t = a.theme.clone();
    let label = if r.width < 48 {
        match p {
            Part::Cpu => "CPU",
            Part::Gpu => "GPU",
            Part::Ane => "ANE",
            Part::Memory => "MEM",
            Part::Thermal => "THERMAL",
            Part::Battery => "BATTERY",
            _ => p.name(),
        }
        .to_uppercase()
    } else {
        p.name().to_uppercase()
    };
    let value = if p == Part::Memory && r.width < 48 {
        let base = if a.cfg.units.bytes == "si" {
            1e9
        } else {
            1024f64.powi(3)
        };
        format!(
            "{:.1}/{:.0}G",
            s.memory_used as f64 / base,
            s.machine.memory as f64 / base
        )
    } else {
        readout(p, &s, a)
    };
    let inner = frame(b, r, &label, &value, a.selection == Some(p), &t);
    a.parts.push((p, r));
    a.hitmap.add(r, 2, Hit::Part(p));
    content(b, inner, p, &s, a);
    if a.follow.is_some() && !matches!(p, Part::Cpu | Part::Soc | Part::Memory) {
        for y in r.y..r.bottom() {
            for x in r.x..r.right() {
                let cell = &mut b[(x, y)];
                cell.set_fg(blend(cell.fg, t.void, 0.45));
            }
        }
    }
    if a.selection == Some(p)
        && a.motion == crate::motion::Motion::Full
        && a.selected_at.elapsed().as_secs_f32() < 0.45
    {
        let age = a.selected_at.elapsed().as_secs_f32() / 0.45;
        let x = (age * r.width.saturating_sub(2) as f32) as u16;
        if let Some(cell) = b.cell_mut((r.x + 1 + x, r.y)) {
            cell.set_fg(t.bright);
        }
    }
}
pub fn content(b: &mut Buffer, r: Rect, p: Part, s: &Sample, a: &App) {
    if r.is_empty() {
        return;
    }
    let t = &a.theme;
    let ascii = a.cfg.ui.glyphs == "ascii";
    match p {
        Part::Cpu | Part::Soc => cpu(b, r, s, a),
        Part::Gpu => gpu(b, r, s, a),
        Part::Ane => ane(b, r, s, a),
        Part::Memory => memory(b, r, s, a),
        Part::Power => power(b, r, s, a),
        Part::Disk | Part::Network => {
            let flow = if p == Part::Disk { &s.disk } else { &s.network };
            if let Some(f) = flow.value() {
                let c = tint(p, t);
                line(
                    b,
                    r,
                    0,
                    format!(
                        "{} {:>12}   {} {:>12}",
                        if p == Part::Disk { "R" } else { "↓" },
                        rate(f.read, &a.cfg.units, p == Part::Network),
                        if p == Part::Disk { "W" } else { "↑" },
                        rate(f.write, &a.cfg.units, p == Part::Network)
                    ),
                    t.text,
                );
                if r.height > 1 {
                    trace(
                        b,
                        row(r, 1, 1),
                        ((f.read + f.write) / 1_000_000.) as f32,
                        a,
                        c,
                    );
                }
                if r.height > 2 {
                    if p == Part::Disk {
                        line(
                            b,
                            r,
                            2,
                            avail(&s.disk_capacity, |(used, total)| {
                                format!(
                                    "{} used of {}",
                                    bytes(*used, &a.cfg.units),
                                    bytes(*total, &a.cfg.units)
                                )
                            }),
                            t.muted,
                        );
                    } else {
                        line(
                            b,
                            r,
                            2,
                            format!(
                                "RX {:.0} pkt/s   TX {:.0} pkt/s   {} errors",
                                f.packets_in, f.packets_out, f.errors
                            ),
                            if f.errors > 0 { t.warn } else { t.muted },
                        );
                    }
                }
            } else {
                unavailable(b, r, flow, t);
            }
        }
        Part::Thermal => thermal(b, r, s, a),
        Part::Fan0 | Part::Fan1 => {
            if let Some(fan) = s
                .fans
                .value()
                .and_then(|f| f.get(if p == Part::Fan0 { 0 } else { 1 }))
            {
                rotor(
                    b,
                    row(r, 0, r.height.saturating_sub(2)),
                    a.anim.phases.get(&fan.name).copied().unwrap_or(0.),
                    t.accent,
                    ascii,
                );
                line(
                    b,
                    r,
                    r.height.saturating_sub(2),
                    format!(
                        "{} rpm · max {}",
                        fan.rpm,
                        fan.max.map_or("N/A".into(), |m| m.to_string())
                    ),
                    t.text,
                );
                line(b, r, r.height - 1, "Physical side assumed", t.muted);
            } else {
                unavailable(b, r, &s.fans, t);
            }
        }
        Part::Battery => battery(b, r, s, a),
        Part::Cell(_) => {}
    }
}
fn cpu(b: &mut Buffer, r: Rect, s: &Sample, a: &App) {
    let t = &a.theme;
    let ascii = a.cfg.ui.glyphs == "ascii";
    if s.tiers.is_empty() {
        line(b, r, 1, "Acquiring core telemetry…", t.muted);
        return;
    }
    let large = r.height >= 16 && r.width >= 60;
    let cols = (r.width / if large { 16 } else { 13 }).max(1);
    let unit_h = if large { 5 } else { 2 };
    let required: u16 = s
        .tiers
        .iter()
        .map(|t| 1 + (t.cores.len() as u16).div_ceil(cols) * unit_h)
        .sum();
    let compressed = required > r.height.saturating_sub(2);
    let mut y = 0;
    for tier in &s.tiers {
        if y >= r.height {
            break;
        }
        let tc = if tier.label.starts_with('E') {
            blend(t.cpu, t.text, 0.45)
        } else {
            t.cpu
        };
        let head = format!(
            "{} CLUSTER  ×{}   {}   ACT {}",
            tier.label,
            tier.cores.len(),
            freq(tier.mhz),
            percent(tier.active)
        );
        line(b, r, y, head, tc);
        y += 1;
        let columns = if compressed {
            (r.width / 12).max(1)
        } else {
            cols
        };
        let h = if compressed { 1 } else { unit_h };
        for (i, c) in tier.cores.iter().enumerate() {
            let x = (i as u16 % columns) * (r.width / columns);
            let cy = y + (i as u16 / columns) * h;
            if cy >= r.height.saturating_sub(1) {
                break;
            }
            let w = (r.width / columns).saturating_sub(1);
            let rect = Rect::new(r.x + x, r.y + cy, w, h.min(r.height - cy));
            let key = format!("{}:{}:{}", tier.label, c.die, c.id);
            let mood = if a.cursor.is_none() {
                a.anim.moods.get(&key).map_or(Mood::Idle, |m| m.mood)
            } else {
                let mut m = crate::motion::MoodTracker::default();
                for historical in a
                    .history
                    .iter()
                    .filter(|h| h.at >= s.at - 10. && h.at <= s.at)
                {
                    if let Some(core) = historical
                        .tiers
                        .iter()
                        .find(|t| t.label == tier.label)
                        .and_then(|t| t.cores.iter().find(|v| v.id == c.id && v.die == c.die))
                    {
                        m.update(core.active, historical.at);
                    }
                }
                m.mood
            };
            let face = face(&tier.label, mood);
            if large && !compressed {
                let inside = frame(b, rect, &format!("{}{}", tier.label, c.id), "", false, t);
                let density = (c.active * (inside.width.saturating_sub(6)) as f32) as usize;
                let phase = if a.motion == crate::motion::Motion::Full {
                    a.anim.phases.get(&key).copied().unwrap_or(0.) as usize
                } else {
                    0
                };
                let lane: String = (0..inside.width.saturating_sub(6) as usize)
                    .map(|j| {
                        if (j + phase) % (inside.width.saturating_sub(6).max(1) as usize) < density
                        {
                            if ascii { '#' } else { '▪' }
                        } else {
                            ' '
                        }
                    })
                    .collect();
                line(b, inside, 0, format!("{lane}{face}"), tc);
                let history: Vec<_> = a
                    .history
                    .iter()
                    .rev()
                    .filter(|h| h.at <= s.at)
                    .take(60)
                    .collect::<Vec<_>>()
                    .into_iter()
                    .rev()
                    .filter_map(|s| {
                        s.tiers
                            .iter()
                            .find(|t| t.label == tier.label)
                            .and_then(|t| t.cores.iter().find(|v| v.id == c.id && v.die == c.die))
                            .map(|c| c.active)
                    })
                    .collect();
                spark(b, row(inside, 1, 1), &history, 1., tc, t, ascii);
                line(
                    b,
                    inside,
                    2,
                    format!("{} {:3.0}%", freq(c.mhz).replace(" ", ""), c.active * 100.),
                    t.text,
                );
            } else {
                line(
                    b,
                    rect,
                    0,
                    format!("{}{}{}{:3.0}", tier.label, c.id, face, c.active * 100.),
                    tc,
                );
                if h > 1 {
                    meter(b, row(rect, 1, 1), c.active, tc, t, ascii);
                }
            }
        }
        y += (tier.cores.len() as u16).div_ceil(columns) * h;
    }
    if r.height > y + 2 {
        let hist = a.history_values(Part::Cpu);
        spark(
            b,
            Rect::new(r.x, r.y + y, r.width, r.height - y - 2),
            &hist,
            1.,
            t.cpu,
            t,
            ascii,
        );
    }
    let footer = format!(
        "CPU {}   {}   UP {}",
        watts(&s.power[0]),
        temp(&s.cpu_temp, &a.cfg.units),
        duration(s.uptime as f64)
    );
    line(b, r, r.height - 1, footer, t.muted);
    if let Some(p) = &a.preview_proc()
        && r.height > 2
    {
        line(
            b,
            r,
            r.height - 2,
            format!(
                "◎ {} · {:.1}% of machine capacity",
                p.name,
                p.cpu / s.core_count() as f32
            ),
            t.cpu,
        );
    }
}
fn gpu(b: &mut Buffer, r: Rect, s: &Sample, a: &App) {
    let t = &a.theme;
    let Some(v) = s.gpu.value() else {
        unavailable(b, r, &s.gpu, t);
        return;
    };
    let h = r.height.saturating_sub(2).max(1);
    let w = if r.width > 40 {
        r.width * 2 / 3
    } else {
        r.width
    };
    let n = w as usize * h as usize;
    let ranks = tile_ranks(n);
    let visual = if a.motion == crate::motion::Motion::Full && a.cursor.is_none() {
        a.anim.gpu.pos
    } else {
        *v
    };
    let lit = (visual * n as f32).round() as usize;
    let phase = if a.motion == crate::motion::Motion::Full {
        a.anim.phases.get("gpu").copied().unwrap_or(0.)
    } else {
        0.
    };
    for y in 0..h {
        for x in 0..w {
            let i = y as usize * w as usize + x as usize;
            let on = ranks[i] < lit;
            let glyph = if on {
                if (phase - (x + y) as f32 / 12.).sin() > 0.3 {
                    '█'
                } else {
                    '▓'
                }
            } else {
                '·'
            };
            text(
                b,
                r,
                x,
                y,
                glyph.to_string(),
                if on { t.gpu } else { t.wire },
            );
        }
    }
    if r.width > 40 {
        let rr = Rect::new(r.x + w + 2, r.y, r.width - w - 2, h);
        let mut grid = BrailleGrid::new(rr.width, rr.height);
        if *v > 0. {
            for y in 0..rr.height * 4 {
                for x in 0..rr.width * 2 {
                    let wave = ((x as f32 * 0.3 + phase).sin() + (y as f32 * 0.4 - phase).sin())
                        * 0.25
                        + 0.5;
                    if wave < *v {
                        grid.set(x as i32, y as i32, blend(t.wire, t.gpu, *v));
                    }
                }
            }
        }
        grid.flush(rr, b, a.cfg.ui.glyphs == "ascii");
    }
    if r.height >= 2 {
        line(
            b,
            r,
            r.height - 2,
            format!(
                "{}   {}",
                avail(&s.gpu_mhz, |m| freq(*m)),
                watts(&s.power[1])
            ),
            t.text,
        );
    }
    if r.height >= 3 {
        line(
            b,
            r,
            r.height - 1,
            format!("{} cores · aggregate utilization", s.machine.gpu_cores),
            t.muted,
        );
    }
}
pub fn tile_ranks(n: usize) -> Vec<usize> {
    let mut order: Vec<_> = (0..n).collect();
    let mut seed = 42u64;
    for i in (1..n).rev() {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        order.swap(i, seed as usize % (i + 1));
    }
    let mut rank = vec![0; n];
    for (i, v) in order.into_iter().enumerate() {
        rank[v] = i;
    }
    rank
}
fn ane(b: &mut Buffer, r: Rect, s: &Sample, a: &App) {
    let t = &a.theme;
    let Some(p) = s.power[2].value() else {
        unavailable(b, r, &s.power[2], t);
        return;
    };
    for y in 0..r.height.saturating_sub(2) {
        let mut out = String::new();
        for i in 0..r.width / 2 {
            out.push(if (*p / 4. * r.width as f32) as u16 > i {
                '◆'
            } else {
                '◇'
            });
            out.push('─');
        }
        line(b, r, y, out, if *p < 0.005 { t.wire } else { t.ane });
    }
    line(b, r, r.height.saturating_sub(2), watts(&s.power[2]), t.text);
    if r.height > 1 {
        line(
            b,
            r,
            r.height - 1,
            if *p < 0.005 { "IDLE" } else { "POWER ACTIVITY" },
            t.muted,
        );
    }
}
fn memory(b: &mut Buffer, r: Rect, s: &Sample, a: &App) {
    let t = &a.theme;
    let Some(m) = s.memory.value() else {
        unavailable(b, r, &s.memory, t);
        return;
    };
    let h = r
        .height
        .saturating_sub(if r.height >= 6 { 3 } else { 1 })
        .max(1);
    let n = r.width as usize * h as usize;
    let counts = crate::metrics::allocate_cells(*m, n);
    let followed = a
        .preview_proc()
        .as_ref()
        .map_or(0, |p| {
            ((p.footprint.value().copied().unwrap_or(p.memory) as f64
                / s.machine.memory.max(1) as f64)
                * n as f64)
                .round() as usize
        })
        .min(counts[1]);
    let glyphs = ['▣', '■', '⣿', '▒', '·'];
    let colors = [
        blend(t.memory, t.text, 0.4),
        t.memory,
        blend(t.memory, t.accent, 0.5),
        t.muted,
        t.wire,
    ];
    let mut i = 0;
    for category in 0..5 {
        for _ in 0..counts[category] {
            text(
                b,
                r,
                (i % r.width as usize) as u16,
                (i / r.width as usize) as u16,
                glyphs[category].to_string(),
                colors[category],
            );
            if category == 1
                && i >= counts[0]
                && i < counts[0] + followed
                && let Some(cell) = b.cell_mut((
                    r.x + (i % r.width as usize) as u16,
                    r.y + (i / r.width as usize) as u16,
                ))
            {
                cell.set_bg(t.wire).set_fg(t.bright);
            }
            i += 1;
        }
    }
    let pressure = s
        .pressure
        .value()
        .map_or(s.pressure.label(), String::as_str);
    let color = match pressure {
        "CRITICAL" => t.crit,
        "WARN" => t.warn,
        _ => t.ok,
    };
    line(
        b,
        r,
        h,
        format!("PRESSURE {pressure}   SWAP {}", bytes(s.swap, &a.cfg.units)),
        color,
    );
    if r.height >= 6 {
        line(
            b,
            r,
            h + 1,
            format!(
                "▣ Wired {}   ■ App {}",
                bytes(m[0], &a.cfg.units),
                bytes(m[1], &a.cfg.units)
            ),
            t.muted,
        );
        line(
            b,
            r,
            h + 2,
            format!(
                "⣿ Comp {}  ▒ Cache {}",
                bytes(m[2], &a.cfg.units),
                bytes(m[3], &a.cfg.units)
            ),
            t.muted,
        );
    }
}
fn power(b: &mut Buffer, r: Rect, s: &Sample, a: &App) {
    let t = &a.theme;
    if r.width < 52 && r.height <= 5 {
        line(
            b,
            r,
            0,
            format!(
                "CPU {}  GPU {}",
                watts(&s.power[0]).trim(),
                watts(&s.power[1]).trim()
            ),
            t.text,
        );
        if r.height > 2 {
            line(
                b,
                r,
                1,
                format!(
                    "ANE {}  RAM {}",
                    watts(&s.power[2]).trim(),
                    watts(&s.power[3]).trim()
                ),
                t.muted,
            );
        }
        line(
            b,
            r,
            r.height - 1,
            format!(
                "Σ {}  SYS {}",
                watts(&s.total_power()).trim(),
                watts(&s.system_power).trim()
            ),
            t.muted,
        );
        return;
    }
    let two = r.width >= 52;
    let column = if two { r.width / 2 } else { r.width };
    for (i, (label, c)) in [
        ("CPU", t.cpu),
        ("GPU", t.gpu),
        ("ANE", t.ane),
        ("DRAM", t.memory),
    ]
    .into_iter()
    .enumerate()
    {
        let x = if two { i as u16 % 2 * column } else { 0 };
        let y = if two { i as u16 / 2 } else { i as u16 };
        if y >= r.height.saturating_sub(1) {
            break;
        }
        let rr = Rect::new(r.x + x, r.y + y, column.saturating_sub(1), 1);
        line(b, rr, 0, format!("{label:4} {}", watts(&s.power[i])), c);
        if rr.width > 25 {
            trace(
                b,
                Rect::new(rr.x + 16, rr.y, rr.width - 16, 1),
                s.power[i].value().copied().unwrap_or(0.),
                a,
                c,
            );
        }
    }
    line(
        b,
        r,
        r.height - 1,
        format!(
            "Σ {}   SYS {}   ≈{:.2} Wh",
            watts(&s.total_power()),
            watts(&s.system_power),
            s.energy_wh
        ),
        t.muted,
    );
}
fn trace(b: &mut Buffer, r: Rect, p: f32, a: &App, c: ratatui::style::Color) {
    let t = &a.theme;
    let spacing = if p <= 0. {
        f32::INFINITY
    } else {
        (14. / (0.4 + 5.6 * ((p.max(0.05) / 0.05).ln() / (40f32 / 0.05).ln()).clamp(0., 1.)))
            .max(3.)
    };
    let phase = if a.motion == crate::motion::Motion::Full {
        a.anim.phases.get("power").copied().unwrap_or(0.)
    } else {
        0.
    };
    for x in 0..r.width {
        let pulse = p > 0. && (phase - x as f32).rem_euclid(spacing) < 1.;
        text(
            b,
            r,
            x,
            0,
            if pulse {
                "•"
            } else if p >= 1. {
                "━"
            } else {
                "─"
            },
            if pulse { c } else { t.wire },
        );
    }
}
fn thermal(b: &mut Buffer, r: Rect, s: &Sample, a: &App) {
    let t = &a.theme;
    line(
        b,
        r,
        0,
        format!(
            "CPU {}    GPU {}",
            temp(&s.cpu_temp, &a.cfg.units),
            temp(&s.gpu_temp, &a.cfg.units)
        ),
        t.text,
    );
    if let Some(fans) = s.fans.value() {
        if r.height >= 6 {
            let w = (r.width / fans.len().max(1) as u16).max(1);
            for (i, f) in fans.iter().enumerate() {
                let rect = Rect::new(r.x + i as u16 * w, r.y + 1, w.min(22), r.height - 3);
                rotor(
                    b,
                    rect,
                    a.anim.phases.get(&f.name).copied().unwrap_or(0.),
                    if f.rpm > 0 { t.accent } else { t.wire },
                    a.cfg.ui.glyphs == "ascii",
                );
                text(
                    b,
                    r,
                    i as u16 * w,
                    r.height - 2,
                    format!("FAN {i} {:>4} rpm", f.rpm),
                    t.muted,
                );
            }
        } else {
            line(
                b,
                r,
                1,
                fans.iter()
                    .enumerate()
                    .map(|(i, f)| format!("F{i} {:>4} rpm", f.rpm))
                    .collect::<Vec<_>>()
                    .join("   "),
                t.muted,
            );
        }
    } else {
        line(
            b,
            r,
            1,
            format!("{} · {}", s.fans.label(), s.fans.reason()),
            t.muted,
        );
    }
}
fn battery(b: &mut Buffer, r: Rect, s: &Sample, a: &App) {
    let t = &a.theme;
    let Some(bat) = s.battery.value() else {
        line(
            b,
            r,
            0,
            format!("{} · {}", s.battery.label(), s.battery.reason()),
            t.muted,
        );
        return;
    };
    let count = 6;
    let w = (r.width / count).max(1);
    for i in 0..count {
        let value = (bat.percent / 100. * 6. - i as f32).clamp(0., 1.);
        meter(
            b,
            Rect::new(r.x + i * w, r.y, w.saturating_sub(1), 1),
            value,
            if bat.percent <= 10. {
                t.warn
            } else {
                t.battery
            },
            t,
            a.cfg.ui.glyphs == "ascii",
        );
    }
    if r.height > 1 {
        line(
            b,
            r,
            1,
            format!(
                "{} {}   health {}",
                if bat.charging {
                    "IN"
                } else if bat.ac {
                    "AC"
                } else {
                    "OUT"
                },
                watts(&bat.watts),
                avail(&bat.health, |v| format!("{v:.0}%"))
            ),
            t.muted,
        );
    }
    if r.height > 2 {
        line(
            b,
            r,
            2,
            format!(
                "{} cycles   {}",
                avail(&bat.cycles, |v| v.to_string()),
                avail(&bat.minutes, |v| format!(
                    "{}:{:02} remaining",
                    v / 60,
                    v % 60
                ))
            ),
            t.muted,
        );
    }
}
