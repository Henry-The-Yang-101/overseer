use super::{
    SizeClass,
    kit::*,
    ports,
    theme::{Theme, blend},
    widgets,
};
use crate::{
    app::{App, Part},
    metrics::ports::PortKind,
};
use ratatui::{
    buffer::Buffer,
    layout::{Alignment, Rect},
    style::{Color, Style},
    symbols::border,
    text::Line,
    widgets::{Block, BorderType, Clear, Widget},
};
use unicode_width::UnicodeWidthStr;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Provenance {
    Photo,
    Approximate,
    Conceptual,
}
#[derive(Clone, Copy, Debug)]
pub struct NRect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}
impl NRect {
    pub const fn new(x: f32, y: f32, w: f32, h: f32) -> Self {
        Self { x, y, w, h }
    }
}
#[derive(Clone, Copy)]
pub struct Component {
    pub part: Part,
    pub label: &'static str,
    pub rect: NRect,
    pub provenance: Provenance,
}
pub const COMPONENTS: &[Component] = &[
    Component {
        part: Part::Network,
        label: "NETWORK",
        rect: NRect::new(0.35, 0.02, 0.30, 0.10),
        provenance: Provenance::Photo,
    },
    Component {
        part: Part::Fan0,
        label: "FAN 0",
        rect: NRect::new(0.065, 0.12, 0.225, 0.34),
        provenance: Provenance::Photo,
    },
    Component {
        part: Part::Fan1,
        label: "FAN 1",
        rect: NRect::new(0.710, 0.12, 0.225, 0.34),
        provenance: Provenance::Photo,
    },
    Component {
        part: Part::Soc,
        label: "M4 Pro",
        rect: NRect::new(0.42, 0.20, 0.16, 0.28),
        provenance: Provenance::Photo,
    },
    Component {
        part: Part::Memory,
        label: "RAM",
        rect: NRect::new(0.30, 0.22, 0.09, 0.21),
        provenance: Provenance::Conceptual,
    },
    Component {
        part: Part::Memory,
        label: "RAM",
        rect: NRect::new(0.61, 0.22, 0.09, 0.21),
        provenance: Provenance::Conceptual,
    },
    Component {
        part: Part::Power,
        label: "PMU",
        rect: NRect::new(0.27, 0.44, 0.14, 0.12),
        provenance: Provenance::Conceptual,
    },
    Component {
        part: Part::Disk,
        label: "SSD",
        rect: NRect::new(0.59, 0.44, 0.14, 0.12),
        provenance: Provenance::Approximate,
    },
    Component {
        part: Part::Battery,
        label: "BMS",
        rect: NRect::new(0.46, 0.63, 0.08, 0.30),
        provenance: Provenance::Photo,
    },
];
pub const CELLS: [NRect; 6] = [
    NRect::new(0.09, 0.60, 0.18, 0.15),
    NRect::new(0.09, 0.78, 0.18, 0.16),
    NRect::new(0.29, 0.60, 0.16, 0.34),
    NRect::new(0.55, 0.60, 0.16, 0.34),
    NRect::new(0.73, 0.60, 0.18, 0.15),
    NRect::new(0.73, 0.78, 0.18, 0.16),
];
pub fn fit_chassis(stage: Rect, aspect: f32, cell_aspect: f32) -> Rect {
    let aspect = aspect.max(0.1) * cell_aspect.max(0.1);
    let h = (stage.width as f32 / aspect)
        .min(stage.height as f32)
        .floor() as u16;
    let mut w = (h as f32 * aspect).floor().min(stage.width as f32) as u16;
    if w > 0 && w.is_multiple_of(2) {
        w -= 1;
    }
    Rect::new(
        stage.x + (stage.width - w) / 2,
        stage.y + (stage.height - h) / 2,
        w,
        h,
    )
}
// Round one canonical half, then reflect integer cells. Paired rectangles cannot drift.
pub fn to_cells(n: NRect, c: Rect, mirror: bool) -> Rect {
    let center = n.x + n.w / 2.;
    let middle = (center - 0.5).abs() < 0.0001;
    let mut w = (n.w * c.width as f32).round() as u16;
    if middle && w.is_multiple_of(2) {
        w += 1;
    }
    w = w.min(c.width);
    let canonical_x = if center > 0.5 { 1. - n.x - n.w } else { n.x };
    let left = grid_x(canonical_x, c.width).min(c.width.saturating_sub(w));
    let x = if middle {
        (c.width - w) / 2
    } else if (center > 0.5) != mirror {
        c.width - left - w
    } else {
        left
    };
    let top = (n.y * c.height as f32).round().clamp(0., c.height as f32) as u16;
    let bottom = ((n.y + n.h) * c.height as f32)
        .round()
        .clamp(0., c.height as f32) as u16;
    Rect::new(c.x + x, c.y + top, w, bottom.saturating_sub(top))
}
fn grid_x(x: f32, width: u16) -> u16 {
    let span = width.saturating_sub(1);
    let left = x.min(1. - x).clamp(0., 0.5);
    // Profile coordinates are decimal fractions; remove float noise before half-cell rounding.
    let left = (left * 10_000.).round() / 10_000.;
    let col = (left * span as f32).round() as u16;
    if x > 0.5 { span - col } else { col }
}
struct Layout {
    components: [Rect; 9],
    cells: [Rect; 6],
    data_y: u16,
    power_y: u16,
    battery_top: u16,
}
impl Layout {
    fn new(c: Rect, mirror: bool) -> Self {
        let mut components: [Rect; 9] =
            std::array::from_fn(|i| to_cells(COMPONENTS[i].rect, c, mirror));
        // Reserve a clear trunk and a short lead after even the longest port label.
        for i in [1, 2] {
            let r = &mut components[i];
            let left = r.x < c.x + c.width / 2;
            let inset = if left {
                (c.x + 7).saturating_sub(r.x)
            } else {
                r.right().saturating_sub(c.right() - 7)
            };
            if left {
                r.x += inset;
            }
            r.width = r.width.saturating_sub(inset);
        }
        components[0].y = c.y + 1;
        components[0].height = components[0].height.max(3);
        components[3].y = components[3].y.max(components[0].bottom() + 1);
        for i in [6, 7] {
            components[i].height = components[i].height.max(4);
        }
        // Leave a row of air between the fans/RAM and the lower board modules.
        let lower_top = components[6].y;
        for i in [1, 2, 4, 5] {
            components[i].height = components[i]
                .height
                .min(lower_top.saturating_sub(components[i].y + u16::from(i == 4 || i == 5)));
        }
        let data_y = components[3]
            .bottom()
            .max(components[6].bottom())
            .max(components[7].bottom());
        // At the shortest physical layout, share a trunk so the six battery outlines
        // still have useful interiors. Wider layouts give power its own lane.
        let power_y = data_y + u16::from(c.height >= 28);
        let battery_top = power_y + 1;
        let battery_h = c.bottom().saturating_sub(battery_top + 1);
        components[8].y = battery_top + 1;
        components[8].height = battery_h.saturating_sub(1);
        let cells = std::array::from_fn(|i| {
            let mut r = to_cells(CELLS[i], c, mirror);
            let upper_h = battery_h.saturating_sub(1) / 2;
            r.y = if i == 1 || i == 5 {
                battery_top + upper_h + 1
            } else {
                battery_top
            };
            r.height = if i == 2 || i == 3 {
                battery_h
            } else if i == 1 || i == 5 {
                battery_h.saturating_sub(upper_h + 1)
            } else {
                upper_h
            };
            r
        });
        Self {
            components,
            cells,
            data_y,
            power_y,
            battery_top,
        }
    }
    fn part(&self, p: Part) -> Rect {
        self.components[COMPONENTS.iter().position(|s| s.part == p).unwrap()]
    }
}
fn alignment(r: Rect, chassis: Rect) -> Alignment {
    let center = chassis.x + chassis.width / 2;
    if r.x <= center && r.right() > center {
        Alignment::Center
    } else if r.x > center {
        Alignment::Right
    } else {
        Alignment::Left
    }
}
fn center_x(r: Rect, chassis: Rect) -> u16 {
    // Even-width side modules attach on the inward center cell of each mirrored pair.
    r.x + if alignment(r, chassis) == Alignment::Right {
        r.width.saturating_sub(1) / 2
    } else {
        r.width / 2
    }
}
fn side_text(b: &mut Buffer, r: Rect, chassis: Rect, y: u16, value: impl AsRef<str>, color: Color) {
    let value = truncate(value.as_ref(), r.width as usize);
    let spare = r.width.saturating_sub(value.width() as u16);
    let x = match alignment(r, chassis) {
        Alignment::Center => spare / 2,
        Alignment::Right => spare,
        _ => 0,
    };
    text(b, r, x, y, value, color);
}
const DASHED: border::Set = border::Set {
    top_left: "┌",
    top_right: "┐",
    bottom_left: "└",
    bottom_right: "┘",
    vertical_left: "┆",
    vertical_right: "┆",
    horizontal_top: "┄",
    horizontal_bottom: "┄",
};
const DOTTED: border::Set = border::Set {
    top_left: "┌",
    top_right: "┐",
    bottom_left: "└",
    bottom_right: "┘",
    vertical_left: "┊",
    vertical_right: "┊",
    horizontal_top: "┈",
    horizontal_bottom: "┈",
};
fn outline(
    b: &mut Buffer,
    r: Rect,
    label: &str,
    p: Provenance,
    color: ratatui::style::Color,
    t: &Theme,
    align: Alignment,
) -> Rect {
    if r.width < 3 || r.height < 2 {
        line(b, r, 0, label, color);
        return r;
    }
    let set = match p {
        Provenance::Photo => border::ROUNDED,
        Provenance::Approximate => DASHED,
        Provenance::Conceptual => DOTTED,
    };
    let block = Block::bordered()
        .border_set(set)
        .border_style(Style::new().fg(color))
        .style(Style::new().bg(t.panel))
        .title_top(
            Line::from(if r.width as usize >= label.len() + 4 {
                format!(" {label} ")
            } else {
                truncate(label, r.width.saturating_sub(2) as usize)
            })
            .alignment(align),
        );
    let inside = block.inner(r);
    Clear.render(r, b);
    block.render(r, b);
    inside
}
pub fn draw(b: &mut Buffer, r: Rect, a: &mut App, size: SizeClass) {
    let t = a.theme.clone();
    let sample = a.sample();
    let verified = sample.machine.model == "Mac16,8";
    let sidebar = if size == SizeClass::Wide { 36 } else { 0 };
    let content = Rect::new(
        r.x,
        r.y,
        r.width
            .saturating_sub(sidebar + if sidebar > 0 { 1 } else { 0 }),
        r.height,
    );
    let title = if verified {
        "MACBOOK PRO 14 / M4 PRO"
    } else {
        "GENERIC SCHEMATIC / NOT A PHYSICAL LAYOUT"
    };
    let inside = frame(
        b,
        content,
        title,
        if a.cells {
            "TELEMETRY [m]"
        } else {
            "PHYSICAL [m]"
        },
        false,
        &t,
    );
    let orientation = if a.cfg.teardown.orientation != "back" {
        "FRONT ORIENTATION · hinge ↑ · MagSafe ← · HDMI/SD →"
    } else {
        "BACK ORIENTATION · hinge ↑ · HDMI/SD ← · MagSafe →"
    };
    line(
        b,
        inside,
        0,
        if verified {
            orientation
        } else {
            "Schematic arrangement · hardware positions are not known"
        },
        t.muted,
    );
    line(b, inside, 1, ports::summary(&sample), t.muted);
    a.hitmap.add(row(inside, 1, 1), 3, Hit::Ports);
    let available = Rect::new(
        inside.x,
        inside.y + 2,
        inside.width,
        inside.height.saturating_sub(4),
    );
    let chassis = fit_chassis(available, 312.6 / 221.2, a.cell_aspect);
    if chassis.width < 64 || chassis.height < 22 {
        navigator(b, available, a);
    } else if !verified {
        generic(b, available, a);
    } else {
        physical(b, chassis, available, a);
    }
    let disclaimer = if a.cells {
        "CELLS ARE DISPLAY SLOTS · NOT PER-CELL BATTERY DATA"
    } else if verified {
        "Layout approximate · wiring symbolic · fan sides assumed"
    } else {
        "GENERIC SCHEMATIC · NOT THE PHYSICAL LAYOUT OF THIS MACHINE"
    };
    line(
        b,
        inside,
        inside.height.saturating_sub(1),
        disclaimer,
        t.muted,
    );
    if sidebar > 0 {
        let rail = Rect::new(content.right() + 1, r.y, sidebar, r.height);
        selected(b, rail, a);
    }
}
fn chassis_outline(b: &mut Buffer, c: Rect, color: Color, cell_aspect: f32, ascii: bool) {
    Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(Style::new().fg(color))
        .render(c, b);
    if ascii {
        return;
    }
    // A modest physical radius, adjusted for the terminal's taller character cells.
    let rx = (c.width as f32 * 0.03).round().clamp(2., 5.) as u16;
    let ry = (rx as f32 / cell_aspect.max(0.1)).round().max(1.) as u16;
    let w = (rx + 1).min(c.width / 2);
    let h = (ry + 1).min(c.height / 2);
    let right = (w * 2 - 1) as f32;
    let bottom = (h * 4 - 1) as f32;
    let mut corners: [BrailleGrid; 4] = std::array::from_fn(|_| BrailleGrid::new(w, h));
    for i in 0..=64 {
        let angle = i as f32 / 64. * std::f32::consts::FRAC_PI_2;
        let x = (right - (right - 1.) * angle.cos()).round() as i32;
        let y = (bottom - (bottom - 2.) * angle.sin()).round() as i32;
        corners[0].set(x, y, color);
        corners[1].set(right as i32 - x, y, color);
        corners[2].set(x, bottom as i32 - y, color);
        corners[3].set(right as i32 - x, bottom as i32 - y, color);
    }
    for (grid, (x, y)) in corners.iter().zip([
        (c.x, c.y),
        (c.right() - w, c.y),
        (c.x, c.bottom() - h),
        (c.right() - w, c.bottom() - h),
    ]) {
        let corner = Rect::new(x, y, w, h);
        for y in corner.y..corner.bottom() {
            for x in corner.x..corner.right() {
                b[(x, y)].set_symbol(" ");
            }
        }
        grid.flush(corner, b, false);
    }
}
fn physical(b: &mut Buffer, c: Rect, available: Rect, a: &mut App) {
    let t = a.theme.clone();
    let s = a.sample();
    let mirror = a.cfg.teardown.orientation != "back";
    let ascii = a.cfg.ui.glyphs == "ascii";
    let layout = Layout::new(c, mirror);
    chassis_outline(b, c, t.wire, a.cell_aspect, ascii);
    let heat = s
        .cpu_temp
        .value()
        .copied()
        .unwrap_or(30.)
        .max(s.gpu_temp.value().copied().unwrap_or(30.));
    let heat_color = t.heat(heat);
    let chip = layout.part(Part::Soc);
    let center = c.x + c.width / 2;
    let bridge_y = chip.y.saturating_sub(1);
    for fan in [layout.part(Part::Fan0), layout.part(Part::Fan1)] {
        let fx = center_x(fan, c);
        let fin_top = c.y + 1;
        let fin_h = fan.y.saturating_sub(fin_top).max(1);
        let fin = Rect::new(fan.x + 1, fin_top, fan.width.saturating_sub(2), fin_h);
        for y in 0..fin.height {
            line(b, fin, y, "║".repeat(fin.width as usize), heat_color);
        }
        // Fins terminate directly at the fan frame, with a solid neck into the heat pipe.
        for x in fx.saturating_sub(1)..=fx + 1 {
            for y in fin_top..=bridge_y {
                text(b, c, x - c.x, y - c.y, "┃", heat_color);
            }
        }
        for x in fx.min(center)..=fx.max(center) {
            for y in bridge_y.saturating_sub(1)..=bridge_y {
                text(b, c, x - c.x, y - c.y, "═", heat_color);
            }
        }
    }
    let fan0 = layout.part(Part::Fan0);
    let fan1 = layout.part(Part::Fan1);
    let pipe_left = fan0.x.min(fan1.x);
    let pipe = Rect::new(
        pipe_left,
        bridge_y.saturating_sub(1),
        fan0.right().max(fan1.right()) - pipe_left,
        2,
    );
    a.parts.push((Part::Thermal, pipe));
    a.hitmap.add(pipe, 1, Hit::Part(Part::Thermal));
    wiring(b, c, a, mirror, &layout);
    for (spec, r) in COMPONENTS.iter().zip(layout.components) {
        let p = spec.part;
        let selected = a.selection == Some(p);
        let activity = crate::app::metric(p, &s);
        let color = if selected {
            t.accent
        } else if a.cfg.teardown.highlight_active && activity > 0.02 {
            widgets::tint(p, &t)
        } else {
            t.wire
        };
        let label = if a.cfg.teardown.labels {
            spec.label
        } else {
            ""
        };
        let inside = outline(b, r, label, spec.provenance, color, &t, alignment(r, c));
        a.parts.push((p, r));
        a.hitmap.add(r, 3, Hit::Part(p));
        match p {
            Part::Fan0 | Part::Fan1 => {
                if let Some(fan) = s
                    .fans
                    .value()
                    .and_then(|f| f.get(if p == Part::Fan0 { 0 } else { 1 }))
                {
                    let rotor_area = Rect::new(
                        inside.x,
                        inside.y,
                        inside.width,
                        inside.height.saturating_sub(1),
                    );
                    let phase = if a.motion != crate::motion::Motion::Off {
                        a.anim.phases.get(&fan.name).copied().unwrap_or(0.)
                    } else {
                        0.
                    };
                    rotor_mirrored(
                        b,
                        rotor_area,
                        phase,
                        if fan.rpm > 0 { t.accent } else { t.wire },
                        ascii,
                        alignment(r, c) == Alignment::Right,
                    );
                    side_text(
                        b,
                        inside,
                        c,
                        inside.height.saturating_sub(1),
                        format!("{} rpm", fan.rpm),
                        t.text,
                    );
                } else {
                    side_text(b, inside, c, 0, s.fans.label(), t.muted);
                }
            }
            Part::Soc => {
                side_text(b, inside, c, 0, format!("C {:.0}%", s.cpu * 100.), t.cpu);
                side_text(
                    b,
                    inside,
                    c,
                    1,
                    s.gpu
                        .value()
                        .map_or("G N/A".into(), |g| format!("G {:.0}%", g * 100.)),
                    t.gpu,
                );
                side_text(b, inside, c, 2, temp(&s.cpu_temp, &a.cfg.units), heat_color);
                let phase = if a.motion == crate::motion::Motion::Full {
                    a.anim.phases.get("ambient").copied().unwrap_or(0.)
                } else {
                    0.
                };
                let center = inside.width / 2;
                for y in (4..inside.height).step_by(2) {
                    for distance in (0..=center.saturating_sub(1)).step_by(3) {
                        let lit = ((distance + y * 3) as f32 * 0.07 - phase).rem_euclid(1.) < 0.1;
                        for x in [center - distance, center + distance] {
                            text(
                                b,
                                inside,
                                x,
                                y,
                                "·",
                                if lit {
                                    blend(t.cpu, t.panel, 0.65)
                                } else {
                                    t.substrate
                                },
                            );
                        }
                    }
                }
            }
            Part::Memory => {
                for y in 0..inside.height.saturating_sub(1) {
                    meter_mirrored(
                        b,
                        row(inside, y, 1),
                        s.memory_ratio(),
                        t.memory,
                        &t,
                        ascii,
                        alignment(r, c) == Alignment::Right,
                    );
                }
                side_text(
                    b,
                    inside,
                    c,
                    inside.height.saturating_sub(1),
                    if inside.width >= 5 {
                        format!("{:.1}G", s.memory_used as f64 / 1024f64.powi(3))
                    } else {
                        format!("{:.0}G", s.memory_used as f64 / 1024f64.powi(3))
                    },
                    t.text,
                );
            }
            Part::Power => {
                side_text(
                    b,
                    inside,
                    c,
                    0,
                    s.total_power()
                        .value()
                        .map_or("N/A".into(), |w| format!("Σ {w:.1}W")),
                    t.accent,
                );
                side_text(
                    b,
                    inside,
                    c,
                    1,
                    s.system_power
                        .value()
                        .map_or("SYS N/A".into(), |w| format!("SYS {w:.0}W")),
                    t.muted,
                );
            }
            Part::Disk => {
                if let Some(flow) = s.disk.value() {
                    for (y, prefix, value) in [(0, "R", flow.read), (1, "W", flow.write)] {
                        let full = format!("{prefix} {}", rate(value, &a.cfg.units, false));
                        let value = if full.width() <= inside.width as usize {
                            full
                        } else {
                            format!("{prefix}{}", compact_rate(value, a, false))
                        };
                        side_text(b, inside, c, y, value, t.disk);
                    }
                } else {
                    side_text(b, inside, c, 0, s.disk.label(), t.muted);
                }
            }
            Part::Battery => {
                side_text(
                    b,
                    inside,
                    c,
                    0,
                    s.battery
                        .value()
                        .map_or("N/A".into(), |v| format!("{:.0}%", v.percent)),
                    t.battery,
                );
                for y in 2..inside.height {
                    let arrow = s.battery.value().map_or("│", |v| {
                        if v.charging {
                            "↓"
                        } else if v.ac {
                            "│"
                        } else {
                            "↑"
                        }
                    });
                    side_text(b, inside, c, y, arrow, t.battery);
                }
            }
            Part::Network => {
                if let Some(flow) = s.network.value() {
                    if inside.height >= 2 {
                        side_text(
                            b,
                            inside,
                            c,
                            0,
                            format!("↓ {}", rate(flow.read, &a.cfg.units, true)),
                            t.net,
                        );
                        side_text(
                            b,
                            inside,
                            c,
                            1,
                            format!("↑ {}", rate(flow.write, &a.cfg.units, true)),
                            t.net,
                        );
                    } else {
                        side_text(
                            b,
                            inside,
                            c,
                            0,
                            format!(
                                "↓{} ↑{}",
                                compact_rate(flow.read, a, true),
                                compact_rate(flow.write, a, true)
                            ),
                            t.net,
                        );
                    }
                } else {
                    side_text(b, inside, c, 0, s.network.label(), t.muted);
                }
            }
            _ => {}
        }
        if a.follow.is_some() && !matches!(p, Part::Soc | Part::Memory) {
            for y in r.y..r.bottom() {
                for x in r.x..r.right() {
                    let cell = &mut b[(x, y)];
                    cell.set_fg(super::theme::blend(cell.fg, t.void, 0.45));
                }
            }
        }
        if selected {
            reticle(b, r, t.bright);
        }
    }
    for (i, r) in layout.cells.into_iter().enumerate() {
        let selected = a.selection == Some(Part::Cell(i as u8));
        if a.cells {
            let p = a.resolve(Part::Cell(i as u8));
            let inside = outline(
                b,
                r,
                &format!("{} / {}", i + 1, p.name().to_uppercase()),
                Provenance::Photo,
                if selected {
                    t.accent
                } else {
                    widgets::tint(p, &t)
                },
                &t,
                alignment(r, c),
            );
            side_text(b, inside, c, 0, widgets::readout(p, &s, a), t.text);
            let hist = a.history_values(p);
            let max = hist.iter().copied().fold(1., f32::max);
            if inside.height >= 3 {
                spark(
                    b,
                    Rect::new(inside.x, inside.y + 1, inside.width, inside.height - 2),
                    &hist,
                    max,
                    widgets::tint(p, &t),
                    &t,
                    ascii,
                );
            }
            if inside.height >= 2 {
                side_text(
                    b,
                    inside,
                    c,
                    inside.height - 1,
                    match p {
                        Part::Cpu => format!("{} tiers · {} cores", s.tiers.len(), s.core_count()),
                        Part::Memory => format!(
                            "PRESSURE {}",
                            s.pressure.value().map_or("N/A", String::as_str)
                        ),
                        Part::Power => format!("SYS {}", watts(&s.system_power)),
                        _ => "60s history".into(),
                    },
                    t.muted,
                );
            }
            a.parts.push((Part::Cell(i as u8), r));
            a.hitmap.add(r, 3, Hit::Part(Part::Cell(i as u8)));
        } else {
            let inside = outline(
                b,
                r,
                if a.cfg.teardown.labels {
                    ["CELL 1", "CELL 2", "CELL 3", "CELL 4", "CELL 5", "CELL 6"][i]
                } else {
                    ""
                },
                Provenance::Photo,
                t.wire,
                &t,
                alignment(r, c),
            );
            side_text(b, inside, c, 0, "▄▄", t.muted);
            if inside.height > 3 {
                side_text(b, inside, c, inside.height / 2, "BATTERY", t.wire);
            }
            a.hitmap.add(r, 2, Hit::Part(Part::Battery));
        }
        if selected {
            reticle(b, r, t.bright);
        }
    }
    for (n, label) in [
        (NRect::new(0.02, 0.62, 0.035, 0.30), "SPK"),
        (NRect::new(0.945, 0.62, 0.035, 0.30), "SPK"),
    ] {
        let mut r = to_cells(n, c, mirror);
        let left = alignment(r, c) == Alignment::Left;
        // Keep the largest vibration frame inside the space beside the batteries.
        if left {
            let edge = layout.cells.iter().map(|cell| cell.x).min().unwrap();
            r.x = r.x.min(edge.saturating_sub(5)).max(c.x + 1);
            r.width = (edge - r.x).min(7);
        } else {
            let edge = layout.cells.iter().map(|cell| cell.right()).max().unwrap();
            let right = r.right().max(edge + 5).min(c.right() - 1);
            r.width = (right - edge).min(7);
            r.x = right - r.width;
        }
        let space = c.bottom().saturating_sub(layout.battery_top + 2);
        r.height = ((space / 3).max(5) + 2).min(space.saturating_sub(1));
        let graph_height = space;
        let graph_y = layout.battery_top + 1;
        r.y = graph_y + (graph_height - r.height) / 2;
        let p = if n.x > 0.5 {
            Part::SpeakerLeft
        } else {
            Part::SpeakerRight
        };
        a.parts.push((p, r));
        a.hitmap.add(r, 4, Hit::Part(p));
        let level = if a.motion == crate::motion::Motion::Full && a.cursor.is_none() {
            a.anim.speakers[usize::from(p == Part::SpeakerRight)]
                .pos
                .clamp(0., 1.)
        } else {
            0.
        };
        let width = 5.min(r.width) + (level * r.width.saturating_sub(5) as f32).round() as u16;
        let height = 5.min(r.height) + (level * r.height.saturating_sub(5) as f32).round() as u16;
        let spare = r.width - width;
        let pill = Rect::new(
            r.x + if left { spare / 2 } else { spare.div_ceil(2) },
            r.y + (r.height - height) / 2,
            width,
            height,
        );
        Block::bordered()
            .border_type(BorderType::Rounded)
            .border_style(Style::new().fg(t.wire))
            .render(pill, b);
        if pill.width >= 5 && pill.height >= 5 {
            Block::bordered()
                .border_type(BorderType::Rounded)
                .border_style(Style::new().fg(t.muted))
                .render(
                    Rect::new(pill.x + 1, pill.y + 1, pill.width - 2, pill.height - 2),
                    b,
                );
        }
        let graph = if left {
            Rect::new(
                available.x,
                graph_y,
                c.x.saturating_sub(available.x + 1),
                graph_height,
            )
        } else {
            Rect::new(
                c.right() + 1,
                graph_y,
                available.right().saturating_sub(c.right() + 1),
                graph_height,
            )
        };
        super::audio::bars(b, graph, p, a, left);
        text(
            b,
            available,
            r.x - available.x + (r.width - 3) / 2,
            r.bottom() - available.y,
            label,
            t.muted,
        );
        if a.selection == Some(p) {
            reticle(b, r, t.bright);
        }
    }
    edge_ports(b, c, a, mirror);
    device_labels(b, c, available, a, mirror);
}
fn compact_rate(value: f64, a: &App, network: bool) -> String {
    let bits = network && a.cfg.units.net == "bits";
    let base = if bits || a.cfg.units.bytes == "si" {
        1000.
    } else {
        1024.
    };
    let mut n = value.max(0.) * if bits { 8. } else { 1. };
    let mut unit = 0;
    while n >= base - 0.5 && unit < 4 {
        n /= base;
        unit += 1;
    }
    let prefix = ["", "K", "M", "G", "T"][unit];
    format!(
        "{n:.0}{prefix}{}/s",
        if bits {
            "b"
        } else if unit == 0 {
            "B"
        } else {
            ""
        }
    )
}
fn generic(b: &mut Buffer, r: Rect, a: &mut App) {
    let t = a.theme.clone();
    let s = a.sample();
    let parts: Vec<_> = [
        Part::Soc,
        Part::Memory,
        Part::Power,
        Part::Disk,
        Part::Network,
        Part::Thermal,
    ]
    .into_iter()
    .chain(if s.battery.value().is_some() {
        Some(Part::Battery)
    } else {
        None
    })
    .collect();
    let cols = 3;
    let rows = (parts.len() as u16).div_ceil(cols);
    let w = r.width / cols;
    let h = r.height / rows;
    for (i, p) in parts.into_iter().enumerate() {
        widgets::panel(
            b,
            Rect::new(
                r.x + i as u16 % cols * w,
                r.y + i as u16 / cols * h,
                w.saturating_sub(1),
                h.saturating_sub(1),
            ),
            p,
            a,
        );
    }
    line(
        b,
        r,
        r.height.saturating_sub(1),
        "Telemetry modules · schematic arrangement",
        t.muted,
    );
}
fn navigator(b: &mut Buffer, r: Rect, a: &mut App) {
    let t = a.theme.clone();
    let s = a.sample();
    let left = r.width * 45 / 100;
    let order = a.part_order();
    let rows = r.height.saturating_sub(2) as usize;
    line(b, r, 0, "COMPONENT NAVIGATOR", t.accent);
    for (i, p) in order.iter().take(rows).enumerate() {
        let selected = a.selection == Some(*p);
        let rr = Rect::new(r.x, r.y + 2 + i as u16, left, 1);
        line(
            b,
            rr,
            0,
            format!("{} {}", if selected { "›" } else { " " }, p.name()),
            if selected { t.accent } else { t.muted },
        );
        a.parts.push((*p, rr));
        a.hitmap.add(rr, 2, Hit::Part(*p));
    }
    let p = a.resolve(a.selection.unwrap_or(Part::Soc));
    let right = Rect::new(
        r.x + left + 1,
        r.y + 1,
        r.width - left - 1,
        r.height.saturating_sub(1),
    );
    let inside = frame(b, right, &p.name().to_uppercase(), "", true, &t);
    line(b, inside, 0, widgets::readout(p, &s, a), t.text);
    if inside.height > 3 {
        let inner = Rect::new(inside.x, inside.y + 2, inside.width, inside.height - 2);
        widgets::content(b, inner, p, &s, a);
    }
}
fn selected(b: &mut Buffer, r: Rect, a: &mut App) {
    let t = &a.theme;
    let p = a.resolve(a.selection.unwrap_or(Part::Soc));
    let s = a.sample();
    let card = Rect::new(r.x, r.y, r.width, (r.height * 43 / 100).max(8));
    let inner = frame(b, card, "SELECTED", "Enter / z", true, t);
    line(b, inner, 1, p.name().to_uppercase(), widgets::tint(p, t));
    line(b, inner, 3, widgets::readout(p, &s, a), t.text);
    let h = (inner.height / 4).max(1);
    let values = a.history_values(p);
    spark(
        b,
        Rect::new(
            inner.x,
            inner.y + 5,
            inner.width,
            h.min(inner.height.saturating_sub(5)),
        ),
        &values,
        values.iter().copied().fold(1., f32::max),
        widgets::tint(p, t),
        t,
        a.cfg.ui.glyphs == "ascii",
    );
    let text_rect = Rect::new(
        inner.x,
        inner.y + (6 + h).min(inner.height),
        inner.width,
        inner.height.saturating_sub(6 + h),
    );
    ratatui::widgets::Paragraph::new(p.note())
        .wrap(ratatui::widgets::Wrap { trim: true })
        .style(Style::new().fg(t.muted))
        .render(text_rect, b);
    let legend = Rect::new(r.x, card.bottom(), r.width, r.height - card.height);
    ports::card(b, legend, a);
    a.hitmap.add(legend, 3, Hit::Ports);
}
fn reticle(b: &mut Buffer, r: Rect, c: ratatui::style::Color) {
    if r.width < 2 || r.height < 2 {
        return;
    }
    for (x, y, s) in [
        (0, 0, "┏"),
        (r.width - 1, 0, "┓"),
        (0, r.height - 1, "┗"),
        (r.width - 1, r.height - 1, "┛"),
    ] {
        text(b, r, x, y, s, c);
    }
}

