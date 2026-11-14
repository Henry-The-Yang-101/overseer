//! Actual indexed geometry, rendered with a depth buffer and interpolated normals.
//! AirPods, Sony and MX Master assets are credited CC-BY meshes. Other built-ins
//! are original product-shaped approximations. See assets/control/SOURCES.md.
use std::f32::consts::{PI, TAU};
#[derive(Clone, Copy, Debug, Default)]
pub struct V(pub f32, pub f32, pub f32);
impl V {
    pub fn plus(self, b: Self) -> Self {
        Self(self.0 + b.0, self.1 + b.1, self.2 + b.2)
    }
    pub fn minus(self, b: Self) -> Self {
        Self(self.0 - b.0, self.1 - b.1, self.2 - b.2)
    }
    pub fn scaled(self, s: f32) -> Self {
        Self(self.0 * s, self.1 * s, self.2 * s)
    }
    pub fn dot(self, b: Self) -> f32 {
        self.0 * b.0 + self.1 * b.1 + self.2 * b.2
    }
    pub fn unit(self) -> Self {
        self.scaled(1. / self.dot(self).sqrt().max(1e-6))
    }
    pub fn yaw(self, a: f32) -> Self {
        let (s, c) = a.sin_cos();
        Self(c * self.0 + s * self.2, self.1, -s * self.0 + c * self.2)
    }
    pub fn pitch(self, a: f32) -> Self {
        let (s, c) = a.sin_cos();
        Self(self.0, c * self.1 - s * self.2, s * self.1 + c * self.2)
    }
}
#[derive(Clone, Copy)]
pub struct Vertex {
    pub p: V,
    pub n: V,
    pub albedo: f32,
    pub part: u32,
}
pub struct Mesh {
    pub vertices: Vec<Vertex>,
    pub faces: Vec<[usize; 3]>,
    pub source: String,
    pub airpods: bool,
}
impl Mesh {
    fn new(source: &str) -> Self {
        Self {
            vertices: vec![],
            faces: vec![],
            source: source.into(),
            airpods: false,
        }
    }
    fn surface(
        &mut self,
        u: usize,
        v: usize,
        part: u32,
        albedo: f32,
        f: impl Fn(f32, f32) -> (V, V),
    ) {
        let start = self.vertices.len();
        for j in 0..=v {
            for i in 0..=u {
                let (p, n) = f(i as f32 / u as f32, j as f32 / v as f32);
                self.vertices.push(Vertex {
                    p,
                    n: n.unit(),
                    albedo,
                    part,
                });
            }
        }
        for j in 0..v {
            for i in 0..u {
                let a = start + j * (u + 1) + i;
                self.faces.push([a, a + 1, a + u + 1]);
                self.faces.push([a + 1, a + u + 2, a + u + 1]);
            }
        }
    }
    fn ellipsoid(&mut self, c: V, r: V, albedo: f32) {
        self.surface(36, 18, 0, albedo, |u, v| {
            let (s, t) = (u * TAU, v * PI);
            let n = V(t.sin() * s.cos(), t.cos(), t.sin() * s.sin());
            (
                c.plus(V(n.0 * r.0, n.1 * r.1, n.2 * r.2)),
                V(n.0 / r.0, n.1 / r.1, n.2 / r.2),
            )
        });
    }
    fn rounded_box(&mut self, c: V, r: V, round: f32, albedo: f32) {
        // Six curved cube faces preserve broad flat panels and smooth corner normals.
        for axis in 0..3 {
            for sign in [-1., 1.] {
                self.surface(12, 8, 0, albedo, |u, v| {
                    let rr = [r.0, r.1, r.2];
                    let mut p = [0.; 3];
                    p[axis] = rr[axis] * sign;
                    p[(axis + 1) % 3] = (u * 2. - 1.) * rr[(axis + 1) % 3];
                    p[(axis + 2) % 3] = (v * 2. - 1.) * rr[(axis + 2) % 3];
                    let q = std::array::from_fn::<_, 3, _>(|i| {
                        p[i].clamp(-rr[i] + round, rr[i] - round)
                    });
                    let n = V(p[0] - q[0], p[1] - q[1], p[2] - q[2]).unit();
                    (c.plus(V(q[0], q[1], q[2]).plus(n.scaled(round))), n)
                });
            }
        }
    }
}
pub fn load(key: &str) -> Mesh {
    // Keychron permits personal CAD derivatives but not their redistribution.
    if key == "keychron-k8" {
        use std::io::Read;
        let path = crate::config::Config::path()
            .parent()
            .unwrap()
            .join("models/keychron-k8.mesh");
        if let Ok(file) = std::fs::File::open(path) {
            let mut bytes = Vec::new();
            if file.take(12 * 1024 * 1024).read_to_end(&mut bytes).is_ok()
                && let Some(mesh) = parse_mesh(&bytes, "Keychron · local CAD", false)
            {
                return mesh;
            }
        }
    }
    match key {
        "airpods-pro" => {
            return embedded(
                include_bytes!("../../assets/control/airpods.mesh"),
                "JY_JY_ · CC BY 4.0",
                true,
            );
        }
        "sony-xm5" => {
            return embedded(
                include_bytes!("../../assets/control/sony-xm5.mesh"),
                "Zalaneke · CC BY 4.0",
                false,
            );
        }
        "mx-master-3s" => {
            return embedded(
                include_bytes!("../../assets/control/mx-master-3s.mesh"),
                "Guibazilla · CC BY 4.0",
                false,
            );
        }
        _ => {}
    }
    let mut m = Mesh::new("Approximate model");
    match key {
        "keychron-k8" | "aula-f99" => {
            let aula = key == "aula-f99";
            m.rounded_box(V(0., -0.12, 0.), V(1.65, 0.16, 0.65), 0.08, 0.38);
            let cols = if aula { 19 } else { 18 };
            for row in 0..6 {
                for col in 0..cols {
                    if !aula
                        && ((14..15).contains(&col)
                            || (row == 4 && col > 13)
                            || (row == 5 && col > 13 && col != 16))
                    {
                        continue;
                    }
                    if row == 5 && (4..9).contains(&col) {
                        continue;
                    }
                    let wide = if row == 5 && col == 3 { 0.48 } else { 0.065 };
                    let x = if row == 5 && col == 3 {
                        -0.44
                    } else {
                        -1.49 + col as f32 * 0.165
                    };
                    m.rounded_box(
                        V(x, 0.1, -0.52 + row as f32 * 0.20),
                        V(wide, 0.065, 0.078),
                        0.025,
                        if col == 0 && row == 0 { 0.95 } else { 0.64 },
                    );
                }
            }
            if aula {
                m.ellipsoid(V(1.43, 0.18, -0.48), V(0.1, 0.10, 0.1), 0.95);
            }
        }
        "mouse" => {
            m.ellipsoid(V(0., -0.12, 0.08), V(0.62, 0.20, 1.08), 0.3);
            m.ellipsoid(V(0., 0.10, 0.24), V(0.57, 0.49, 0.88), 0.65);
            for x in [-0.28, 0.28] {
                m.rounded_box(V(x, 0.21, -0.58), V(0.235, 0.1, 0.39), 0.08, 0.68);
            }
            m.ellipsoid(V(0., 0.34, -0.65), V(0.075, 0.11, 0.21), 0.98);
            m.rounded_box(V(0., 0.50, -0.13), V(0.06, 0.012, 0.07), 0.01, 0.25);
        }
        "iphone" | "ipad-pro" => {
            let tablet = key == "ipad-pro";
            let w = if tablet { 0.98 } else { 0.65 };
            let h = 1.35;
            m.rounded_box(V(0., 0., 0.), V(w, h, 0.085), 0.08, 0.75);
            m.rounded_box(
                V(0., 0., 0.083),
                V(w - 0.045, h - 0.045, 0.015),
                0.013,
                0.10,
            );
            m.rounded_box(
                V(0., h - 0.09, 0.106),
                V(if tablet { 0.04 } else { 0.20 }, 0.035, 0.012),
                0.011,
                0.02,
            );
            m.rounded_box(
                V(-w + 0.27, h - 0.27, -0.11),
                V(0.23, 0.23, 0.045),
                0.04,
                0.4,
            );
            for (x, y) in [
                (-w + 0.16, h - 0.15),
                (-w + 0.16, h - 0.38),
                (-w + 0.39, h - 0.27),
            ] {
                m.ellipsoid(V(x, y, -0.16), V(0.088, 0.088, 0.03), 0.09);
            }
            m.source = "Approximate model".into();
        }
        "buds-fe" => {
            m.rounded_box(V(0., -0.65, 0.), V(0.85, 0.30, 0.65), 0.25, 0.85);
            m.rounded_box(V(0., -0.10, -0.55), V(0.84, 0.45, 0.13), 0.12, 0.70);
            for x in [-0.8, 0.8] {
                m.ellipsoid(V(x, 0.65, 0.), V(0.32, 0.38, 0.29), 0.65);
                m.ellipsoid(V(x * 0.81, 0.46, 0.16), V(0.14, 0.14, 0.20), 0.20);
                m.rounded_box(V(x, 0.70, 0.25), V(0.22, 0.24, 0.06), 0.05, 0.45);
            }
        }
        _ => m.rounded_box(V(0., 0., 0.), V(0.85, 1., 0.25), 0.2, 0.7),
    }
    m
}
fn embedded(bytes: &[u8], source: &str, airpods: bool) -> Mesh {
    parse_mesh(bytes, source, airpods).expect("validated bundled mesh")
}
fn parse_mesh(bytes: &[u8], source: &str, airpods: bool) -> Option<Mesh> {
    if bytes.len() < 12 || &bytes[..4] != b"OSM1" {
        return None;
    }
    let u32_at = |i| u32::from_le_bytes(bytes[i..i + 4].try_into().unwrap());
    let float = |i| f32::from_le_bytes(bytes[i..i + 4].try_into().unwrap());
    let nv = u32_at(4) as usize;
    let nf = u32_at(8) as usize;
    if nv == 0 || nv > 200_000 || nf > 300_000 || bytes.len() != 12 + nv * 32 + nf * 12 {
        return None;
    }
    let mut m = Mesh::new(source);
    m.airpods = airpods;
    for i in 0..nv {
        let o = 12 + i * 32;
        let vertex = Vertex {
            p: V(float(o), float(o + 4), float(o + 8)),
            n: V(float(o + 12), float(o + 16), float(o + 20)),
            albedo: float(o + 24),
            part: u32_at(o + 28),
        };
        if [
            vertex.p.0,
            vertex.p.1,
            vertex.p.2,
            vertex.n.0,
            vertex.n.1,
            vertex.n.2,
            vertex.albedo,
        ]
        .iter()
        .any(|v| !v.is_finite() || v.abs() > 8.)
            || vertex.part > 3
            || vertex.p.dot(vertex.p) > 16.
        {
            return None;
        }
        m.vertices.push(vertex);
    }
    for i in 0..nf {
        let o = 12 + nv * 32 + i * 12;
        let face = [
            u32_at(o) as usize,
            u32_at(o + 4) as usize,
            u32_at(o + 8) as usize,
        ];
        if face.iter().any(|&index| index >= nv) {
            return None;
        }
        m.faces.push(face);
    }
    Some(m)
}
