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
        Page::Memory => p.memory as f32,
        Page::Energy => power_estimate(p, s).value().copied().unwrap_or(0.),
        Page::Disk => (p.read + p.write) as f32,
        Page::Network => p.connections.value().copied().unwrap_or(0) as f32,
        Page::Gpu => 0.,
    }
}
pub fn draw(b: &mut Buffer, r: Rect, a: &mut App, size: SizeClass) {
    let t = a.theme.clone();
    let s = a.sample();
    let page = a.page;
    let title = if page == Page::Network {
        "TOP NETWORK TASKS · VISIBLE SOCKETS".into()
    } else {
        format!("TOP {} TASKS", page.name().to_uppercase())
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
    if page == Page::Gpu {
        line(
            b,
            inner,
            1,
            "⊘ Per-process GPU time is not exposed by this collector.",
            t.muted,
        );
        line(
            b,
            inner,
            2,
            "The GPU totals above are measured. No task usage is estimated.",
            t.muted,
        );
        return;
    }
    let source = match page {
        Page::Cpu => "CPU time · 100% = one core · bars use whole-machine capacity",
        Page::Memory => "Resident footprint · compressed / private memory unavailable",
        Page::Energy => "CPU POWER ≈ · measured CPU watts × CPU-time share; excludes GPU / system",
        Page::Disk => "READ + WRITE · bars relative to top task · sampled every 5s",
        Page::Network => "Socket ownership · 15s scan · visible records only; per-task traffic N/A",
        _ => "",
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
        inner.width.saturating_sub(36)
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
        Page::Network => "SOCKETS",
        _ => "",
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
            Page::Cpu => "CPU TIME   THREADS  MEM",
            Page::Memory => "CPU%    THREADS  COMP",
            Page::Energy => "CPU%    GPU%    WAKES",
            Page::Disk => "READ/s       WRITE/s       READ TOTAL / WRITTEN",
            Page::Network => "CPU%    MEM       USER",
            _ => "",
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
                "No visible socket owners yet. macOS may restrict access."
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
        Page::Memory => format!("{:>11}", bytes(p.memory, &a.cfg.units)),
        Page::Energy => format!(
            "{:>11}",
            avail(&power_estimate(p, &s), |v| format!("≈ {v:.2} W"))
        ),
        Page::Disk => format!("{:>11}", rate(p.read + p.write, &a.cfg.units, false)),
        Page::Network => format!("{:>8}", avail(&p.connections, |v| v.to_string())),
        _ => "N/A".into(),
    };
    text(b, r, value_x, 0, value, fg);
    if rank <= 3 {
        for x in value_x..(value_x + 12).min(r.width) {
            b[(r.x + x, r.y)].set_style(Style::new().add_modifier(Modifier::BOLD));
        }
    }
    let ratio = match page {
        Page::Cpu => p.cpu / (s.core_count() as f32 * 100.),
        Page::Memory => p.memory as f32 / s.machine.memory.max(1) as f32,
        _ => primary(p, page, &s) / top,
    };
    if bar_x + bar_w < r.width {
        meter(
            b,
            Rect::new(r.x + bar_x, r.y, bar_w, 1),
            ratio,
            color,
            t,
            a.cfg.ui.glyphs == "ascii",
        );
    }
    if !compact && history_x + 14 < r.width {
        let mut last = -1.;
        let hist: Vec<_> = a
            .history
            .iter()
            .filter(|h| h.at <= s.at && h.at >= s.at - 120.)
            .filter(|h| {
                let changed = h.process_at != last;
                last = h.process_at;
                changed
            })
            .filter_map(|h| {
                h.processes
                    .iter()
                    .find(|v| v.key() == p.key())
                    .map(|p| primary(p, page, h))
            })
            .collect();
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
        Page::Energy => format!("{:>5.1}%   N/A     N/A", p.cpu),
        Page::Disk => format!(
            "{:<12} {:<12} {} / {}",
            rate(p.read, &a.cfg.units, false),
            rate(p.write, &a.cfg.units, false),
            bytes(p.total_read, &a.cfg.units),
            bytes(p.total_write, &a.cfg.units)
        ),
        Page::Network => format!(
            "{:>5.1}%   {:<10} {}",
            p.cpu,
            bytes(p.memory, &a.cfg.units),
            p.user
        ),
        _ => String::new(),
    };
    text(b, r, secondary_x, 0, secondary, t.muted);
}
pub fn overview(b: &mut Buffer, r: Rect, p: &Proc, a: &App) {
    let t = &a.theme;
    line(b, r, 1, &p.name, t.accent);
    line(
        b,
        r,
        3,
        format!("PID {:>6}   parent {}", p.pid, p.parent),
        t.text,
    );
    line(b, r, 4, format!("USER {}", p.user), t.muted);
    line(b, r, 5, format!("STATE {}", p.status), t.muted);
    line(
        b,
        r,
        7,
        format!(
            "CPU {:>6.1}%    MEM {}",
            p.cpu,
            bytes(p.memory, &a.cfg.units)
        ),
        t.text,
    );
    if r.height > 9 {
        meter(
            b,
            row(r, 8, 1),
            p.cpu / (a.sample().core_count() * 100) as f32,
            t.cpu,
            t,
            a.cfg.ui.glyphs == "ascii",
        );
    }
    line(
        b,
        r,
        10,
        format!("UP {}", duration(p.runtime as f64)),
        t.muted,
    );
    line(
        b,
        r,
        12,
        format!("READ  {}", rate(p.read, &a.cfg.units, false)),
        t.muted,
    );
    line(
        b,
        r,
        13,
        format!("WRITE {}", rate(p.write, &a.cfg.units, false)),
        t.muted,
    );
    line(b, r, 15, "GPU / NETWORK ATTRIBUTION", t.muted);
    line(b, r, 16, "Not exposed by this collector", t.muted);
    if r.height > 19 {
        line(b, r, 18, "COMMAND", t.muted);
        Paragraph::new(if p.command.is_empty() {
            "Unavailable"
        } else {
            &p.command
        })
        .style(Style::new().fg(t.text))
        .wrap(Wrap { trim: false })
        .render(
            Rect::new(r.x, r.y + 19, r.width, r.height.saturating_sub(22)),
            b,
        );
    }
    if r.height > 3 {
        line(
            b,
            r,
            r.height - 2,
            "f follow   X signal   Enter details",
            t.accent,
        );
    }
}
