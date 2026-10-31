use super::{SizeClass, kit::*, processes, theme::blend, widgets};
use crate::{
    app::{App, Layer, Page, Part},
    metrics::{Avail, Sample},
    motion::Motion,
};
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Modifier, Style},
};

pub fn draw(b: &mut Buffer, r: Rect, a: &mut App, size: SizeClass) {
    strip(b, row(r, 0, 1), a, size);
    let body = Rect::new(r.x, r.y + 2, r.width, r.height.saturating_sub(2));
    let mut visual_h = if a.expanded {
        body.height
    } else {
        (body.height
            * match size {
                SizeClass::Wide => 58,
                SizeClass::Regular => 52,
                _ => 36,
            }
            / 100)
            .max(if size == SizeClass::Compact { 7 } else { 5 })
            .min(body.height.saturating_sub(6))
    };
    if a.page == Page::Cpu && !a.expanded {
        let minimum = if size == SizeClass::Compact { 10 } else { 19 };
        visual_h = visual_h.max(minimum.min(body.height.saturating_sub(6)));
    }
    let visual = Rect::new(body.x, body.y, body.width, visual_h);
    let side_width = if size == SizeClass::Wide { 35 } else { 0 };
    let main = Rect::new(
        visual.x,
        visual.y,
        visual
            .width
            .saturating_sub(if side_width > 0 { side_width + 1 } else { 0 }),
        visual.height,
    );
    let s = a.sample();
    let t = a.theme.clone();
    let color = widgets::tint(a.page.part(), &t);
    let title = if a.page == Page::Cpu {
        format!(
            "CPU · {} · {}",
            a.page.title(),
            avail(&s.power_mode, |mode| mode.label().into())
        )
    } else {
        format!("{} · {}", a.page.label().to_uppercase(), a.page.title())
    };
    let subtitle = if a.expanded {
        "i details · z restore tasks"
    } else {
        "i details · z expand"
    };
    let (inner, panel) = if a.page == Page::Gpu {
        let (fabric, donut) = gpu_panels(main, a.cell_aspect);
        let inner = frame(b, fabric, &title, subtitle, false, &t);
        gpu(b, inner, a, &s);
        let donut_inner = frame(b, donut, "VISUAL", "", false, &t);
        super::donut::draw(b, donut_inner, a);
        (inner, fabric)
    } else {
        let mut inner = frame(b, main, &title, subtitle, false, &t);
        if size == SizeClass::Regular && inner.height > 7 {
            let stats = Rect::new(inner.x, inner.bottom() - 2, inner.width, 2);
            inner.height -= 2;
            inline_stats(b, stats, a, &s);
        }
        match a.page {
            Page::Cpu => cpu(b, inner, a, &s),
            Page::Memory => memory(b, inner, a, &s),
            Page::Energy => energy(b, inner, a, &s),
            Page::Disk | Page::Network => flow(b, inner, a, &s),
            Page::Gpu => unreachable!(),
        }
        (inner, main)
    };
    if side_width > 0 {
        let side = Rect::new(main.right() + 1, visual.y, side_width, visual.height);
        let rr = frame(
            b,
            side,
            match a.page {
                Page::Cpu => "UTILIZATION",
                Page::Memory => "PRESSURE / VM",
                Page::Energy => "THERMALS / BATTERY",
                Page::Disk => "VOLUMES",
                Page::Network => "LINK / SOCKETS",
                Page::Gpu => "UTILIZATION",
            },
            "",
            false,
            &t,
        );
        side_card(b, rr, a, &s);
    }
    if !a.expanded {
        a.task_rect = Rect::new(
            body.x,
            body.y + visual_h,
            body.width,
            body.height - visual_h,
        );
        processes::draw(b, a.task_rect, a, size);
    } else {
        a.task_rect = Rect::default();
    }
    if let Some(p) = a.selected_proc().or_else(|| a.displayed_follow())
        && panel.width > 95
        && inner.height > 4
    {
        let label = format!(
            "{} {}",
            if a.follow.as_ref().is_some_and(|f| f.key() == p.key()) {
                "◎"
            } else {
                "›"
            },
            p.name
        );
        text(
            b,
            panel,
            panel.width.saturating_sub(29),
            panel.height.saturating_sub(1),
            truncate(&label, 25),
            color,
        );
    }
}
fn headline(p: Page, s: &Sample, a: &App) -> String {
    match p {
        Page::Cpu => percent(s.cpu),
        Page::Memory => bytes(s.memory_used, &a.cfg.units)
            .replace(" GiB", "G")
            .replace(" GB", "G"),
        Page::Energy => {
            if s.system_power.value().is_some() {
                watts(&s.system_power)
            } else {
                format!("Σ{}", watts(&s.total_power()))
            }
        }
        Page::Disk | Page::Network => {
            avail(if p == Page::Disk { &s.disk } else { &s.network }, |v| {
                rate(v.read + v.write, &a.cfg.units, p == Page::Network)
            })
        }
        Page::Gpu => avail(&s.gpu, |v| percent(*v)),
    }
}
fn strip(b: &mut Buffer, r: Rect, a: &mut App, size: SizeClass) {
    let s = a.sample();
    let t = &a.theme;
    let drawer = matches!(a.layers.last(), Some(Layer::Process { .. }));
    text(b, r, 0, 0, "‹", if drawer { t.wire } else { t.accent });
    if size == SizeClass::Compact {
        text(
            b,
            r,
            3,
            0,
            format!(
                "{} {}  ›",
                a.page.label().to_uppercase(),
                headline(a.page, &s, a)
            ),
            widgets::tint(a.page.part(), t),
        );
        for (i, p) in Page::ALL.iter().enumerate() {
            let x = r.width.saturating_sub(14) + i as u16 * 2;
            text(
                b,
                r,
                x,
                0,
                if *p == a.page { "●" } else { "○" },
                if *p == a.page { t.accent } else { t.wire },
            );
            a.hitmap
                .add(Rect::new(r.x + x, r.y, 2, 1), 10, Hit::Page(*p));
        }
    } else {
        let slot = r.width.saturating_sub(4) / 6;
        for (i, p) in Page::ALL.iter().enumerate() {
            let rr = Rect::new(r.x + 2 + i as u16 * slot, r.y, slot, 1);
            let active = *p == a.page;
            let warning = (*p == Page::Memory && s.pressure.value().is_some_and(|v| v != "NORMAL"))
                || (*p == Page::Energy && s.thermal.value().is_some_and(|v| v != "NOMINAL"));
            let label = format!(
                " {} {}{} ",
                p.label().to_uppercase(),
                headline(*p, &s, a),
                if warning { " !" } else { "" }
            );
            b.set_style(
                rr,
                Style::new().bg(if active {
                    widgets::tint(p.part(), t)
                } else {
                    t.void
                }),
            );
            text(
                b,
                rr,
                0,
                0,
                truncate(&label, slot as usize),
                if active {
                    t.void
                } else if warning {
                    t.warn
                } else {
                    t.muted
                },
            );
            a.hitmap.add(rr, 10, Hit::Page(*p));
        }
        text(
            b,
            r,
            r.width - 1,
            0,
            "›",
            if drawer { t.wire } else { t.accent },
        );
    }
}
fn cpu(b: &mut Buffer, r: Rect, a: &App, s: &Sample) {
    let reserve = if r.height >= 15 {
        s.tiers.len() as u16 + 1
    } else {
        0
    };
    let core_r = row(r, 0, r.height.saturating_sub(reserve));
    widgets::content(b, core_r, Part::Cpu, s, a);
    if reserve == 0 {
        return;
    }
    for (i, tier) in s.tiers.iter().enumerate() {
        dvfs(
            b,
            row(r, core_r.height + i as u16, 1),
            &format!("{} DVFS", tier.label),
            &tier.freqs,
            tier.mhz,
            a,
        );
    }
    let y = r.height.saturating_sub(1);
    let split = avail(&s.cpu_split, |v| {
        format!(
            "USER {:.0}%  SYS {:.0}%  IDLE {:.0}%",
            v[0] * 100.,
            v[1] * 100.,
            v[2] * 100.
        )
    });
    line(
        b,
        r,
        y,
        format!(
            "{split}   LOAD {:.2} / {:.2} / {:.2}   {}",
            s.load[0],
            s.load[1],
            s.load[2],
            temp(&s.cpu_temp, &a.cfg.units)
        ),
        a.theme.muted,
    );
}
fn dvfs(b: &mut Buffer, r: Rect, label: &str, freqs: &[u32], current: u32, a: &App) {
    text(b, r, 0, 0, label, a.theme.muted);
    let mut freqs: Vec<_> = freqs.iter().copied().filter(|v| *v > 0).collect();
    freqs.sort_unstable();
    freqs.dedup();
    if freqs.is_empty() {
        text(b, r, 9, 0, "frequency table unavailable", a.theme.muted);
        return;
    }
    let min = *freqs.first().unwrap() as f32;
    let max = *freqs.last().unwrap() as f32;
    let rail = r.width.saturating_sub(38);
    if rail < 8 {
        return;
    }
    for i in 0..rail {
        text(b, r, 10 + i, 0, "─", a.theme.wire);
    }
    for f in &freqs {
        let x = ((*f as f32 - min) / (max - min).max(1.) * (rail - 1) as f32).round() as u16;
        text(b, r, 10 + x, 0, "╷", a.theme.muted);
    }
    let x = ((current as f32 - min) / (max - min).max(1.) * (rail - 1) as f32)
        .clamp(0., (rail - 1) as f32)
        .round() as u16;
    text(b, r, 10 + x, 0, "◆", a.theme.accent);
    text(
        b,
        r,
        12 + rail,
        0,
        format!("{:.2} / {:.2} GHz", current as f32 / 1000., max / 1000.),
        a.theme.text,
    );
}
fn memory(b: &mut Buffer, r: Rect, a: &App, s: &Sample) {
    let t = &a.theme;
    if r.height < 8 {
        widgets::content(b, r, Part::Memory, s, a);
        return;
    }
    let mh = r.height.saturating_sub(7).max(2);
    let matrix = Rect::new(r.x, r.y + 1, r.width, mh + 3);
    line(
        b,
        r,
        0,
        format!(
            "{} of {} · 1 cell ≈ {} · aggregate VM categories",
            bytes(s.memory_used, &a.cfg.units),
            bytes(s.machine.memory, &a.cfg.units),
            bytes(
                s.machine.memory / (r.width as u64 * mh as u64).max(1),
                &a.cfg.units
            )
        ),
        t.memory,
    );
    widgets::content(b, matrix, Part::Memory, s, a);
    let data: Vec<_> = a
        .history
        .iter()
        .filter(|h| h.at <= s.at && h.at >= s.at - 300.)
        .map(|h| h.memory_ratio())
        .collect();
    let y = r.height.saturating_sub(2);
    text(b, r, 0, y, "USED 5m", t.muted);
    spark(
        b,
        Rect::new(r.x + 9, r.y + y, r.width.saturating_sub(10), 1),
        &data,
        1.,
        t.memory,
        t,
        false,
    );
    if let Some(m) = s.memory.value() {
        line(
            b,
            r,
            r.height - 1,
            format!(
                "FREE {}   COMP {}   PAGE IN {}  OUT {}",
                bytes(m[4], &a.cfg.units),
                avail(&s.compressed_original, |v| format!(
                    "{:.1}:1",
                    *v as f64 / m[2].max(1) as f64
                )),
                avail(&s.vm_rates, |v| rate(v[0], &a.cfg.units, false)),
                avail(&s.vm_rates, |v| rate(v[1], &a.cfg.units, false))
            ),
            t.muted,
        );
    }
}
fn energy(b: &mut Buffer, r: Rect, a: &App, s: &Sample) {
    let t = &a.theme;
    let source = s.battery.value().map_or("AC / SOURCE N/A", |v| {
        if v.ac { "AC ADAPTER" } else { "BATTERY" }
    });
    if r.height < 8 {
        line(
            b,
            r,
            0,
            format!(
                "{source}  →  SYSTEM {}   Σ SoC {}",
                watts(&s.system_power),
                watts(&s.total_power())
            ),
            t.accent,
        );
        line(
            b,
            r,
            1,
            format!(
                "CPU {}    GPU {}    ANE {}",
                watts(&s.power[0]),
                watts(&s.power[1]),
                watts(&s.power[2])
            ),
            t.text,
        );
        line(
            b,
            r,
            2,
            format!(
                "CPU {}  GPU {}   {}",
                temp(&s.cpu_temp, &a.cfg.units),
                temp(&s.gpu_temp, &a.cfg.units),
                avail(&s.thermal, |v| v.clone())
            ),
            t.muted,
        );
        if r.height > 3 {
            line(b, r, 3, super::insights::power(s), t.muted);
        }
        return;
    }
    let system = matches!(s.system_power, Avail::Ok(_));
    let trunk = s.system_power.value().copied().unwrap_or(0.);
    let soc = s.total_power();
    let rest = if system {
        soc.value().map(|v| (trunk - v).max(0.))
    } else {
        None
    };
    let branch_x = (r.width * 43 / 100).clamp(27, 53);
    let branch_w = r.width.saturating_sub(branch_x + 21);
    line(b, r, 1, source, t.battery);
    if system {
        text(b, r, 0, 2, "┃", t.battery);
        text(b, r, 0, 3, "└━━━━ SYSTEM ━━━━━┳", t.accent);
        text(b, r, 6, 4, watts(&s.system_power), t.bright);
        for y in 4..=8.min(r.height - 1) {
            text(b, r, 18, y, "┃", t.wire);
        }
    } else {
        line(b, r, 3, "System sensor unavailable", t.warn);
        line(b, r, 4, "Independent SoC rails ↓", t.muted);
    }
    let vals = [
        s.power[0].value().copied(),
        s.power[1].value().copied(),
        s.power[2].value().copied(),
        rest,
    ];
    let labels = ["CPU", "GPU", "ANE", "REST ≈"];
    let colors = [t.cpu, t.gpu, t.ane, t.muted];
    let scale = if system {
        trunk.max(1.)
    } else {
        vals.iter().flatten().copied().fold(1., f32::max)
    };
    for i in 0..4 {
        let y = 2 + i as u16 * 2;
        if y >= r.height {
            break;
        }
        if i == 3 && rest.is_none() {
            continue;
        }
        if system {
            for x in 19..branch_x.saturating_sub(1) {
                text(b, r, x, y, "─", t.wire);
            }
            text(b, r, 18, y, if i == 3 { "┗" } else { "┣" }, colors[i]);
        }
        text(b, r, branch_x, y, labels[i], colors[i]);
        text(
            b,
            r,
            branch_x + 8,
            y,
            vals[i].map_or("N/A".into(), |v| format!("{v:.2} W")),
            t.text,
        );
        if let Some(v) = vals[i] {
            meter(
                b,
                Rect::new(r.x + branch_x + 19, r.y + y, branch_w, 1),
                v / scale,
                colors[i],
                t,
                a.cfg.ui.glyphs == "ascii",
            );
        }
    }
    if r.height > 12
        && let Some(p) = a.preview_proc()
    {
        line(
            b,
            r,
            11,
            format!(
                "› {} · CPU allocation {} · estimated from CPU time",
                p.name,
                watts(&super::processes::power_estimate(&p, s))
            ),
            t.accent,
        );
    }
    if r.height > 10 {
        line(
            b,
            r,
            10,
            format!(
                "DRAM {} · GPU SRAM {} · shown separately; never added to Σ",
                watts(&s.power[3]),
                watts(&s.power[4])
            ),
            t.muted,
        );
    }
    if r.height > 12 {
        text(b, r, 0, r.height - 3, "THERMAL 15m", t.muted);
        let history: Vec<_> = a
            .history
            .iter()
            .filter(|h| h.at <= s.at && h.at >= s.at - 900.)
            .collect();
        let w = r.width.saturating_sub(14) as usize;
        for i in 0..w {
            if let Some(sample) = history.get(i * history.len() / w.max(1)) {
                let c = match sample.thermal.value().map(String::as_str) {
                    Some("NOMINAL") => t.ok,
                    Some("FAIR") => t.warn,
                    Some("SERIOUS" | "CRITICAL") => t.crit,
                    _ => t.wire,
                };
                text(b, r, 14 + i as u16, r.height - 3, "━", c);
            }
        }
    }
    line(
        b,
        r,
        r.height - 1,
        format!(
            "Σ CPU + GPU + ANE {}    SESSION {:.3} Wh    REST ≈ system − Σ",
            watts(&soc),
            s.energy_wh
        ),
        t.muted,
    );
}
fn flow(b: &mut Buffer, r: Rect, a: &App, s: &Sample) {
    let network = a.page == Page::Network;
    let t = &a.theme;
    let color = if network { t.net } else { t.disk };
    let data = if network { &s.network } else { &s.disk };
    let Some(f) = data.value() else {
        unavailable(b, r, data, t);
        return;
    };
    if r.height < 8 {
        line(
            b,
            r,
            0,
            format!(
                "{}   {} {}   {} {}",
                f.name,
                if network { "RX" } else { "READ" },
                rate(f.read, &a.cfg.units, network),
                if network { "TX" } else { "WRITE" },
                rate(f.write, &a.cfg.units, network)
            ),
            color,
        );
        if r.height > 1 {
            line(
                b,
                r,
                1,
                if network {
                    super::insights::wifi(s)
                } else {
                    super::insights::disk(s)
                },
                t.muted,
            );
        }
        if r.height > 3 {
            butterfly(
                b,
                Rect::new(r.x, r.y + 2, r.width, r.height - 2),
                a,
                s,
                network,
            );
        }
        return;
    }
    if network {
        let interfaces: Vec<_> = s
            .interfaces
            .iter()
            .filter(|f| a.cfg.systems.show_loopback || !f.name.starts_with("lo"))
            .take(3)
            .collect();
        for (i, f) in interfaces.iter().enumerate() {
            let y = i as u16 * 2;
            text(b, r, 0, y, truncate(&f.name, 8), color);
            text(
                b,
                r,
                9,
                y,
                if f.read + f.write > 0. { "◂" } else { "·" },
                color,
            );
            let len = (r.width / 2).saturating_sub(17);
            for x in 0..len {
                let phase = if a.motion == Motion::Full {
                    a.anim.phases.get("power").copied().unwrap_or(0.) as u16
                } else {
                    0
                };
                text(
                    b,
                    r,
                    11 + x,
                    y,
                    if f.read + f.write > 0. && (x + phase).is_multiple_of(9) {
                        "•"
                    } else {
                        "─"
                    },
                    if f.read + f.write > 0. { color } else { t.wire },
                );
            }
            text(b, r, 12 + len, y, "┤ NIC ├─ SoC", t.muted);
            text(
                b,
                r,
                r.width / 2 + 12,
                y,
                format!(
                    "↓ {}  ↑ {}",
                    rate(f.read, &a.cfg.units, true),
                    rate(f.write, &a.cfg.units, true)
                ),
                t.text,
            );
        }
    } else {
        line(
            b,
            r,
            0,
            format!(
                "READ {}  →   WRITE {}  ←",
                rate(f.read, &a.cfg.units, false),
                rate(f.write, &a.cfg.units, false)
            ),
            color,
        );
        let planes = (r.width / 20).clamp(2, 5);
        for i in 0..planes {
            let rr = Rect::new(r.x + i * 18, r.y + 1, 16, 4);
            let rr = frame(b, rr, &format!("NAND {:02}", i + 1), "", false, t);
            line(b, rr, 0, "▥ ▥ ▥ ▥ ▥ ▥", color);
            line(b, rr, 1, "╵ ╵ ╵ ╵ ╵ ╵", t.wire);
        }
        line(
            b,
            r,
            5,
            format!(
                "{} · i for all devices / operation sizes",
                super::insights::disk(s)
            ),
            t.muted,
        );
    }
    let prior: Vec<_> = a
        .history
        .iter()
        .filter(|h| h.at <= s.at && h.at >= s.at - 300.)
        .filter_map(|h| {
            if network {
                h.network.value()
            } else {
                h.disk.value()
            }
        })
        .collect();
    let read_peak = prior.iter().map(|f| f.read).fold(0., f64::max);
    let write_peak = prior.iter().map(|f| f.write).fold(0., f64::max);
    line(
        b,
        r,
        6,
        format!(
            "PEAK {} {}  /  {} {}",
            if network { "↓" } else { "READ" },
            rate(read_peak, &a.cfg.units, network),
            if network { "↑" } else { "WRITE" },
            rate(write_peak, &a.cfg.units, network)
        ),
        t.muted,
    );
    let ch = r.height.saturating_sub(8);
    if ch >= 3 {
        butterfly(b, Rect::new(r.x, r.y + 7, r.width, ch), a, s, network);
    }
    line(
        b,
        r,
        r.height - 1,
        format!(
            "SINCE BOOT  {} {}  /  {} {}{}",
            if network { "↓" } else { "READ" },
            bytes(f.total_read, &a.cfg.units),
            if network { "↑" } else { "WRITE" },
            bytes(f.total_write, &a.cfg.units),
            if f.errors > 0 {
                format!("   ✕ {} errors", f.errors)
            } else {
                String::new()
            }
        ),
        t.muted,
    );
}
fn butterfly(b: &mut Buffer, r: Rect, a: &App, s: &Sample, network: bool) {
    let t = &a.theme;
    let color = if network { t.net } else { t.disk };
    let data: Vec<_> = a
        .history
        .iter()
        .filter(|h| h.at <= s.at && h.at >= s.at - 300.)
        .filter_map(|h| {
            if network {
                h.network.value()
            } else {
                h.disk.value()
            }
        })
        .collect();
    if data.is_empty() || r.height < 2 {
        return;
    }
    let peak = data.iter().map(|v| v.read.max(v.write)).fold(1., f64::max);
    let chart = Rect::new(r.x + 12, r.y, r.width.saturating_sub(14), r.height);
    let mut g = BrailleGrid::new(chart.width, chart.height);
    let mid = (chart.height as f32 * 4. - 1.) / 2.;
    g.line((0., mid), ((chart.width * 2) as f32 - 1., mid), t.wire);
    for x in 0..chart.width as usize * 2 {
        let i = x * data.len() / (chart.width as usize * 2).max(1);
        let f = data[i];
        g.line(
            (x as f32, mid),
            (x as f32, mid - f.read as f32 / peak as f32 * (mid - 1.)),
            color,
        );
        g.line(
            (x as f32, mid),
            (x as f32, mid + f.write as f32 / peak as f32 * (mid - 1.)),
            blend(color, t.muted, 0.45),
        );
    }
    g.flush(chart, b, a.cfg.ui.glyphs == "ascii");
    text(b, r, 0, 0, if network { "DOWN ▲" } else { "READ ▲" }, color);
    text(
        b,
        r,
        0,
        r.height - 1,
        if network { "UP   ▼" } else { "WRITE▼" },
        t.muted,
    );
    if r.height > 3 {
        text(b, r, 0, r.height / 2, "5m / shared", t.muted);
    }
}
fn gpu(b: &mut Buffer, r: Rect, a: &App, s: &Sample) {
    if r.is_empty() {
        return;
    }
    let reserve = if r.height > 8 { 5 } else { 2 };
    gpu_fabric(
        b,
        Rect::new(r.x, r.y, r.width, r.height.saturating_sub(reserve)),
        a,
        s,
    );
    if r.height >= 3 {
        let data: Vec<_> = a
            .history
            .iter()
            .filter(|h| h.at <= s.at && h.at >= s.at - 300.)
            .filter_map(|h| h.gpu.value().copied())
            .collect();
        let y = if r.height > 8 {
            r.height - 4
        } else {
            r.height - 2
        };
        text(b, r, 0, y, "UTIL 5m", a.theme.muted);
        spark(
            b,
            Rect::new(r.x + 9, r.y + y, r.width.saturating_sub(9), 1),
            &data,
            1.,
            a.theme.gpu,
            &a.theme,
            a.cfg.ui.glyphs == "ascii",
        );
    }
    if r.height > 8 {
        dvfs(
            b,
            row(r, r.height - 3, 1),
            "GPU DVFS",
            &s.gpu_freqs,
            s.gpu_mhz.value().copied().unwrap_or(0),
            a,
        );
    }
    line(
        b,
        r,
        r.height - 1,
        format!(
            "GPU {}  SRAM {}  {}   ANE {} · aggregate utilization",
            watts(&s.power[1]),
            watts(&s.power[4]),
            temp(&s.gpu_temp, &a.cfg.units),
            watts(&s.power[2])
        ),
        a.theme.muted,
    );
}

