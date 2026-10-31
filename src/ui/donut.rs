//! A Rust torus renderer in the spirit of donut.c: surface samples, a depth
//! buffer, and ASCII luminance. Math background: https://www.a1k0n.net/2011/07/20/donut-math.html
use super::{kit::text, theme::blend};
use crate::{app::App, motion::Motion};
use ratatui::{buffer::Buffer, layout::Rect};
use std::f32::consts::{FRAC_1_SQRT_2, TAU};

const RAMP: &[u8] = b".,-~:;=!*#$@";

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
struct Params {
    width: u32,
    height: u32,
    aspect: f32,
    scale: f32,
    sin_a: f32,
    cos_a: f32,
    sin_b: f32,
    cos_b: f32,
}
impl Params {
    fn new(width: u16, height: u16, aspect: f32, phase: f32) -> Self {
        let aspect = aspect.clamp(1., 3.);
        let (sin_a, cos_a) = (1. + 2. * TAU * phase).sin_cos();
        let (sin_b, cos_b) = (0.35 + TAU * phase).sin_cos();
        // Perspective silhouette of a radius-3 bounding sphere at distance 6.
        // Reserve one column at each side and one row above/below.
        // Allow an extra 6% for visible clearance through the entire rotation.
        // This envelope stays fixed for every rotation; only a resize changes scale.
        let diameter = (width as f32 - 2.)
            .min((height as f32 - 2.) * aspect)
            .max(0.);
        Self {
            width: width as u32,
            height: height as u32,
            aspect,
            scale: diameter * 3_f32.sqrt() / 2. * 0.94,
            sin_a,
            cos_a,
            sin_b,
            cos_b,
        }
    }
    fn resolution(self) -> usize {
        ((self.width as f32).min(self.height as f32 * self.aspect) * 4.)
            .ceil()
            .clamp(160., 640.) as usize
    }
}

#[repr(C)]
#[derive(Clone, Copy)]
struct SurfacePoint {
    position: [f32; 4],
    normal: [f32; 4],
}
fn surface(resolution: usize) -> Vec<SurfacePoint> {
    let tube_steps = (resolution / 3).max(64);
    let mut points = Vec::with_capacity(resolution * tube_steps);
    for ring in 0..resolution {
        let (s, c) = (ring as f32 * TAU / resolution as f32).sin_cos();
        for tube in 0..tube_steps {
            let (st, ct) = (tube as f32 * TAU / tube_steps as f32).sin_cos();
            points.push(SurfacePoint {
                position: [(2. + ct) * c, st, (2. + ct) * s, 0.],
                normal: [ct * c, st, ct * s, 0.],
            });
        }
    }
    points
}
fn rotate(v: [f32; 4], p: Params) -> [f32; 3] {
    let y = v[1] * p.cos_a - v[2] * p.sin_a;
    [
        v[0] * p.cos_b - y * p.sin_b,
        v[0] * p.sin_b + y * p.cos_b,
        v[1] * p.sin_a + v[2] * p.cos_a,
    ]
}
fn software(p: Params, points: &[SurfacePoint]) -> Vec<u32> {
    let mut frame = vec![0; (p.width * p.height) as usize];
    if p.scale == 0. {
        return frame;
    }
    for point in points {
        let v = rotate(point.position, p);
        let inverse_depth = 1. / (6. + v[2]);
        let x = (p.width as f32 / 2. + p.scale * v[0] * inverse_depth).floor() as i32;
        let y = (p.height as f32 / 2. - p.scale * v[1] * inverse_depth / p.aspect).floor() as i32;
        if x < 0 || y < 0 || x >= p.width as i32 || y >= p.height as i32 {
            continue;
        }
        let n = rotate(point.normal, p);
        let luminance = ((n[1] - n[2]) * FRAC_1_SQRT_2).clamp(0., 1.);
        let shade = (luminance * (RAMP.len() - 1) as f32).round() as u32 + 1;
        // Depth in the high bits makes a single max operation select the nearest
        // sample and its shade together. Zero means empty; even unlit surfaces occlude.
        let sample = ((inverse_depth * 1_000_000.) as u32) << 8 | shade;
        let pixel = &mut frame[y as usize * p.width as usize + x as usize];
        *pixel = (*pixel).max(sample);
    }
    frame
}

