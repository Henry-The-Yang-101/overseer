use super::{kit::*, theme::Theme};
use crate::{
    app::App,
    metrics::{
        Avail, Sample,
        ports::{Port, PortKind},
    },
};
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::Color,
    widgets::{Clear, Widget},
};

pub fn find(s: &Sample, kind: PortKind) -> Option<&Port> {
    s.ports.value()?.iter().find(|p| p.kind == kind)
}
pub fn indicator(s: &Sample, kind: PortKind, t: &Theme) -> (&'static str, Color, &'static str) {
    if s.at - s.ports_at > 10. && s.ports.value().is_some() {
        return ("?", t.warn, "STALE");
    }
    match find(s, kind).map(|p| &p.connected) {
        Some(Avail::Ok(true)) => ("●", t.ok, "CONNECTED"),
        Some(Avail::Ok(false)) => ("○", t.muted, "EMPTY"),
        _ => ("?", t.muted, "UNKNOWN"),
    }
}
pub fn summary(s: &Sample) -> String {
    let Some(ports) = s.ports.value() else {
        return format!("PORTS · {}   [p]", s.ports.label());
    };
    if s.at - s.ports_at > 10. {
        return "PORTS · stale reading   [p]".into();
    }
    let connected = ports
        .iter()
        .filter(|p| p.connected == Avail::Ok(true))
        .count();
    let known = ports
        .iter()
        .filter(|p| matches!(p.connected, Avail::Ok(_)))
        .count();
    format!(
        "PORTS · {connected} connected{}   [p]",
        if known < ports.len() {
            " · some unknown"
        } else {
            ""
        }
    )
}
pub fn card(b: &mut Buffer, r: Rect, a: &App) {
    let t = &a.theme;
    let rr = frame(b, r, "PHYSICAL PORTS", "p details", false, t);
    contents(b, rr, a, false);
}
pub fn modal(b: &mut Buffer, r: Rect, a: &App) {
    Clear.render(r, b);
    let rr = frame(b, r, "PHYSICAL PORTS", "Esc / p close", true, &a.theme);
    contents(b, rr, a, true);
}
fn contents(b: &mut Buffer, r: Rect, a: &App, expanded: bool) {
    let s = a.sample();
    let t = &a.theme;
    line(b, r, 0, summary(&s).trim_end_matches("   [p]"), t.text);
    let Some(ports) = s.ports.value() else {
        let reason = s.ports.reason();
        line(b, r, 2, truncate(reason, r.width as usize), t.muted);
        line(b, r, 4, "Unknown does not mean empty.", t.muted);
        return;
    };
    let double = r.height as usize >= ports.len() * 2 + 6;
    let start = if double { 2 } else { 1 };
    for (i, port) in ports.iter().enumerate() {
        let y = start + i as u16 * if double { 2 } else { 1 };
        if y >= r.height.saturating_sub(1) {
            break;
        }
        let (dot, color, status) = indicator(&s, port.kind, t);
        line(b, r, y, format!("{dot}  {}", port.kind.label()), color);
        text(b, r, r.width.saturating_sub(9), y, status, color);
        if double {
            text(
                b,
                r,
                3,
                y + 1,
                truncate(&port.detail, r.width.saturating_sub(3) as usize),
                t.muted,
            );
        } else if expanded && r.width >= 58 {
            text(
                b,
                r,
                15,
                y,
                truncate(&port.detail, r.width.saturating_sub(26) as usize),
                t.muted,
            );
        }
    }
    let end = start + ports.len() as u16 * if double { 2 } else { 1 };
    if r.height >= end + 5 {
        line(b, r, end + 1, "● attached  ○ empty  ? unknown", t.muted);
        line(
            b,
            r,
            end + 2,
            if expanded {
                "USB-C IDs come from macOS; placement is approximate."
            } else {
                "Wiring & pulses are symbolic."
            },
            t.muted,
        );
    }
    if r.height >= end + 4 {
        let input = crate::metrics::ports::power_input(&s);
        let label = input
            .value()
            .map(|p| {
                if expanded {
                    format!("Power → BMS · ≈ {} (inferred)", p.label())
                } else {
                    format!("≈ {} → BMS · inferred", p.label())
                }
            })
            .unwrap_or_else(|| {
                if s.battery.value().is_some_and(|b| !b.ac) {
                    "Power · running on battery".into()
                } else {
                    "Power input · not identified".into()
                }
            });
        line(b, r, r.height.saturating_sub(3), label, t.battery);
    }
    line(
        b,
        r,
        r.height.saturating_sub(1),
        if a.cursor.is_some() {
            "Recorded connections · replay"
        } else if a.demo.is_some() {
            "Synthetic connections · demo"
        } else {
            "Updates every 3s · macOS"
        },
        t.muted,
    );
}