fn gpu_fabric(b: &mut Buffer, r: Rect, a: &App, s: &Sample) {
    if r.height == 0 || r.width == 0 {
        return;
    }
    let footer = u16::from(r.height >= 5);
    let art = Rect::new(r.x, r.y, r.width, r.height.saturating_sub(footer));
    if let Some(value) = s.gpu.value() {
        let value = value.clamp(0., 1.);
        let phase = if a.motion == Motion::Full {
            a.anim.phases.get("gpu").copied().unwrap_or(0.)
        } else {
            0.
        };
        let visual = if a.motion == Motion::Full && a.cursor.is_none() {
            a.anim.gpu.pos.clamp(0., 1.)
        } else {
            value
        };
        let ranks = widgets::tile_ranks(art.width as usize * art.height as usize);
        let lit = (visual * ranks.len() as f32).round() as usize;
        for y in 0..art.height {
            for x in 0..art.width {
                let on = ranks[y as usize * art.width as usize + x as usize] < lit;
                let glyph = if on {
                    if (phase - (x + y) as f32 / 12.).sin() > 0.3 {
                        "█"
                    } else {
                        "▓"
                    }
                } else {
                    "·"
                };
                text(
                    b,
                    art,
                    x,
                    y,
                    glyph,
                    if on { a.theme.gpu } else { a.theme.wire },
                );
            }
        }
    } else if art.height > 0 {
        line(b, art, art.height / 2, s.gpu.label(), a.theme.muted);
    }
    if footer > 0 {
        line(
            b,
            r,
            r.height - 1,
            format!(
                "{} cores · {} · {} · aggregate utilization",
                s.machine.gpu_cores,
                avail(&s.gpu_mhz, |m| freq(*m)),
                watts(&s.power[1])
            ),
            a.theme.muted,
        );
    }
}

