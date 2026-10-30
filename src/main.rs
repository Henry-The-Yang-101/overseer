use clap::Parser;
use crossterm::{
    event::{
        self, DisableFocusChange, DisableMouseCapture, EnableFocusChange, EnableMouseCapture,
        Event, KeyEventKind,
    },
    execute,
};
use overseer::{app::App, config::Config, metrics, motion::Motion, ui};
use ratatui::{Terminal, backend::TestBackend};
use std::{
    io::{self, IsTerminal, Write},
    path::PathBuf,
    time::{Duration, Instant},
};
#[derive(Parser, Debug)]
#[command(
    version,
    about = "Overseer · a live hardware observatory for your terminal"
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
    #[arg(long, help = "Use explicitly labelled synthetic telemetry")]
    demo: bool,
    #[arg(long,default_value="balanced",value_parser=clap::builder::PossibleValuesParser::new(metrics::SCENARIOS))]
    scenario: String,
    #[arg(long)]
    ascii: bool,
    #[arg(long)]
    no_animation: bool,
    #[arg(long)]
    no_boot: bool,
    #[arg(long)]
    debug: bool,
    #[arg(long,value_parser=["teardown","systems"])]
    view: Option<String>,
    #[arg(long, value_parser=["cpu","memory","energy","disk","network","gpu"], help="Open a Systems resource page")]
    page: Option<String>,
    #[arg(long,value_parser=clap::builder::PossibleValuesParser::new(ui::theme::NAMES))]
    theme: Option<String>,
    #[arg(
        long,
        help = "Show hardware availability and glyph coverage, then exit"
    )]
    probe: bool,
    #[arg(long, help = "Export a frame to .ansi, .txt or .svg (requires --demo)")]
    snapshot: Option<PathBuf>,
    #[arg(long, default_value = "160x46")]
    size: String,
    #[arg(long, default_value_t = 179.0)]
    at: f64,
    #[arg(long, help = "Start teardown with telemetry cells")]
    cells: bool,
    #[arg(long, help = "Collect one real telemetry sample as JSON, then exit")]
    sample: bool,
}
#[derive(clap::Subcommand, Debug)]
enum Command {
    /// Agent collection and hook transport, independent of the hardware TUI.
    Agents(overseer::agents::cli::Cli),
}
fn main() -> anyhow::Result<()> {
    let c = Cli::parse();
    if let Some(Command::Agents(agents)) = c.command {
        return overseer::agents::cli::run(agents);
    }
    let mut cfg = Config::load()?;
    if let Some(t) = c.theme {
        cfg.ui.theme = t;
    }
    if let Some(v) = c.view {
        cfg.ui.default_view = v;
    }
    if let Some(page) = c.page {
        cfg.systems.page = page;
        cfg.ui.default_view = "systems".into();
    }
    if c.ascii {
        cfg.ui.glyphs = "ascii".into();
    }
    if c.no_animation {
        cfg.ui.motion = "off".into();
    }
    if c.no_boot {
        cfg.ui.show_boot = false;
    }
    if c.probe || c.sample {
        let rx = metrics::spawn_collector();
        let s = rx.recv_timeout(Duration::from_secs(20))?;
        if let Some(e) = &s.error {
            eprintln!("{e}");
        }
        if c.sample {
            println!("{}", serde_json::to_string_pretty(&s)?);
        } else {
            println!(
                "OVERSEER / SENSOR PROBE\n{} · {}\n",
                s.machine.chip, s.machine.model
            );
            for (k, v) in [
                (
                    "CPU topology",
                    format!("{} cores / {} tiers", s.core_count(), s.tiers.len()),
                ),
                ("GPU", s.gpu.reason().into()),
                ("Temperature", s.cpu_temp.reason().into()),
                ("System power", s.system_power.reason().into()),
                ("Fans", s.fans.reason().into()),
                ("Battery", s.battery.reason().into()),
                ("Physical ports", s.ports.reason().into()),
                ("Memory pressure", s.pressure.reason().into()),
            ] {
                println!("{k:20} {v}");
            }
            println!(
                "\nGlyphs: ╭─╮ ┏━┓ ░▒▓█ ⣀⣤⣶⣿ ◈ ◇ ϟ ⇄\nIf these misalign, use --ascii. JetBrains Mono and Iosevka work well."
            );
        }
        return Ok(());
    }
    let mut app = App::new(cfg, c.demo.then_some(c.scenario.clone()));
    app.debug = c.debug;
    app.cells = c.cells;
    if let Some(path) = c.snapshot {
        anyhow::ensure!(
            c.demo,
            "--snapshot requires --demo so exported data is reproducible"
        );
        let (w, h) = c
            .size
            .split_once('x')
            .ok_or_else(|| anyhow::anyhow!("size must be WIDTHxHEIGHT"))?;
        let (w, h): (u16, u16) = (w.parse()?, h.parse()?);
        anyhow::ensure!(
            w > 0 && h > 0 && w <= 500 && h <= 200,
            "size must be within 1x1 and 500x200"
        );
        app.history.clear();
        for i in 0..=c.at.clamp(0., 3600.) as usize {
            app.ingest(metrics::demo(&c.scenario, i as f64));
        }
        app.motion = Motion::Off;
        app.boot = false;
        app.cfg.ui.onboarding_seen = true;
        app.toasts.clear();
        app.persist = false;
        let mut terminal = Terminal::new(TestBackend::new(w, h))?;
        terminal.draw(|f| ui::draw(f, &mut app, Duration::ZERO))?;
        let b = terminal.backend().buffer();
        let text = match path.extension().and_then(|s| s.to_str()) {
            Some("svg") => ui::export_svg(b),
            Some("txt") => (0..h)
                .map(|y| (0..w).map(|x| b[(x, y)].symbol()).collect::<String>())
                .collect::<Vec<_>>()
                .join("\n"),
            _ => tachyonfx::buffer_to_ansi_string(b, true),
        };
        std::fs::write(&path, text)?;
        println!("Saved {}", path.display());
        return Ok(());
    }
    anyhow::ensure!(
        io::stdin().is_terminal() && io::stdout().is_terminal(),
        "Overseer needs an interactive terminal. Try --demo --snapshot preview.svg or --probe."
    );
    if let Ok(ws) = crossterm::terminal::window_size()
        && ws.width > 0
        && ws.height > 0
        && ws.columns > 0
        && ws.rows > 0
    {
        app.cell_aspect =
            (ws.height as f32 / ws.rows as f32) / (ws.width as f32 / ws.columns as f32);
    }
    let rx = if !c.demo {
        Some(metrics::spawn_collector())
    } else {
        None
    };
    let mouse = app.cfg.ui.mouse;
    let old = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = execute!(io::stdout(), DisableMouseCapture, DisableFocusChange);
        ratatui::restore();
        old(info);
    }));
    let mut terminal = ratatui::init();
    let _guard = Restore;
    execute!(io::stdout(), EnableFocusChange)?;
    if mouse {
        execute!(io::stdout(), EnableMouseCapture)?;
    }
    let mut last = Instant::now();
    let mut demo_tick = Instant::now();
    let mut dirty = true;
    let mut last_sample = -1.;
    let mut ui_tick = Instant::now();
    loop {
        if let Some(rx) = &rx {
            while let Ok(s) = rx.try_recv() {
                if app.demo.is_none() {
                    app.ingest(s);
                    dirty = true;
                }
            }
        }
        if app.demo.is_some() && demo_tick.elapsed() >= Duration::from_secs(1) {
            let s = metrics::demo(app.demo.as_ref().unwrap(), app.latest_time() + 1.);
            app.ingest(s);
            demo_tick = Instant::now();
            dirty = true;
        }
        let now = Instant::now();
        let dt = now - last;
        last = now;
        app.tick(dt);
        let time = app.sample().at;
        if (time - last_sample).abs() > 0.001 {
            dirty = true;
            last_sample = time;
        }
        // Status aging, asynchronous inspectors, and toast expiry still redraw with motion off.
        if ui_tick.elapsed() >= Duration::from_secs(1) {
            dirty = true;
            ui_tick = Instant::now();
        }
        if dirty || app.motion == Motion::Full || app.boot {
            let began = Instant::now();
            terminal.draw(|f| ui::draw(f, &mut app, dt))?;
            app.render_ms = began.elapsed().as_secs_f64() * 1000.;
            dirty = false;
        }
        let wait = Duration::from_secs_f32(1. / app.fps).saturating_sub(now.elapsed());
        if event::poll(wait)? {
            match event::read()? {
                Event::Key(k) if k.kind == KeyEventKind::Press => {
                    app.on_key(k);
                    dirty = true;
                }
                Event::Mouse(m) => {
                    app.on_mouse(m);
                    dirty = true;
                }
                Event::Resize(..) => dirty = true,
                Event::FocusLost => app.focused = false,
                Event::FocusGained => {
                    app.focused = true;
                    dirty = true;
                }
                _ => {}
            }
        }
        if app.quit {
            break;
        }
    }
    drop(_guard);
    if app.persist {
        app.cfg.save()?;
    }
    Ok(())
}
struct Restore;
impl Drop for Restore {
    fn drop(&mut self) {
        let _ = execute!(io::stdout(), DisableMouseCapture, DisableFocusChange);
        ratatui::restore();
        let _ = io::stdout().flush();
    }
}