// Coordinates retain the underside basis; the default front orientation mirrors every layer.
const PORTS: [(PortKind, bool, f32, &str); 7] = [
    (PortKind::MagSafe, false, 0.15, "MAG"),
    (PortKind::UsbC(1), false, 0.25, "TB1"),
    (PortKind::UsbC(2), false, 0.35, "TB2"),
    (PortKind::Audio, false, 0.45, "3.5"),
    (PortKind::Hdmi, true, 0.18, "HDMI"),
    (PortKind::UsbC(3), true, 0.30, "TB3"),
    (PortKind::SdCard, true, 0.42, "SD"),
];
fn edge_ports(b: &mut Buffer, c: Rect, a: &mut App, mirror: bool) {
    let s = a.sample();
    let t = &a.theme;
    for (kind, left, y, label) in PORTS {
        let left = left != mirror;
        let (dot, color, _) = ports::indicator(&s, kind, t);
        let y = (y * c.height as f32).round() as u16;
        let x = if left { 0 } else { c.width.saturating_sub(1) };
        text(b, c, x, y, dot, color);
        let x = if left {
            1
        } else {
            c.width.saturating_sub(label.len() as u16 + 1)
        };
        text(b, c, x, y, label, color);
        a.hitmap.add(
            Rect::new(
                c.x + if left { 0 } else { x },
                c.y + y,
                label.len() as u16 + 1,
                1,
            ),
            6,
            Hit::Ports,
        );
    }
}
fn device_labels(b: &mut Buffer, c: Rect, area: Rect, a: &mut App, mirror: bool) {
    let s = a.sample();
    for left in [true, false] {
        let width = if left {
            c.x.saturating_sub(area.x + 2)
        } else {
            area.right().saturating_sub(c.right() + 2)
        };
        if width < 8 {
            continue;
        }
        let x = if left { area.x } else { c.right() + 2 };
        let mut next_y = area.y + 1;
        for (kind, side, y, _) in PORTS {
            if (side != mirror) != left || ports::indicator(&s, kind, &a.theme).2 != "CONNECTED" {
                continue;
            }
            let Some(port) = ports::find(&s, kind) else {
                continue;
            };
            let y = (c.y + (y * c.height as f32).round() as u16).max(next_y);
            if y >= area.bottom() {
                break;
            }
            let r = Rect::new(x, y, width, area.bottom() - y);
            let names = if port.devices.is_empty() {
                "Device name unavailable".into()
            } else {
                port.devices.join(" · ")
            };
            let title = format!("{} {}", if left { "→" } else { "←" }, kind.label());
            line(b, r, 0, title, a.theme.ok);
            let mut rows = vec![String::new()];
            for word in names.split_whitespace() {
                let last = rows.last_mut().unwrap();
                if !last.is_empty() && last.width() + word.width() + 1 > width as usize {
                    rows.push(String::new());
                }
                let last = rows.last_mut().unwrap();
                if !last.is_empty() {
                    last.push(' ');
                }
                last.push_str(word);
            }
            let count = (rows.len() as u16).min(5).min(r.height.saturating_sub(2)) + 1;
            for (i, value) in rows
                .iter()
                .take(count.saturating_sub(1) as usize)
                .enumerate()
            {
                line(
                    b,
                    r,
                    i as u16 + 1,
                    truncate(value, width as usize),
                    a.theme.text,
                );
            }
            if rows.len() >= count as usize {
                line(
                    b,
                    r,
                    count.saturating_sub(1),
                    "… p for details",
                    a.theme.muted,
                );
            }
            a.hitmap
                .add(Rect::new(r.x, r.y, r.width, count), 5, Hit::Ports);
            next_y = y + count + 1;
        }
    }
}
fn wiring(b: &mut Buffer, c: Rect, a: &App, mirror: bool, layout: &Layout) {
    let t = &a.theme;
    let s = a.sample();
    let phase = if a.motion == crate::motion::Motion::Full {
        a.anim.phases.get("ambient").copied().unwrap_or(0.)
    } else {
        0.
    };
    let moving = a.motion == crate::motion::Motion::Full;
    let wire = blend(t.wire, t.panel, 0.25);
    let signal = blend(t.accent, t.panel, 0.68);
    let center = c.x + c.width / 2;
    let soc = layout.part(Part::Soc);
    let network = layout.part(Part::Network);
    trace(
        b,
        c,
        &[(center, network.bottom() - 1), (center, soc.y)],
        wire,
        moving.then_some((phase, signal)),
    );
    for r in [layout.components[4], layout.components[5]] {
        let right = r.x > center;
        for (i, y) in [r.y + 1, r.bottom().saturating_sub(2)]
            .into_iter()
            .enumerate()
        {
            let (from, to) = if right {
                (r.x, soc.right() - 1)
            } else {
                (r.right() - 1, soc.x)
            };
            trace(
                b,
                c,
                &[(from, y), (to, y)],
                wire,
                moving.then_some((phase + i as f32 * 0.5, signal)),
            );
        }
    }
    for r in [layout.part(Part::Disk), layout.part(Part::Power)] {
        let right = r.x > center;
        let from = if right { r.x } else { r.right() - 1 };
        let to = if right { soc.right() - 1 } else { soc.x };
        let y = soc.bottom() - 2;
        trace(
            b,
            c,
            &[(from, r.y + 1), (to, r.y + 1), (to, y)],
            wire,
            moving.then_some((phase, signal)),
        );
    }
    let bms = layout.part(Part::Battery);
    for r in [
        layout.cells[0],
        layout.cells[2],
        layout.cells[3],
        layout.cells[4],
    ] {
        let x = center_x(r, c);
        trace(
            b,
            c,
            &[
                (x, r.y),
                (x, layout.power_y),
                (center, layout.power_y),
                (center, bms.y),
            ],
            wire,
            moving.then_some((phase, signal)),
        );
    }
    let pmu = layout.part(Part::Power);
    let pmu_x = center_x(pmu, c);
    trace(
        b,
        c,
        &[(pmu_x, pmu.bottom() - 1), (pmu_x, layout.power_y)],
        wire,
        moving.then_some((phase, signal)),
    );
    let power_input = crate::metrics::ports::power_input(&s);
    let mut power_path = None;
    for (i, (kind, left, y, label)) in PORTS.iter().enumerate() {
        let left = *left != mirror;
        let y = c.y + (y * c.height as f32).round() as u16;
        let lane = grid_x(0.03, c.width).max(5) + 1;
        let edge = if left {
            c.x + label.len() as u16 + 1
        } else {
            c.right() - label.len() as u16 - 2
        };
        let lane = if left {
            c.x + lane
        } else {
            c.right() - 1 - lane
        };
        let target = if left { center - 2 } else { center + 2 };
        let path = [
            (edge, y),
            (lane, y),
            (lane, layout.data_y),
            (target, layout.data_y),
            (target, soc.bottom() - 1),
        ];
        let connected = ports::indicator(&s, *kind, t).2 == "CONNECTED";
        trace(
            b,
            c,
            &path,
            blend(wire, t.panel, 0.15),
            (moving && connected).then_some((phase + i as f32 * 0.17, signal)),
        );
        if power_input.value() == Some(kind) {
            power_path = Some([
                (edge, y),
                (lane, y),
                (lane, layout.power_y),
                (center, layout.power_y),
                (center, bms.y),
            ]);
        }
    }
    // Paint the active input last so another port sharing the trunk cannot hide it.
    if let Some(path) = power_path {
        trace(
            b,
            c,
            &path,
            blend(t.battery, t.panel, 0.70),
            moving.then_some((phase, blend(t.battery, t.panel, 0.35))),
        );
    }
}
fn trace(
    b: &mut Buffer,
    c: Rect,
    points: &[(u16, u16)],
    color: Color,
    pulse: Option<(f32, Color)>,
) {
    let mut cells = Vec::new();
    for pair in points.windows(2) {
        let (mut x, mut y) = pair[0];
        let (tx, ty) = pair[1];
        cells.push((x, y));
        while (x, y) != (tx, ty) {
            if x != tx {
                x = (x as i32 + if x < tx { 1 } else { -1 }) as u16;
            } else {
                y = (y as i32 + if y < ty { 1 } else { -1 }) as u16;
            }
            cells.push((x, y));
        }
    }
    cells.dedup();
    for (i, &(x, y)) in cells.iter().enumerate() {
        let mut dirs = [false; 4]; // up, right, down, left
        for j in [i.checked_sub(1), (i + 1 < cells.len()).then_some(i + 1)]
            .into_iter()
            .flatten()
        {
            let (nx, ny) = cells[j];
            dirs[if ny < y {
                0
            } else if nx > x {
                1
            } else if ny > y {
                2
            } else {
                3
            }] = true;
        }
        let existing = match b[(x, y)].symbol() {
            "│" => [true, false, true, false],
            "─" => [false, true, false, true],
            "╭" => [false, true, true, false],
            "╮" => [false, false, true, true],
            "╰" => [true, true, false, false],
            "╯" => [true, false, false, true],
            "├" => [true, true, true, false],
            "┤" => [true, false, true, true],
            "┬" => [false, true, true, true],
            "┴" => [true, true, false, true],
            "┼" => [true; 4],
            _ => [false; 4],
        };
        for i in 0..4 {
            dirs[i] |= existing[i];
        }
        let glyph = match dirs {
            [true, true, true, true] => "┼",
            [true, true, true, false] => "├",
            [true, false, true, true] => "┤",
            [false, true, true, true] => "┬",
            [true, true, false, true] => "┴",
            [false, true, true, false] => "╭",
            [false, false, true, true] => "╮",
            [true, true, false, false] => "╰",
            [true, false, false, true] => "╯",
            [true, _, _, _] | [_, _, true, _] => "│",
            _ => "─",
        };
        text(b, c, x - c.x, y - c.y, glyph, color);
    }
    if let Some((phase, color)) = pulse
        && !cells.is_empty()
    {
        let i = (phase.fract() * cells.len() as f32) as usize % cells.len();
        text(b, c, cells[i].0 - c.x, cells[i].1 - c.y, "·", color);
    }
}
