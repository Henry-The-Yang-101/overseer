use crate::{
    config::Config,
    metrics::{self, Avail, Proc, Sample},
    motion::{Anim, Motion},
    ui::{
        kit::{Hit, HitMap},
        theme::{NAMES, Theme},
    },
};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::{Position, Rect};
use std::{
    collections::{HashMap, HashSet, VecDeque},
    sync::Arc,
    time::{Duration, Instant},
};
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Screen {
    Teardown,
    Systems,
}
impl Screen {
    pub fn name(self) -> &'static str {
        match self {
            Self::Teardown => "teardown",
            Self::Systems => "systems",
        }
    }
    pub fn parse(s: &str) -> Self {
        if matches!(s, "systems" | "processes") {
            Self::Systems
        } else {
            Self::Teardown
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Page {
    Cpu,
    Memory,
    Energy,
    Disk,
    Network,
    Gpu,
}
impl Page {
    pub const ALL: [Self; 6] = [
        Self::Cpu,
        Self::Memory,
        Self::Energy,
        Self::Disk,
        Self::Network,
        Self::Gpu,
    ];
    pub fn index(self) -> usize {
        Self::ALL.iter().position(|p| *p == self).unwrap()
    }
    pub fn name(self) -> &'static str {
        ["cpu", "memory", "energy", "disk", "network", "gpu"][self.index()]
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::Disk => "storage",
            _ => self.name(),
        }
    }
    pub fn title(self) -> &'static str {
        [
            "EXECUTION FLOOR",
            "ADDRESS MATRIX",
            "POWER FLOW",
            "NAND ARRAY",
            "SIGNAL TRACES",
            "SHADER FABRIC",
        ][self.index()]
    }
    pub fn part(self) -> Part {
        [
            Part::Cpu,
            Part::Memory,
            Part::Power,
            Part::Disk,
            Part::Network,
            Part::Gpu,
        ][self.index()]
    }
    pub fn parse(s: &str) -> Self {
        Self::ALL
            .into_iter()
            .find(|p| p.name() == s)
            .unwrap_or(Self::Cpu)
    }
    pub fn step(self, n: i32) -> Self {
        Self::ALL[(self.index() as i32 + n).rem_euclid(6) as usize]
    }
    pub fn for_part(p: Part) -> Self {
        match p {
            Part::Gpu | Part::Ane => Self::Gpu,
            Part::Memory => Self::Memory,
            Part::Disk => Self::Disk,
            Part::Network => Self::Network,
            Part::Cpu | Part::Soc => Self::Cpu,
            _ => Self::Energy,
        }
    }
    pub fn default_sort(self) -> usize {
        [0, 1, 5, 6, 7, 8][self.index()]
    }
}
#[derive(Clone, Default)]
pub struct TaskListState {
    pub sort: usize,
    pub reverse: bool,
    pub tree: bool,
    pub collapsed: HashSet<u32>,
    pub selected: Option<(u32, u64)>,
    pub scroll: usize,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Part {
    Cpu,
    Gpu,
    Ane,
    Power,
    Memory,
    Disk,
    Network,
    Thermal,
    Battery,
    Fan0,
    Fan1,
    Soc,
    SpeakerLeft,
    SpeakerRight,
    Cell(u8),
}
impl Part {
    pub fn selection_group(self) -> Self {
        if self == Self::Fan1 { Self::Fan0 } else { self }
    }
    pub fn selection_name(self) -> &'static str {
        if self.selection_group() == Self::Fan0 {
            "Fans"
        } else {
            self.name()
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            Self::Cpu => "CPU complex",
            Self::Gpu => "GPU fabric",
            Self::Ane => "Neural engine",
            Self::Power => "Power rails",
            Self::Memory => "Unified memory",
            Self::Disk => "Storage",
            Self::Network => "Network",
            Self::Thermal => "Thermal system",
            Self::Battery => "Battery pack",
            Self::Fan0 => "Fan 0",
            Self::Fan1 => "Fan 1",
            Self::Soc => "System on chip",
            Self::SpeakerLeft => "Left speaker",
            Self::SpeakerRight => "Right speaker",
            Self::Cell(_) => "Telemetry cell",
        }
    }
    pub fn parse(s: &str) -> Self {
        match s {
            "gpu" => Self::Gpu,
            "ane" => Self::Ane,
            "power" => Self::Power,
            "memory" => Self::Memory,
            "storage" => Self::Disk,
            "network" => Self::Network,
            "thermal" => Self::Thermal,
            "battery" => Self::Battery,
            _ => Self::Cpu,
        }
    }
    pub fn note(self) -> &'static str {
        match self {
            Self::SpeakerLeft | Self::SpeakerRight => {
                "Stereo system output, including headphones and external outputs. Waveforms are not a microphone or a measurement of individual speaker drivers. Nothing is saved. Toggle capture for both channels in this inspector."
            }
            Self::Cpu | Self::Soc => {
                "Instruction lanes are symbolic: density follows active residency, speed follows clock frequency. Process-to-core mapping is not measurable."
            }
            Self::Gpu => {
                "Lit tiles encode aggregate GPU residency. Tiles and the framebuffer are symbolic, not individual GPU cores or screen contents. Process GPU rates use driver-reported GPU time and may exceed 100%."
            }
            Self::Ane => {
                "The lattice encodes measured ANE watts, not individual neural-engine operations."
            }
            Self::Power => {
                "Σ is CPU + GPU + ANE only. DRAM and GPU SRAM are shown separately. System power is the SMC PSTR reading; unavailable is never zero."
            }
            Self::Memory => {
                "Aggregate VM categories, not physical addresses. Live app/cache categories are derived estimates. Pressure is the macOS level, not a percentage inferred from used RAM."
            }
            Self::Disk => {
                "Root volume throughput and capacity. APFS volumes share storage; they are not added together. IOPS are not exposed by this collector."
            }
            Self::Network => {
                "The busiest physical interface. Receive/transmit rates use elapsed sample time. Wi-Fi signal and per-process network attribution are not exposed."
            }
            Self::Fan0 | Self::Fan1 => {
                "Rotor speed is scaled and capped to prevent strobing. The RPM number is measured."
            }
            Self::Thermal => {
                "Pipe color follows the maximum measured CPU/GPU temperature. No spatial temperature gradient is inferred. Thermal state comes from macOS."
            }
            Self::Battery => {
                "Segments represent pack charge, not individual cells. Per-cell readings are unavailable. Health and time remaining are estimates."
            }
            Self::Cell(_) => {
                "Cells are display slots, not per-cell battery data. The displayed subsystem is configurable in the command palette."
            }
        }
    }
}
pub const METRIC_PARTS: &[Part] = &[
    Part::Cpu,
    Part::Gpu,
    Part::Ane,
    Part::Power,
    Part::Memory,
    Part::Disk,
    Part::Network,
    Part::Thermal,
    Part::Battery,
];
pub const TEAR_PARTS: &[Part] = &[
    Part::Fan0,
    Part::Soc,
    Part::Memory,
    Part::Disk,
    Part::Power,
    Part::Battery,
    Part::Thermal,
    Part::Network,
    Part::SpeakerLeft,
    Part::SpeakerRight,
];
#[derive(Clone, Copy, Debug)]
pub enum Dir {
    Left,
    Right,
    Up,
    Down,
}
#[derive(Clone, Debug)]
pub enum Action {
    Quit,
    Help,
    Ports,
    SystemsInfo,
    Palette,
    GoTo(Screen),
    ToggleView,
    GoPage(Page),
    OpenPartPage,
    Collapse(bool),
    PageRows(i32),
    Loopback,
    Next(i32),
    Move(Dir),
    Inspect,
    InspectPart(Part),
    ZoomPart(Part),
    SetWindow(usize),
    Zoom,
    Back,
    Search,
    Sort,
    Reverse,
    Tree,
    Follow,
    Signal,
    PlayPause,
    Step(i32),
    Spike(i32),
    Window(i32),
    Speed(i32),
    Live,
    Theme,
    Motion,
    Cells,
    Labels,
    Highlight,
    Mirror,
    Ascii,
    SetTheme(usize),
    Demo(String),
    Assign(usize, Part),
}
#[derive(Clone, Debug)]
pub struct Command {
    pub name: String,
    pub key: &'static str,
    pub action: Action,
}
pub fn commands() -> Vec<Command> {
    let mut c = vec![];
    for (name, key, action) in [
        ("Open Teardown", "1", Action::GoTo(Screen::Teardown)),
        ("Open Systems", "2", Action::GoTo(Screen::Systems)),
        (
            "Open selected component in Systems",
            "g",
            Action::OpenPartPage,
        ),
        ("Show / hide loopback interfaces", "", Action::Loopback),
        ("Search processes", "/", Action::Search),
        ("Show keyboard controls", "?", Action::Help),
        ("Show connected physical ports", "p", Action::Ports),
        ("Pause / play history", "Space", Action::PlayPause),
        ("Return to live", "End", Action::Live),
        ("Next history spike", "}", Action::Spike(1)),
        ("Previous history spike", "{", Action::Spike(-1)),
        ("Next history window", "]", Action::Window(1)),
        ("Increase replay speed", "+", Action::Speed(1)),
        ("Cycle motion level", "a", Action::Motion),
        ("Toggle ASCII glyphs", "", Action::Ascii),
        ("Toggle telemetry cells", "m", Action::Cells),
        ("Toggle component labels", "L", Action::Labels),
        ("Toggle activity highlights", "H", Action::Highlight),
        ("Mirror teardown orientation", "o", Action::Mirror),
        ("Sort processes", "s", Action::Sort),
        ("Reverse process order", "S", Action::Reverse),
        ("Toggle process tree", "T", Action::Tree),
        ("Follow selected process", "f", Action::Follow),
        ("Send signal to selected process", "X", Action::Signal),
        ("Quit Overseer", "q", Action::Quit),
    ] {
        c.push(Command {
            name: name.into(),
            key,
            action,
        });
    }
    for page in Page::ALL {
        c.push(Command {
            name: format!("Systems: {}", page.label().to_uppercase()),
            key: "← →",
            action: Action::GoPage(page),
        });
    }
    for (i, n) in NAMES.iter().enumerate() {
        c.push(Command {
            name: format!("Theme: {n}"),
            key: "t",
            action: Action::SetTheme(i),
        });
    }
    for p in METRIC_PARTS {
        c.push(Command {
            name: format!("Inspect {}", p.name()),
            key: "Enter",
            action: Action::InspectPart(*p),
        });
    }
    for p in [
        Part::Fan0,
        Part::Fan1,
        Part::Soc,
        Part::SpeakerLeft,
        Part::SpeakerRight,
    ] {
        c.push(Command {
            name: format!("Inspect {}", p.name()),
            key: "Enter",
            action: Action::InspectPart(p),
        });
    }
    for p in METRIC_PARTS
        .iter()
        .copied()
        .chain([Part::Fan0, Part::Fan1, Part::Soc])
    {
        c.push(Command {
            name: format!("Zoom {}", p.name()),
            key: "z",
            action: Action::ZoomPart(p),
        });
    }
    for (i, name) in ["1m", "5m", "15m", "1h"].iter().enumerate() {
        c.push(Command {
            name: format!("Window: {name}"),
            key: "[ ]",
            action: Action::SetWindow(i),
        });
    }
    for s in metrics::SCENARIOS {
        c.push(Command {
            name: format!("Demo: {s}"),
            key: "",
            action: Action::Demo((*s).into()),
        });
    }
    for cell in 0..6 {
        for p in METRIC_PARTS {
            c.push(Command {
                name: format!("Assign cell {} to {}", cell + 1, p.name()),
                key: "",
                action: Action::Assign(cell, *p),
            });
        }
    }
    c
}
pub fn map_key(k: KeyEvent, s: Screen) -> Option<Action> {
    use KeyCode::*;
    Some(match k.code {
        Char('c') if k.modifiers.contains(KeyModifiers::CONTROL) => Action::Quit,
        Char('q') => Action::Quit,
        Char('?') => Action::Help,
        Char('p') => Action::Ports,
        Char(':') => Action::Palette,
        Char('/') => Action::Search,
        Char('1') => Action::GoTo(Screen::Teardown),
        Char('2') => Action::GoTo(Screen::Systems),
        Char('g') if s == Screen::Teardown => Action::OpenPartPage,
        Left if s == Screen::Systems && k.modifiers.contains(KeyModifiers::SHIFT) => {
            Action::Collapse(true)
        }
        Right if s == Screen::Systems && k.modifiers.contains(KeyModifiers::SHIFT) => {
            Action::Collapse(false)
        }
        PageUp if s == Screen::Systems => Action::PageRows(-1),
        PageDown if s == Screen::Systems => Action::PageRows(1),
        Char('v') => Action::ToggleView,
        Tab => Action::Next(1),
        BackTab => Action::Next(-1),
        Left | Char('h') => Action::Move(Dir::Left),
        Right | Char('l') => Action::Move(Dir::Right),
        Up | Char('k') => Action::Move(Dir::Up),
        Down | Char('j') => Action::Move(Dir::Down),
        Enter => Action::Inspect,
        Char('z') => Action::Zoom,
        Esc => Action::Back,
        Char(' ') => Action::PlayPause,
        Char(',') => Action::Step(-1),
        Char('.') => Action::Step(1),
        Char('<') => Action::Step(-10),
        Char('>') => Action::Step(10),
        Char('{') => Action::Spike(-1),
        Char('}') => Action::Spike(1),
        Char('[') => Action::Window(-1),
        Char(']') => Action::Window(1),
        Char('+') | Char('=') => Action::Speed(1),
        Char('-') => Action::Speed(-1),
        End => Action::Live,
        Char('t') => Action::Theme,
        Char('a') => Action::Motion,
        Char('m') if s == Screen::Teardown => Action::Cells,
        Char('L') if s == Screen::Teardown => Action::Labels,
        Char('H') if s == Screen::Teardown => Action::Highlight,
        Char('o') if s == Screen::Teardown => Action::Mirror,
        Char('s') if s == Screen::Systems => Action::Sort,
        Char('i') if s == Screen::Systems => Action::SystemsInfo,
        Char('S') if s == Screen::Systems => Action::Reverse,
        Char('T') if s == Screen::Systems => Action::Tree,
        Char('f') => Action::Follow,
        Char('X') if s == Screen::Systems => Action::Signal,
        _ => return None,
    })
}
#[derive(Clone, Debug)]
pub enum Layer {
    Help,
    Ports,
    SystemsInfo {
        tab: usize,
        scroll: usize,
        index: usize,
        expanded: HashSet<String>,
        sample: Arc<Sample>,
    },
    Palette {
        query: String,
        index: usize,
    },
    Inspector(Part),
    Process {
        process: Proc,
        tab: usize,
    },
    Signal {
        process: Proc,
        force: bool,
    },
}
#[derive(Clone, Debug)]
pub struct Toast {
    pub text: String,
    pub born: Instant,
    pub critical: bool,
    pub key: String,
}
#[derive(Clone, Debug)]
pub struct ProcessRow {
    pub process: Proc,
    pub depth: usize,
    pub exited: bool,
    pub new: bool,
}
type TaskHistory = HashMap<(u32, u64), VecDeque<(f64, [f32; 6])>>;
pub struct App {
    pub audio: crate::audio::Capture,
    pub cfg: Config,
    pub screen: Screen,
    pub page: Page,
    pub lists: [TaskListState; 6],
    pub expanded: bool,
    pub task_rect: Rect,
    pub slide_direction: i32,
    pub theme: Theme,
    pub motion: Motion,
    pub demo: Option<String>,
    pub history: VecDeque<Arc<Sample>>,
    pub empty: Arc<Sample>,
    pub cursor: Option<f64>,
    pub frozen: Option<Arc<Sample>>,
    pub inspect_core: usize,
    pub playing: bool,
    pub speed: usize,
    pub window: usize,
    pub selection: Option<Part>,
    pub selected_at: Instant,
    pub zoom: Option<Part>,
    pub layers: Vec<Layer>,
    pub search: Option<String>,
    pub searching: bool,
    pub sort: usize,
    pub reverse: bool,
    pub tree: bool,
    pub collapsed: HashSet<u32>,
    pub rows: Vec<ProcessRow>,
    pub selected_process: Option<(u32, u64)>,
    pub scroll: usize,
    pub follow: Option<Proc>,
    pub follow_exited: Option<Instant>,
    pub cells: bool,
    pub quit: bool,
    pub focused: bool,
    pub boot: bool,
    pub started: Instant,
    pub last_received: Instant,
    pub last_input: Instant,
    pub last_row_at: f64,
    pub last_click: Option<(Hit, Instant)>,
    pub hover: Option<(Hit, Instant)>,
    pub anim: Anim,
    pub donut: std::cell::RefCell<crate::ui::donut::Renderer>,
    pub hitmap: HitMap,
    pub parts: Vec<(Part, Rect)>,
    pub rail: Rect,
    pub toasts: VecDeque<Toast>,
    pub cooldown: HashMap<String, Instant>,
    pub ghosts: Vec<(Proc, Instant)>,
    pub debug: bool,
    pub render_ms: f64,
    pub fps: f32,
    pub cell_aspect: f32,
    pub fx: tachyonfx::EffectManager<u8>,
    pub transition: bool,
    pub theme_from: Option<(Theme, Instant)>,
    pub persist: bool,
    pub energy_wh: f64,
    pub spike_marks: Vec<f64>,
    pub system_events: VecDeque<metrics::insights::Event>,
    pub detail_rx: Option<std::sync::mpsc::Receiver<metrics::details::ProcessDetails>>,
    pub detail_data: Option<metrics::details::ProcessDetails>,
    pub detail_request: Option<((u32, u64), usize)>,
    pub detail_scroll: usize,
    pub task_history: TaskHistory,
    task_history_at: f64,
    archive_processes: Arc<Vec<Proc>>,
    archive_process_at: f64,
}
pub const WINDOWS: [u32; 4] = [60, 300, 900, 3600];
impl App {
    pub fn new(cfg: Config, demo: Option<String>) -> Self {
        let screen = Screen::parse(&cfg.ui.default_view);
        let theme = Theme::get(&cfg.ui.theme);
        let motion = Motion::parse(&cfg.ui.motion);
        let window = match cfg.timeline.window.as_str() {
            "1m" => 0,
            "15m" => 2,
            "1h" => 3,
            _ => 1,
        };
        let page = Page::parse(&cfg.systems.page);
        let lists = std::array::from_fn(|i| {
            let page = Page::ALL[i];
            let sort = cfg
                .systems
                .sorts
                .get(page.name())
                .map(|s| sort_index(s))
                .unwrap_or(page.default_sort());
            TaskListState {
                sort,
                ..TaskListState::default()
            }
        });
        let sort = lists[page.index()].sort;
        let now = Instant::now();
        let mut a = Self {
            boot: cfg.ui.show_boot && motion == Motion::Full,
            cfg,
            screen,
            page,
            lists,
            expanded: false,
            task_rect: Rect::default(),
            slide_direction: 1,
            theme,
            motion,
            demo,
            history: VecDeque::new(),
            empty: Arc::new(Sample::default()),
            cursor: None,
            frozen: None,
            audio: crate::audio::Capture::default(),
            inspect_core: 0,
            playing: false,
            speed: 0,
            window,
            selection: (screen == Screen::Systems).then_some(Part::Cpu),
            selected_at: now,
            zoom: None,
            layers: vec![],
            search: None,
            searching: false,
            sort,
            reverse: false,
            tree: false,
            collapsed: HashSet::new(),
            rows: vec![],
            selected_process: None,
            scroll: 0,
            follow: None,
            follow_exited: None,
            cells: false,
            quit: false,
            focused: true,
            started: now,
            last_received: now,
            last_input: now - Duration::from_secs(5),
            last_row_at: -1.,
            last_click: None,
            hover: None,
            anim: Anim::default(),
            donut: std::cell::RefCell::default(),
            hitmap: HitMap::default(),
            parts: vec![],
            rail: Rect::default(),
            toasts: VecDeque::new(),
            cooldown: HashMap::new(),
            ghosts: vec![],
            debug: false,
            render_ms: 0.,
            fps: 12.,
            cell_aspect: 2.05,
            fx: tachyonfx::EffectManager::default(),
            transition: false,
            theme_from: None,
            persist: true,
            energy_wh: 0.,
            spike_marks: vec![],
            system_events: VecDeque::new(),
            detail_rx: None,
            detail_data: None,
            detail_request: None,
            detail_scroll: 0,
            task_history: HashMap::new(),
            task_history_at: -5.,
            archive_processes: Arc::new(vec![]),
            archive_process_at: -15.,
        };
        if let Some(s) = a.demo.clone() {
            for i in 0..180 {
                a.ingest(metrics::demo(&s, i as f64));
            }
            a.boot = false;
            a.persist = false;
            a.toasts.clear();
        }
        a
    }
    pub fn sample(&self) -> Arc<Sample> {
        if let (Some(time), Some(s)) = (self.cursor, self.frozen.as_ref())
            && (time - s.at).abs() < 0.001
        {
            return s.clone();
        }

        if let Some(t) = self.cursor {
            self.history
                .iter()
                .rev()
                .find(|s| s.at <= t)
                .cloned()
                .or_else(|| self.history.front().cloned())
                .unwrap_or_else(|| self.empty.clone())
        } else {
            self.history
                .back()
                .cloned()
                .unwrap_or_else(|| self.empty.clone())
        }
    }
    pub fn latest_time(&self) -> f64 {
        self.history.back().map_or(0., |s| s.at)
    }
    pub fn ingest(&mut self, mut s: Sample) {
        if let Some(e) = &s.error {
            self.toast("collector", e.clone(), false);
            if s.machine.chip.is_empty() {
                return;
            }
        }
        if let Some(prev) = self.history.back() {
            self.system_events
                .extend(metrics::insights::events(prev, &s));
            let dt = (s.at - prev.at).clamp(0., 5.);
            if let Some(w) = s.system_power.value() {
                self.energy_wh += *w as f64 * dt / 3600.;
            }
        }
        if matches!(
            s.thermal.value().map(String::as_str),
            Some("SERIOUS" | "CRITICAL")
        ) {
            self.toast(
                "thermal",
                format!("Thermal state {}", s.thermal.value().unwrap()),
                true,
            );
        } else {
            self.toasts.retain(|t| t.key != "thermal");
        }
        if let Some(b) = s.battery.value() {
            if b.percent <= 10. && !b.ac {
                self.toast(
                    "battery",
                    format!("Battery low · {:.0}% remaining", b.percent),
                    b.percent <= 5.,
                );
            } else {
                self.toasts.retain(|t| t.key != "battery");
            }
        }
        for (key, v) in [
            ("CPU temperature", &s.cpu_temp),
            ("System power", &s.system_power),
        ] {
            if let metrics::Avail::Failed(e) = v {
                self.toast(key, format!("{key}: {e}"), false);
            }
        }
        if let Some(f) = self.follow.as_ref() {
            if let Some(p) = s.processes.iter().find(|p| p.key() == f.key()) {
                self.follow = Some(p.clone());
                self.follow_exited = None;
            } else if self.follow_exited.is_none() {
                self.follow_exited = Some(Instant::now());
                self.toast("follow", "Followed process exited".into(), false);
            }
        }
        self.record_task_history(&s);
        s.energy_wh = self.energy_wh;
        self.anim.ingest(&s);
        self.last_received = Instant::now();
        let v = s.cpu.max(s.gpu.value().copied().unwrap_or(0.));
        let prior: Vec<_> = self
            .history
            .iter()
            .rev()
            .take(60)
            .map(|s| s.cpu.max(s.gpu.value().copied().unwrap_or(0.)))
            .collect();
        if prior.len() >= 10 {
            let mean = prior.iter().sum::<f32>() / prior.len() as f32;
            let sd =
                (prior.iter().map(|v| (v - mean).powi(2)).sum::<f32>() / prior.len() as f32).sqrt();
            if v > 0.5
                && v > mean + 2.5 * sd
                && self
                    .spike_marks
                    .last()
                    .is_none_or(|last| s.at - last >= 10.)
            {
                self.spike_marks.push(s.at);
            }
        }
        // Retain full process tables every 15s; the newest live table stays at 5s cadence.
        // Shared metadata and Arc snapshots keep one hour of history bounded.
        if let Some(prev) = self.history.back_mut() {
            let prev = Arc::make_mut(prev);
            prev.processes = self.archive_processes.clone();
            prev.process_at = self.archive_process_at.max(0.);
        }
        if s.at - self.archive_process_at >= 15. || self.archive_processes.is_empty() {
            self.archive_processes = s.processes.clone();
            self.archive_process_at = s.process_at;
        }
        self.history.push_back(Arc::new(s));
        let retention = match self.cfg.timeline.retention.as_str() {
            "1m" => 60.,
            "5m" => 300.,
            "15m" => 900.,
            _ => 3600.,
        };
        while self.history.len() > 3601
            || self
                .history
                .front()
                .is_some_and(|first| self.latest_time() - first.at > retention)
        {
            self.history.pop_front();
        }
        self.spike_marks
            .retain(|t| *t >= self.history.front().map_or(0., |s| s.at));
        self.system_events
            .retain(|e| e.at >= self.history.front().map_or(0., |s| s.at));
        if let Some(c) = self.cursor
            && let Some(first) = self.history.front()
            && c < first.at
        {
            self.cursor = Some(first.at);
            self.toast("history", "Oldest retained sample reached".into(), false);
        }
        if self.last_input.elapsed() > Duration::from_millis(1500) || self.rows.is_empty() {
            self.refresh_rows(false);
        }
    }
    pub fn toast(&mut self, key: &str, text: String, critical: bool) {
        if self
            .cooldown
            .get(key)
            .is_some_and(|t| t.elapsed() < Duration::from_secs(60))
        {
            return;
        }
        self.cooldown.insert(key.into(), Instant::now());
        self.toasts.push_back(Toast {
            text,
            born: Instant::now(),
            critical,
            key: key.into(),
        });
        while self.toasts.len() > 3 {
            self.toasts.pop_front();
        }
    }
    pub fn info(&mut self, s: String) {
        self.toasts.retain(|t| t.key != "info");
        self.toasts.push_back(Toast {
            text: s,
            born: Instant::now(),
            critical: false,
            key: "info".into(),
        });
    }
    pub fn tick(&mut self, dt: Duration) {
        self.audio
            .update(self.cfg.teardown.audio_enabled && self.demo.is_none());
        if let Some(rx) = &self.detail_rx
            && let Ok(data) = rx.try_recv()
        {
            self.detail_data = Some(data);
            self.detail_rx = None;
        }
        if let Some(Layer::Process { process, tab }) = self.layers.last() {
            if *tab >= 3
                && self.demo.is_none()
                && self.cursor.is_none()
                && self.detail_request != Some((process.key(), *tab))
            {
                self.detail_rx = Some(metrics::details::request(process.clone(), *tab));
                self.detail_request = Some((process.key(), *tab));
                self.detail_data = None;
                self.detail_scroll = 0;
            }
        } else {
            self.detail_request = None;
        }

        if self.boot && !self.history.is_empty() && self.started.elapsed().as_secs_f32() > 1.6 {
            self.boot = false;
            self.transition = true;
        }
        if self.playing
            && let Some(c) = self.cursor
        {
            let n = c + dt.as_secs_f64() * (1u32 << self.speed) as f64;
            if n >= self.latest_time() {
                self.cursor = None;
                self.playing = false;
            } else {
                self.cursor = Some(n);
            }
        }
        self.toasts
            .retain(|t| t.critical || t.born.elapsed() < Duration::from_secs(4));
        self.ghosts
            .retain(|(_, t)| t.elapsed() < Duration::from_secs(2));
        if self
            .follow_exited
            .is_some_and(|t| t.elapsed() > Duration::from_secs(30))
        {
            self.follow = None;
            self.follow_exited = None;
        }
        let s = self.sample();
        self.fps = if self.motion == Motion::Off {
            2.
        } else if !self.focused || self.motion == Motion::Reduced {
            4.
        } else if self.cfg.teardown.audio_enabled
            && self.cursor.is_none()
            && (self.screen == Screen::Teardown
                || matches!(self.zoom, Some(Part::SpeakerLeft | Part::SpeakerRight))
                || matches!(
                    self.layers.last(),
                    Some(Layer::Inspector(Part::SpeakerLeft | Part::SpeakerRight))
                ))
        {
            60.
        } else if (self.screen == Screen::Systems && self.page == Page::Gpu)
            || s.energy() > 0.08
            || s.fans.value().is_some_and(|v| v.iter().any(|f| f.rpm > 0))
            || self.fx.is_running()
        {
            self.cfg.ui.fps as f32
        } else {
            12.
        };
        self.anim.tick(
            &s,
            dt.as_secs_f32().min(0.25),
            self.motion,
            self.cursor.is_none(),
            self.fps,
        );
        if self.last_input.elapsed() > Duration::from_millis(1500) {
            self.refresh_rows(false);
        }
    }
    fn record_task_history(&mut self, s: &Sample) {
        if s.process_at - self.task_history_at < 5. {
            return;
        }
        self.task_history_at = s.process_at;
        let max_io = s
            .processes
            .iter()
            .map(|p| p.read + p.write)
            .fold(1., f64::max);
        let max_net = s
            .processes
            .iter()
            .filter_map(|p| p.connections.value())
            .copied()
            .max()
            .unwrap_or(1)
            .max(1) as f32;
        let score = |p: &Proc| {
            (p.cpu / (s.core_count() as f32 * 100.))
                .max(p.gpu.value().copied().unwrap_or(0.) / 100.)
                .max(p.memory as f32 / s.machine.memory.max(1) as f32)
                .max(((p.read + p.write) / max_io) as f32)
                .max(p.connections.value().copied().unwrap_or(0) as f32 / max_net)
        };
        let mut candidates: Vec<_> = s.processes.iter().collect();
        candidates.sort_by(|a, b| score(b).total_cmp(&score(a)).then(a.pid.cmp(&b.pid)));
        candidates.truncate(64);
        if let Some(f) = self
            .follow
            .as_ref()
            .and_then(|f| s.processes.iter().find(|p| p.key() == f.key()))
            && !candidates.iter().any(|p| p.key() == f.key())
        {
            candidates.push(f);
        }
        for p in candidates {
            let values = std::array::from_fn(|i| {
                if Page::ALL[i] == Page::Gpu {
                    p.gpu.value().copied().unwrap_or(f32::NAN)
                } else {
                    crate::ui::processes::primary(p, Page::ALL[i], s)
                }
            });
            let points = self.task_history.entry(p.key()).or_default();
            points.push_back((s.process_at, values));
            while points.len() > 24 {
                points.pop_front();
            }
        }
        self.task_history
            .retain(|_, points| points.back().is_some_and(|p| s.at - p.0 <= 60.));
    }
    pub fn task_values(&self, p: &Proc, page: Page) -> Vec<f32> {
        let s = self.sample();
        if let Some(points) = self.task_history.get(&p.key()) {
            let points: Vec<_> = points
                .iter()
                .filter(|(at, _)| *at <= s.at && *at >= s.at - 120.)
                .map(|(_, v)| v[page.index()])
                .filter(|v| v.is_finite())
                .collect();
            if points.len() > 1 {
                return points;
            }
        }
        let mut last = -1.;
        self.history
            .iter()
            .filter(|h| h.at <= s.at && h.at >= s.at - 120.)
            .filter(|h| {
                let changed = h.process_at != last;
                last = h.process_at;
                changed
            })
            .filter_map(|h| {
                h.processes
                    .iter()
                    .find(|v| v.key() == p.key())
                    .and_then(|p| {
                        if page == Page::Gpu {
                            p.gpu.value().copied()
                        } else {
                            Some(crate::ui::processes::primary(p, page, h))
                        }
                    })
            })
            .collect()
    }
    pub fn preview_proc(&self) -> Option<Proc> {
        if self.screen == Screen::Systems {
            self.selected_proc().or_else(|| self.displayed_follow())
        } else {
            self.displayed_follow()
        }
    }
    pub fn displayed_follow(&self) -> Option<Proc> {
        let f = self.follow.as_ref()?;
        self.sample()
            .processes
            .iter()
            .find(|p| p.key() == f.key())
            .cloned()
    }
    pub fn history_values(&self, part: Part) -> Vec<f32> {
        let end = self.sample().at;
        self.history
            .iter()
            .filter(|s| s.at >= end - 60. && s.at <= end && metric_available(part, s))
            .map(|s| metric(part, s))
            .collect()
    }
    pub fn select(&mut self, p: Part) {
        self.selection = Some(p.selection_group());
        self.selected_at = Instant::now();
    }
    pub fn part_order(&self) -> Vec<Part> {
        if self.screen != Screen::Teardown {
            METRIC_PARTS.to_vec()
        } else if self.cells {
            (0..6)
                .map(Part::Cell)
                .chain([Part::Fan0, Part::Soc, Part::SpeakerLeft, Part::SpeakerRight])
                .collect()
        } else {
            TEAR_PARTS.to_vec()
        }
    }
    pub fn resolve(&self, p: Part) -> Part {
        if let Part::Cell(i) = p {
            self.cfg
                .teardown
                .cells
                .get(i as usize)
                .map(|s| Part::parse(s))
                .unwrap_or(Part::Cpu)
        } else {
            p
        }
    }
    pub fn move_part(&mut self, d: Dir) {
        if let Some((_, r)) = self.parts.iter().find(|(p, _)| Some(*p) == self.selection) {
            let candidates: Vec<_> = self
                .parts
                .iter()
                .copied()
                .filter(|(p, _)| Some(p.selection_group()) != self.selection)
                .collect();
            if let Some(p) = neighbor(*r, d, &candidates, self.cell_aspect) {
                self.select(p);
            }
        } else {
            self.select(self.part_order()[0]);
        }
    }
    pub fn change_page(&mut self, page: Page) {
        if page == self.page {
            return;
        }
        self.lists[self.page.index()] = TaskListState {
            sort: self.sort,
            reverse: self.reverse,
            tree: self.tree,
            collapsed: self.collapsed.clone(),
            selected: self.selected_process,
            scroll: self.scroll,
        };
        self.slide_direction = if self.page.step(-1) == page { -1 } else { 1 };
        self.page = page;
        self.cfg.systems.page = page.name().into();
        let state = &self.lists[page.index()];
        self.sort = state.sort;
        self.reverse = state.reverse;
        self.tree = state.tree;
        self.collapsed = state.collapsed.clone();
        self.selected_process = state.selected;
        self.scroll = state.scroll;
        self.rows.clear();
        self.ghosts.clear();
        self.last_row_at = -1.;
        self.refresh_rows(true);
        self.transition = true;
    }
    pub fn move_row(&mut self, n: i32) {
        if self.rows.is_empty() {
            return;
        }
        let i = self
            .rows
            .iter()
            .position(|r| Some(r.process.key()) == self.selected_process)
            .map(|i| i as i32)
            .unwrap_or(if n > 0 { -1 } else { 1 });
        let j = (i + n).clamp(0, self.rows.len() as i32 - 1) as usize;
        self.selected_process = Some(self.rows[j].process.key());
    }
    pub fn selected_proc(&self) -> Option<Proc> {
        self.rows
            .iter()
            .find(|r| Some(r.process.key()) == self.selected_process && !r.exited)
            .map(|r| r.process.clone())
    }
    pub fn back(&mut self) {
        if self.layers.pop().is_some() {
            return;
        }
        if self.search.as_ref().is_some_and(|s| !s.is_empty()) {
            self.search = Some(String::new());
            self.refresh_rows(true);
            return;
        }
        if self.search.take().is_some() {
            self.searching = false;
            return;
        }
        if self.expanded {
            self.expanded = false;
            return;
        }
        if self.zoom.take().is_some() {
            return;
        }
        if self.screen == Screen::Systems && self.selected_process.take().is_some() {
            return;
        }
        if self.screen == Screen::Teardown && self.selection.take().is_some() {
            return;
        }
        if self.follow.take().is_some() {
            self.follow_exited = None;
            return;
        }
        if self.cursor.take().is_some() {
            self.playing = false;
            self.transition = true;
            self.refresh_rows(true);
        }
    }
    pub fn act(&mut self, a: Action) {
        match a {
            Action::Quit => self.quit = true,
            Action::Back => self.back(),
            Action::Help => self.layers.push(Layer::Help),
            Action::Ports => self.layers.push(Layer::Ports),
            Action::SystemsInfo => {
                if self.screen == Screen::Systems {
                    self.layers.push(Layer::SystemsInfo {
                        tab: 0,
                        scroll: 0,
                        index: 0,
                        expanded: HashSet::new(),
                        sample: self.sample(),
                    });
                }
            }
            Action::Palette => self.layers.push(Layer::Palette {
                query: String::new(),
                index: 0,
            }),
            Action::GoTo(s) => {
                self.screen = s;
                self.zoom = None;
                self.selection = (s == Screen::Systems).then_some(Part::Cpu);
                self.transition = true;
            }
            Action::ToggleView => self.act(Action::GoTo(if self.screen == Screen::Teardown {
                Screen::Systems
            } else {
                Screen::Teardown
            })),
            Action::GoPage(page) => {
                self.act(Action::GoTo(Screen::Systems));
                self.change_page(page);
            }
            Action::OpenPartPage => {
                if let Some(part) = self.selection {
                    self.act(Action::GoPage(Page::for_part(self.resolve(part))));
                }
            }
            Action::Loopback => self.cfg.systems.show_loopback = !self.cfg.systems.show_loopback,
            Action::PageRows(n) => {
                self.move_row(n * self.task_rect.height.saturating_sub(4).max(1) as i32)
            }
            Action::Collapse(collapse) => {
                if let Some(p) = self.selected_process {
                    if collapse {
                        self.collapsed.insert(p.0);
                    } else {
                        self.collapsed.remove(&p.0);
                    }
                    self.refresh_rows(true);
                }
            }
            Action::Next(n) => {
                if self.screen == Screen::Systems {
                    self.change_page(self.page.step(n));
                } else {
                    let p = self.part_order();
                    let i = p
                        .iter()
                        .position(|p| Some(*p) == self.selection)
                        .map_or(if n > 0 { -1 } else { 0 }, |i| i as i32);
                    self.select(p[(i + n).rem_euclid(p.len() as i32) as usize]);
                    if self.zoom.is_some() {
                        self.zoom = self.selection;
                    }
                }
            }
            Action::Move(d) => {
                if self.screen == Screen::Systems {
                    match d {
                        Dir::Up => {
                            if !self.expanded {
                                self.move_row(-1);
                            }
                        }
                        Dir::Down => {
                            if !self.expanded {
                                self.move_row(1);
                            }
                        }
                        Dir::Left => self.change_page(self.page.step(-1)),
                        Dir::Right => self.change_page(self.page.step(1)),
                    }
                } else if self.zoom.is_some() {
                    self.act(Action::Next(if matches!(d, Dir::Left | Dir::Up) {
                        -1
                    } else {
                        1
                    }));
                } else {
                    self.move_part(d);
                }
            }
            Action::Inspect => {
                if self.screen == Screen::Systems {
                    if self.expanded {
                        self.layers.push(Layer::Inspector(self.page.part()));
                        return;
                    }
                    if let Some(p) = self.selected_proc() {
                        self.layers.push(Layer::Process { process: p, tab: 0 });
                    }
                } else if let Some(part) = self.selection {
                    self.layers.push(Layer::Inspector(part));
                }
            }
            Action::InspectPart(p) => {
                self.select(p);
                self.layers.push(Layer::Inspector(p.selection_group()));
            }
            Action::ZoomPart(p) => {
                self.select(p);
                self.zoom = self.selection;
                self.transition = true;
            }
            Action::SetWindow(i) => {
                self.window = i.min(3);
                self.cfg.timeline.window = ["1m", "5m", "15m", "1h"][self.window].into();
            }
            Action::Zoom => {
                if self.screen == Screen::Systems {
                    self.expanded = !self.expanded;
                } else if let Some(part) = self.selection {
                    self.zoom = Some(part);
                    self.transition = true;
                }
            }
            Action::Search => {
                self.screen = Screen::Systems;
                self.search.get_or_insert_with(String::new);
                self.expanded = false;
                self.searching = true;
                self.refresh_rows(true);
            }
            Action::Sort => {
                let mut options = vec![];
                for sort in [self.page.default_sort(), 0, 1, 4, 3, 2] {
                    if !options.contains(&sort) {
                        options.push(sort);
                    }
                }
                let i = options.iter().position(|s| *s == self.sort).unwrap_or(0);
                self.sort = options[(i + 1) % options.len()];
                self.cfg.systems.sorts.insert(
                    self.page.name().into(),
                    SORT_NAMES[self.sort].to_lowercase(),
                );
                self.refresh_rows(true);
            }
            Action::Reverse => {
                self.reverse = !self.reverse;
                self.refresh_rows(true);
            }
            Action::Tree => {
                self.tree = !self.tree;
                self.refresh_rows(true);
            }
            Action::Follow => {
                if let Some(p) = self.selected_proc() {
                    if self.follow.as_ref().is_some_and(|f| f.key() == p.key()) {
                        self.follow = None;
                    } else {
                        self.follow = Some(p);
                        self.follow_exited = None;
                    }
                } else {
                    self.info("Select a task with ↓ in Systems first".into());
                }
            }
            Action::Signal => {
                if self.demo.is_some() {
                    self.info("Signals are disabled in synthetic demos".into());
                } else if self.cursor.is_some() {
                    self.info("Signals are disabled during history replay".into());
                } else if let Some(p) = self.selected_proc() {
                    if p.pid <= 1 || p.pid == std::process::id() || p.name.as_ref() == "kernel_task"
                    {
                        self.info("This process is protected".into());
                    } else {
                        self.layers.push(Layer::Signal {
                            process: p,
                            force: false,
                        });
                    }
                }
            }
            Action::PlayPause => {
                if self.cursor.is_none() || self.playing {
                    let snapshot = self.sample();
                    self.cursor = Some(snapshot.at);
                    self.frozen = Some(snapshot);
                    self.playing = false;
                } else {
                    self.frozen = None;
                    self.playing = true;
                }
                self.refresh_rows(true);
            }
            Action::Step(n) => {
                self.frozen = None;
                let i = self
                    .history
                    .iter()
                    .rposition(|s| s.at <= self.sample().at)
                    .unwrap_or(0);
                let next =
                    (i as i32 + n).clamp(0, self.history.len().saturating_sub(1) as i32) as usize;
                self.cursor = Some(self.history.get(next).map_or(0., |s| s.at));
                self.playing = false;
                self.refresh_rows(true);
            }
            Action::Spike(d) => {
                self.frozen = None;
                let t = self.sample().at;
                let spikes = self.spikes();
                let next = if d < 0 {
                    spikes.into_iter().rev().find(|x| *x < t - 0.5)
                } else {
                    spikes.into_iter().find(|x| *x > t + 0.5)
                };
                if let Some(t) = next {
                    self.cursor = Some(t);
                    self.playing = false;
                    self.refresh_rows(true);
                } else {
                    self.info("No more recorded spikes in that direction".into());
                }
            }
            Action::Window(d) => {
                self.window = (self.window as i32 + d).rem_euclid(4) as usize;
                self.cfg.timeline.window = ["1m", "5m", "15m", "1h"][self.window].into();
            }
            Action::Speed(d) => self.speed = (self.speed as i32 + d).clamp(0, 3) as usize,
            Action::Live => {
                self.frozen = None;
                self.cursor = None;
                self.playing = false;
                self.transition = true;
                self.refresh_rows(true);
            }
            Action::Theme => {
                let i = NAMES
                    .iter()
                    .position(|n| *n == self.theme.name)
                    .unwrap_or(0);
                self.act(Action::SetTheme((i + 1) % NAMES.len()));
            }
            Action::SetTheme(i) => {
                self.theme_from = Some((self.theme.clone(), Instant::now()));
                self.theme = Theme::get(NAMES[i % NAMES.len()]);
                self.cfg.ui.theme = self.theme.name.into();
                self.info(format!("Theme · {}", self.theme.name));
            }
            Action::Motion => {
                self.motion = self.motion.next();
                self.cfg.ui.motion = self.motion.name().into();
                self.fx = tachyonfx::EffectManager::default();
                self.info(format!("Motion · {}", self.motion.name()));
            }
            Action::Cells => {
                self.cells = !self.cells;
                self.transition = true;
            }
            Action::Labels => self.cfg.teardown.labels = !self.cfg.teardown.labels,
            Action::Highlight => {
                self.cfg.teardown.highlight_active = !self.cfg.teardown.highlight_active
            }
            Action::Mirror => {
                self.cfg.teardown.orientation = if self.cfg.teardown.orientation == "front" {
                    "back"
                } else {
                    "front"
                }
                .into()
            }
            Action::Ascii => {
                self.cfg.ui.glyphs = if self.cfg.ui.glyphs == "ascii" {
                    "unicode"
                } else {
                    "ascii"
                }
                .into()
            }
            Action::Demo(s) => {
                self.frozen = None;
                self.demo = Some(s.clone());
                self.persist = false;
                self.history.clear();
                self.spike_marks.clear();
                self.system_events.clear();
                self.archive_processes = Arc::new(vec![]);
                self.task_history.clear();
                self.task_history_at = -5.;
                self.archive_process_at = -15.;
                self.last_row_at = -1.;
                self.cursor = None;
                self.follow = None;
                self.rows.clear();
                for i in 0..180 {
                    self.ingest(metrics::demo(&s, i as f64));
                }
                self.info(format!("Synthetic scenario · {s}"));
            }
            Action::Assign(i, p) => {
                self.cfg.teardown.cells[i] = match p {
                    Part::Cpu => "cpu",
                    Part::Gpu => "gpu",
                    Part::Ane => "ane",
                    Part::Power => "power",
                    Part::Memory => "memory",
                    Part::Disk => "storage",
                    Part::Network => "network",
                    Part::Thermal => "thermal",
                    _ => "battery",
                }
                .into();
                self.info(format!("Cell {} · {}", i + 1, p.name()));
            }
        }
    }
    pub fn on_key(&mut self, k: KeyEvent) {
        self.last_input = Instant::now();
        self.cfg.ui.onboarding_seen = true;
        if self.boot {
            self.boot = false;
            return;
        }
        if k.code == KeyCode::Char('c') && k.modifiers.contains(KeyModifiers::CONTROL) {
            self.quit = true;
            return;
        }
        if let Some(layer) = self.layers.last_mut() {
            match layer {
                Layer::Signal { process, force } => {
                    match k.code {
                        KeyCode::Esc => {
                            self.layers.pop();
                        }
                        KeyCode::Up | KeyCode::Down | KeyCode::Char('j') | KeyCode::Char('k') => {
                            *force = !*force
                        }
                        KeyCode::Char(c) if (!*force && c == 'y') || (*force && c == 'Y') => {
                            let p = process.clone();
                            let f = *force;
                            self.layers.pop();
                            if self.cursor.is_none() && self.demo.is_none() {
                                self.info(match metrics::send_signal(&p, f) {
                                    Ok(()) => format!(
                                        "{} sent to {} ({})",
                                        if f { "SIGKILL" } else { "SIGTERM" },
                                        p.name,
                                        p.pid
                                    ),
                                    Err(e) => e.to_string(),
                                });
                            }
                        }
                        _ => {}
                    }
                    return;
                }
                Layer::Palette { query, index } => {
                    match k.code {
                        KeyCode::Esc => {
                            self.layers.pop();
                        }
                        KeyCode::Char(c) if !k.modifiers.contains(KeyModifiers::CONTROL) => {
                            query.push(c);
                            *index = 0;
                        }
                        KeyCode::Backspace => {
                            query.pop();
                            *index = 0;
                        }
                        KeyCode::Down => *index += 1,
                        KeyCode::Up => *index = index.saturating_sub(1),
                        KeyCode::Enter => {
                            let list = filtered_commands(query);
                            let action = list
                                .get((*index).min(list.len().saturating_sub(1)))
                                .map(|c| c.action.clone());
                            self.layers.pop();
                            if let Some(a) = action {
                                self.act(a);
                            }
                        }
                        _ => {}
                    }
                    return;
                }
                Layer::Ports => {
                    if matches!(k.code, KeyCode::Esc | KeyCode::Char('p') | KeyCode::Enter) {
                        self.layers.pop();
                    }
                    return;
                }
                Layer::SystemsInfo {
                    tab,
                    scroll,
                    index,
                    expanded,
                    sample,
                } => {
                    match k.code {
                        KeyCode::Char('[') => {
                            self.window = (self.window + 3) % 4;
                            *scroll = 0;
                        }
                        KeyCode::Char(']') => {
                            self.window = (self.window + 1) % 4;
                            *scroll = 0;
                        }
                        KeyCode::Esc | KeyCode::Char('i') => {
                            self.layers.pop();
                        }
                        KeyCode::Tab | KeyCode::Right => {
                            *tab = (*tab + 1) % 3;
                            *scroll = 0;
                        }
                        KeyCode::BackTab | KeyCode::Left => {
                            *tab = (*tab + 2) % 3;
                            *scroll = 0;
                        }
                        KeyCode::PageDown => {
                            *scroll = scroll.saturating_add(8);
                        }
                        KeyCode::PageUp => {
                            *scroll = scroll.saturating_sub(8);
                        }
                        KeyCode::Down | KeyCode::Char('j') => {
                            if *tab == 2 {
                                *index = index.saturating_add(1);
                                *scroll = usize::MAX;
                            } else {
                                *scroll = scroll.saturating_add(1);
                            }
                        }
                        KeyCode::Up | KeyCode::Char('k') => {
                            if *tab == 2 {
                                *index = index.saturating_sub(1);
                                *scroll = usize::MAX;
                            } else {
                                *scroll = scroll.saturating_sub(1);
                            }
                        }
                        KeyCode::Enter if *tab == 2 => {
                            let groups = crate::ui::insights::app_groups(sample, self.page);
                            if let Some(g) = groups.get(*index)
                                && !expanded.remove(&g.key)
                            {
                                expanded.insert(g.key.clone());
                            }
                        }
                        _ => {}
                    }
                    return;
                }
                Layer::Help => {
                    if matches!(k.code, KeyCode::Esc | KeyCode::Char('?') | KeyCode::Enter) {
                        self.layers.pop();
                    }
                    return;
                }
                Layer::Process { tab, .. } => {
                    match k.code {
                        KeyCode::Left | KeyCode::Char('h') | KeyCode::BackTab => {
                            *tab = (*tab + 4) % 5;
                            self.detail_scroll = 0;
                        }
                        KeyCode::Right | KeyCode::Char('l') | KeyCode::Tab => {
                            *tab = (*tab + 1) % 5;
                            self.detail_scroll = 0;
                        }
                        KeyCode::Down | KeyCode::Char('j') | KeyCode::Up | KeyCode::Char('k') => {
                            let n = if matches!(k.code, KeyCode::Down | KeyCode::Char('j')) {
                                1
                            } else {
                                -1
                            };
                            self.move_row(n);
                            if let Some(p) = self.selected_proc()
                                && let Some(Layer::Process { process, .. }) = self.layers.last_mut()
                            {
                                *process = p;
                            }
                            self.detail_scroll = 0;
                        }
                        KeyCode::PageDown => self.detail_scroll += 8,
                        KeyCode::PageUp => {
                            self.detail_scroll = self.detail_scroll.saturating_sub(8)
                        }
                        KeyCode::Esc => {
                            self.layers.pop();
                        }
                        _ => {}
                    }
                    return;
                }
                Layer::Inspector(p) => {
                    if matches!(p, Part::SpeakerLeft | Part::SpeakerRight)
                        && matches!(k.code, KeyCode::Char('e') | KeyCode::Enter)
                    {
                        self.cfg.teardown.audio_enabled = !self.cfg.teardown.audio_enabled;
                        self.audio
                            .update(self.cfg.teardown.audio_enabled && self.demo.is_none());
                    }
                    if matches!(p, Part::Cpu | Part::Soc) {
                        match k.code {
                            KeyCode::Left
                            | KeyCode::Up
                            | KeyCode::Char('h')
                            | KeyCode::Char('k') => {
                                self.inspect_core = self.inspect_core.saturating_sub(1)
                            }
                            KeyCode::Right
                            | KeyCode::Down
                            | KeyCode::Char('l')
                            | KeyCode::Char('j') => self.inspect_core += 1,
                            _ => {}
                        }
                    }
                    if k.code == KeyCode::Esc {
                        self.layers.pop();
                    }
                    return;
                }
            }
        }
        if self.searching {
            match k.code {
                KeyCode::Esc => self.back(),
                KeyCode::Enter => self.searching = false,
                KeyCode::Char(c) if !k.modifiers.contains(KeyModifiers::CONTROL) => {
                    self.search.as_mut().unwrap().push(c);
                    self.refresh_rows(true);
                }
                KeyCode::Backspace => {
                    self.search.as_mut().unwrap().pop();
                    self.refresh_rows(true);
                }
                KeyCode::Up => self.move_row(-1),
                KeyCode::Down => self.move_row(1),
                _ => {}
            }
            return;
        }
        if matches!(self.zoom, Some(Part::SpeakerLeft | Part::SpeakerRight))
            && matches!(k.code, KeyCode::Char('e') | KeyCode::Enter)
        {
            self.cfg.teardown.audio_enabled = !self.cfg.teardown.audio_enabled;
            self.audio
                .update(self.cfg.teardown.audio_enabled && self.demo.is_none());
            return;
        }
        if let Some(a) = map_key(k, self.screen) {
            self.act(a);
        }
    }
    pub fn on_mouse(&mut self, m: MouseEvent) {
        if !self.layers.is_empty() {
            return;
        }
        let p = Position::new(m.column, m.row);
        let hit = self.hitmap.at(p).cloned();
        match m.kind {
            MouseEventKind::Down(MouseButton::Left) => {
                if let Some(h) = hit {
                    let double = self.last_click.as_ref().is_some_and(|(old, t)| {
                        *old == h && t.elapsed() < Duration::from_millis(350)
                    });
                    self.last_click = Some((h.clone(), Instant::now()));
                    match h {
                        Hit::Part(p) => {
                            self.select(p);
                            if double {
                                self.zoom = self.selection;
                            }
                        }
                        Hit::Process(p) => {
                            self.selected_process = Some(p);
                            if double {
                                self.act(Action::Inspect);
                            }
                        }
                        Hit::Screen(s) => self.act(Action::GoTo(s)),
                        Hit::Page(p) => self.change_page(p),
                        Hit::Rail => self.scrub(m.column),
                        Hit::Ports => self.act(Action::Ports),
                        _ => {}
                    }
                }
            }
            MouseEventKind::Drag(MouseButton::Left) if self.rail.contains(p) => {
                self.scrub(m.column)
            }
            MouseEventKind::ScrollUp => {
                if self.rail.contains(p) {
                    self.act(Action::Step(-1));
                } else if self.screen == Screen::Systems {
                    self.move_row(-3);
                }
            }
            MouseEventKind::ScrollDown => {
                if self.rail.contains(p) {
                    self.act(Action::Step(1));
                } else if self.screen == Screen::Systems {
                    self.move_row(3);
                }
            }
            MouseEventKind::Moved if hit.as_ref() != self.hover.as_ref().map(|(h, _)| h) => {
                self.hover = hit.map(|h| (h, Instant::now()));
            }
            _ => {}
        }
    }
    fn scrub(&mut self, x: u16) {
        self.frozen = None;
        let end = self.latest_time();
        let first = self.history.front().map_or(end, |s| s.at);
        let begin = (end - WINDOWS[self.window] as f64).max(first);
        let f =
            x.saturating_sub(self.rail.x) as f64 / self.rail.width.saturating_sub(1).max(1) as f64;
        self.cursor = Some(begin + (end - begin) * f.clamp(0., 1.));
        self.playing = false;
        self.refresh_rows(true);
    }
    pub fn spikes(&self) -> Vec<f64> {
        self.spike_marks.clone()
    }
    pub fn refresh_rows(&mut self, force: bool) {
        let s = self.sample();
        if !force
            && (s.process_at - self.last_row_at).abs() < 0.001
            && !self.rows.iter().any(|r| r.exited)
        {
            return;
        }
        let new_snapshot = (s.process_at - self.last_row_at).abs() > 0.001;
        self.last_row_at = s.process_at;
        let old: HashMap<_, _> = self
            .rows
            .iter()
            .filter(|r| !r.exited)
            .map(|r| (r.process.key(), r.process.clone()))
            .collect();
        if self.cursor.is_none() && new_snapshot {
            for (p, t) in &old {
                if !s.processes.iter().any(|n| n.key() == *p) {
                    self.ghosts.push((t.clone(), Instant::now()));
                }
            }
        }
        let mut all: Vec<Proc> = s.processes.as_ref().clone();
        if self.page == Page::Gpu {
            all.retain(|p| {
                p.gpu.value().is_some() || matches!(p.gpu, Avail::Acquiring | Avail::Permission(_))
            });
        }
        if self.page == Page::Network {
            all.retain(|p| s.extra.traffic(p, s.at).is_some());
        }
        let sort = self.sort;
        all.sort_by(|a, b| {
            let cmp = match sort {
                0 => b.cpu.total_cmp(&a.cpu),
                1 => b.footprint.value().cmp(&a.footprint.value()),
                2 => a.pid.cmp(&b.pid),
                3 => a.name.cmp(&b.name),
                4 => b.cpu_time.cmp(&a.cpu_time),
                5 => b.cpu.total_cmp(&a.cpu),
                6 => (b.read + b.write).total_cmp(&(a.read + a.write)),
                7 => crate::ui::processes::primary(b, Page::Network, &s)
                    .total_cmp(&crate::ui::processes::primary(a, Page::Network, &s)),
                8 => b
                    .gpu
                    .value()
                    .partial_cmp(&a.gpu.value())
                    .unwrap_or(std::cmp::Ordering::Equal),
                _ => a.pid.cmp(&b.pid),
            }
            .then(a.pid.cmp(&b.pid));
            if self.reverse { cmp.reverse() } else { cmp }
        });
        let query = self.search.as_deref().unwrap_or("");
        let mut rows = vec![];
        if self.tree && query.is_empty() {
            let ids: HashSet<_> = all.iter().map(|p| p.pid).collect();
            let mut children: HashMap<u32, Vec<&Proc>> = HashMap::new();
            for p in &all {
                children
                    .entry(if ids.contains(&p.parent) && p.parent != p.pid {
                        p.parent
                    } else {
                        0
                    })
                    .or_default()
                    .push(p);
            }
            let mut visited = HashSet::new();
            tree_rows(0, 0, &children, &self.collapsed, &mut visited, &mut rows);
            for p in &all {
                if !visited.contains(&p.pid) && !ids.contains(&p.parent) {
                    rows.push(ProcessRow {
                        process: p.clone(),
                        depth: 0,
                        exited: false,
                        new: false,
                    });
                }
            }
        } else {
            for p in all {
                if process_matches(&p, query) {
                    rows.push(ProcessRow {
                        new: !old.contains_key(&p.key()) && !old.is_empty(),
                        process: p,
                        depth: 0,
                        exited: false,
                    });
                }
            }
        }
        if self.cursor.is_none() {
            for (p, t) in &self.ghosts {
                if t.elapsed() < Duration::from_secs(2) && process_matches(p, query) {
                    rows.push(ProcessRow {
                        process: p.clone(),
                        depth: 0,
                        exited: true,
                        new: false,
                    });
                }
            }
        }
        if !rows
            .iter()
            .any(|r| Some(r.process.key()) == self.selected_process)
        {
            self.selected_process = None;
        }
        self.rows = rows;
    }
}
fn tree_rows(
    id: u32,
    depth: usize,
    children: &HashMap<u32, Vec<&Proc>>,
    collapsed: &HashSet<u32>,
    visited: &mut HashSet<u32>,
    out: &mut Vec<ProcessRow>,
) {
    if depth > 32 {
        return;
    }
    if let Some(ps) = children.get(&id) {
        for p in ps {
            if visited.insert(p.pid) {
                out.push(ProcessRow {
                    process: (*p).clone(),
                    depth,
                    exited: false,
                    new: false,
                });
                if !collapsed.contains(&p.pid) {
                    tree_rows(p.pid, depth + 1, children, collapsed, visited, out);
                }
            }
        }
    }
}
pub fn fuzzy(query: &str, haystack: &str) -> Option<(u32, Vec<u32>)> {
    use nucleo_matcher::{
        Config, Matcher, Utf32Str,
        pattern::{CaseMatching, Normalization, Pattern},
    };
    let mut m = Matcher::new(Config::DEFAULT);
    let p = Pattern::parse(query, CaseMatching::Ignore, Normalization::Smart);
    let mut buf = vec![];
    let mut indices = vec![];
    p.indices(Utf32Str::new(haystack, &mut buf), &mut m, &mut indices)
        .map(|s| (s, indices))
}
pub fn process_matches(p: &Proc, q: &str) -> bool {
    let mut words = vec![];
    for w in q.split_whitespace() {
        if let Some(id) = w.strip_prefix("pid:") {
            if p.pid.to_string() != id {
                return false;
            }
        } else if let Some(u) = w.strip_prefix("user:") {
            if !p.user.to_lowercase().contains(&u.to_lowercase()) {
                return false;
            }
        } else {
            words.push(w);
        }
    }
    let q = words.join(" ");
    q.is_empty() || fuzzy(&q, &format!("{} {}", p.name, p.command)).is_some()
}
pub fn filtered_commands(query: &str) -> Vec<Command> {
    let mut c: Vec<_> = commands()
        .into_iter()
        .filter_map(|c| fuzzy(query, &c.name).map(|(score, _)| (score, c)))
        .collect();
    c.sort_by_key(|(s, _)| std::cmp::Reverse(*s));
    c.into_iter().map(|(_, c)| c).collect()
}
pub fn metric(p: Part, s: &Sample) -> f32 {
    match p {
        Part::Cpu | Part::Soc => s.cpu,
        Part::Gpu => *s.gpu.value().unwrap_or(&0.),
        Part::Memory => s.memory_ratio(),
        Part::Ane => *s.power[2].value().unwrap_or(&0.),
        Part::Power => s.total_power().value().copied().unwrap_or(0.),
        Part::Disk => s.disk.value().map_or(0., |f| (f.read + f.write) as f32),
        Part::Network => s.network.value().map_or(0., |f| (f.read + f.write) as f32),
        Part::Thermal => s.cpu_temp.value().copied().unwrap_or(0.),
        Part::Battery => s.battery.value().map_or(0., |b| b.percent),
        Part::Fan0 | Part::Fan1 => s
            .fans
            .value()
            .and_then(|f| f.get(if p == Part::Fan0 { 0 } else { 1 }))
            .map_or(0., |f| f.rpm as f32),
        Part::Cell(_) => s.cpu,
        Part::SpeakerLeft | Part::SpeakerRight => 0.,
    }
}
pub fn neighbor(from: Rect, d: Dir, parts: &[(Part, Rect)], aspect: f32) -> Option<Part> {
    let center = |r: Rect| {
        (
            r.x as f32 + r.width as f32 / 2.,
            (r.y as f32 + r.height as f32 / 2.) * aspect,
        )
    };
    let o = center(from);
    parts
        .iter()
        .filter_map(|(id, r)| {
            let p = center(*r);
            let (dx, dy) = (p.0 - o.0, p.1 - o.1);
            let (along, across) = match d {
                Dir::Left => (-dx, dy),
                Dir::Right => (dx, dy),
                Dir::Up => (-dy, dx),
                Dir::Down => (dy, dx),
            };
            (along > 0.5).then_some((*id, along + 2. * across.abs()))
        })
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|p| p.0)
}

