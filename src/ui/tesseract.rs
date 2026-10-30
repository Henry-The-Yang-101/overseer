//! A 4D rotation projected into 3D, then depth-shaded into terminal cells.
//! Metal computes both vertex projection and rasterization; text output stays on the CPU.
use super::{
    kit::{BrailleGrid, text},
    theme::blend,
};
use crate::{app::App, motion::Motion};
use ratatui::{buffer::Buffer, layout::Rect};

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
struct Params {
    width: u32,
    height: u32,
    phase: f32,
    aspect: f32,
}

#[derive(Default)]
pub struct Renderer {
    key: Option<(Params, bool)>,
    pixels: Vec<f32>,
    accelerated: bool,
    #[cfg(target_os = "macos")]
    metal: Option<Option<gpu::Metal>>,
}
impl Renderer {
    fn render(&mut self, p: Params, accelerate: bool) -> &[f32] {
        if self.key == Some((p, accelerate)) {
            return &self.pixels;
        }
        self.accelerated = false;
        #[cfg(target_os = "macos")]
        if accelerate {
            let metal = self.metal.get_or_insert_with(|| gpu::Metal::new().ok());
            if let Some(engine) = metal {
                if let Some(pixels) = engine.render(p) {
                    self.pixels = pixels;
                    self.accelerated = true;
                } else {
                    // A failed device/command is disabled for the rest of this session.
                    *metal = None;
                }
            }
        }
        if !self.accelerated {
            self.pixels = software(p);
        }
        self.key = Some((p, accelerate));
        &self.pixels
    }
}

pub fn draw(b: &mut Buffer, r: Rect, a: &App) {
    if r.width == 0 || r.height == 0 {
        return;
    }
    let phase = if a.motion == Motion::Full {
        a.anim.phases.get("tesseract").copied().unwrap_or(0.)
    } else {
        0.
    };
    let p = Params {
        width: r.width as u32 * 2,
        height: r.height as u32 * 4,
        phase,
        aspect: a.cell_aspect.clamp(1., 3.),
    };
    let mut renderer = a.tesseract.borrow_mut();
    // Static exports and reduced motion need no GPU submission.
    let pixels = renderer.render(p, a.motion == Motion::Full);
    if a.cfg.ui.glyphs == "ascii" {
        const RAMP: &[u8] = b" .:-=+*#%@";
        for y in 0..r.height {
            for x in 0..r.width {
                let mut brightness: f32 = 0.;
                let mut coverage = 0.;
                for dy in 0..4 {
                    for dx in 0..2 {
                        let v =
                            pixels[((y as u32 * 4 + dy) * p.width + x as u32 * 2 + dx) as usize];
                        brightness = brightness.max(v);
                        coverage += if v > 0. { 0.125 } else { 0. };
                    }
                }
                if brightness > 0. {
                    let shade = brightness * (0.45 + 0.55 * coverage);
                    let glyph =
                        RAMP[(shade * (RAMP.len() - 1) as f32).round().max(1.) as usize] as char;
                    text(
                        b,
                        r,
                        x,
                        y,
                        glyph.to_string(),
                        blend(a.theme.wire, a.theme.gpu, brightness),
                    );
                }
            }
        }
    } else {
        let mut grid = BrailleGrid::new(r.width, r.height);
        // All eight dots share one terminal foreground color. Use the strongest
        // highlight, so a dim final sample cannot erase a lit foreground edge.
        for y in 0..r.height as u32 {
            for x in 0..r.width as u32 {
                let mut brightness = 0_f32;
                for dy in 0..4 {
                    for dx in 0..2 {
                        brightness =
                            brightness.max(pixels[((y * 4 + dy) * p.width + x * 2 + dx) as usize]);
                    }
                }
                let color = if brightness > 0.7 {
                    blend(a.theme.gpu, a.theme.text, (brightness - 0.7) / 0.3)
                } else {
                    blend(a.theme.wire, a.theme.gpu, brightness / 0.7)
                };
                for dy in 0..4 {
                    for dx in 0..2 {
                        if pixels[((y * 4 + dy) * p.width + x * 2 + dx) as usize] > 0. {
                            grid.set((x * 2 + dx) as i32, (y * 4 + dy) as i32, color);
                        }
                    }
                }
            }
        }
        grid.flush(r, b, false);
    }
}

