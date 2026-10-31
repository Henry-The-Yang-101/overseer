use super::kit::*;
use crate::{
    app::{App, Part},
    audio::Frame,
};
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::Style,
    widgets::{Block, BorderType, Clear, Widget},
};
pub fn frame(a: &App) -> Frame {
    if !a.cfg.teardown.audio_enabled || a.cursor.is_some() {
        return Frame {
            status: String::new(),
            channels: vec![],
        };
    }
    if a.demo.is_some() {
        let time = if a.motion == crate::motion::Motion::Full {
            a.started.elapsed().as_secs_f32()
        } else {
            a.sample().at as f32
        };
        let channels = (0..2)
            .map(|c| {
                let envelope =
                    0.02 + 0.30 * ((time * 3. + c as f32 * 0.8).sin() * 0.5 + 0.5).powi(2);
                (0..64)
                    .map(|i| (i as f32 * 0.31 + c as f32 + time).sin() * envelope)
                    .collect()
            })
            .collect();
        Frame {
            status: "Synthetic stereo waveform · demo".into(),
            channels,
        }
    } else {
        let mut frame = a.audio.snapshot();
        if frame.channels.iter().all(Vec::is_empty) {
            frame.status.clear();
        }
        frame
    }
}
pub fn waveform(b: &mut Buffer, r: Rect, p: Part, a: &App, vertical: bool) {
    if r.is_empty() {
        return;
    }
    let frame = frame(a);
    // Missing samples share the same flat baseline as silent audio.
    let samples = frame
        .channels
        .get(usize::from(p == Part::SpeakerRight))
        .filter(|samples| !samples.is_empty())
        .map(Vec::as_slice)
        .unwrap_or(&[0.]);
    let mut g = BrailleGrid::new(r.width, r.height);
    let count = if vertical {
        r.height as usize * 4
    } else {
        r.width as usize * 2
    };
    let mut previous = None;
    for i in 0..count {
        let v = (samples[i * samples.len() / count] * 4.).clamp(-1., 1.);
        let point = if vertical {
            ((r.width as f32 * 2. - 1.) * (0.5 + v * 0.45), i as f32)
        } else {
            (i as f32, (r.height as f32 * 4. - 1.) * (0.5 - v * 0.45))
        };
        if let Some(old) = previous {
            g.line(old, point, a.theme.accent);
        }
        previous = Some(point);
    }
    g.flush(r, b, a.cfg.ui.glyphs == "ascii");
}
pub fn inspector(b: &mut Buffer, r: Rect, p: Part, a: &App) {
    Clear.render(r, b);
    let t = &a.theme;
    let rr = super::kit::frame(b, r, &p.name().to_uppercase(), "Esc close", true, t);
    line(
        b,
        rr,
        0,
        if a.cfg.teardown.audio_enabled {
            "[ Enter / e: turn capture OFF ]"
        } else {
            "[ Enter / e: turn capture ON ]"
        },
        t.accent,
    );
    line(
        b,
        rr,
        2,
        truncate(&frame(a).status, rr.width as usize),
        t.text,
    );
    let graph = Rect::new(rr.x, rr.y + 4, rr.width, rr.height.saturating_sub(9));
    waveform(b, graph, p, a, false);
    line(
        b,
        rr,
        rr.height.saturating_sub(4),
        "Stereo system output · display gain 4×",
        t.muted,
    );
    line(
        b,
        rr,
        rr.height.saturating_sub(3),
        "Includes headphones / external outputs; not driver telemetry.",
        t.muted,
    );
    line(
        b,
        rr,
        rr.height.saturating_sub(2),
        "Both channels share capture. Audio is never saved.",
        t.muted,
    );
}

/// Two centered sprites, selected directly from the current waveform's amplitude.
pub fn grille(b: &mut Buffer, r: Rect, p: Part, a: &App, frame: &Frame) -> Rect {
    let enlarged = a.motion == crate::motion::Motion::Full
        && a.cursor.is_none()
        && frame.level(usize::from(p == Part::SpeakerRight)) >= 0.03;
    let size = if enlarged { 7 } else { 5 };
    // Odd dimensions keep both sprites on exactly the same center cell.
    let width = size.min(r.width.saturating_sub(u16::from(r.width.is_multiple_of(2))));
    let height = size.min(
        r.height
            .saturating_sub(u16::from(r.height.is_multiple_of(2))),
    );
    let pill = Rect::new(
        r.x + (r.width - width) / 2,
        r.y + (r.height - height) / 2,
        width,
        height,
    );
    Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(Style::new().fg(a.theme.wire))
        .render(pill, b);
    if width >= 5 && height >= 5 {
        Block::bordered()
            .border_type(BorderType::Rounded)
            .border_style(Style::new().fg(a.theme.muted))
            .render(Rect::new(pill.x + 1, pill.y + 1, width - 2, height - 2), b);
    }
    pill
}

/// Mirrored amplitude bars with a bell envelope, half-width at the outer rows.
pub fn bars(b: &mut Buffer, r: Rect, p: Part, a: &App, left: bool, frame: &Frame) {
    if r.is_empty() {
        return;
    }
    let Some(samples) = frame.channels.get(usize::from(p == Part::SpeakerRight)) else {
        return;
    };
    let half_height = r.height.saturating_sub(1) / 2;
    for y in 0..r.height {
        let start = y as usize * samples.len() / r.height as usize;
        let end = (((y + 1) as usize * samples.len() / r.height as usize).max(start + 1))
            .min(samples.len());
        let level =
            (samples[start..end].iter().map(|v| v * v).sum::<f32>() / (end - start) as f32).sqrt();
        let distance = if half_height == 0 {
            0.
        } else {
            1. - y.min(r.height - 1 - y) as f32 / half_height as f32
        };
        let envelope = (-std::f32::consts::LN_2 * distance * distance).exp();
        // Limit the audio level first so loud signals retain the bell shape.
        let width = ((level * 4.).clamp(0., 1.) * envelope * r.width as f32).round() as u16;
        for x in 0..width {
            text(
                b,
                r,
                if left { r.width - 1 - x } else { x },
                y,
                if a.cfg.ui.glyphs == "ascii" {
                    "="
                } else {
                    "▬"
                },
                a.theme.accent,
            );
        }
    }
}