pub fn metric_available(p: Part, s: &Sample) -> bool {
    match p {
        Part::Cpu | Part::Soc => !s.tiers.is_empty(),
        Part::Gpu => s.gpu.value().is_some(),
        Part::Ane => s.power[2].value().is_some(),
        Part::Power => s.total_power().value().is_some(),
        Part::Memory => s.machine.memory > 0,
        Part::Disk => s.disk.value().is_some(),
        Part::Network => s.network.value().is_some(),
        Part::Thermal => s.cpu_temp.value().is_some(),
        Part::Battery => s.battery.value().is_some(),
        Part::Fan0 | Part::Fan1 => s
            .fans
            .value()
            .is_some_and(|fs| fs.len() > if p == Part::Fan0 { 0 } else { 1 }),
        Part::Cell(_) | Part::SpeakerLeft | Part::SpeakerRight => false,
    }
}

pub const SORT_NAMES: [&str; 9] = [
    "CPU", "MEMORY", "PID", "NAME", "TIME", "POWER", "IO", "NETWORK", "GPU",
];
pub fn sort_index(s: &str) -> usize {
    if s.eq_ignore_ascii_case("connections") {
        return 7;
    }
    SORT_NAMES
        .iter()
        .position(|n| n.eq_ignore_ascii_case(s))
        .unwrap_or(0)
}
