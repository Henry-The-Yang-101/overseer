mod model;
use super::{
    SizeClass,
    kit::*,
    theme::{Theme, blend},
};
use crate::{
    app::{App, Part},
    control::{ControlAction as C, Tab},
    motion::Motion,
    now_playing::State,
};
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Style},
};
use std::f32::consts::{PI, TAU};

fn phase(a: &App) -> f32 {
    if a.motion == Motion::Full {
        a.started.elapsed().as_secs_f32()
    } else {
        0.
    }
}
fn value<T>(v: Option<T>, f: impl FnOnce(T) -> String) -> String {
    v.map(f).unwrap_or_else(|| "—".into())
}
fn split(r: Rect, width: u16) -> (Rect, Rect) {
    let w = width.min(r.width);
    let gap = u16::from(w < r.width);
    (
        Rect::new(r.x, r.y, w, r.height),
        Rect::new(
            r.x + w + gap,
            r.y,
            r.width.saturating_sub(w + gap),
            r.height,
        ),
    )
}
fn button(b: &mut Buffer, r: Rect, x: u16, y: u16, label: &str, action: C, a: &mut App) {
    if x >= r.width || y >= r.height {
        return;
    }
    let width = (unicode_width::UnicodeWidthStr::width(label) as u16).min(r.width - x);
    text(b, r, x, y, label, a.theme.accent);
    a.hitmap.add(
        Rect::new(r.x + x, r.y + y, width, 1),
        15,
        Hit::Control(action),
    );
}
fn field(b: &mut Buffer, r: Rect, y: u16, label: &str, v: impl AsRef<str>, t: &Theme) {
    let lw = 12.min(r.width.saturating_sub(8));
    text(
        b,
        r,
        0,
        y,
        truncate(label, lw.saturating_sub(1) as usize),
        t.muted,
    );
    text(b, r, lw, y, v, t.text);
}
pub fn draw(b: &mut Buffer, r: Rect, a: &mut App, _size: SizeClass) {
    let mut x = 0;
    for tab in Tab::ALL {
        let label = format!(" {} ", tab.name());
        let w = label.len() as u16;
        if x + w <= r.width {
            let rr = Rect::new(r.x + x, r.y, w, 1);
            b.set_style(
                rr,
                Style::new()
                    .fg(if a.control.tab == tab {
                        a.theme.bright
                    } else {
                        a.theme.muted
                    })
                    .bg(if a.control.tab == tab {
                        a.theme.substrate
                    } else {
                        a.theme.void
                    }),
            );
            b.set_stringn(
                rr.x,
                rr.y,
                &label,
                w as usize,
                Style::new().fg(if a.control.tab == tab {
                    a.theme.bright
                } else {
                    a.theme.muted
                }),
            );
            a.hitmap.add(rr, 10, Hit::ControlTab(tab));
        }
        x += w + 1;
    }
    let body = row(r, 2, r.height.saturating_sub(2));
    match a.control.tab {
        Tab::Wifi => wifi(b, body, a),
        Tab::Bluetooth => bluetooth(b, body, a),
        Tab::Display => display(b, body, a),
        Tab::Sound => sound(b, body, a),
        Tab::NowPlaying => now_playing(b, body, a),
    }
}
pub fn status(b: &mut Buffer, r: Rect, a: &App) {
    let c = &a.control;
    let age = c.updated[c.tab.index()].map_or_else(
        || "acquiring".into(),
        |t| format!("sample {}s ago", t.elapsed().as_secs()),
    );
    let message = if let Some(message) = &c.message {
        message.clone()
    } else if let Some(error) = &c.errors[c.tab.index()] {
        format!("Unavailable · {error} · {age}")
    } else if a.demo.is_some() {
        "SYNTHETIC".into()
    } else {
        format!(
            "{} · {age}{}",
            if matches!(c.tab, Tab::Wifi | Tab::Bluetooth | Tab::Display) {
                "VIEW ONLY"
            } else {
                "LIVE"
            },
            if c.tab == Tab::Wifi {
                " · ICMP 1.1.1.1"
            } else {
                ""
            }
        )
    };
    line(
        b,
        r,
        0,
        message,
        if c.errors[c.tab.index()].is_some() {
            a.theme.warn
        } else {
            a.theme.muted
        },
    );
}
fn wifi(b: &mut Buffer, r: Rect, a: &mut App) {
    let t = a.theme.clone();
    let time = phase(a);
    let bottom_h = if r.height >= 22 {
        11
    } else if r.height >= 15 {
        7
    } else {
        5
    };
    let top = row(r, 0, r.height.saturating_sub(bottom_h));
    let bottom = row(r, top.height, bottom_h);
    let (signal, meta) = split(top, (r.width * 42 / 100).max(20));
    let v = frame(b, signal, "WI-FI / SIGNAL FIELD", "", false, &t);
    let c = &a.control;
    let w = &c.wifi;
    let strength = w
        .rssi
        .map(|v| ((v + 90) as f32 / 55.).clamp(0., 1.))
        .unwrap_or(0.);
    if v.height > 2 {
        let canvas = row(v, 0, v.height.saturating_sub(2));
        let mut grid = BrailleGrid::new(canvas.width, canvas.height);
        let cx = canvas.width as f32;
        let cy = canvas.height as f32 * 3.1;
        let radius = (canvas.width as f32 * 0.82).min(canvas.height as f32 * 3.8);
        for ring in 1..=5 {
            let rr = radius * ring as f32 / 5.;
            let pulse = ((time * 0.6 - ring as f32 / 5.).sin() * 0.5 + 0.5) * 0.3;
            let color = if w.connected && ring as f32 / 5. <= strength + 0.15 {
                blend(t.wire, t.accent, 0.55 + pulse)
            } else {
                t.substrate
            };
            let mut prev = None;
            for i in 0..=90 {
                let angle = PI * 1.12 + PI * 0.76 * i as f32 / 90.;
                let p = (cx + rr * angle.cos(), cy + rr * angle.sin());
                if let Some(old) = prev {
                    grid.line(old, p, color);
                }
                prev = Some(p);
            }
        }
        grid.flush(canvas, b, a.cfg.ui.glyphs == "ascii");
        text(
            b,
            canvas,
            canvas.width / 2,
            canvas.height.saturating_sub(1),
            "●",
            if w.connected { t.accent } else { t.muted },
        );
    }
    line(
        b,
        v,
        v.height.saturating_sub(2),
        value(w.rssi, |v| format!("{v} dBm  ·  {}", w.status)),
        t.bright,
    );
    line(
        b,
        v,
        v.height.saturating_sub(1),
        value(w.tx_mbps, |v| format!("{v:.0} Mb/s negotiated link")),
        t.muted,
    );
    let m = frame(b, meta, "CURRENT NETWORK", "", false, &t);
    let privacy = if w.connected {
        "SSID hidden by macOS"
    } else {
        "Not connected"
    };
    line(b, m, 0, w.ssid.as_deref().unwrap_or(privacy), t.bright);
    field(
        b,
        m,
        2,
        "Radio",
        format!(
            "{} · ch {} · {}",
            w.band.as_deref().unwrap_or("—"),
            value(w.channel, |v| v.to_string()),
            value(w.width, |v| format!("{v} MHz"))
        ),
        &t,
    );
    field(
        b,
        m,
        3,
        "Protocol",
        format!(
            "{} · {}",
            w.phy.as_deref().unwrap_or("—"),
            w.security.as_deref().unwrap_or("—")
        ),
        &t,
    );
    field(
        b,
        m,
        4,
        "Noise / SNR",
        format!(
            "{} / {}",
            value(w.noise, |v| format!("{v} dBm")),
            value(w.rssi.zip(w.noise), |(s, n)| format!("{} dB", s - n))
        ),
        &t,
    );
    field(b, m, 5, "Address", w.ipv4.join(", "), &t);
    field(b, m, 6, "Gateway", w.gateway.as_deref().unwrap_or("—"), &t);
    field(
        b,
        m,
        7,
        "BSSID",
        w.bssid.as_deref().unwrap_or("Hidden by macOS"),
        &t,
    );
    field(b, m, 8, "System DNS", w.dns.join(", "), &t);
    field(
        b,
        m,
        9,
        "Interface",
        format!(
            "{} · default {}",
            w.interface,
            w.primary_interface.as_deref().unwrap_or("—")
        ),
        &t,
    );
    field(
        b,
        m,
        10,
        "IPv6",
        w.ipv6.first().map(String::as_str).unwrap_or("—"),
        &t,
    );
    field(
        b,
        m,
        11,
        "Country",
        w.country.as_deref().unwrap_or("Hidden by macOS"),
        &t,
    );
    field(b, m, 12, "Adapter MAC", w.mac.as_deref().unwrap_or("—"), &t);
    let stats = c.latency.stats();
    let p = frame(
        b,
        bottom,
        "LATENCY / 1.1.1.1",
        "last 120 probes · ICMP",
        false,
        &t,
    );
    let statsline = format!(
        "RTT {}   jitter {}   loss {}   range {}–{} ms",
        value(stats.mean, |n| format!("{n:.1} ms")),
        value(stats.jitter, |n| format!("{n:.1} ms")),
        value(stats.loss, |n| format!("{n:.1}%")),
        value(stats.min, |n| format!("{n:.0}")),
        value(stats.max, |n| format!("{n:.0}"))
    );
    line(b, p, 0, statsline, t.text);
    let footer = u16::from(p.height >= 7);
    let graph = row(p, 1, p.height.saturating_sub(2 + footer));
    let mut grid = BrailleGrid::new(graph.width, graph.height);
    let max = stats.max.unwrap_or(50.).max(30.) as f32 * 1.15;
    let mut previous = None;
    let first = c.latency.samples.front().map_or(0., |p| p.at);
    let last = c.latency.samples.back().map_or(1., |p| p.at);
    for sample in &c.latency.samples {
        let x = ((sample.at - first) / (last - first).max(1.)) as f32
            * graph.width.saturating_sub(1) as f32
            * 2.;
        if let Some(ms) = sample.ms {
            let p = (
                x,
                (1. - ms as f32 / max).clamp(0., 1.) * (graph.height * 4).saturating_sub(1) as f32,
            );
            if let Some(old) = previous {
                grid.line(old, p, if sample.loaded { t.warn } else { t.accent });
            }
            previous = Some(p);
        } else {
            previous = None;
            text(
                b,
                graph,
                (x / 2.) as u16,
                graph.height.saturating_sub(1),
                "×",
                t.crit,
            );
        }
    }
    grid.flush(graph, b, a.cfg.ui.glyphs == "ascii");
    if c.latency.samples.is_empty() {
        line(
            b,
            graph,
            0,
            c.ping_error
                .as_deref()
                .unwrap_or("Waiting for ICMP replies…"),
            t.muted,
        );
    }
    let testline = if let Some(start) = c.speed_started {
        format!(
            "Testing · {}s · download + upload · s cancels",
            start.elapsed().as_secs()
        )
    } else if let Some(error) = &c.speed_error {
        error.clone()
    } else if let Some(result) = &c.speed {
        format!(
            "↓ {} Mb/s    ↑ {} Mb/s    idle {} ms    {} RPM",
            value(result.down, |v| format!("{v:.1}")),
            value(result.up, |v| format!("{v:.1}")),
            value(result.idle_ms, |v| format!("{v:.1}")),
            value(result.responsiveness, |v| format!("{v:.0}"))
        )
    } else {
        "[s speed test]".into()
    };
    let test_y = p.height.saturating_sub(1 + footer);
    line(b, p, test_y, testline, t.bright);
    if footer > 0 {
        line(
            b,
            p,
            p.height.saturating_sub(1),
            "12s · yellow: under load",
            t.muted,
        );
    }
    a.hitmap
        .add(row(p, test_y, 1), 10, Hit::Control(C::SpeedTest));
}
fn bluetooth(b: &mut Buffer, r: Rect, a: &mut App) {
    let t = a.theme.clone();
    let (side, stage) = split(r, if r.width >= 110 { 30 } else { 23 });
    let list = frame(b, side, "KNOWN DEVICES", "", false, &t);
    let count = a.control.bluetooth.devices.len();
    let stride = if list.height >= count as u16 * 2 + 2 {
        2
    } else {
        1
    };
    let visible = list.height.saturating_sub(2) as usize / stride as usize;
    let start = a
        .control
        .device_index
        .saturating_sub(visible.saturating_sub(1));
    for (i, d) in a
        .control
        .bluetooth
        .devices
        .iter()
        .enumerate()
        .skip(start)
        .take(visible)
    {
        let y = (i - start) as u16 * stride;
        let rr = row(list, y, stride);
        if i == a.control.device_index {
            b.set_style(rr, Style::new().bg(t.substrate));
        }
        text(
            b,
            rr,
            0,
            0,
            if d.connected { "●" } else { "○" },
            if d.connected { t.cpu } else { t.muted },
        );
        text(
            b,
            rr,
            2,
            0,
            truncate(&d.name, rr.width.saturating_sub(2) as usize),
            if i == a.control.device_index {
                t.bright
            } else {
                t.muted
            },
        );
        if stride == 2 {
            text(
                b,
                rr,
                2,
                1,
                if d.connected {
                    "connected"
                } else {
                    "disconnected"
                },
                if d.connected { t.cpu } else { t.wire },
            );
        }
        a.hitmap.add(rr, 10, Hit::ControlSelect(i));
    }
    line(
        b,
        list,
        list.height.saturating_sub(1),
        format!(
            "{} known · radio {}",
            count,
            match a.control.bluetooth.powered {
                Some(true) => "on",
                Some(false) => "off",
                None => "unknown",
            }
        ),
        t.muted,
    );
    let Some(device) = a.control.selected_device().cloned() else {
        let rr = frame(b, stage, "BLUETOOTH", "view only", false, &t);
        line(b, rr, 1, "Loading devices…", t.muted);
        return;
    };
    let metadata_h = if stage.height >= 22 { 9 } else { 5 };
    let scene = row(stage, 0, stage.height.saturating_sub(metadata_h));
    let view = frame(
        b,
        scene,
        &device.name.to_uppercase(),
        if device.connected {
            "CONNECTED"
        } else {
            "DISCONNECTED"
        },
        false,
        &t,
    );
    let key = device.model();
    let render = row(
        view,
        0,
        view.height
            .saturating_sub(if key == "airpods-pro" { 2 } else { 1 }),
    );
    // Demos can change selection without ever starting background hardware work.
    if a.demo.is_some() {
        a.control.prepare_demo_model();
    }
    if let Some(mesh) = &a.control.model {
        let animated = device.connected && a.motion == Motion::Full;
        let elapsed = a.control.device_since.elapsed().as_secs_f32();
        let opening = if animated {
            (elapsed / 1.6).clamp(0., 1.)
        } else {
            1.
        };
        let opening = 1. - (1. - opening).powi(3);
        let angle = if animated {
            0.35 + (elapsed - 1.6).max(0.) * 0.28
        } else {
            0.35
        };
        model::draw(
            b,
            render,
            mesh,
            angle,
            opening,
            device.connected,
            a.cell_aspect,
            key,
            &t,
        );
        line(
            b,
            view,
            view.height.saturating_sub(1),
            &mesh.source,
            t.muted,
        );
    } else {
        line(
            b,
            render,
            0,
            if a.control.model_note.is_empty() {
                "Loading geometry…"
            } else {
                &a.control.model_note
            },
            t.muted,
        );
    }
    if key == "airpods-pro" {
        line(
            b,
            view,
            view.height.saturating_sub(2),
            format!(
                "L {}     CASE {}     R {}",
                battery(device.left),
                battery(device.case),
                battery(device.right)
            ),
            if device.connected { t.cpu } else { t.muted },
        );
    }
    let details = frame(
        b,
        row(stage, scene.height, metadata_h),
        "DEVICE DETAILS",
        if device.connected {
            "OS-reported"
        } else {
            "cached values"
        },
        false,
        &t,
    );
    field(
        b,
        details,
        0,
        "Battery",
        if key == "airpods-pro" {
            format!(
                "L {} · R {} · case {}",
                battery(device.left),
                battery(device.right),
                battery(device.case)
            )
        } else {
            battery(device.battery)
        },
        &t,
    );
    field(
        b,
        details,
        1,
        "Firmware",
        device.firmware.as_deref().unwrap_or("Not reported"),
        &t,
    );
    field(b, details, 2, "Address", &device.id, &t);
    field(
        b,
        details,
        3,
        "Identity",
        format!(
            "{} · {}",
            device.vendor.as_deref().unwrap_or("—"),
            device.product.as_deref().unwrap_or("—")
        ),
        &t,
    );
    field(
        b,
        details,
        4,
        "Signal",
        value(device.rssi, |v| format!("{v} dBm · OS cache")),
        &t,
    );
    field(
        b,
        details,
        5,
        "Services",
        device.services.as_deref().unwrap_or("Not reported"),
        &t,
    );
}
fn battery(v: Option<u8>) -> String {
    value(v, |v| format!("{v}%"))
}
fn display(b: &mut Buffer, r: Rect, a: &mut App) {
    let t = a.theme.clone();
    let time = phase(a);
    let (map, side) = split(
        r,
        if r.width >= 100 {
            r.width.saturating_sub(36)
        } else {
            r.width * 55 / 100
        },
    );
    let plot = frame(b, map, "DISPLAY / SPATIAL MAP", "", false, &t);
    let ds = &a.control.displays;
    if ds.is_empty() {
        line(b, plot, 1, "Waiting for connected displays…", t.muted);
        return;
    }
    let minx = ds.iter().map(|d| d.x).fold(f64::INFINITY, f64::min);
    let miny = ds.iter().map(|d| d.y).fold(f64::INFINITY, f64::min);
    let maxx = ds
        .iter()
        .map(|d| d.x + d.width)
        .fold(f64::NEG_INFINITY, f64::max);
    let maxy = ds
        .iter()
        .map(|d| d.y + d.height)
        .fold(f64::NEG_INFINITY, f64::max);
    let scale = (plot.width.saturating_sub(6) as f64 / (maxx - minx).max(1.))
        .min(plot.height.saturating_sub(6) as f64 * a.cell_aspect as f64 / (maxy - miny).max(1.));
    let totalw = (maxx - minx) * scale;
    let totalh = (maxy - miny) * scale / a.cell_aspect as f64;
    let ox = plot.x + (plot.width as f64 - totalw).max(0.) as u16 / 2;
    let oy = plot.y + (plot.height as f64 - totalh).max(0.) as u16 / 2;
    for (i, d) in ds.iter().enumerate() {
        let x = ox + ((d.x - minx) * scale) as u16;
        let y = oy + ((d.y - miny) * scale / a.cell_aspect as f64) as u16;
        let w = ((d.width * scale) as u16)
            .max(4)
            .min(plot.right().saturating_sub(x));
        let h = ((d.height * scale / a.cell_aspect as f64) as u16)
            .max(3)
            .min(plot.bottom().saturating_sub(y));
        let rr = Rect::new(x, y, w, h);
        let v = frame(
            b,
            rr,
            &format!("{}{}", i + 1, if d.main { " / MAIN" } else { "" }),
            "",
            i == a.control.display_index,
            &t,
        );
        let strength = d.brightness.unwrap_or(0.6);
        for yy in 0..v.height {
            for xx in 0..v.width {
                let wave =
                    (xx as f32 * 0.22 + yy as f32 * 0.5 - time * 0.6 + i as f32).sin() * 0.5 + 0.5;
                if (xx + yy * 2) % 4 == 0 {
                    text(
                        b,
                        v,
                        xx,
                        yy,
                        if wave > 0.72 { "╱" } else { "·" },
                        blend(t.substrate, t.accent, wave * (0.18 + strength * 0.4)),
                    );
                }
            }
        }
        if v.width >= 14 {
            line(
                b,
                v,
                v.height / 2,
                format!("{} × {}", d.pixel_width, d.pixel_height),
                t.bright,
            );
        }
        if v.height > 2 && v.width >= 14 {
            line(
                b,
                v,
                v.height / 2 + 1,
                format!(
                    "{} Hz · {}×",
                    value(d.refresh, |v| format!("{v:.0}")),
                    d.scale
                ),
                t.text,
            );
        }
        a.hitmap.add(rr, 12, Hit::ControlSelect(i));
    }
    let details = frame(b, side, "CONNECTED DISPLAYS", "", false, &t);
    for (i, d) in ds.iter().enumerate().take(4) {
        let rr = row(details, i as u16 * 2, 2);
        text(
            b,
            rr,
            0,
            0,
            format!(
                "{} {}",
                if i == a.control.display_index {
                    "›"
                } else {
                    " "
                },
                d.name
            ),
            if i == a.control.display_index {
                t.bright
            } else {
                t.muted
            },
        );
        a.hitmap.add(rr, 10, Hit::ControlSelect(i));
    }
    let d = &ds[a.control.display_index.min(ds.len() - 1)];
    let start = (ds.len().min(4) as u16 * 2 + 1).min(9);
    field(
        b,
        details,
        start,
        "Pixels",
        format!("{} × {}", d.pixel_width, d.pixel_height),
        &t,
    );
    field(
        b,
        details,
        start + 1,
        "Desktop",
        format!("{:.0} × {:.0}", d.width, d.height),
        &t,
    );
    field(
        b,
        details,
        start + 2,
        "Origin",
        format!("({:.0}, {:.0})", d.x, d.y),
        &t,
    );
    field(
        b,
        details,
        start + 3,
        "Refresh",
        value(d.refresh, |v| format!("{v:.0} Hz")),
        &t,
    );
    field(
        b,
        details,
        start + 4,
        "Brightness",
        value(d.brightness, |v| format!("{:.0}%", v * 100.)),
        &t,
    );
    field(b, details, start + 5, "Color", &d.color_space, &t);
    field(
        b,
        details,
        start + 6,
        "Scale",
        format!("{}× · EDR {:.1}×", d.scale, d.edr),
        &t,
    );
    field(
        b,
        details,
        start + 7,
        "Rotation",
        format!(
            "{:.0}° · {}",
            d.rotation,
            if d.mirrored { "mirrored" } else { "extended" }
        ),
        &t,
    );
    field(
        b,
        details,
        start + 8,
        "Panel",
        if d.builtin { "built-in" } else { "external" },
        &t,
    );
    if d.mm_width > 0. && d.mm_height > 0. {
        field(
            b,
            details,
            start + 9,
            "Size",
            format!(
                "{:.1}″ · {:.0} × {:.0} mm",
                d.mm_width.hypot(d.mm_height) / 25.4,
                d.mm_width,
                d.mm_height
            ),
            &t,
        );
    }
    field(
        b,
        details,
        start + 10,
        "Vendor / ID",
        format!(
            "{} / {}",
            value(d.vendor, |n| format!("{n:04X}")),
            value(d.product, |n| format!("{n:04X}"))
        ),
        &t,
    );
}
fn sound(b: &mut Buffer, r: Rect, a: &mut App) {
    let t = a.theme.clone();
    let (sidebar, stage) = split(r, if r.width >= 100 { 34 } else { 25 });
    let list = frame(b, sidebar, "AUDIO ROUTING", "", false, &t);
    button(b, list, 0, 0, "[o output]", C::Output, a);
    button(b, list, 12, 0, "[i input]", C::Input, a);
    line(
        b,
        list,
        2,
        if a.control.input {
            "INPUT DEVICES"
        } else {
            "OUTPUT DEVICES"
        },
        t.muted,
    );
    let devices = a.control.audio_devices();
    let visible = list.height.saturating_sub(5) as usize / 3;
    let start = a
        .control
        .audio_index
        .saturating_sub(visible.saturating_sub(1));
    for (i, d) in devices.iter().enumerate().skip(start).take(visible) {
        let rr = row(list, 4 + (i - start) as u16 * 3, 3);
        if i == a.control.audio_index {
            b.set_style(rr, Style::new().bg(t.substrate));
        }
        text(
            b,
            rr,
            0,
            0,
            truncate(&d.name, rr.width as usize),
            if i == a.control.audio_index {
                t.bright
            } else {
                t.text
            },
        );
        let selected = if a.control.input {
            d.default_input
        } else {
            d.default_output
        };
        text(
            b,
            rr,
            0,
            1,
            format!(
                "{} · {} ch · {} kHz",
                if selected {
                    "● default"
                } else {
                    "○ available"
                },
                if a.control.input { d.inputs } else { d.outputs },
                value(d.sample_rate, |s| format!("{:.0}", s / 1000.))
            ),
            if selected { t.cpu } else { t.muted },
        );
        a.hitmap.add(rr, 10, Hit::ControlSelect(i));
    }
    let panel = frame(
        b,
        stage,
        "SOUND / PHASE PORTRAIT",
        "system output · L × R",
        false,
        &t,
    );
    let visual = row(panel, 1, panel.height.saturating_sub(7));
    let audio = super::audio::frame(a);
    let mut grid = BrailleGrid::new(visual.width, visual.height);
    let (cx, cy) = (visual.width as f32, visual.height as f32 * 2.);
    let radius = (cx * 0.78).min(cy * 0.85);
    let time = phase(a);
    for ring in [0.4, 0.7, 1.] {
        let mut prev = None;
        for i in 0..=120 {
            let a = i as f32 / 120. * TAU;
            let p = (cx + radius * ring * a.cos(), cy + radius * ring * a.sin());
            if let Some(old) = prev {
                grid.line(
                    old,
                    p,
                    blend(
                        t.substrate,
                        t.wire,
                        ((a - time * 0.35).cos().max(0.)).powi(12) * 0.65,
                    ),
                );
            }
            prev = Some(p);
        }
    }
    grid.line((cx - radius, cy), (cx + radius, cy), t.wire);
    grid.line((cx, cy - radius), (cx, cy + radius), t.wire);
    if audio.channels.len() >= 2 {
        let left = &audio.channels[0];
        let right = &audio.channels[1];
        let mut prev = None;
        for (i, (&l, &rr)) in left.iter().zip(right).enumerate() {
            let p = (
                cx + (l * 4.).clamp(-1., 1.) * radius,
                cy - (rr * 4.).clamp(-1., 1.) * radius,
            );
            if let Some(old) = prev {
                grid.line(old, p, blend(t.wire, t.cpu, 0.35 + i as f32 / 64. * 0.65));
            }
            prev = Some(p);
        }
    }
    grid.flush(visual, b, a.cfg.ui.glyphs == "ascii");
    if audio.channels.is_empty() {
        line(
            b,
            visual,
            visual.height / 2,
            if a.cfg.teardown.audio_enabled {
                "No samples · e off/on"
            } else {
                "Capture off · e enable"
            },
            t.muted,
        );
    }
    let output = a.control.sound.output();
    let name = output.map_or("No output available", |d| d.name.as_str());
    line(b, panel, panel.height.saturating_sub(6), name, t.bright);
    let volume = output.and_then(|d| d.volume);
    let muted = output.and_then(|d| d.muted) == Some(true);
    line(
        b,
        panel,
        panel.height.saturating_sub(5),
        format!(
            "{}  {}",
            value(volume, |v| format!("VOLUME {:.0}%", v * 100.)),
            if muted { "MUTED" } else { "" }
        ),
        t.text,
    );
    meter(
        b,
        row(panel, panel.height.saturating_sub(4), 1),
        volume.unwrap_or(0.),
        if muted { t.muted } else { t.cpu },
        &t,
        a.cfg.ui.glyphs == "ascii",
    );
    button(
        b,
        panel,
        0,
        panel.height.saturating_sub(2),
        "[ − ]",
        C::Volume(-1),
        a,
    );
    button(
        b,
        panel,
        7,
        panel.height.saturating_sub(2),
        "[ + ]",
        C::Volume(1),
        a,
    );
    button(
        b,
        panel,
        14,
        panel.height.saturating_sub(2),
        "[m mute]",
        C::Mute,
        a,
    );
    button(
        b,
        list,
        0,
        list.height.saturating_sub(1),
        "[Enter set device]",
        C::Activate,
        a,
    );
}
fn album(b: &mut Buffer, r: Rect, a: &App) {
    let t = &a.theme;
    let image_aspect = a.control.artwork.as_ref().map_or(1., |image| {
        image.width() as f32 / image.height().max(1) as f32
    });
    let width = r
        .width
        .saturating_sub(4)
        .min((r.height.saturating_sub(2) as f32 * a.cell_aspect * image_aspect) as u16);
    let height = (width as f32 / a.cell_aspect / image_aspect).round() as u16;
    let rect = Rect::new(
        r.x + (r.width - width) / 2,
        r.y + (r.height - height) / 2,
        width,
        height,
    );
    if let Some(image) = &a.control.artwork {
        let ramp = b" .,:;irsXA253hMHGS#9B&@";
        for y in 0..rect.height {
            for x in 0..rect.width {
                let pixel = image.get_pixel(
                    (x as u32 * image.width() / rect.width as u32).min(image.width() - 1),
                    (y as u32 * image.height() / rect.height as u32).min(image.height() - 1),
                );
                let [red, green, blue] = pixel.0;
                let l = (0.2126 * red as f32 + 0.7152 * green as f32 + 0.0722 * blue as f32) / 255.;
                let ch = ramp[(l.sqrt() * (ramp.len() - 1) as f32) as usize] as char;
                let fg = Color::Rgb(
                    red.saturating_add(20),
                    green.saturating_add(20),
                    blue.saturating_add(20),
                );
                b[(rect.x + x, rect.y + y)]
                    .set_char(ch)
                    .set_fg(fg)
                    .set_bg(Color::Rgb(red / 4, green / 4, blue / 4));
            }
        }
    } else {
        let time = phase(a);
        let mut grid = BrailleGrid::new(rect.width, rect.height);
        let (cx, cy) = (rect.width as f32, rect.height as f32 * 2.);
        let radius = cx.min(cy) * 0.85;
        for ring in 1..=12 {
            let rr = radius * ring as f32 / 12.;
            let mut prev = None;
            for i in 0..=120 {
                let angle = i as f32 / 120. * TAU;
                let p = (cx + rr * angle.cos(), cy + rr * angle.sin());
                if let Some(old) = prev {
                    grid.line(
                        old,
                        p,
                        blend(
                            t.wire,
                            t.accent,
                            ((angle - time * 0.3).cos() * 0.5 + 0.5) * 0.3,
                        ),
                    );
                }
                prev = Some(p);
            }
        }
        grid.flush(rect, b, a.cfg.ui.glyphs == "ascii");
        line(b, r, r.height.saturating_sub(1), "No artwork", t.muted);
    }
}
fn clock_time(seconds: f64) -> String {
    let s = seconds.max(0.) as u64;
    format!("{}:{:02}", s / 60, s % 60)
}
fn now_playing(b: &mut Buffer, r: Rect, a: &mut App) {
    let t = a.theme.clone();
    let (left, right) = split(r, r.width * 44 / 100);
    let cover = frame(b, left, "ALBUM ART", "", false, &t);
    album(b, cover, a);
    let panel = frame(b, right, "NOW PLAYING", "", false, &t);
    let compact = panel.height < 18;
    let metadata = match &a.control.now {
        State::Active(m) => Some(m.clone()),
        state => {
            line(
                b,
                panel,
                0,
                match state {
                    State::Waiting => "Loading player…",
                    State::Idle => "Nothing playing",
                    State::Failed(e) => e.as_str(),
                    _ => unreachable!(),
                },
                t.muted,
            );
            None
        }
    };
    if let Some(m) = &metadata {
        line(
            b,
            panel,
            u16::from(!compact),
            m.title
                .as_deref()
                .filter(|s| !s.is_empty())
                .unwrap_or("Untitled audio"),
            t.bright,
        );
        line(
            b,
            panel,
            if compact { 1 } else { 3 },
            m.artist.as_deref().unwrap_or("Unknown artist"),
            t.cpu,
        );
        if panel.height >= 12 {
            line(
                b,
                panel,
                if compact { 2 } else { 4 },
                m.album.as_deref().unwrap_or(""),
                t.muted,
            );
        }
        let player = a
            .control
            .sound
            .sources
            .iter()
            .find(|s| s.bundle == m.bundle_identifier)
            .map_or(m.bundle_identifier.as_str(), |s| s.name.as_str());
        line(
            b,
            panel,
            if compact {
                if panel.height >= 12 { 3 } else { 2 }
            } else {
                6
            },
            format!(
                "{} · {player}",
                match m.playing {
                    Some(true) => "▶",
                    Some(false) => "Ⅱ",
                    None => "—",
                }
            ),
            t.muted,
        );
        if panel.height >= 26 {
            let others: Vec<_> = a
                .control
                .sound
                .sources
                .iter()
                .filter(|s| s.bundle != m.bundle_identifier)
                .collect();
            if !others.is_empty() {
                line(b, panel, 8, "ALSO OUTPUTTING AUDIO", t.muted);
                for (i, source) in others.iter().take(3).enumerate() {
                    line(b, panel, 9 + i as u16, &source.name, t.text);
                }
            }
        }
    }
    let transport = if compact {
        if panel.height >= 12 { 5 } else { 3 }
    } else if panel.height >= 26 {
        14
    } else {
        8
    };
    let position = metadata.as_ref().and_then(|m| m.position());
    let duration = metadata.as_ref().and_then(|m| m.duration_seconds());
    line(
        b,
        panel,
        transport,
        format!(
            "{} / {}",
            value(position, clock_time),
            value(duration, clock_time)
        ),
        t.text,
    );
    let seek = row(panel, transport + 1, 1);
    meter(
        b,
        seek,
        position.zip(duration).map_or(0., |(p, d)| (p / d) as f32),
        t.accent,
        &t,
        a.cfg.ui.glyphs == "ascii",
    );
    if duration.is_some() {
        for x in 0..seek.width {
            a.hitmap.add(
                Rect::new(seek.x + x, seek.y, 1, seek.height),
                15,
                Hit::Control(C::SeekPercent(
                    (x as u32 * 1000 / seek.width.saturating_sub(1).max(1) as u32) as u16,
                )),
            );
        }
    }
    button(b, panel, 0, transport + 2, "[p ‹]", C::Previous, a);
    button(
        b,
        panel,
        7,
        transport + 2,
        if metadata.as_ref().is_some_and(|m| m.playing == Some(true)) {
            "[Space Ⅱ]"
        } else {
            "[Space ▶]"
        },
        C::TogglePlayback,
        a,
    );
    button(b, panel, 18, transport + 2, "[n ›]", C::Next, a);
    let wave_y = transport + if compact { 3 } else { 4 };
    let wave = row(panel, wave_y, panel.height.saturating_sub(wave_y + 2));
    super::audio::waveform(b, wave, Part::SpeakerLeft, a, false);
    if panel.height > transport + 5 {
        let samples = super::audio::frame(a);
        line(
            b,
            panel,
            panel.height.saturating_sub(2),
            if !a.cfg.teardown.audio_enabled {
                "Capture off · e enable"
            } else if samples.channels.is_empty() {
                "No samples · e off/on"
            } else {
                "SYSTEM MIX"
            },
            t.muted,
        );
    }
    let vol = a.control.sound.output().and_then(|d| d.volume);
    let y = panel.height.saturating_sub(1);
    button(b, panel, 0, y, "[−]", C::Volume(-1), a);
    button(b, panel, 4, y, "[+]", C::Volume(1), a);
    text(
        b,
        panel,
        9,
        y,
        value(vol, |v| format!("{:.0}%", v * 100.)),
        t.text,
    );
}
