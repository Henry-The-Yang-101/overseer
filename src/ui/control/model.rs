use crate::{
    control::models::{Mesh, V},
    ui::{
        kit::text,
        theme::{Theme, blend, rgb},
    },
};
use ratatui::{buffer::Buffer, layout::Rect};

/// Perspective, smooth normals, depth testing, a broad key light, rim light and
/// a tight specular highlight. Geometry stays independent of terminal size.
#[allow(clippy::too_many_arguments)]
pub fn draw(
    b: &mut Buffer,
    r: Rect,
    mesh: &Mesh,
    angle: f32,
    opening: f32,
    connected: bool,
    aspect: f32,
    key: &str,
    t: &Theme,
) {
    if r.is_empty() {
        return;
    }
    let w = r.width as usize;
    let h = r.height as usize;
    let mut depth = vec![f32::NEG_INFINITY; w * h];
    let mut shades = vec![None; w * h];
    let aspect = aspect.clamp(1.2, 3.);
    let scale = (w as f32 / 4.4).min(h as f32 * aspect / 3.7) * 0.88;
    let pitch = if key.contains("keychron") || key.contains("aula") {
        0.72
    } else if key.contains("mouse") || key.contains("master") {
        0.55
    } else {
        0.13
    };
    let transformed: Vec<_> = mesh
        .vertices
        .iter()
        .map(|v| {
            let mut p = v.p;
            let mut n = v.n;
            if mesh.airpods {
                if v.part == 1 || v.part == 2 {
                    let closed = V(p.0 * 0.36, p.1 - 1.05, p.2 - 0.12);
                    p = closed.scaled(1. - opening).plus(p.scaled(opening));
                } else if v.part == 3 {
                    let pivot = V(0., -0.22, -0.33);
                    let a = (1. - opening) * 1.55;
                    p = p.minus(pivot).pitch(a).plus(pivot);
                    n = n.pitch(a);
                }
            }
            p = p.yaw(angle).pitch(pitch);
            n = n.yaw(angle).pitch(pitch);
            let perspective = 6. / (6. - p.2);
            (
                V(
                    w as f32 * 0.5 + p.0 * scale * perspective,
                    h as f32 * 0.53 - p.1 * scale / aspect * perspective,
                    p.2,
                ),
                n,
                v.albedo,
            )
        })
        .collect();
    let light = V(-0.45, 0.8, 1.2).unit();
    let half = light.plus(V(0., 0., 1.)).unit();
    for [i, j, k] in &mesh.faces {
        let (p0, n0, a0) = transformed[*i];
        let (p1, n1, a1) = transformed[*j];
        let (p2, n2, a2) = transformed[*k];
        let edge = |a: V, c: V, x: f32, y: f32| (x - a.0) * (c.1 - a.1) - (y - a.1) * (c.0 - a.0);
        let area = edge(p0, p1, p2.0, p2.1);
        if area.abs() < 1e-5 {
            continue;
        }
        let x0 = p0.0.min(p1.0).min(p2.0).floor().max(0.) as usize;
        let x1 = (p0.0.max(p1.0).max(p2.0).ceil().max(0.) as usize).min(w.saturating_sub(1));
        let y0 = p0.1.min(p1.1).min(p2.1).floor().max(0.) as usize;
        let y1 = (p0.1.max(p1.1).max(p2.1).ceil().max(0.) as usize).min(h.saturating_sub(1));
        if x0 >= w || y0 >= h {
            continue;
        }
        for y in y0..=y1 {
            for x in x0..=x1 {
                let (xv, yv) = (x as f32 + 0.5, y as f32 + 0.5);
                let u = edge(p1, p2, xv, yv) / area;
                let v = edge(p2, p0, xv, yv) / area;
                let z = 1. - u - v;
                if u < -0.001 || v < -0.001 || z < -0.001 {
                    continue;
                }
                let d = u * p0.2 + v * p1.2 + z * p2.2;
                let index = y * w + x;
                if d <= depth[index] {
                    continue;
                }
                depth[index] = d;
                let normal = n0.scaled(u).plus(n1.scaled(v)).plus(n2.scaled(z)).unit();
                let albedo = u * a0 + v * a1 + z * a2;
                let diffuse = normal.dot(light).max(0.);
                let specular = normal.dot(half).max(0.).powi(36) * 0.8;
                let rim = (1. - normal.2.abs()).powi(3) * 0.28;
                let shade = (albedo * (0.20 + 0.67 * diffuse) + specular + rim).clamp(0., 1.);
                shades[index] = Some((shade, specular));
            }
        }
    }
    let base = if connected {
        rgb(0x70dfcc)
    } else {
        rgb(0x899397)
    };
    let ramp = b" .:;=+*#%@";
    for y in 0..h {
        for x in 0..w {
            if let Some((shade, specular)) = shades[y * w + x] {
                let glyph =
                    ramp[(shade * (ramp.len() - 1) as f32).round().max(1.) as usize] as char;
                let color = blend(
                    blend(t.panel, base, 0.25 + shade * 0.75),
                    if connected {
                        rgb(0xe2fff9)
                    } else {
                        rgb(0xd9dfe1)
                    },
                    specular * 0.8,
                );
                text(b, r, x as u16, y as u16, glyph.to_string(), color);
            }
        }
    }
}
