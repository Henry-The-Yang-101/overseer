use ratatui::style::Color;
pub const NAMES: &[&str] = &["holo", "phosphor", "amber", "mono", "neon-pcb", "contrast"];
#[derive(Clone, Debug)]
pub struct Theme {
    pub name: &'static str,
    pub void: Color,
    pub panel: Color,
    pub substrate: Color,
    pub wire: Color,
    pub accent: Color,
    pub bright: Color,
    pub text: Color,
    pub muted: Color,
    pub cpu: Color,
    pub gpu: Color,
    pub memory: Color,
    pub ane: Color,
    pub disk: Color,
    pub net: Color,
    pub battery: Color,
    pub ok: Color,
    pub warn: Color,
    pub crit: Color,
    pub synthetic: Color,
}
pub const fn rgb(v: u32) -> Color {
    Color::Rgb((v >> 16) as u8, (v >> 8) as u8, v as u8)
}
pub fn blend(a: Color, b: Color, t: f32) -> Color {
    let (Color::Rgb(ar, ag, ab), Color::Rgb(br, bg, bb)) = (a, b) else {
        return if t < 0.5 { a } else { b };
    };
    let f = |a: u8, b: u8| (a as f32 + (b as f32 - a as f32) * t.clamp(0., 1.)).round() as u8;
    Color::Rgb(f(ar, br), f(ag, bg), f(ab, bb))
}
impl Theme {
    pub fn get(name: &str) -> Self {
        let mut t = Self {
            name: "holo",
            void: rgb(0x070c10),
            panel: rgb(0x0a1219),
            substrate: rgb(0x152a36),
            wire: rgb(0x284b5d),
            accent: rgb(0x54d6ee),
            bright: rgb(0xd0f5ff),
            text: rgb(0xd5e5eb),
            muted: rgb(0x88a5b5),
            cpu: rgb(0x66dec0),
            gpu: rgb(0xa2a7ff),
            memory: rgb(0x71bcf2),
            ane: rgb(0xe5a0d2),
            disk: rgb(0xbaabe8),
            net: rgb(0xb1d988),
            battery: rgb(0x75d7ac),
            ok: rgb(0x75d7ac),
            warn: rgb(0xffbf69),
            crit: rgb(0xff6e83),
            synthetic: rgb(0xede09a),
        };
        match name {
            "phosphor" => {
                t.name = "phosphor";
                t.void = rgb(0x020702);
                t.panel = rgb(0x061009);
                t.accent = rgb(0x54ed7b);
                t.text = rgb(0xc7f5cf);
                t.muted = rgb(0x89b995);
                t.wire = rgb(0x285d35);
                t.substrate = rgb(0x102919);
                t.monochrome();
                t.warn = rgb(0xe8ff59);
            }
            "amber" => {
                t.name = "amber";
                t.void = rgb(0x0a0600);
                t.panel = rgb(0x151005);
                t.accent = rgb(0xffbb52);
                t.text = rgb(0xffe2a8);
                t.muted = rgb(0xc1a377);
                t.wire = rgb(0x6a4c21);
                t.substrate = rgb(0x2c200e);
                t.monochrome();
                t.warn = rgb(0xfff5d0);
                t.crit = rgb(0xff6846);
            }
            "mono" => {
                t.name = "mono";
                t.void = rgb(0x080808);
                t.panel = rgb(0x111111);
                t.accent = rgb(0xeeeeee);
                t.text = rgb(0xdddddd);
                t.muted = rgb(0x999999);
                t.wire = rgb(0x444444);
                t.substrate = rgb(0x222222);
                t.monochrome();
                t.warn = rgb(0xffffff);
            }
            "neon-pcb" => {
                t.name = "neon-pcb";
                t.void = rgb(0x04140c);
                t.panel = rgb(0x082116);
                t.substrate = rgb(0x183b27);
                t.wire = rgb(0x805c35);
                t.accent = rgb(0xdda366);
                t.text = rgb(0xe5f0e8);
                t.muted = rgb(0x9db2a4);
            }
            "contrast" => {
                t.name = "contrast";
                t.void = rgb(0x000000);
                t.panel = rgb(0x000000);
                t.wire = rgb(0x999999);
                t.accent = rgb(0x70efff);
                t.text = rgb(0xffffff);
                t.muted = rgb(0xc9c9c9);
                t.substrate = rgb(0x333333);
            }
            _ => {}
        }
        t
    }
    fn monochrome(&mut self) {
        self.cpu = self.accent;
        self.gpu = blend(self.accent, self.text, 0.25);
        self.memory = blend(self.accent, self.text, 0.5);
        self.ane = self.gpu;
        self.disk = self.memory;
        self.net = self.accent;
        self.battery = self.accent;
        self.ok = self.text;
        self.bright = self.text;
    }
    pub fn heat(&self, c: f32) -> Color {
        let t = ((c - 30.) / 75.).clamp(0., 1.);
        if t < 0.55 {
            blend(self.wire, self.accent, t / 0.55)
        } else if t < 0.8 {
            blend(self.accent, self.warn, (t - 0.55) / 0.25)
        } else {
            blend(self.warn, self.crit, (t - 0.8) / 0.2)
        }
    }
}
pub fn quantize(c: Color) -> Color {
    thread_local! { static CACHE:std::cell::RefCell<std::collections::HashMap<Color,Color>>=std::cell::RefCell::new(std::collections::HashMap::new()); }
    CACHE.with(|cache| {
        let mut cache = cache.borrow_mut();
        if let Some(v) = cache.get(&c) {
            return *v;
        }
        let value = quantize_uncached(c);
        if cache.len() > 4096 {
            cache.clear();
        }
        cache.insert(c, value);
        value
    })
}
fn quantize_uncached(c: Color) -> Color {
    let Color::Rgb(r, g, b) = c else { return c };
    let mut best = (0, u32::MAX);
    for i in 16u16..256 {
        let (rr, gg, bb) = if i < 232 {
            let x = i - 16;
            let level = |n: u16| if n == 0 { 0 } else { 55 + n * 40 };
            (level(x / 36), level(x / 6 % 6), level(x % 6))
        } else {
            let v = 8 + (i - 232) * 10;
            (v, v, v)
        };
        let d = (r as i32 - rr as i32).pow(2)
            + (g as i32 - gg as i32).pow(2)
            + (b as i32 - bb as i32).pow(2);
        if (d as u32) < best.1 {
            best = (i, d as u32);
        }
    }
    Color::Indexed(best.0 as u8)
}
