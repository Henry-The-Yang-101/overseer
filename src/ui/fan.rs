use super::{
    kit::*,
    theme::{Theme, blend},
};
use crate::metrics::Fan;
use ratatui::{
    buffer::Buffer,
    layout::{Alignment, Rect},
    style::{Color, Style},
    text::Line,
    widgets::{Block, BorderType, Clear, Widget},
};

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
            .border_type(BorderType::Rounded)
            .border_style(Style::new().fg(color))
            .title_top(Line::from(label).alignment(Alignment::Center));
        let inside = block.inner(r);
        block.render(r, b);
        return inside;
    }
    let mut grid = BrailleGrid::new(r.width, r.height);
    let point = |x: f32, y: f32| {
        (
            (if mirror { 1. - x } else { x }) * (r.width * 2 - 1) as f32,
            y * (r.height * 4 - 1) as f32,
        )
    };
    for (from, to) in [
        ((0.12, 0.), (0.98, 0.)),
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
    if r.width < 2 || r.height < 2 {
        return;
    }
    let left = (r.width - 2) / 2;
    let label_x = if mirror { r.width - left - 2 } else { left };
    let label_y = (r.height - 1) / 2;
    // Align the hub to the two-by-two readout, keeping text upright when mirrored.
    let cx = label_x as f32 * 2. + 1.5;
    let cy = label_y as f32 * 4. + 3.5;
    let radius = (cx
        .min(r.width as f32 * 2. - 1. - cx)
        .min(cy)
        .min(r.height as f32 * 4. - 1. - cy)
        - 1.)
        .max(1.);
    let color = if fan.rpm > 0 {
        theme.accent
    } else {
        theme.wire
    };
    let mut grid = BrailleGrid::new(r.width, r.height);
    if radius >= 5. {
        let hub = (radius * 0.46).max(5.5).min(radius * 0.75);
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
    let digits = format!("{:04}", fan.rpm);
    if digits.len() == 4 {
        for (row, value) in [&digits[..2], &digits[2..]].into_iter().enumerate() {
            let label = Rect::new(r.x + label_x, r.y + label_y + row as u16, 2, 1);
            b.set_style(label, Style::new().bg(theme.panel));
            text(b, r, label_x, label_y + row as u16, value, theme.text);
        }
    } else {
        // Preserve an unexpected larger reading rather than truncating telemetry.
        text(
            b,
            r,
            r.width.saturating_sub(digits.len() as u16) / 2,
            label_y,
            digits,
            theme.text,
        );
    }
}
