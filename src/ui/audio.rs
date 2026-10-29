use super::kit::*;
use crate::{
    app::{App, Part},
    audio::Frame,
};
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    widgets::{Clear, Widget},
};
pub fn frame(a: &App) -> Frame {
    let status = if !a.cfg.teardown.audio_enabled {
        Some("Capture off · Enter in speaker inspector to enable")
    } else if a.cursor.is_some() {
        Some("Audio is live only · unavailable in timeline replay")
    } else {
        None
    };
    if let Some(status) = status {
        return Frame {
            status: status.into(),
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
        a.audio.snapshot()
    }
}
pub fn waveform(b: &mut Buffer, r: Rect, p: Part, a: &App, vertical: bool) {
    if r.is_empty() {
        return;
    }
    let frame = frame(a);
    let Some(samples) = frame.channels.get(usize::from(p == Part::SpeakerRight)) else {
        if vertical {
            for y in 0..r.height {
                text(b, r, r.width / 2, y, "┆", a.theme.wire);
            }
        } else {
            line(
                b,
                r,
                0,
                truncate(&frame.status, r.width as usize),
                a.theme.muted,
            );
        }
        return;
    };
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

/// Mirrored amplitude bars: each row summarizes a short slice of system output.
pub fn bars(b: &mut Buffer, r: Rect, p: Part, a: &App, left: bool) {
    if r.is_empty() {
        return;
    }
    let frame = frame(a);
    let Some(samples) = frame.channels.get(usize::from(p == Part::SpeakerRight)) else {
        return;
    };
    for y in 0..r.height {
        let start = y as usize * samples.len() / r.height as usize;
        let end = (((y + 1) as usize * samples.len() / r.height as usize).max(start + 1))
            .min(samples.len());
        let level =
            (samples[start..end].iter().map(|v| v * v).sum::<f32>() / (end - start) as f32).sqrt();
        let width = (level * 4. * r.width as f32)
            .round()
            .clamp(0., r.width as f32) as u16;
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
