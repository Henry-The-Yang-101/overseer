use super::{
    kit::*,
    theme::{Theme, blend},
};
use crate::{
    app::{App, Part},
    metrics::{Fan, Sample},
    motion::Motion,
};
use ratatui::{
    buffer::Buffer,
    layout::{Alignment, Rect},
    style::{Color, Style},
    text::Line,
    widgets::{Block, BorderType, Borders, Clear, Widget},
};

/// Terminal columns at the open exhaust mouth, shared with the grille above it.
pub fn exhaust(r: Rect, mirror: bool) -> Rect {
    if r.is_empty() || r.width < 10 || r.height < 6 {
        return Rect::new(r.x, r.y, r.width, 1);
    }
    let span = (r.width * 2 - 1) as f32;
    let outer = (span * 0.12).round() as u16 / 2;
    let inner = (span * 0.98).round() as u16 / 2;
    let x = if mirror { r.width - 1 - inner } else { outer };
    Rect::new(r.x + x, r.y, inner - outer + 1, 1)
}

/// A mirrored blower shroud: straight exhaust neck above a rounded lower chamber.
pub fn housing(
    b: &mut Buffer,
    r: Rect,
    label: &str,
    color: Color,
    theme: &Theme,
    mirror: bool,
    ascii: bool,
) -> Rect {
    Clear.render(r, b);
    b.set_style(r, Style::new().bg(theme.panel));
    if r.width < 10 || r.height < 6 {
        let block = Block::bordered()
            .borders(Borders::LEFT | Borders::RIGHT | Borders::BOTTOM)
            .border_type(BorderType::Rounded)
            .border_style(Style::new().fg(color))
            .title_top(Line::from(label).alignment(Alignment::Center));
        block.render(r, b);
        return inner(r, 1, 1);
    }
    let mut grid = BrailleGrid::new(r.width, r.height);
    let point = |x: f32, y: f32| {
        (
            (if mirror { 1. - x } else { x }) * (r.width * 2 - 1) as f32,
            y * (r.height * 4 - 1) as f32,
        )
    };
    for (from, to) in [
        ((0.98, 0.), (0.98, 0.48)),
        ((0.02, 0.48), (0.12, 0.24)),
        ((0.12, 0.24), (0.12, 0.)),
    ] {
        grid.line(point(from.0, from.1), point(to.0, to.1), color);
    }
    let mut old = point(0.98, 0.48);
    for i in 1..=64 {
        let angle = i as f32 / 64. * std::f32::consts::PI;
        let next = point(0.5 + 0.48 * angle.cos(), 0.48 + 0.50 * angle.sin());
        grid.line(old, next, color);
        old = next;
    }
    grid.flush(r, b, ascii);
    if !label.is_empty() {
        let label = format!(" {label} ");
        let inset = (r.width as f32 * 0.12).ceil() as u16;
        let x = if mirror {
            r.width.saturating_sub(inset + label.len() as u16)
        } else {
            inset
        };
        text(b, r, x, 0, label, color);
    }
    inner(r, 1, 1)
}