fn gpu_panels(r: Rect, cell_aspect: f32) -> (Rect, Rect) {
    let aspect = cell_aspect.clamp(1., 3.);
    // Size the bordered donut window in screen pixels, independently of its pose.
    // Keep room for telemetry on narrow terminals and shrink both dimensions together.
    let width = ((r.height as f32 * aspect).round() as u16).min(r.width.saturating_sub(1) / 2);
    let height = ((width as f32 / aspect).round() as u16).min(r.height);
    (
        Rect::new(r.x, r.y, r.width.saturating_sub(width + 1), r.height),
        Rect::new(r.right() - width, r.y, width, height),
    )
}
fn side_card(b: &mut Buffer, r: Rect, a: &App, s: &Sample) {
    let t = &a.theme;
    let mut y = 0;
    if matches!(a.page, Page::Cpu | Page::Gpu) {
        let active = if a.page == Page::Cpu {
            Some(s.cpu)
        } else {
            s.gpu.value().copied()
        };
        let scaled = if a.page == Page::Cpu {
            Some(
                s.tiers
                    .iter()
                    .map(|v| v.scaled * v.cores.len() as f32)
                    .sum::<f32>()
                    / s.core_count() as f32,
            )
        } else {
            s.gpu_scaled.value().copied()
        };
        if let Some(v) = active {
            dual_ring(
                b,
                Rect::new(
                    r.x + (r.width.saturating_sub(24)) / 2,
                    r.y,
                    24.min(r.width),
                    7.min(r.height),
                ),
                v,
                scaled.unwrap_or(0.),
                widgets::tint(a.page.part(), t),
                a,
            );
        } else {
            line(b, r, 2, "UTILIZATION N/A", t.muted);
        }
        y = 8;
        line(
            b,
            r,
            y,
            format!(
                "ACTIVE {} · SCALED {}",
                active.map_or("N/A".into(), percent),
                scaled.map_or("N/A".into(), percent)
            ),
            t.muted,
        );
        y += 2;
    }
    let rows: Vec<(String, Color)> = match a.page {
        Page::Cpu => vec![
            (
                avail(&s.cpu_split, |v| {
                    format!("USER {:.1}%  SYS {:.1}%", v[0] * 100., v[1] * 100.)
                }),
                t.text,
            ),
            (
                format!(
                    "LOAD {:.2} / {:.2} / {:.2}",
                    s.load[0], s.load[1], s.load[2]
                ),
                t.text,
            ),
            (
                format!(
                    "{} tasks · {} threads visible",
                    s.processes.len(),
                    s.processes
                        .iter()
                        .filter_map(|p| p.threads.value())
                        .sum::<u32>()
                ),
                t.muted,
            ),
            (
                format!(
                    "CPU {}  {}",
                    temp(&s.cpu_temp, &a.cfg.units),
                    watts(&s.power[0])
                ),
                t.cpu,
            ),
            (format!("UPTIME {}", duration(s.uptime as f64)), t.muted),
            ("Wakes / instructions: N/A".into(), t.muted),
        ],
        Page::Memory => vec![
            (
                avail(&s.pressure, |v| v.clone()).to_string(),
                if s.pressure.value().is_some_and(|v| v != "NORMAL") {
                    t.warn
                } else {
                    t.ok
                },
            ),
            (
                format!(
                    "PRESSURE {}",
                    avail(&s.pressure_percent, |v| format!("{v:.0}%"))
                ),
                t.memory,
            ),
            ("100 − macOS free percentage".into(), t.muted),
            (format!("SWAP {}", bytes(s.swap, &a.cfg.units)), t.text),
            (
                format!(
                    "IN  {}",
                    avail(&s.vm_rates, |v| rate(v[2], &a.cfg.units, false))
                ),
                t.text,
            ),
            (
                format!(
                    "OUT {}",
                    avail(&s.vm_rates, |v| rate(v[3], &a.cfg.units, false))
                ),
                t.text,
            ),
            (
                format!(
                    "FAULTS {}",
                    avail(&s.vm_rates, |v| format!("{:.0}/s", v[4]))
                ),
                t.muted,
            ),
            ("DRAM bandwidth: N/A".into(), t.muted),
            ("Matrix depicts categories,".into(), t.muted),
            ("not physical addresses.".into(), t.muted),
        ],
        Page::Energy => {
            let mut v = vec![
                (
                    if s.extra.stale(s.at) {
                        "ADAPTER STALE".into()
                    } else {
                        format!(
                            "ADAPTER {}",
                            avail(&s.extra.adapter, |v| v
                                .watts
                                .map_or("watts N/A".into(), |w| format!("{w:.0} W rated")))
                        )
                    },
                    t.battery,
                ),
                (
                    if s.extra.stale(s.at) {
                        "SLEEP ASSERTIONS STALE".into()
                    } else {
                        format!(
                            "SLEEP ASSERTIONS {} · i reasons",
                            avail(&s.extra.assertions, |v| v.len().to_string())
                        )
                    },
                    t.muted,
                ),
                (
                    format!(
                        "CPU {}  GPU {}",
                        temp(&s.cpu_temp, &a.cfg.units),
                        temp(&s.gpu_temp, &a.cfg.units)
                    ),
                    t.text,
                ),
                (
                    format!("THERMAL {}", avail(&s.thermal, |v| v.clone())),
                    t.ok,
                ),
            ];
            if let Some(fans) = s.fans.value() {
                for f in fans {
                    v.push((format!("{} {:>5} rpm", f.name, f.rpm), t.muted));
                }
            } else {
                v.push((s.fans.reason().into(), t.muted));
            }
            if let Some(bat) = s.battery.value() {
                v.extend([
                    (
                        format!(
                            "PACK {:.0}% · {}",
                            bat.percent,
                            if bat.charging {
                                "charging"
                            } else if bat.ac {
                                "AC"
                            } else {
                                "battery"
                            }
                        ),
                        t.battery,
                    ),
                    (format!("PACK POWER {}", watts(&bat.watts)), t.text),
                    (
                        format!("HEALTH {}", avail(&bat.health, |v| format!("{v:.0}%"))),
                        t.muted,
                    ),
                    (
                        format!("CYCLES {}", avail(&bat.cycles, |v| v.to_string())),
                        t.muted,
                    ),
                    (
                        format!("TIME {}", avail(&bat.minutes, |v| format!("{v} min"))),
                        t.muted,
                    ),
                ]);
            } else {
                v.push((s.battery.reason().into(), t.muted));
            }
            v
        }
        Page::Disk => {
            let mut v = vec![(super::insights::disk(s), t.disk)];
            if !s.extra.stale(s.at)
                && let Some(disks) = s.extra.disks.value()
            {
                for d in disks.iter().take(2) {
                    v.push((
                        format!(
                            "{} avg R {} / W {}",
                            d.name,
                            d.read_size
                                .map_or("idle".into(), |v| bytes(v as u64, &a.cfg.units)),
                            d.write_size
                                .map_or("idle".into(), |v| bytes(v as u64, &a.cfg.units))
                        ),
                        t.muted,
                    ));
                }
            }
            for d in &s.volumes {
                v.push((
                    format!("{}{}", d.name, if d.external { " · external" } else { "" }),
                    t.disk,
                ));
                v.push((
                    format!(
                        "{} · {}",
                        d.mount,
                        if d.read_only {
                            "read only"
                        } else {
                            "read / write"
                        }
                    ),
                    t.muted,
                ));
                v.push((
                    format!(
                        "{} / {}",
                        bytes(d.used, &a.cfg.units),
                        bytes(d.total, &a.cfg.units)
                    ),
                    t.text,
                ));
            }
            v.push(("APFS volumes share capacity.".into(), t.muted));
            v
        }
        Page::Network => {
            let mut v = vec![];
            if !s.extra.stale(s.at) {
                if let Some(w) = s.extra.wifi.value() {
                    v.push((format!("Wi-Fi {} · {} dBm", w.interface, w.rssi), t.net));
                    v.push((
                        format!(
                            "Noise {} · SNR {}",
                            w.noise.map_or("N/A".into(), |n| format!("{n} dBm")),
                            w.noise
                                .map_or("N/A".into(), |n| format!("{} dB", w.rssi - n))
                        ),
                        t.text,
                    ));
                    v.push((
                        format!(
                            "CH {} · {} link",
                            w.channel,
                            w.tx_mbps.map_or("N/A".into(), |n| format!("{n:.0} Mb/s"))
                        ),
                        t.muted,
                    ));
                } else {
                    v.push((format!("Wi-Fi {}", s.extra.wifi.label()), t.muted));
                }
                if let Some(routes) = s.extra.routes.value() {
                    for route in routes.iter().take(2) {
                        v.push((
                            format!("{} route: {}", route.family, route.interface),
                            t.net,
                        ));
                    }
                }
            } else {
                v.push(("Link details STALE · i details".into(), t.warn));
            }
            if let Some(f) = s.network.value() {
                v.push((format!("{} · physical interface", f.name), t.net));
                for ip in f.addresses.iter().take(2) {
                    v.push((ip.clone(), t.text));
                }
                v.push((
                    format!("↓ {:.0}  ↑ {:.0} packets/s", f.packets_in, f.packets_out),
                    t.muted,
                ));
            }
            v.push(("TCP · visible sockets only".into(), t.net));
            if let Some(states) = s.connection_states.value() {
                for (state, n) in states {
                    v.push((format!("{state:<18} {n:>5}"), t.text));
                }
            } else {
                v.push((s.connection_states.reason().into(), t.muted));
            }
            v
        }
        Page::Gpu => vec![
            (
                format!(
                    "FREQ {}",
                    avail(&s.gpu_mhz, |v| format!("{:.2} GHz", *v as f32 / 1000.))
                ),
                t.gpu,
            ),
            (
                format!("GPU {}  SRAM {}", watts(&s.power[1]), watts(&s.power[4])),
                t.text,
            ),
            (format!("TEMP {}", temp(&s.gpu_temp, &a.cfg.units)), t.text),
            (format!("ANE ◇─◇─◇ {}", watts(&s.power[2])), t.ane),
            ("Tiles: usage · Visual: animation".into(), t.muted),
        ],
    };
    for (label, c) in rows {
        if y >= r.height {
            break;
        }
        line(b, r, y, label, c);
        y += if r.height > 19 { 2 } else { 1 };
    }
}
fn dual_ring(b: &mut Buffer, r: Rect, active: f32, scaled: f32, color: Color, a: &App) {
    let mut g = BrailleGrid::new(r.width, r.height);
    let cx = r.width as f32 - 0.5;
    let cy = r.height as f32 * 2. - 0.5;
    let rad = (cx.min(cy) - 1.).max(1.);
    for (radius, value) in [(rad, active), (rad * 0.78, scaled)] {
        for i in 0..180 {
            let theta = i as f32 / 180. * std::f32::consts::TAU - std::f32::consts::FRAC_PI_2;
            g.set(
                (cx + radius * theta.cos()) as i32,
                (cy + radius * theta.sin()) as i32,
                if i as f32 / 180. < value {
                    color
                } else {
                    a.theme.wire
                },
            );
        }
    }
    g.flush(r, b, a.cfg.ui.glyphs == "ascii");
    let label = format!("{:>3.0}%", active * 100.);
    text(
        b,
        r,
        r.width.saturating_sub(4) / 2,
        r.height / 2,
        label,
        color,
    );
    if r.height > 0 {
        b.set_style(
            row(r, r.height / 2, 1),
            Style::new().add_modifier(Modifier::BOLD),
        );
    }
}