#[derive(Default)]
pub struct Renderer {
    key: Option<(Params, bool)>,
    frame: Vec<u32>,
    resolution: usize,
    points: Vec<SurfacePoint>,
    accelerated: bool,
    #[cfg(target_os = "macos")]
    metal: Option<Option<gpu::Metal>>,
}
impl Renderer {
    fn render(&mut self, p: Params, accelerate: bool) -> &[u32] {
        if self.key == Some((p, accelerate)) {
            return &self.frame;
        }
        if self.resolution != p.resolution() {
            self.resolution = p.resolution();
            self.points = surface(self.resolution);
        }
        self.accelerated = false;
        #[cfg(target_os = "macos")]
        if accelerate && p.scale > 0. {
            let metal = self.metal.get_or_insert_with(|| gpu::Metal::new().ok());
            if let Some(engine) = metal {
                if let Some(frame) = engine.render(p, &self.points) {
                    self.frame = frame;
                    self.accelerated = true;
                } else {
                    *metal = None;
                }
            }
        }
        if !self.accelerated {
            self.frame = software(p, &self.points);
        }
        self.key = Some((p, accelerate));
        &self.frame
    }
}

pub fn draw(b: &mut Buffer, r: Rect, a: &App) {
    if r.width == 0 || r.height == 0 {
        return;
    }
    let phase = if a.motion == Motion::Full {
        a.anim.phases.get("donut").copied().unwrap_or(0.)
    } else {
        0.
    };
    let p = Params::new(r.width, r.height, a.cell_aspect, phase);
    let mut renderer = a.donut.borrow_mut();
    let frame = renderer.render(p, a.motion == Motion::Full);
    for (i, &pixel) in frame.iter().enumerate() {
        let shade = (pixel & 0xff) as usize;
        if shade == 0 {
            continue;
        }
        let brightness = (shade - 1) as f32 / (RAMP.len() - 1) as f32;
        let color = if brightness > 0.75 {
            blend(a.theme.gpu, a.theme.text, (brightness - 0.75) * 2.)
        } else {
            blend(a.theme.muted, a.theme.gpu, brightness / 0.75)
        };
        text(
            b,
            r,
            (i % r.width as usize) as u16,
            (i / r.width as usize) as u16,
            (RAMP[shade - 1] as char).to_string(),
            color,
        );
    }
}