pub fn impeller(
    b: &mut Buffer,
    r: Rect,
    phase: f32,
    fan: &Fan,
    theme: &Theme,
    mirror: bool,
    ascii: bool,
) {
    draw_impeller(
        b,
        r,
        phase,
        fan,
        theme,
        ImpellerStyle {
            mirror,
            ascii,
            readout: true,
        },
    );
}
struct ImpellerStyle {
    mirror: bool,
    ascii: bool,
    readout: bool,
}
fn draw_impeller(
    b: &mut Buffer,
    r: Rect,
    phase: f32,
    fan: &Fan,
    theme: &Theme,
    style: ImpellerStyle,
) {
    let ImpellerStyle {
        mirror,
        ascii,
        readout,
    } = style;
    if r.width < 2 || r.height < 2 {
        return;
    }
    let hub_left = (r.width - 2) / 2;
    let label_x = if mirror {
        r.width - hub_left - 2
    } else {
        hub_left
    };
    let label_y = (r.height - 1) / 2;
    let cx = label_x as f32 * 2. + 1.5;
    let radius_at = |cy: f32| {
        (cx.min(r.width as f32 * 2. - 1. - cx)
            .min(cy)
            .min(r.height as f32 * 4. - 1. - cy)
            - 1.)
            .max(1.)
    };
    let hub_radius = |radius: f32| (radius * 0.46).max(5.5).min(radius * 0.75);
    let number = fan.rpm.to_string();
    let inline_cy = label_y as f32 * 4. + 1.5;
    let inline_radius = radius_at(inline_cy);
    // Reserve at least four digits, a blank column on each side, and vertical
    // clearance inside the circle. The layout stays stable as the speed changes.
    let padded_half_width = number.len().max(4) as f32 + 2.;
    let inline = inline_radius >= 5. && hub_radius(inline_radius) >= padded_half_width.hypot(3.);
    // Center the circle on either one row or the compact two-by-two readout.
    let cy = if inline { inline_cy } else { inline_cy + 2. };
    let radius = radius_at(cy);
    // Units can sit a little closer to the hub edge than the number-only layout.
    let with_unit = inline && hub_radius(radius) >= (padded_half_width + 3.).hypot(2.);
    let color = if fan.rpm > 0 {
        theme.accent
    } else {
        theme.wire
    };
    let mut grid = BrailleGrid::new(r.width, r.height);
    if radius >= 5. {
        let hub = hub_radius(radius);
        grid.circle(cx, cy, radius, blend(color, theme.panel, 0.35));
        grid.circle(cx, cy, hub, color);
        // Short, closely spaced vanes match a centrifugal intake. Halve the visual
        // phase for twice the vane count so the existing speed limit still avoids aliasing.
        for i in 0..18 {
            let angle = (phase * 0.5 + i as f32 / 18.) * std::f32::consts::TAU;
            let mut old = None;
            for step in 0..=4 {
                let f = step as f32 / 4.;
                let distance = hub + 1. + (radius - hub - 1.5).max(0.) * f;
                let x = distance * (angle - 0.25 * f).cos();
                let point = (
                    cx + if mirror { -x } else { x },
                    cy + distance * (angle - 0.25 * f).sin(),
                );
                if let Some(from) = old {
                    grid.line(from, point, color);
                }
                old = Some(point);
            }
        }
        grid.flush(r, b, ascii);
    }
    if !readout {
        return;
    }
    let digits = if inline {
        number
    } else {
        format!("{:04}", fan.rpm)
    };
    if !inline && digits.len() == 4 {
        for (row, value) in [&digits[..2], &digits[2..]].into_iter().enumerate() {
            let label = Rect::new(r.x + label_x, r.y + label_y + row as u16, 2, 1);
            b.set_style(label, Style::new().bg(theme.panel));
            text(b, r, label_x, label_y + row as u16, value, theme.text);
        }
    } else {
        // Keep the number upright and centered, including unexpected longer readings.
        let label = if with_unit {
            format!("{digits} rpm")
        } else {
            digits
        };
        let width = label.len() as u16;
        let centered = r.width.saturating_sub(width) / 2;
        // Keep the left fan's text offset within the hub on both sides. Mirroring
        // the text's rounding separately shifts odd-width labels inside the circle.
        let x = (label_x as i32 + centered as i32 - hub_left as i32)
            .clamp(0, r.width.saturating_sub(width) as i32) as u16;
        text(b, r, x, label_y, label, theme.text);
    }
}

fn halves(r: Rect) -> [Rect; 2] {
    let gap = if r.width >= 20 { 3 } else { 1 }.min(r.width);
    let width = r.width.saturating_sub(gap);
    let left = width / 2;
    [
        Rect::new(r.x, r.y, left, r.height),
        Rect::new(r.x + left + gap, r.y, width - left, r.height),
    ]
}
fn aligned(b: &mut Buffer, r: Rect, y: u16, value: impl AsRef<str>, color: Color, right: bool) {
    let value = truncate(value.as_ref(), r.width as usize);
    let x = if right {
        r.width.saturating_sub(value.chars().count() as u16)
    } else {
        0
    };
    text(b, r, x, y, value, color);
}

pub fn summary(b: &mut Buffer, r: Rect, s: &Sample, a: &App) {
    summary_layout(b, r, s, a, false);
}