fn rotate(p: &mut [f32; 4], a: usize, b: usize, angle: f32) {
    let (s, c) = angle.sin_cos();
    (p[a], p[b]) = (p[a] * c - p[b] * s, p[a] * s + p[b] * c);
}
fn vertices(phase: f32) -> [[f32; 4]; 16] {
    let angle = phase * std::f32::consts::TAU;
    std::array::from_fn(|i| {
        let mut v = std::array::from_fn(|axis| if i & (1 << axis) == 0 { -1. } else { 1. });
        // XW rotation takes the inner cube through the outer one and back.
        rotate(&mut v, 0, 3, angle);
        rotate(&mut v, 1, 3, 0.28 * angle.sin());
        let perspective4 = 2.6 / (3.6 - v[3]);
        for c in &mut v[..3] {
            *c *= perspective4;
        }
        rotate(&mut v, 0, 2, 0.48 + 0.12 * angle.sin());
        rotate(&mut v, 1, 2, -0.35);
        let perspective3 = 5. / (6. - v[2]);
        v[0] *= perspective3;
        v[1] *= perspective3;
        v[3] = 0.;
        v
    })
}
fn software(p: Params) -> Vec<f32> {
    let vertices = vertices(p.phase);
    let scale = (p.width as f32 / 2.).min(p.height as f32 * p.aspect / 4.) / 3.6;
    let radius = 0.055_f32.max(0.48 / scale);
    let mut pixels = vec![0.; (p.width * p.height) as usize];
    for y in 0..p.height {
        for x in 0..p.width {
            let px = (x as f32 + 0.5 - p.width as f32 / 2.) / (2. * scale);
            let py = -(y as f32 + 0.5 - p.height as f32 / 2.) * p.aspect / (4. * scale);
            let mut depth = f32::NEG_INFINITY;
            let mut light = 0.;
            for i in 0..16 {
                for axis in 0..4 {
                    if i & (1 << axis) != 0 {
                        continue;
                    }
                    let a = vertices[i];
                    let b = vertices[i | (1 << axis)];
                    let dx = b[0] - a[0];
                    let dy = b[1] - a[1];
                    let len2 = (dx * dx + dy * dy).max(0.000001);
                    let t = (((px - a[0]) * dx + (py - a[1]) * dy) / len2).clamp(0., 1.);
                    let nx = (px - a[0] - t * dx) / radius;
                    let ny = (py - a[1] - t * dy) / radius;
                    let d2 = nx * nx + ny * ny;
                    if d2 > 1. {
                        continue;
                    }
                    let nz = (1. - d2).sqrt();
                    let z = a[2] + t * (b[2] - a[2]) + radius * nz;
                    if z > depth {
                        depth = z;
                        let diffuse = (0.35 - 0.25 * nx + 0.3 * ny + 0.55 * nz).clamp(0.18, 1.);
                        let fog = (0.65 + 0.16 * z).clamp(0.3, 1.);
                        light = diffuse * fog;
                    }
                }
            }
            pixels[(y * p.width + x) as usize] = light;
        }
    }
    pixels
}

