use super::{kit::*, processes, theme::blend, widgets};
use crate::{
    app::{App, Layer, Part, filtered_commands, metric},
    motion::Motion,
};
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::Style,
    widgets::{Clear, Paragraph, Widget, Wrap},
};
pub fn centered(r: Rect, w: u16, h: u16) -> Rect {
    let (w, h) = (w.min(r.width), h.min(r.height));
    Rect::new(r.x + (r.width - w) / 2, r.y + (r.height - h) / 2, w, h)
}
pub fn draw(b: &mut Buffer, r: Rect, a: &mut App) {
    let t = a.theme.clone();
    if let Some(layer) = a.layers.last().cloned() {
        let dim = if matches!(layer, Layer::Process { .. }) && r.width >= 98 {
            a.task_rect
        } else {
            r
        };
        for y in dim.y..dim.bottom() {
            for x in dim.x..dim.right() {
                let c = &mut b[(x, y)];
                c.set_fg(blend(c.fg, t.void, 0.55));
            }
        }
        match layer {
            Layer::Help => help(b, centered(r, 108, 28), a),
            Layer::Ports => super::ports::modal(b, centered(r, 72, 25), a),
            Layer::SystemsInfo { .. } => super::insights::modal(b, centered(r, 116, 34), a),
            Layer::Palette { query, index } => {
                let rect = centered(r, 76, 18);
                Clear.render(rect, b);
                let rr = frame(b, rect, "COMMAND PALETTE", "Esc back", true, &t);
                line(b, rr, 0, format!("› {query}▏"), t.accent);
                line(
                    b,
                    rr,
                    1,
                    "Find a view, component, setting, or action",
                    t.muted,
                );
                let commands = filtered_commands(&query);
                let index = index.min(commands.len().saturating_sub(1));
                if let Some(Layer::Palette { index: i, .. }) = a.layers.last_mut() {
                    *i = index;
                }
                let visible = rr.height.saturating_sub(4) as usize;
                let offset = index.saturating_sub(visible.saturating_sub(1));
                for (i, c) in commands.iter().skip(offset).take(visible).enumerate() {
                    let row0 = row(rr, i as u16 + 3, 1);
                    if i + offset == index {
                        b.set_style(row0, Style::new().bg(t.substrate));
                    }
                    line(
                        b,
                        row0,
                        0,
                        format!("{} {}", if i + offset == index { "›" } else { " " }, c.name),
                        if i + offset == index {
                            t.bright
                        } else {
                            t.text
                        },
                    );
                    text(
                        b,
                        row0,
                        row0.width.saturating_sub(c.key.len() as u16 + 1),
                        0,
                        c.key,
                        t.muted,
                    );
                }
                if commands.is_empty() {
                    line(b, rr, 4, "No matching commands", t.muted);
                }
                line(
                    b,
                    rr,
                    rr.height - 1,
                    "↑↓ choose   Enter run   Esc close",
                    t.muted,
                );
            }
            Layer::Inspector(p) => detail(b, centered(r, 112, 32), a, p, false),
            Layer::Signal { process, force } => {
                let rect = centered(r, 72, 15);
                Clear.render(rect, b);
                let rr = frame(b, rect, "SEND SIGNAL", "Esc cancel", true, &t);
                line(
                    b,
                    rr,
                    1,
                    format!("{} · PID {} · {}", process.name, process.pid, process.user),
                    t.text,
                );
                line(
                    b,
                    rr,
                    3,
                    format!(
                        "CPU {:.1}% · MEM {} · up {}",
                        process.cpu,
                        bytes(process.memory, &a.cfg.units),
                        duration(process.runtime as f64)
                    ),
                    t.muted,
                );
                line(
                    b,
                    rr,
                    5,
                    format!(
                        "{} SIGTERM  Ask the process to quit",
                        if !force { "›" } else { " " }
                    ),
                    if !force { t.accent } else { t.muted },
                );
                line(
                    b,
                    rr,
                    7,
                    format!(
                        "{} SIGKILL  Force quit; unsaved work may be lost",
                        if force { "›" } else { " " }
                    ),
                    if force { t.crit } else { t.muted },
                );
                line(
                    b,
                    rr,
                    10,
                    if force {
                        "Y confirm force quit   ↑↓ choose   Esc cancel"
                    } else {
                        "y confirm   ↑↓ choose   Esc cancel"
                    },
                    if force { t.crit } else { t.accent },
                );
                line(b, rr, 11, "Enter does not send a signal.", t.muted);
            }
            Layer::Process { process, tab } => {
                let rect = if r.width < 98 {
                    r
                } else {
                    let w = r.width * if r.width >= 148 { 40 } else { 55 } / 100;
                    Rect::new(r.right() - w, a.task_rect.y, w, a.task_rect.height)
                };
                Clear.render(rect, b);
                let rr = frame(
                    b,
                    rect,
                    &format!("{} / PID {}", process.name, process.pid),
                    "Esc back",
                    true,
                    &t,
                );
                let tabs = ["Overview", "History", "Tree", "Files", "Network"];
                let mut x = 0;
                for (i, label) in tabs.iter().enumerate() {
                    text(
                        b,
                        rr,
                        x,
                        0,
                        format!(" {label} "),
                        if i == tab { t.accent } else { t.muted },
                    );
                    x += label.len() as u16 + 3;
                }
                let body = Rect::new(rr.x, rr.y + 2, rr.width, rr.height.saturating_sub(3));
                let sample = a.sample();
                let current = sample
                    .processes
                    .iter()
                    .find(|p| p.key() == process.key())
                    .unwrap_or(&process);
                match tab {
                    0 => processes::overview(b, body, current, a),
                    1 => {
                        line(b, body, 0, "CPU · one core = 100%", t.cpu);
                        let history: Vec<_> = a
                            .history
                            .iter()
                            .filter(|s| s.at <= sample.at)
                            .filter_map(|s| {
                                s.processes
                                    .iter()
                                    .find(|p| p.key() == current.key())
                                    .map(|p| p.cpu)
                            })
                            .collect();
                        let max = history.iter().copied().fold(100., f32::max);
                        spark(
                            b,
                            Rect::new(body.x, body.y + 2, body.width, body.height / 3),
                            &history,
                            max,
                            t.cpu,
                            &t,
                            a.cfg.ui.glyphs == "ascii",
                        );
                        let y = body.height / 2;
                        line(b, body, y, "MEMORY · resident bytes", t.memory);
                        let mem: Vec<_> = a
                            .history
                            .iter()
                            .filter(|s| s.at <= sample.at)
                            .filter_map(|s| {
                                s.processes
                                    .iter()
                                    .find(|p| p.key() == current.key())
                                    .map(|p| p.memory as f32)
                            })
                            .collect();
                        let max = mem.iter().copied().fold(1., f32::max);
                        spark(
                            b,
                            Rect::new(
                                body.x,
                                body.y + y + 2,
                                body.width,
                                body.height.saturating_sub(y + 3),
                            ),
                            &mem,
                            max,
                            t.memory,
                            &t,
                            a.cfg.ui.glyphs == "ascii",
                        );
                    }
                    2 => {
                        line(b, body, 0, "ANCESTORS / CHILDREN", t.muted);
                        let mut chain = vec![];
                        let mut parent = current.parent;
                        let mut seen = std::collections::HashSet::new();
                        while parent > 0 && seen.insert(parent) && chain.len() < 8 {
                            if let Some(p) = sample.processes.iter().find(|p| p.pid == parent) {
                                chain.push(p);
                                parent = p.parent;
                            } else {
                                break;
                            }
                        }
                        let mut y = 2;
                        for p in chain.into_iter().rev() {
                            line(b, body, y, format!("{} #{}", p.name, p.pid), t.muted);
                            y += 1;
                        }
                        line(
                            b,
                            body,
                            y,
                            format!("› {} #{}", current.name, current.pid),
                            t.accent,
                        );
                        y += 1;
                        for p in sample.processes.iter().filter(|p| p.parent == current.pid) {
                            line(
                                b,
                                body,
                                y,
                                format!("  └ {} #{} · {:.1}%", p.name, p.pid, p.cpu),
                                t.text,
                            );
                            y += 1;
                        }
                    }
                    3 | 4 => {
                        line(
                            b,
                            body,
                            1,
                            if tab == 3 { "OPEN FILES" } else { "SOCKETS" },
                            t.accent,
                        );
                        if a.cursor.is_some() {
                            line(
                                b,
                                body,
                                3,
                                "Files and sockets are not retained in history.",
                                t.muted,
                            );
                        } else if a.demo.is_some() {
                            line(
                                b,
                                body,
                                3,
                                "Synthetic demo: no real process access.",
                                t.muted,
                            );
                        } else if let Some(data) = &a.detail_data {
                            if data.key == process.key() && data.tab == tab {
                                if let Some(lines) = data.lines.value() {
                                    let count = body.height.saturating_sub(4) as usize;
                                    let offset =
                                        a.detail_scroll.min(lines.len().saturating_sub(count));
                                    for (i, l) in lines.iter().skip(offset).take(count).enumerate()
                                    {
                                        line(
                                            b,
                                            body,
                                            i as u16 + 3,
                                            truncate(l, body.width as usize),
                                            if i == 0 && offset == 0 {
                                                t.muted
                                            } else {
                                                t.text
                                            },
                                        );
                                    }
                                } else {
                                    line(
                                        b,
                                        body,
                                        3,
                                        format!("{} · {}", data.lines.label(), data.lines.reason()),
                                        t.muted,
                                    );
                                }
                            } else {
                                line(b, body, 3, "Reading process details…", t.muted);
                            }
                        } else {
                            line(b, body, 3, "Reading process details…", t.muted);
                        }
                    }
                    _ => {}
                }
                line(
                    b,
                    rr,
                    rr.height - 1,
                    "←→ tabs   ↑↓ task   PgUp/Dn scroll   Esc close",
                    t.muted,
                );
            }
        }
    }
    if a.layers.is_empty() {
        let mut y = r.y;
        for toast in a.toasts.iter().filter(|t| t.key != "info").take(3) {
            let width = (toast.text.len() as u16 + 5).clamp(32, 72).min(r.width);
            let rect = Rect::new(r.right() - width, y, width, 4.min(r.height));
            Clear.render(rect, b);
            let rr = frame(
                b,
                rect,
                if toast.critical { "ALERT" } else { "NOTICE" },
                "",
                false,
                &t,
            );
            paragraph(
                b,
                rr,
                &toast.text,
                if toast.critical { t.crit } else { t.text },
            );
            y += 4;
            if y + 4 > r.bottom() {
                break;
            }
        }
    }
}
fn paragraph(b: &mut Buffer, r: Rect, text: &str, c: ratatui::style::Color) {
    Paragraph::new(text)
        .wrap(Wrap { trim: true })
        .style(Style::new().fg(c))
        .render(r, b);
}
fn help(b: &mut Buffer, r: Rect, a: &App) {
    let t = &a.theme;
    Clear.render(r, b);
    let rr = frame(b, r, "OVERSEER / CONTROLS", "Esc close", true, t);
    let entries = vec![
        "1 Teardown · 2 Systems · v switches between them",
        "",
        "SYSTEMS   ← → or Tab / Shift+Tab switches resource pages",
        "          ↑ ↓ selects tasks · Enter opens the inspector",
        "          / filters · s sort · S reverse · T tree · ⇧← ⇧→ folds tree",
        "          z expands visualizer · f pins task · X opens signal prompt",
        "DRAWER    ← → tabs · ↑ ↓ tasks · PgUp/PgDn scroll · Esc closes",
        "TEARDOWN  Tab selects · arrows navigate · g opens resource page",
        "          Enter inspects · z zooms · m telemetry cells · L labels",
        "          H highlights · o mirrors · p connected ports",
        "",
        "HISTORY   Space pause / play · , . step · < > step ten",
        "          { } spikes · [ ] window · + - speed · End returns live",
        "LOOK      t theme · a motion · : command palette · ? help",
        "MOUSE     Click page / part / task · double-click inspect",
        "          Drag timeline to scrub · wheel to scroll tasks",
        "",
        "VALUES    ≈ estimate · N/A unavailable · zero means measured zero",
        "SIGNALS   y sends SIGTERM; Y sends SIGKILL. Enter never confirms.",
        "          Signals disabled in demo / replay. PID identity is checked.",
        "Esc backs out one layer. q or Ctrl+C quits and restores the terminal.",
    ];
    for (i, l) in entries.iter().enumerate() {
        line(
            b,
            rr,
            i as u16 + 1,
            l,
            if i == 0 { t.accent } else { t.text },
        );
    }
}
pub fn boot(b: &mut Buffer, r: Rect, a: &App) {
    let t = &a.theme;
    let rr = frame(
        b,
        centered(r, 88, 18),
        "O V E R S E E R",
        "POWER-ON SELF TEST",
        true,
        t,
    );
    let s = a.sample();
    line(
        b,
        rr,
        1,
        if s.machine.chip.is_empty() {
            "Connecting to hardware telemetry…".into()
        } else {
            format!("{} · {}", s.machine.chip, s.machine.model)
        },
        t.text,
    );
    let rows = [
        (
            "SOC TOPOLOGY",
            if s.tiers.is_empty() {
                "ACQUIRING".into()
            } else {
                format!("{} cores / {} tiers", s.core_count(), s.tiers.len())
            },
        ),
        ("IOREPORT POWER", s.power[0].reason().into()),
        ("SMC TEMPERATURE", s.cpu_temp.reason().into()),
        ("SYSTEM POWER", s.system_power.reason().into()),
        ("FAN CONTROLLER", s.fans.reason().into()),
        ("BATTERY", s.battery.reason().into()),
        ("MEMORY PRESSURE", s.pressure.reason().into()),
        ("PER-PROCESS GPU", "not exposed".into()),
    ];
    let visible = if a.motion == Motion::Full {
        (a.started.elapsed().as_secs_f32() * 10.) as usize
    } else {
        8
    };
    for (i, (key, value)) in rows.iter().take(visible).enumerate() {
        line(
            b,
            rr,
            3 + i as u16,
            format!("POST  {key:20}  {value}"),
            t.muted,
        );
    }
    meter(
        b,
        row(rr, 12, 1),
        if a.history.is_empty() {
            (a.started.elapsed().as_secs_f32() / 2.).min(0.9)
        } else {
            1.
        },
        t.accent,
        t,
        false,
    );
    line(
        b,
        rr,
        14,
        "Any key skips · sensors continue sampling in the background",
        t.muted,
    );
}
pub fn detail(b: &mut Buffer, r: Rect, a: &mut App, part: Part, zoom: bool) {
    let t = a.theme.clone();
    let p = a.resolve(part);
    if matches!(p, Part::SpeakerLeft | Part::SpeakerRight) {
        super::audio::inspector(b, r, p, a);
        return;
    }
    if matches!(p, Part::Fan0 | Part::Fan1) {
        super::fan::inspector(b, r, a, zoom);
        return;
    }
    let s = a.sample();
    let cores: Vec<_> = s
        .tiers
        .iter()
        .flat_map(|t| t.cores.iter().map(move |c| (t, c)))
        .collect();
    let core = if matches!(p, Part::Cpu | Part::Soc) && !zoom {
        cores.get(a.inspect_core % cores.len().max(1)).copied()
    } else {
        None
    };
    let active = core.map_or(s.cpu, |(_, c)| c.active);
    Clear.render(r, b);
    let inside = frame(
        b,
        r,
        &core.map_or_else(
            || p.name().to_uppercase(),
            |(_, c)| format!("{} / {}", p.name().to_uppercase(), c.channel),
        ),
        if zoom {
            "ZOOM · ← → siblings · Esc back"
        } else {
            if core.is_some() {
                "← → core · Esc back"
            } else {
                "INSPECTOR · Esc back"
            }
        },
        true,
        &t,
    );
    let wide = inside.width >= 85;
    let header_h = if wide {
        inside.height * 50 / 100
    } else {
        inside.height * 45 / 100
    };
    let widget = Rect::new(
        inside.x,
        inside.y + 1,
        inside.width,
        header_h.saturating_sub(1),
    );
    if matches!(p, Part::Cpu | Part::Soc) && wide {
        let ring_w = 24.min(widget.width / 2);
        arc(
            b,
            Rect::new(widget.x, widget.y, ring_w, widget.height.min(10)),
            active,
            t.cpu,
            &t,
            a.cfg.ui.glyphs == "ascii",
        );
        digits(
            b,
            Rect::new(
                widget.x + ring_w + 3,
                widget.y + 1,
                widget.width - ring_w - 3,
                3,
            ),
            (active * 100.).round() as u32,
            t.cpu,
        );
        text(b, widget, ring_w + 3, 5, "% ACTIVE RESIDENCY", t.muted);
        text(
            b,
            widget,
            ring_w + 3,
            7,
            core.map_or_else(
                || format!("{} cores / {} tiers", s.core_count(), s.tiers.len()),
                |(_, c)| format!("{} · scaled {}", freq(c.mhz), percent(c.scaled)),
            ),
            t.text,
        );
        text(b, widget, ring_w + 3, 8, watts(&s.power[0]), t.text);
    } else {
        widgets::content(b, widget, p, &s, a);
    }
    let hist_y = header_h + 1;
    let hist_h = inside.height.saturating_sub(header_h + 7).max(1);
    let history: Vec<_> = a
        .history
        .iter()
        .filter(|h| {
            h.at >= s.at - crate::app::WINDOWS[a.window] as f64
                && h.at <= s.at
                && crate::app::metric_available(p, h)
        })
        .filter_map(|h| {
            if let Some((tier, c)) = core {
                h.tiers
                    .iter()
                    .find(|t| t.label == tier.label)
                    .and_then(|t| t.cores.iter().find(|v| v.id == c.id && v.die == c.die))
                    .map(|c| c.active)
            } else {
                Some(metric(p, h))
            }
        })
        .collect();
    let max = history.iter().copied().fold(1., f32::max);
    let chart = Rect::new(
        inside.x,
        inside.y + hist_y,
        inside.width,
        hist_h.min(inside.height.saturating_sub(hist_y)),
    );
    let plot = stretch(&history, chart.width as usize * 2);
    spark(
        b,
        chart,
        &plot,
        max,
        widgets::tint(p, &t),
        &t,
        a.cfg.ui.glyphs == "ascii",
    );
    line(
        b,
        inside,
        hist_y + hist_h,
        format!(
            "{} history   min {:.1}   peak {:.1}   {}",
            ["1m", "5m", "15m", "1h"][a.window],
            history
                .iter()
                .copied()
                .fold(f32::INFINITY, f32::min)
                .min(max),
            max,
            widgets::readout(p, &s, a)
        ),
        t.muted,
    );
    if history.is_empty() {
        line(b, chart, 0, "No recorded readings for this sensor", t.muted);
    }
    let note_y = inside.height.saturating_sub(4);
    line(b, inside, note_y, "WHAT THIS SHOWS", t.accent);
    paragraph(
        b,
        Rect::new(
            inside.x,
            inside.y + note_y + 1,
            inside.width,
            3.min(inside.height.saturating_sub(note_y + 1)),
        ),
        p.note(),
        t.muted,
    );
}
