use super::{SizeClass, kit::*, widgets};
use crate::{
    app::{App, Page, ProcessRow, SORT_NAMES},
    metrics::{Avail, Proc, Sample},
};
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Modifier, Style},
    widgets::{Paragraph, Widget, Wrap},
};

pub fn power_estimate(p: &Proc, s: &Sample) -> Avail<f32> {
    let Some(watts) = s.power[0].value() else {
        return Avail::Unsupported("CPU power unavailable".into());
    };
    let total = s
        .processes
        .iter()
        .map(|p| p.cpu.max(0.))
        .sum::<f32>()
        .max(s.cpu * s.core_count() as f32 * 100.);
    Avail::Estimated(if total > 0. {
        watts * p.cpu.max(0.) / total
    } else {
        0.
    })
}
pub fn primary(p: &Proc, page: Page, s: &Sample) -> f32 {
    match page {
        Page::Cpu => p.cpu,
        Page::Memory => p.footprint.value().copied().unwrap_or(0) as f32,
        Page::Energy => power_estimate(p, s).value().copied().unwrap_or(0.),
        Page::Disk => (p.read + p.write) as f32,
        Page::Network => s
            .extra
            .traffic(p, s.at)
            .map_or(0., |v| (v.read + v.write) as f32),
        Page::Gpu => p.gpu.value().copied().unwrap_or(0.),
    }
}
pub fn draw(b: &mut Buffer, r: Rect, a: &mut App, size: SizeClass) {
    let t = a.theme.clone();
    let s = a.sample();
    let page = a.page;
    let title = if page == Page::Network {
        "TOP NETWORK TASKS · TRAFFIC".into()
    } else {
        format!("TOP {} TASKS", page.label().to_uppercase())
    };
    let held = a.last_input.elapsed().as_millis() < 1500;
    let right = format!(
        "{} · {} {}{}",
        a.rows.iter().filter(|r| !r.exited).count(),
        SORT_NAMES[a.sort],
        if a.reverse { "↑" } else { "↓" },
        if held { " · held" } else { "" }
    );
    let inner = frame(b, r, &title, &right, false, &t);
    let source = match page {
        Page::Cpu => "CPU time · 100% = one core · bars use whole-machine capacity",
        Page::Memory => "Physical footprint · macOS task accounting · compressed bytes unavailable",
        Page::Energy => "CPU POWER ≈ · measured CPU watts × CPU-time share; excludes GPU / system",
        Page::Disk => "READ + WRITE · bars relative to top task · sampled every 5s",
        Page::Network => {
            "Measured RX + TX · external interfaces · ~5s samples · unavailable ≠ zero"
        }
        Page::Gpu => "Driver GPU time · >100% possible · ~5s · bars relative to top task",
    };
    let caption = if let Some(q) = a.search.as_ref().filter(|q| !q.is_empty() || a.searching) {
        format!("/ {q}{}", if a.searching { "▏" } else { "" })
    } else {
        source.into()
    };
    line(
        b,
        inner,
        0,
        caption,
        if a.searching { t.accent } else { t.muted },
    );
    let compact = size == SizeClass::Compact;
    let name_x = if compact { 4 } else { 12 };
    let name_w = if compact {
        inner.width.saturating_sub(40)
    } else {
        (inner.width / 4).clamp(20, 38)
    };
    let value_x = name_x + name_w + 1;
    let bar_x = value_x + 13;
    let bar_w = if compact {
        8
    } else {
        (inner.width / 9).clamp(8, 22)
    };
    let history_x = bar_x + bar_w + 2;
    let secondary_x = if compact {
        bar_x + bar_w + 1
    } else {
        history_x + 17
    };
    let primary_label = match page {
        Page::Cpu => "CPU%",
        Page::Memory => "FOOTPRINT",
        Page::Energy => "CPU POWER ≈",
        Page::Disk => "READ + WRITE",
        Page::Network => "RX + TX",
        Page::Gpu => "GPU%",
    };
    text(b, inner, 0, 1, " #", t.muted);
    if !compact {
        text(b, inner, 4, 1, "PID", t.muted);
    }
    text(b, inner, name_x, 1, "NAME", t.muted);
    text(b, inner, value_x, 1, primary_label, t.accent);
    if !compact {
        text(b, inner, history_x, 1, "2 MIN", t.muted);
    }
    text(
        b,
        inner,
        secondary_x,
        1,
        match page {
            Page::Cpu if compact => "MEM",
            Page::Disk if compact => "WRITE/s",
            Page::Cpu => "CPU TIME   THREADS  MEM",
            Page::Memory => "CPU%    THREADS  COMP",
            Page::Energy => "CPU%    GPU%    WAKES",
            Page::Disk => "READ/s       WRITE/s       READ TOTAL / WRITTEN",
            Page::Network if compact => "TX/s",
            Page::Network => "RX/s         TX/s         SOCKETS",
            Page::Gpu if compact => "CPU%",
            Page::Gpu => "CPU%    THREADS  MEM",
        },
        t.muted,
    );
    let top = a
        .rows
        .iter()
        .map(|r| primary(&r.process, page, &s))
        .fold(1., f32::max);
    let mut y = 2;
    if let Some(p) = a.displayed_follow() {
        let row = ProcessRow {
            process: p,
            depth: 0,
            exited: false,
            new: false,
        };
        draw_row(
            b,
            row_at(inner, y),
            &row,
            0,
            true,
            name_x,
            name_w,
            value_x,
            bar_x,
            bar_w,
            history_x,
            secondary_x,
            compact,
            top,
            a,
        );
        y += 1;
    }
    let available = inner.height.saturating_sub(y) as usize;
    if let Some(index) = a
        .rows
        .iter()
        .position(|r| Some(r.process.key()) == a.selected_process)
    {
        if index < a.scroll {
            a.scroll = index;
        }
        if index >= a.scroll + available.max(1) {
            a.scroll = index + 1 - available.max(1);
        }
    }
    a.scroll = a.scroll.min(a.rows.len().saturating_sub(available));
    for (i, row) in a.rows.iter().skip(a.scroll).take(available).enumerate() {
        let rr = row_at(inner, y + i as u16);
        draw_row(
            b,
            rr,
            row,
            a.scroll + i + 1,
            false,
            name_x,
            name_w,
            value_x,
            bar_x,
            bar_w,
            history_x,
            secondary_x,
            compact,
            top,
            a,
        );
        a.hitmap.add(rr, 2, Hit::Process(row.process.key()));
    }
    if a.rows.is_empty() {
        line(
            b,
            inner,
            y + 1,
            if page == Page::Network {
                "No measured traffic yet. Allow two samples; i shows collection status."
            } else if page == Page::Gpu && a.search.as_ref().is_none_or(|q| q.is_empty()) {
                match &s.process_gpu {
                    Avail::Ok(_) => {
                        "No readable GPU time counters. Only reporting processes appear here."
                    }
                    other => other.reason(),
                }
            } else if s.processes.is_empty() {
                "Acquiring task snapshot…"
            } else {
                "No matching tasks. Esc clears the filter."
            },
            t.muted,
        );
    }
}
fn row_at(r: Rect, y: u16) -> Rect {
    Rect::new(r.x, r.y + y, r.width, 1)
}
#[allow(clippy::too_many_arguments)]
fn draw_row(
    b: &mut Buffer,
    r: Rect,
    row: &ProcessRow,
    rank: usize,
    pinned: bool,
    name_x: u16,
    name_w: u16,
    value_x: u16,
    bar_x: u16,
    bar_w: u16,
    history_x: u16,
    secondary_x: u16,
    compact: bool,
    top: f32,
    a: &App,
) {
    let p = &row.process;
    let s = a.sample();
    let t = &a.theme;
    let page = a.page;
    let color = widgets::tint(page.part(), t);
    let selected = Some(p.key()) == a.selected_process;
    if selected || pinned {
        b.set_style(r, Style::new().bg(t.substrate));
    }
    let fg = if row.exited {
        t.muted
    } else if selected || rank <= 3 {
        t.bright
    } else {
        t.text
    };
    text(
        b,
        r,
        0,
        0,
        if pinned {
            " ◎".into()
        } else {
            format!("{rank:02}{}", if selected { "›" } else { " " })
        },
        if rank <= 3 || selected || pinned {
            color
        } else {
            t.muted
        },
    );
    if !compact {
        text(b, r, 4, 0, format!("{:>6}", p.pid), t.muted);
    }
    let prefix = if a.tree {
        format!(
            "{}{}",
            "  ".repeat(row.depth.min(8)),
            if a.collapsed.contains(&p.pid) {
                "▸ "
            } else if row.depth > 0 {
                "└ "
            } else {
                ""
            }
        )
    } else if row.exited {
        "× ".into()
    } else {
        String::new()
    };
    text(
        b,
        r,
        name_x,
        0,
        truncate(&format!("{prefix}{}", p.name), name_w as usize),
        fg,
    );
    let value = match page {
        Page::Cpu => format!(
            "{:>8.1}%",
            if a.cfg.systems.cpu_mode == "machine" {
                p.cpu / s.core_count() as f32
            } else {
                p.cpu
            }
        ),
        Page::Memory => format!("{:>11}", avail(&p.footprint, |v| bytes(*v, &a.cfg.units))),
        Page::Energy => format!(
            "{:>11}",
            avail(&power_estimate(p, &s), |v| format!("{v:.2} W"))
        ),
        Page::Disk => format!("{:>11}", rate(p.read + p.write, &a.cfg.units, false)),
        Page::Network => format!(
            "{:>11}",
            s.extra.traffic(p, s.at).map_or("N/A".into(), |v| rate(
                v.read + v.write,
                &a.cfg.units,
                true
            ))
        ),
        Page::Gpu => format!(
            "{:>10}",
            if matches!(p.gpu, Avail::Permission(_)) {
                "???%".into()
            } else {
                avail(&p.gpu, |v| format!("{v:.1}%"))
            }
        ),
    };
    text(b, r, value_x, 0, value, fg);
    if rank <= 3 {
        for x in value_x..(value_x + 12).min(r.width) {
            b[(r.x + x, r.y)].set_style(Style::new().add_modifier(Modifier::BOLD));
        }
    }
    let ratio = match page {
        Page::Cpu => p.cpu / (s.core_count() as f32 * 100.),
        Page::Memory => {
            p.footprint.value().copied().unwrap_or(0) as f32 / s.machine.memory.max(1) as f32
        }
        _ => primary(p, page, &s) / top,
    };
    let gpu_readable = page != Page::Gpu || p.gpu.value().is_some();
    if gpu_readable && bar_x + bar_w < r.width {
        if page == Page::Disk {
            let half = bar_w / 2;
            for x in 0..bar_w {
                text(b, r, bar_x + x, 0, "─", t.wire);
            }
            let read = (p.read as f32 / top * half as f32)
                .ceil()
                .clamp(0., half as f32) as u16;
            let write = (p.write as f32 / top * half as f32)
                .ceil()
                .clamp(0., half as f32) as u16;
            for x in 0..read {
                text(b, r, bar_x + half - x - 1, 0, "▰", color);
            }
            for x in 0..write {
                text(b, r, bar_x + half + 1 + x, 0, "▰", t.accent);
            }
            text(b, r, bar_x + half, 0, "│", t.muted);
        } else {
            meter(
                b,
                Rect::new(r.x + bar_x, r.y, bar_w, 1),
                ratio,
                color,
                t,
                a.cfg.ui.glyphs == "ascii",
            );
        }
    }
    if gpu_readable && !compact && history_x + 14 < r.width {
        let hist = a.task_values(p, page);
        let max = hist.iter().copied().fold(1., f32::max);
        spark(
            b,
            Rect::new(r.x + history_x, r.y, 14, 1),
            &hist,
            max,
            color,
            t,
            a.cfg.ui.glyphs == "ascii",
        );
    }
    let secondary = match page {
        Page::Cpu => {
            if compact {
                bytes(p.memory, &a.cfg.units)
            } else {
                format!(
                    "{:<10} {:>5}  {}",
                    duration(p.cpu_time as f64 / 1000.),
                    avail(&p.threads, |v| v.to_string()),
                    bytes(p.memory, &a.cfg.units)
                )
            }
        }
        Page::Memory => format!(
            "{:>5.1}%   {}   N/A",
            p.cpu,
            avail(&p.threads, |v| v.to_string())
        ),
        Page::Energy => format!(
            "{:>5.1}%   {:<7} {}",
            p.cpu,
            avail(&p.gpu, |v| format!("{v:.1}%")),
            avail(&p.wakeups, |v| format!("{v:.0}/s"))
        ),
        Page::Gpu => {
            let restricted = matches!(p.threads, Avail::Permission(_));
            let cpu = if restricted {
                "???%".into()
            } else {
                format!("{:.1}%", p.cpu)
            };
            if compact {
                cpu
            } else {
                let threads = if restricted {
                    "???".into()
                } else {
                    avail(&p.threads, |v| v.to_string())
                };
                let memory = if restricted {
                    "???".into()
                } else {
                    format!("{:.1}", p.memory as f64 / 1_048_576.)
                };
                format!("{cpu:>6}   {threads:>5}    {memory:>7} MiB")
            }
        }
        Page::Disk if compact => rate(p.write, &a.cfg.units, false),
        Page::Disk => format!(
            "{:<12} {:<12} {} / {}",
            rate(p.read, &a.cfg.units, false),
            rate(p.write, &a.cfg.units, false),
            bytes(p.total_read, &a.cfg.units),
            bytes(p.total_write, &a.cfg.units)
        ),
        Page::Network => s.extra.traffic(p, s.at).map_or("N/A".into(), |v| {
            if compact {
                rate(v.write, &a.cfg.units, true)
            } else {
                format!(
                    "{:<12} {:<12} {}",
                    rate(v.read, &a.cfg.units, true),
                    rate(v.write, &a.cfg.units, true),
                    avail(&p.connections, |n| n.to_string())
                )
            }
        }),
    };
    text(b, r, secondary_x, 0, secondary, t.muted);
}
pub fn overview(b: &mut Buffer, r: Rect, p: &Proc, a: &App) {
    let t = &a.theme;
    line(b, r, 0, &p.name, t.accent);
    line(
        b,
        r,
        1,
        format!("PID {} · parent {} · {}", p.pid, p.parent, p.status),
        t.text,
    );
    line(
        b,
        r,
        2,
        format!(
            "USER {} · {} threads",
            p.user,
            avail(&p.threads, |v| v.to_string())
        ),
        t.muted,
    );
    line(
        b,
        r,
        4,
        format!("CPU {:.1}% · RSS {}", p.cpu, bytes(p.memory, &a.cfg.units)),
        t.text,
    );
    line(
        b,
        r,
        5,
        format!(
            "FOOTPRINT {}",
            avail(&p.footprint, |v| bytes(*v, &a.cfg.units))
        ),
        t.memory,
    );
    line(
        b,
        r,
        6,
        format!(
            "READ {} · WRITE {}",
            rate(p.read, &a.cfg.units, false),
            rate(p.write, &a.cfg.units, false)
        ),
        t.muted,
    );
    line(
        b,
        r,
        7,
        format!(
            "CPU TIME {} · UP {}",
            duration(p.cpu_time as f64 / 1000.),
            duration(p.runtime as f64)
        ),
        t.muted,
    );
    if a.screen == crate::app::Screen::Systems {
        let s = a.sample();
        let extra = if a.page == Page::Memory {
            format!(
                "Δ 1/5/15m: {} / {} / {}",
                super::insights::growth(p, &s, a, 60.),
                super::insights::growth(p, &s, a, 300.),
                super::insights::growth(p, &s, a, 900.)
            )
        } else if a.page == Page::Gpu {
            format!(
                "GPU {} · {}",
                avail(&p.gpu, |v| format!("{v:.1}%")),
                p.gpu.reason()
            )
        } else if a.page == Page::Energy {
            if s.extra.stale(s.at) {
                "Sleep blockers: STALE".into()
            } else {
                format!(
                    "Sleep assertions: {} · i on Systems for reasons",
                    avail(&s.extra.assertions, |v| v
                        .iter()
                        .filter(|v| v.pid == p.pid)
                        .count()
                        .to_string())
                )
            }
        } else {
            s.extra
                .traffic(p, s.at)
                .map_or("Network rates: N/A".into(), |v| {
                    format!(
                        "RX {} · TX {}",
                        rate(v.read, &a.cfg.units, true),
                        rate(v.write, &a.cfg.units, true)
                    )
                })
        };
        line(b, r, 8, extra, t.accent);
    }
    if r.height > 10 {
        line(b, r, 9, "COMMAND", t.muted);
        Paragraph::new(if p.command.is_empty() {
            "Unavailable"
        } else {
            &p.command
        })
        .style(Style::new().fg(t.text))
        .wrap(Wrap { trim: false })
        .render(
            Rect::new(r.x, r.y + 10, r.width, r.height.saturating_sub(10)),
            b,
        );
    }
}