#[cfg(target_os = "macos")]
mod gpu {
    use super::{Params, SurfacePoint};
    use metal::*;
    pub(super) struct Metal {
        device: Device,
        queue: CommandQueue,
        pipeline: ComputePipelineState,
        points: Buffer,
        output: Buffer,
    }
    impl Metal {
        pub(super) fn new() -> Result<Self, String> {
            objc::rc::autoreleasepool(|| {
                let device = Device::system_default().ok_or("No Metal device")?;
                let library = device
                    .new_library_with_source(include_str!("donut.metal"), &CompileOptions::new())?;
                let function = library.get_function("draw_donut", None)?;
                let pipeline = device.new_compute_pipeline_state_with_function(&function)?;
                Ok(Self {
                    queue: device.new_command_queue(),
                    points: device.new_buffer(4, MTLResourceOptions::StorageModeShared),
                    output: device.new_buffer(4, MTLResourceOptions::StorageModeShared),
                    device,
                    pipeline,
                })
            })
        }
        pub(super) fn render(&mut self, p: Params, points: &[SurfacePoint]) -> Option<Vec<u32>> {
            objc::rc::autoreleasepool(|| {
                let count = p.width as u64 * p.height as u64;
                let mesh_bytes = std::mem::size_of_val(points) as u64;
                if self.points.length() != mesh_bytes {
                    // Geometry is generated in Rust and uploaded only when resolution changes.
                    self.points = self.device.new_buffer_with_data(
                        points.as_ptr().cast(),
                        mesh_bytes,
                        MTLResourceOptions::StorageModeShared,
                    );
                }
                if self.output.length() < count * 4 {
                    self.output = self
                        .device
                        .new_buffer(count * 4, MTLResourceOptions::StorageModeShared);
                }
                let command = self.queue.new_command_buffer();
                command.set_label("Overseer donut projection + ASCII shading");
                let clear = command.new_blit_command_encoder();
                clear.fill_buffer(&self.output, NSRange::new(0, count * 4), 0);
                clear.end_encoding();
                let encoder = command.new_compute_command_encoder();
                encoder.set_compute_pipeline_state(&self.pipeline);
                encoder.set_bytes(
                    0,
                    std::mem::size_of::<Params>() as u64,
                    (&p as *const Params).cast(),
                );
                encoder.set_buffer(1, Some(&self.points), 0);
                encoder.set_buffer(2, Some(&self.output), 0);
                let samples = points.len() as u32;
                encoder.set_bytes(3, 4, (&samples as *const u32).cast());
                let group = self.pipeline.thread_execution_width();
                encoder.dispatch_thread_groups(
                    MTLSize::new((samples as u64).div_ceil(group), 1, 1),
                    MTLSize::new(group, 1, 1),
                );
                encoder.end_encoding();
                command.commit();
                command.wait_until_completed();
                if command.status() != MTLCommandBufferStatus::Completed {
                    return None;
                }
                // Completion makes the shared atomic depth/shade buffer safe to read.
                Some(
                    unsafe {
                        std::slice::from_raw_parts(
                            self.output.contents().cast::<u32>(),
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
    fn torus_has_a_hole_fits_the_pane_and_respects_motion_controls() {
        let p = Params::new(80, 40, 2., 0.);
        let points = surface(p.resolution());
        let mut facing = p;
        (facing.sin_a, facing.cos_a) = (1., 0.);
        (facing.sin_b, facing.cos_b) = (0., 1.);
        let front = software(facing, &points);
        assert_eq!(
            front[(p.height / 2 * p.width + p.width / 2) as usize],
            0,
            "the hole must stay open"
        );
        assert!(front.iter().any(|v| v & 255 >= 10));
        assert!(front.iter().any(|v| v & 255 == 1));
        // Check geometry before either renderer can silently discard clipped samples.
        let envelope_points = surface(160);
        for (w, h, aspect) in [(10, 5, 2.05), (23, 11, 2.05), (49, 24, 2.05), (80, 40, 1.6)] {
            let mut scale = None;
            for step in 0..360 {
                let p = Params::new(w, h, aspect, step as f32 / 360.);
                assert_eq!(*scale.get_or_insert(p.scale), p.scale);
                for point in &envelope_points {
                    let v = rotate(point.position, p);
                    let depth = 6. + v[2];
                    let x = (p.width as f32 / 2. + p.scale * v[0] / depth).floor();
                    let y = (p.height as f32 / 2. - p.scale * v[1] / depth / p.aspect).floor();
                    assert!(
                        x >= 1. && x < w as f32 - 1.,
                        "horizontal clipping at {step}°"
                    );
                    assert!(y >= 1. && y < h as f32 - 1., "vertical clipping at {step}°");
                }
            }
        }
        for (w, h) in [(0, 0), (1, 1), (28, 4), (31, 11), (80, 40), (180, 70)] {
            let mut renderer = Renderer::default();
            for step in 0..16 {
                let p = Params::new(w, h, 2., step as f32 / 16.);
                let frame = renderer.render(p, false);
                assert_eq!(frame.len(), w as usize * h as usize);
                assert!(frame.iter().all(|v| v & 255 <= RAMP.len() as u32));
                if w >= 28 {
                    assert!(frame.iter().any(|v| *v != 0));
                    assert!(frame[..w as usize].iter().all(|v| *v == 0));
                    assert!(
                        frame[(h as usize - 1) * w as usize..]
                            .iter()
                            .all(|v| *v == 0)
                    );
                    assert!((0..h as usize).all(
                        |y| frame[y * w as usize] == 0 && frame[(y + 1) * w as usize - 1] == 0
                    ));
                }
            }
        }
        assert_ne!(
            software(p, &points),
            software(Params::new(80, 40, 2., 0.1), &points)
        );
        let mut renderer = Renderer::default();
        renderer.render(p, false);
        let cached = renderer.frame.as_ptr();
        renderer.render(p, false);
        assert_eq!(cached, renderer.frame.as_ptr());
        let mut anim = crate::motion::Anim::default();
        let s = crate::metrics::demo("idle", 0.);
        anim.tick(&s, 0.1, Motion::Full, true, 30.);
        let phase = anim.phases["donut"];
        assert!(phase > 0.);
        for (motion, live) in [
            (Motion::Full, false),
            (Motion::Reduced, true),
            (Motion::Off, true),
        ] {
            anim.tick(&s, 0.1, motion, live, 30.);
            assert_eq!(anim.phases["donut"], phase);
        }
    }
    #[cfg(target_os = "macos")]
    #[test]
    #[ignore = "requires a real Metal device; run explicitly on macOS"]
    fn metal_matches_rust_and_survives_resize() {
        let mut renderer = Renderer::default();
        renderer.render(Params::new(80, 24, 2., 0.37), true);
        assert!(renderer.accelerated, "Metal initialization failed");
        let mut elapsed = std::time::Duration::ZERO;
        for (w, h) in [(80, 24), (160, 60), (28, 6)] {
            for frame in 0..12 {
                let p = Params::new(w, h, 2., frame as f32 / 12.);
                let start = std::time::Instant::now();
                let actual = renderer.render(p, true).to_vec();
                elapsed += start.elapsed();
                assert!(renderer.accelerated, "Metal compilation/dispatch failed");
                let expected = software(p, &renderer.points);
                // Tiny FP differences can move a boundary sample to the next cell.
                let difference = actual
                    .iter()
                    .zip(&expected)
                    .filter(|(a, b)| (**a & 255) != (**b & 255))
                    .count();
                assert!(
                    (difference as f32 / actual.len() as f32) < 0.01,
                    "GPU/CPU mismatch: {difference} cells"
                );
                assert!(actual.iter().any(|v| *v != 0));
            }
        }
        eprintln!(
            "Metal donut: {:.3} ms/frame (warm pipeline; includes resizes)",
            elapsed.as_secs_f64() * 1000. / 36.
        );
    }
}
