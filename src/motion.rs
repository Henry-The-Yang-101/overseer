use crate::metrics::Sample;
use std::collections::HashMap;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Motion {
    Full,
    Reduced,
    Off,
}
impl Motion {
    pub fn parse(s: &str) -> Self {
        match s {
            "reduced" => Self::Reduced,
            "off" => Self::Off,
            _ => Self::Full,
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            Self::Full => "full",
            Self::Reduced => "reduced",
            Self::Off => "off",
        }
    }
    pub fn next(self) -> Self {
        match self {
            Self::Full => Self::Reduced,
            Self::Reduced => Self::Off,
            Self::Off => Self::Full,
        }
    }
}
#[derive(Default, Clone, Copy, Debug)]
pub struct Spring {
    pub pos: f32,
    vel: f32,
}
impl Spring {
    pub fn step(&mut self, target: f32, dt: f32, smooth: f32) {
        let omega = 2. / smooth.max(0.001);
        let x = omega * dt;
        let exp = 1. / (1. + x + 0.48 * x * x + 0.235 * x * x * x);
        let change = self.pos - target;
        let temp = (self.vel + omega * change) * dt;
        self.vel = (self.vel - omega * temp) * exp;
        self.pos = target + (change + temp) * exp;
    }
    pub fn snap(&mut self, v: f32) {
        self.pos = v;
        self.vel = 0.;
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Default)]
pub enum Mood {
    #[default]
    Asleep,
    Idle,
    Light,
    Busy,
    High,
    Frantic,
}
#[derive(Default, Clone, Debug)]
pub struct MoodTracker {
    pub mood: Mood,
    since: f64,
    hot_since: Option<f64>,
}
impl MoodTracker {
    pub fn update(&mut self, a: f32, now: f64) -> Mood {
        self.hot_since = if a >= 0.90 {
            self.hot_since.or(Some(now))
        } else {
            None
        };
        let mut target = Mood::Asleep;
        for (entry, m) in [
            (0.70, Mood::High),
            (0.40, Mood::Busy),
            (0.15, Mood::Light),
            (0.03, Mood::Idle),
        ] {
            let exit = if m == Mood::Idle { 0.01 } else { entry - 0.04 };
            if a >= if self.mood >= m { exit } else { entry } {
                target = m;
                break;
            }
        }
        if self.hot_since.is_some_and(|t| now - t >= 5.) {
            target = Mood::Frantic;
        }
        if target != self.mood && now - self.since >= 0.8 {
            self.mood = target;
            self.since = now;
        }
        self.mood
    }
}
pub fn face(label: &str, m: Mood) -> &'static str {
    match (label.starts_with('P'), m) {
        (true, Mood::Asleep) => "[-_-]",
        (false, Mood::Asleep) => "(-_-)",
        (true, Mood::Busy) => "[>_>]",
        (false, Mood::Busy) => "(>_>)",
        (true, Mood::High) => "[@_@]",
        (false, Mood::High) => "(@_@)",
        (true, Mood::Frantic) => "[@O@]",
        (false, Mood::Frantic) => "(@O@)",
        (true, _) => "[o_o]",
        _ => "(o_o)",
    }
}
pub fn visual_rev_per_sec(rpm: u32, max: Option<u32>, blades: u32, fps: f32) -> f32 {
    if rpm == 0 {
        return 0.;
    }
    (0.2 + 1.8 * (rpm as f32 / max.unwrap_or(5700).max(1) as f32).min(1.))
        .min(0.45 * fps / blades.max(1) as f32)
}
#[derive(Default)]
pub struct Anim {
    pub energy: Spring,
    pub cpu: Spring,
    pub gpu: Spring,
    pub phases: HashMap<String, f32>,
    pub moods: HashMap<String, MoodTracker>,
}
impl Anim {
    pub fn ingest(&mut self, s: &Sample) {
        for t in &s.tiers {
            for c in &t.cores {
                self.moods
                    .entry(format!("{}:{}:{}", t.label, c.die, c.id))
                    .or_default()
                    .update(c.active, s.at);
            }
        }
    }
    pub fn tick(&mut self, s: &Sample, dt: f32, motion: Motion, live: bool, fps: f32) {
        if motion != Motion::Full || !live {
            self.cpu.snap(s.cpu);
            self.gpu.snap(*s.gpu.value().unwrap_or(&0.));
            self.energy.snap(s.energy());
        } else {
            self.cpu.step(s.cpu, dt, 0.4);
            self.gpu.step(*s.gpu.value().unwrap_or(&0.), dt, 0.4);
            self.energy.step(s.energy(), dt, 3.);
        }
        if motion == Motion::Reduced
            && let Some(fans) = s.fans.value()
        {
            for f in fans {
                self.phases.insert(
                    f.name.clone(),
                    if f.rpm > 0 {
                        (s.at.floor() as f32 * 0.035).fract()
                    } else {
                        0.
                    },
                );
            }
        }
        if motion != Motion::Full || !live {
            return;
        }
        // Decorative signal motion also runs at idle; it does not represent throughput.
        let ambient = self.phases.entry("ambient".into()).or_default();
        *ambient = (*ambient + dt / 8.).fract();
        if s.gpu.value().is_some_and(|v| *v > 0.) {
            let p = self.phases.entry("gpu".into()).or_default();
            *p = (*p + s.gpu_mhz.value().copied().unwrap_or(0) as f32 / 1400. * dt)
                .rem_euclid(std::f32::consts::TAU);
        }

        if let Some(fans) = s.fans.value() {
            for f in fans {
                let p = self.phases.entry(f.name.clone()).or_default();
                *p = (*p + visual_rev_per_sec(f.rpm, f.max, 9, fps) * dt).fract();
            }
        }
        for t in &s.tiers {
            for c in &t.cores {
                let p = self
                    .phases
                    .entry(format!("{}:{}:{}", t.label, c.die, c.id))
                    .or_default();
                *p =
                    (*p + (4. + 14. * c.mhz as f32 / t.max_mhz.max(1) as f32) * dt).rem_euclid(32.);
            }
        }
        let p = self.phases.entry("power".into()).or_default();
        *p = (*p + 14. * dt).rem_euclid(1000.);
    }
}