fn summary_layout(b: &mut Buffer, r: Rect, s: &Sample, a: &App, pair: bool) {
    if r.is_empty() {
        return;
    }
    let t = &a.theme;
    let art = row(r, 0, r.height.saturating_sub(3));
    let panes = if pair {
        halves(art)
    } else {
        [Rect::default(), art]
    };
    for (i, art) in panes.into_iter().enumerate() {
        let Some(fan) = s.fans.value().and_then(|fans| fans.get(i)) else {
            continue;
        };
        let height = art.height.min(art.width / 2);
        let width = height.saturating_mul(2);
        let bounds = Rect::new(
            art.x + (art.width - width) / 2,
            art.y + (art.height - height) / 2,
            width,
            height,
        );
        let phase = if a.motion == Motion::Off {
            0.
        } else {
            a.anim.phases.get(&fan.name).copied().unwrap_or(0.)
        };
        draw_impeller(
            b,
            inner(bounds, 1, 1),
            phase,
            fan,
            t,
            ImpellerStyle {
                mirror: i == 1,
                ascii: a.cfg.ui.glyphs == "ascii",
                readout: false,
            },
        );
    }
    for (i, pane) in halves(r).into_iter().enumerate() {
        let fan = s.fans.value().and_then(|fans| fans.get(i));
        let speed = fan.map_or_else(
            || {
                if s.fans.value().is_some() {
                    "Not detected".into()
                } else {
                    s.fans.label().into()
                }
            },
            |f| format!("{} rpm", f.rpm),
        );
        aligned(
            b,
            pane,
            pane.height.saturating_sub(2),
            format!("{} {speed}", ["LEFT", "RIGHT"][i]),
            t.text,
            i == 1,
        );
        if pane.height >= 2 {
            let maximum = fan
                .and_then(|f| f.max)
                .map_or("Max N/A".into(), |max| format!("Max {max} rpm"));
            aligned(b, pane, pane.height - 1, maximum, t.muted, i == 1);
        }
    }
}

pub fn inspector(b: &mut Buffer, r: Rect, a: &App, zoom: bool) {
    Clear.render(r, b);
    let t = &a.theme;
    let inside = frame(
        b,
        r,
        "FANS",
        if zoom {
            "ZOOM · Esc back"
        } else {
            "INSPECTOR · Esc back"
        },
        true,
        t,
    );
    let s = a.sample();
    let history_h = if inside.height >= 22 {
        6
    } else if inside.height >= 14 {
        4
    } else {
        0
    };
    let note_h = u16::from(inside.height >= 18) * 2;
    let gap = u16::from(history_h > 0);
    let body_h = inside.height.saturating_sub(history_h + note_h + gap);
    summary_layout(b, row(inside, 0, body_h), &s, a, true);
    if history_h > 0 {
        let history_area = Rect::new(inside.x, inside.y + body_h + gap, inside.width, history_h);
        for (i, pane) in halves(history_area).into_iter().enumerate() {
            aligned(
                b,
                pane,
                0,
                format!(
                    "{} · {} history",
                    ["LEFT", "RIGHT"][i],
                    ["1m", "5m", "15m", "1h"][a.window]
                ),
                t.muted,
                i == 1,
            );
            let history: Vec<_> = a
                .history
                .iter()
                .filter(|sample| {
                    sample.at <= s.at && sample.at >= s.at - crate::app::WINDOWS[a.window] as f64
                })
                .filter_map(|sample| {
                    sample
                        .fans
                        .value()
                        .and_then(|fans| fans.get(i))
                        .map(|fan| fan.rpm as f32)
                })
                .collect();
            let graph = row(pane, 1, pane.height.saturating_sub(1));
            if history.is_empty() {
                aligned(b, graph, 0, "No recorded readings", t.muted, i == 1);
            } else {
                spark(
                    b,
                    graph,
                    &history,
                    history.iter().copied().fold(1., f32::max),
                    t.accent,
                    t,
                    a.cfg.ui.glyphs == "ascii",
                );
            }
        }
    }
    if note_h > 0 {
        line(
            b,
            inside,
            inside.height - 1,
            truncate(Part::Fan0.note(), inside.width as usize),
            t.muted,
        );
    }
}
