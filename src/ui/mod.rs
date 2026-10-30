pub mod audio;
pub mod insights;
pub mod kit;
pub mod overlays;
pub mod ports;
pub mod processes;
pub mod systems;
pub mod teardown;
pub mod tesseract;
pub mod theme;
pub mod widgets;
use crate::{
    app::{App, Layer, Screen},
    motion::Motion,
};
use kit::*;
use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Style},
    text::{Line, Span},
    widgets::{Paragraph, Widget},
};
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum SizeClass {
    TooSmall,
    Compact,
    Regular,
    Wide,
}
impl SizeClass {
    pub fn of(a: Rect) -> Self {
        match (a.width, a.height) {
            (w, h) if w < 60 || h < 18 => Self::TooSmall,
            (w, h) if w < 100 || h < 30 => Self::Compact,
            (w, h) if w < 150 || h < 42 => Self::Regular,
            _ => Self::Wide,
        }
    }
}
pub fn draw(f: &mut Frame, a: &mut App, dt: std::time::Duration) {
    let area = f.area();
    let b = f.buffer_mut();
    let t = a.theme.clone();
    b.set_style(area, Style::new().bg(t.void).fg(t.text));
    a.hitmap.0.clear();
    a.parts.clear();
    let size = SizeClass::of(area);
    if size == SizeClass::TooSmall {
        let w = area.width.min(48);
        let h = area.height.min(7);
        let r = Rect::new((area.width - w) / 2, (area.height - h) / 2, w, h);
        let r = frame(b, r, "OVERSEER", "", true, &t);
        line(b, r, 1, "A little more room to see your machine.", t.text);
        line(
            b,
            r,
            3,
            format!("Needs 60 × 18 · current {} × {}", area.width, area.height),
            t.muted,
        );
        return;
    }
    let top = Rect::new(0, 0, area.width, 3);
    let rail_h = if size >= SizeClass::Regular { 3 } else { 2 };
    let stage = Rect::new(1, 3, area.width - 2, area.height - 3 - rail_h - 1);
    let rail = Rect::new(1, area.height - rail_h - 1, area.width - 2, rail_h);
    let dock = Rect::new(1, area.height - 1, area.width - 2, 1);
    chrome(b, top, a, size);
    if a.boot {
        overlays::boot(b, stage, a);
    } else if let Some(p) = a.zoom {
        overlays::detail(b, stage, a, p, true);
    } else {
        match a.screen {
            Screen::Teardown => teardown::draw(b, stage, a, size),
            Screen::Systems => systems::draw(b, stage, a, size),
        }
    }
    if a.transition && a.motion == Motion::Full {
        a.fx.add_unique_effect(
            1,
            tachyonfx::fx::sweep_in(
                if a.slide_direction < 0 {
                    tachyonfx::Motion::RightToLeft
                } else {
                    tachyonfx::Motion::LeftToRight
                },
                10,
                0,
                t.void,
                (180, tachyonfx::Interpolation::QuadOut),
            )
            .with_area(stage),
        );
        a.transition = false;
    }
    if a.motion == Motion::Full {
        a.fx.process_effects(dt.into(), b, stage);
    } else {
        a.transition = false;
    }
    if a.cursor.is_some() {
        for y in stage.y..stage.bottom() {
            for x in stage.x..stage.right() {
                let c = &mut b[(x, y)];
                if let Color::Rgb(r, g, bl) = c.fg {
                    let lum = (r as f32 * 0.2126 + g as f32 * 0.7152 + bl as f32 * 0.0722) as u8;
                    c.set_fg(theme::blend(c.fg, Color::Rgb(lum, lum, lum), 0.35));
                }
            }
        }
    }
    timeline(b, rail, a);
    status_line(b, row(rail, rail.height - 1, 1), a);
    hints(b, dock, a, size);
    overlays::draw(b, stage, a);
    if let Some((old, since)) = &a.theme_from
        && a.motion == Motion::Full
        && since.elapsed().as_secs_f32() < 0.25
    {
        let progress = since.elapsed().as_secs_f32() / 0.25;
        let pairs = [
            (t.void, old.void),
            (t.panel, old.panel),
            (t.substrate, old.substrate),
            (t.wire, old.wire),
            (t.accent, old.accent),
            (t.text, old.text),
            (t.muted, old.muted),
            (t.cpu, old.cpu),
            (t.gpu, old.gpu),
            (t.memory, old.memory),
            (t.ane, old.ane),
            (t.net, old.net),
            (t.battery, old.battery),
        ];
        for cell in &mut b.content {
            for (new, old) in pairs {
                if cell.fg == new {
                    cell.fg = theme::blend(old, new, progress);
                    break;
                }
            }
            for (new, old) in pairs {
                if cell.bg == new {
                    cell.bg = theme::blend(old, new, progress);
                    break;
                }
            }
        }
    }
    if a.cfg.ui.glyphs == "ascii" {
        ascii(b);
    }
    if a.cfg.ui.transparent_bg {
        for c in &mut b.content {
            if c.bg == t.void {
                c.set_bg(Color::Reset);
            }
        }
    }
    let truecolor = std::env::var("COLORTERM").is_ok_and(|v| v == "truecolor" || v == "24bit");
    if !truecolor {
        for c in &mut b.content {
            c.fg = theme::quantize(c.fg);
            c.bg = theme::quantize(c.bg);
        }
    }
}
fn chrome(b: &mut ratatui::buffer::Buffer, r: Rect, a: &mut App, size: SizeClass) {
    let t = &a.theme;
    let s = a.sample();
    let status = if let Some(d) = &a.demo {
        {
            let state = if a.cursor.is_some() {
                format!(
                    " · {} −{}",
                    if a.playing { "REPLAY" } else { "PAUSED" },
                    duration(a.latest_time() - s.at)
                )
            } else {
                String::new()
            };
            if size == SizeClass::Compact {
                format!("SYNTHETIC{state}")
            } else {
                format!("SYNTHETIC / {d}{state}")
            }
        }
    } else if a.last_received.elapsed().as_secs() > 3 {
        format!("STALE {}s", a.last_received.elapsed().as_secs())
    } else if a.history.is_empty() {
        "ACQUIRING".into()
    } else if a.cursor.is_some() {
        format!(
            "{} −{}",
            if a.playing {
                format!("REPLAY {}x", 1 << a.speed)
            } else {
                "PAUSED".into()
            },
            duration(a.latest_time() - s.at)
        )
    } else {
        "● LIVE".into()
    };
    text(b, r, 1, 0, "O V E R S E E R", t.accent);
    let info = if s.machine.chip.is_empty() {
        "Hardware observatory".into()
    } else if size == SizeClass::Compact {
        s.machine.chip.replace("Apple ", "")
    } else {
        format!(
            "{}  /  {} cores  /  {}",
            s.machine.chip,
            s.machine.cpu_count,
            bytes(s.machine.memory, &a.cfg.units)
        )
    };
    let status_width = unicode_width::UnicodeWidthStr::width(status.as_str()) as u16;
    text(
        b,
        r,
        18,
        0,
        truncate(&info, r.width.saturating_sub(status_width + 21) as usize),
        t.muted,
    );
    let sw = unicode_width::UnicodeWidthStr::width(status.as_str()) as u16;
    if sw + 18 < r.width {
        text(
            b,
            r,
            r.width - sw - 1,
            0,
            &status,
            if a.demo.is_some() {
                t.synthetic
            } else if a.last_received.elapsed().as_secs() > 3 {
                t.warn
            } else {
                t.ok
            },
        );
    }
    let mut x = 1;
    for (screen, name) in [
        (Screen::Teardown, "1 Teardown"),
        (Screen::Systems, "2 Systems"),
    ] {
        let selected = a.screen == screen;
        let label = format!(" {} ", name);
        let w = label.len() as u16;
        b.set_string(
            r.x + x,
            r.y + 2,
            &label,
            Style::new()
                .fg(if selected { t.void } else { t.muted })
                .bg(if selected { t.accent } else { t.void }),
        );
        a.hitmap.add(Rect::new(x, 2, w, 1), 10, Hit::Screen(screen));
        x += w + 1;
    }
    if let Some(p) = &a.displayed_follow() {
        let label = format!(
            "◎ {} #{}  {:.1}%  {}{}",
            p.name,
            p.pid,
            p.cpu,
            bytes(p.memory, &a.cfg.units),
            if a.follow_exited.is_some() {
                " · EXITED"
            } else {
                ""
            }
        );
        text(
            b,
            r,
            x + 2,
            2,
            label,
            if a.follow_exited.is_some() {
                t.crit
            } else {
                t.cpu
            },
        );
    } else if size >= SizeClass::Regular {
        let label = format!(
            "THERMAL {}   SYS {}",
            s.thermal.value().map_or(s.thermal.label(), String::as_str),
            watts(&s.system_power)
        );
        text(
            b,
            r,
            r.width.saturating_sub(label.len() as u16 + 2),
            2,
            label,
            t.muted,
        );
    }
}
fn timeline(b: &mut ratatui::buffer::Buffer, r: Rect, a: &mut App) {
    let t = &a.theme;
    let end = a.latest_time();
    let start =
        (end - crate::app::WINDOWS[a.window] as f64).max(a.history.front().map_or(0., |s| s.at));
    let label = ["1m", "5m", "15m", "1h"][a.window];
    text(b, r, 0, 0, label, t.muted);
    let chart = Rect::new(r.x + 4, r.y, r.width.saturating_sub(11), 1);
    a.rail = chart;
    a.hitmap.add(chart, 10, Hit::Rail);
    let data: Vec<_> = a
        .history
        .iter()
        .filter(|s| s.at >= start)
        .map(|s| s.cpu)
        .collect();
    spark(b, chart, &data, 1., t.cpu, t, a.cfg.ui.glyphs == "ascii");
    let gpu: Vec<_> = a
        .history
        .iter()
        .filter(|s| s.at >= start)
        .map(|s| s.gpu.value().copied().unwrap_or(0.))
        .collect();
    let pooled = stretch(&gpu, chart.width as usize);
    for (i, v) in pooled.iter().enumerate() {
        if *v > 0.5 {
            text(b, chart, i as u16, 0, "⠉", t.gpu);
        }
    }
    let xpos = |time: f64| {
        (((time - start) / (end - start).max(1.)).clamp(0., 1.)
            * chart.width.saturating_sub(1) as f64) as u16
    };
    for spike in a.spikes() {
        if spike >= start {
            text(b, chart, xpos(spike), 0, "▾", t.warn);
        }
    }
    if a.screen == Screen::Systems {
        for event in &a.system_events {
            if event.at >= start && event.at <= end {
                text(b, chart, xpos(event.at), 0, "│", t.accent);
            }
        }
    }
    text(b, chart, xpos(a.sample().at), 0, "◆", t.bright);
    text(
        b,
        r,
        r.width.saturating_sub(6),
        0,
        if a.cursor.is_some() { "PAST" } else { "NOW" },
        if a.cursor.is_some() { t.accent } else { t.ok },
    );
    if r.height > 2 {
        let duration = duration(end - start);
        text(b, r, 4, 1, format!("−{duration}"), t.muted);
        text(
            b,
            r,
            r.width / 2,
            1,
            if a.screen == Screen::Systems {
                "CPU / GPU · │ events · click to scrub"
            } else {
                "CPU / GPU · click to scrub"
            },
            t.muted,
        );
    }
}
// Passive messages have a reserved row: they never paint over content or keyboard hints.
fn status_line(b: &mut ratatui::buffer::Buffer, r: Rect, a: &App) {
    if !a.layers.is_empty() || a.boot {
        return;
    }
    let t = &a.theme;
    let message = if let Some(info) = a.toasts.iter().rev().find(|t| t.key == "info") {
        info.text.clone()
    } else if let Some((Hit::Part(p), since)) = &a.hover
        && since.elapsed().as_millis() > 250
    {
        format!(
            "{} · {} · Enter inspect / z zoom",
            p.name(),
            widgets::readout(*p, &a.sample(), a)
        )
    } else if !a.cfg.ui.onboarding_seen && a.started.elapsed().as_secs() < 12 {
        let i = (a.started.elapsed().as_secs() / 4).min(2) as usize;
        [
            "WELCOME · Tab selects · Enter inspects · Esc goes back",
            "2 opens Systems · ← → switches resource pages",
            "p shows connected ports · ? controls · : commands",
        ][i]
            .into()
    } else {
        return;
    };
    line(b, r, 0, truncate(&message, r.width as usize), t.muted);
}
fn hints(b: &mut ratatui::buffer::Buffer, r: Rect, a: &App, size: SizeClass) {
    let t = &a.theme;
    let h = if let Some(layer) = a.layers.last() {
        match layer {
            Layer::Ports => "p / Esc close ports",
            Layer::SystemsInfo { .. } => {
                "←→ tabs   ↑↓ scroll / apps   Enter expand app   Esc close"
            }
            Layer::Help => "Esc close controls",
            Layer::Palette { .. } => "↑↓ choose   Enter run   Esc close",
            Layer::Signal { force, .. } => {
                if *force {
                    "Y confirm SIGKILL   Esc cancel"
                } else {
                    "y confirm SIGTERM   Esc cancel"
                }
            }
            Layer::Inspector(crate::app::Part::SpeakerLeft | crate::app::Part::SpeakerRight) => {
                "Enter / e toggle capture   Esc close"
            }
            Layer::Inspector(p)
                if matches!(a.resolve(*p), crate::app::Part::Cpu | crate::app::Part::Soc) =>
            {
                "←→ choose core   Esc close"
            }
            Layer::Inspector(_) => "Esc close inspector",
            Layer::Process { .. } => "←→ inspector tabs   ↑↓ task   PgUp/PgDn scroll   Esc close",
        }
    } else if a.searching {
        "Type to filter   ↑↓ choose   Enter done   Esc clear"
    } else if a.cursor.is_some() {
        ", . step   < > ×10   { } spikes   Space play   End live   ? help"
    } else if a.zoom.is_some() {
        "← → sibling   Enter details   Esc back"
    } else if a.screen == Screen::Systems {
        "←→ pages   ↑↓ tasks   Enter inspect   i stats / history / apps   / filter   z expand"
    } else if a.screen == Screen::Teardown && size == SizeClass::Compact {
        "Tab select   Enter inspect   p ports   m cells   2 Systems   ? help"
    } else if a.screen == Screen::Teardown {
        "Tab select   g system page   Enter inspect   z zoom   m cells   p ports   2 Systems   ? help"
    } else if size == SizeClass::Compact {
        "Tab select   Enter inspect   v teardown   Space pause   ? help"
    } else {
        "Tab select   Enter inspect   v teardown   Space pause   / processes   ? help   : commands"
    };
    let mut spans = vec![];
    for (i, part) in h.split("   ").enumerate() {
        if i > 0 {
            spans.push(Span::raw("   "));
        }
        if let Some((key, label)) = part.split_once(' ') {
            spans.push(Span::styled(key, Style::new().fg(t.accent)));
            spans.push(Span::styled(format!(" {label}"), Style::new().fg(t.muted)));
        } else {
            spans.push(Span::raw(part));
        }
    }
    let hint_area = if a.debug && r.width > 130 {
        Rect::new(r.x, r.y, r.width.saturating_sub(20), r.height)
    } else {
        r
    };
    Paragraph::new(Line::from(spans)).render(hint_area, b);
    if a.debug && r.width > 130 {
        let s = format!("{:2.0}fps {:.2}ms", a.fps, a.render_ms);
        text(b, r, r.width - s.len() as u16, 0, s, t.muted);
    }
}
pub fn ascii(b: &mut ratatui::buffer::Buffer) {
    for c in &mut b.content {
        let s = c.symbol().to_owned();
        let out: String = s
            .chars()
            .map(|ch| match ch {
                '\u{2800}'..='\u{28ff}' => ':',
                '█' | '▓' | '▒' | '░' | '■' | '▣' | '▰' => '#',
                '▉'..='▏' => '|',
                '◈' | '◇' | '◆' | '●' | '◎' => '*',
                '≈' => '~',
                '→' | '▸' | '›' => '>',
                '←' | '◂' | '‹' => '<',
                '↑' | '▲' => '^',
                '↓' | '▾' => 'v',
                'ϟ' => 'W',
                '╱' | '╲' => '/',
                '−' | '—' => '-',
                '·' | '…' => '.',
                _ => ch,
            })
            .collect();
        c.set_symbol(&out);
    }
}
/// Faithful vector export of the actual terminal buffer, useful for visual regression review.
pub fn export_svg(b: &ratatui::buffer::Buffer) -> String {
    fn escape(s: &str) -> String {
        s.replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
            .replace('"', "&quot;")
    }
    fn color(c: Color) -> String {
        match c {
            Color::Rgb(r, g, b) => format!("#{r:02x}{g:02x}{b:02x}"),
            Color::Indexed(i) => {
                if i >= 232 {
                    let v = 8 + (i - 232) * 10;
                    format!("#{v:02x}{v:02x}{v:02x}")
                } else if i >= 16 {
                    let n = i - 16;
                    let l = |v: u8| if v == 0 { 0 } else { 55 + v * 40 };
                    format!("#{:02x}{:02x}{:02x}", l(n / 36), l(n / 6 % 6), l(n % 6))
                } else {
                    "#aabbcc".into()
                }
            }
            _ => "#d5e5eb".into(),
        }
    }
    let (w, h) = (
        b.area.width as usize * 9 + 32,
        b.area.height as usize * 18 + 32,
    );
    let mut out = format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{w}\" height=\"{h}\" viewBox=\"0 0 {w} {h}\"><rect width=\"100%\" height=\"100%\" fill=\"#070c10\"/><g font-family=\"JetBrains Mono,Menlo,monospace\" font-size=\"14\">"
    );
    for y in 0..b.area.height {
        for x in 0..b.area.width {
            let c = &b[(x, y)];
            let (px, py) = (16 + x as usize * 9, 16 + y as usize * 18);
            if c.bg != Color::Reset {
                out.push_str(&format!(
                    "<rect x=\"{px}\" y=\"{py}\" width=\"9\" height=\"18\" fill=\"{}\"/>",
                    color(c.bg)
                ));
            }
        }
    }
    for y in 0..b.area.height {
        for x in 0..b.area.width {
            let c = &b[(x, y)];
            if c.symbol().trim().is_empty() {
                continue;
            }
            out.push_str(&format!(
                "<text x=\"{}\" y=\"{}\" fill=\"{}\"{}>{}</text>",
                16 + x as usize * 9,
                30 + y as usize * 18,
                color(c.fg),
                if c.modifier.contains(ratatui::style::Modifier::BOLD) {
                    " font-weight=\"bold\""
                } else {
                    ""
                },
                escape(c.symbol())
            ));
        }
    }
    out.push_str("</g></svg>");
    out
}