#[cfg(target_os = "macos")]
mod gpu {
    use super::Params;
    use metal::*;
    pub(super) struct Metal {
        device: Device,
        queue: CommandQueue,
        project: ComputePipelineState,
        shade: ComputePipelineState,
        vertices: Buffer,
        output: Buffer,
    }
    impl Metal {
        pub(super) fn new() -> Result<Self, String> {
            objc::rc::autoreleasepool(|| {
                let device = Device::system_default().ok_or("No Metal device")?;
                let library = device.new_library_with_source(
                    include_str!("tesseract.metal"),
                    &CompileOptions::new(),
                )?;
                let project_function = library.get_function("project", None)?;
                let shade_function = library.get_function("shade", None)?;
                let project = device.new_compute_pipeline_state_with_function(&project_function)?;
                let shade = device.new_compute_pipeline_state_with_function(&shade_function)?;
                Ok(Self {
                    queue: device.new_command_queue(),
                    vertices: device.new_buffer(16 * 16, MTLResourceOptions::StorageModeShared),
                    output: device.new_buffer(4, MTLResourceOptions::StorageModeShared),
                    device,
                    project,
                    shade,
                })
            })
        }
        pub(super) fn render(&mut self, p: Params) -> Option<Vec<f32>> {
            objc::rc::autoreleasepool(|| {
                let count = p.width as u64 * p.height as u64;
                if self.output.length() < count * 4 {
                    self.output = self
                        .device
                        .new_buffer(count * 4, MTLResourceOptions::StorageModeShared);
                }
                let command = self.queue.new_command_buffer();
                command.set_label("Overseer tesseract projection + shading");
                // Separate encoders establish the dependency on the projected vertices.
                for (pipeline, count) in [(&self.project, 16), (&self.shade, count)] {
                    let encoder = command.new_compute_command_encoder();
                    encoder.set_compute_pipeline_state(pipeline);
                    encoder.set_bytes(
                        0,
                        std::mem::size_of::<Params>() as u64,
                        (&p as *const Params).cast(),
                    );
                    encoder.set_buffer(1, Some(&self.vertices), 0);
                    encoder.set_buffer(2, Some(&self.output), 0);
                    let group = pipeline.thread_execution_width();
                    encoder.dispatch_thread_groups(
                        MTLSize::new(count.div_ceil(group), 1, 1),
                        MTLSize::new(group, 1, 1),
                    );
                    encoder.end_encoding();
                }
                command.commit();
                command.wait_until_completed();
                if command.status() != MTLCommandBufferStatus::Completed {
                    return None;
                }
                // Shared storage is CPU-readable after completion; output holds count f32s.
                Some(
                    unsafe {
                        std::slice::from_raw_parts(
                            self.output.contents().cast::<f32>(),
                            count as usize,
                        )
                    }
                    .to_vec(),
                )
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn folding_loop_is_bounded_and_motion_controls_freeze_it() {
        let start = vertices(0.);
        let end = vertices(1.);
        for (a, b) in start.iter().zip(end.iter()) {
            for axis in 0..3 {
                assert!((a[axis] - b[axis]).abs() < 0.00001);
            }
        }
        for frame in 0..180 {
            for v in vertices(frame as f32 / 180.) {
                assert!(v.iter().all(|c| c.is_finite()));
                assert!(
                    v[0].abs() < 2.5 && v[1].abs() < 2.5,
                    "clipped vertex: {v:?}"
                );
            }
        }
        let p = Params {
            width: 120,
            height: 72,
            phase: 0.,
            aspect: 2.,
        };
        let first = software(p);
        assert!(first.iter().any(|v| *v > 0.7));
        assert!(first.iter().any(|v| *v > 0. && *v < 0.4));
        assert_ne!(first, software(Params { phase: 0.2, ..p }));
        let mut anim = crate::motion::Anim::default();
        let s = crate::metrics::demo("idle", 0.);
        anim.tick(&s, 0.1, Motion::Full, true, 30.);
        let phase = anim.phases["tesseract"];
        assert!(phase > 0.); // Idle GPUs still animate.
        for (motion, live) in [
            (Motion::Full, false),
            (Motion::Reduced, true),
            (Motion::Off, true),
        ] {
            anim.tick(&s, 0.1, motion, live, 30.);
            assert_eq!(anim.phases["tesseract"], phase);
        }
    }

    #[cfg(target_os = "macos")]
    #[test]
    #[ignore = "requires a real Metal device; run explicitly on macOS"]
    fn metal_matches_software_and_reuses_buffers_after_resize() {
        let mut metal = gpu::Metal::new().expect("Metal shader compilation/device unavailable");
        let mut gpu_time = std::time::Duration::ZERO;
        for (width, height) in [(120, 72), (240, 160), (40, 24)] {
            for frame in 0..12 {
                let p = Params {
                    width,
                    height,
                    phase: frame as f32 / 12.,
                    aspect: 2.,
                };
                let before = std::time::Instant::now();
                let actual = metal.render(p).expect("Metal dispatch failed");
                gpu_time += before.elapsed();
                let expected = software(p);
                let error: f32 = actual
                    .iter()
                    .zip(&expected)
                    .map(|(a, b)| (a - b).abs())
                    .sum::<f32>()
                    / actual.len() as f32;
                assert!(actual.iter().all(|v| v.is_finite() && *v >= 0. && *v <= 1.));
                assert!(error < 0.001, "GPU/CPU divergence: {error}");
            }
        }
        eprintln!(
            "Metal projection + shading + readback: {:.3} ms/frame (36 frames, three sizes)",
            gpu_time.as_secs_f64() * 1000. / 36.
        );
        let mut renderer = Renderer::default();
        let p = Params {
            width: 120,
            height: 72,
            phase: 0.2,
            aspect: 2.,
        };
        renderer.render(p, true);
        assert!(renderer.accelerated, "app renderer did not select Metal");
        let before = renderer.pixels.as_ptr();
        renderer.render(p, true);
        assert_eq!(
            before,
            renderer.pixels.as_ptr(),
            "paused redraw must reuse the frame"
        );
    }
}
