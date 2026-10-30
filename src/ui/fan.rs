use super::{kit::*, theme::Theme};
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