fn inline_stats(b: &mut Buffer, r: Rect, a: &App, s: &Sample) {
    let (first, second) = match a.page {
        Page::Cpu => (
            format!(
                "{} TASKS · {} threads visible · UP {}",
                s.processes.len(),
                s.processes
                    .iter()
                    .filter_map(|p| p.threads.value())
                    .sum::<u32>(),
                duration(s.uptime as f64)
            ),
            "Outer = active residency · inner = frequency-scaled utilization".into(),
        ),
        Page::Memory => (
            format!(
                "PRESSURE {} · {}   SWAP {}",
                avail(&s.pressure, |v| v.clone()),
                avail(&s.pressure_percent, |v| format!("{v:.0}%")),
                bytes(s.swap, &a.cfg.units)
            ),
            format!(
                "SWAP IN {}  OUT {} · DRAM bandwidth N/A",
                avail(&s.vm_rates, |v| rate(v[2], &a.cfg.units, false)),
                avail(&s.vm_rates, |v| rate(v[3], &a.cfg.units, false))
            ),
        ),
        Page::Energy => (
            format!(
                "CPU {}  GPU {}   {}   {}",
                temp(&s.cpu_temp, &a.cfg.units),
                temp(&s.gpu_temp, &a.cfg.units),
                avail(&s.thermal, |v| v.clone()),
                avail(&s.fans, |f| f
                    .iter()
                    .map(|v| format!("{} {}rpm", v.name, v.rpm))
                    .collect::<Vec<_>>()
                    .join(" · "))
            ),
            super::insights::power(s),
        ),
        Page::Disk => (
            s.volumes
                .first()
                .map_or("Volume information unavailable".into(), |v| {
                    format!(
                        "{} {} · {} / {}",
                        v.name,
                        v.mount,
                        bytes(v.used, &a.cfg.units),
                        bytes(v.total, &a.cfg.units)
                    )
                }),
            format!(
                "{} · {} volumes · i operation sizes",
                super::insights::disk(s),
                s.volumes.len()
            ),
        ),
        Page::Network => (
            format!(
                "{} · ↓ {:.0} / ↑ {:.0} packets/s",
                s.network.value().map_or("N/A", |v| v.name.as_str()),
                s.network.value().map_or(0., |v| v.packets_in),
                s.network.value().map_or(0., |v| v.packets_out)
            ),
            super::insights::wifi(s),
        ),
        Page::Gpu => (
            format!(
                "ACTIVE {}  SCALED {}   {} GPU cores",
                avail(&s.gpu, |v| percent(*v)),
                avail(&s.gpu_scaled, |v| percent(*v)),
                s.machine.gpu_cores
            ),
            format!(
                "GPU {}   SRAM {}   ANE {}   {}",
                watts(&s.power[1]),
                watts(&s.power[4]),
                watts(&s.power[2]),
                temp(&s.gpu_temp, &a.cfg.units)
            ),
        ),
    };
    line(b, r, 0, first, a.theme.text);
    line(b, r, 1, second, a.theme.muted);
}

#[cfg(test)]
mod fabric_tests {
    use super::*;
    #[test]
    fn donut_window_is_square_and_leaves_the_rest_for_telemetry() {
        for aspect in [1., 1.6, 2.05, 3.] {
            for width in 0..240 {
                let r = Rect::new(7, 3, width, 18);
                let (left, right) = gpu_panels(r, aspect);
                assert!(left.width >= right.width);
                assert!(left.right() <= right.x);
                assert_eq!(right.right(), r.right());
                assert!(right.height <= r.height);
                assert!((right.width as f32 / aspect - right.height as f32).abs() <= 0.5);
            }
        }
    }
}
