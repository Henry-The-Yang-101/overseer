//! Rendering/retention stress test; use a release build. No hardware collection or audio.
use clap::Parser;
use overseer::{
    app::{Action, App, Layer, Page, Screen},
    config::Config,
    metrics::{self, Avail, extra},
    motion::Motion,
    ui,
};
use ratatui::{Terminal, backend::TestBackend};
use std::{
    sync::Arc,
    time::{Duration, Instant},
};

#[derive(Parser)]
struct Args {
    #[arg(long, default_value_t = 725, value_parser = clap::value_parser!(u32).range(1..=10000))]
    processes: u32,
    #[arg(long, default_value_t = 2, value_parser = clap::value_parser!(u32).range(1..=24))]
    hours: u32,
    #[arg(long, default_value_t = 120, value_parser = clap::value_parser!(u32).range(1..))]
    frames: u32,
}

#[cfg(target_os = "macos")]
fn heap_usage() {
    // Layout and null-zone semantics from the macOS SDK's malloc/malloc.h.
    #[repr(C)]
    #[derive(Default)]
    struct Statistics {
        blocks_in_use: libc::c_uint,
        size_in_use: usize,
        max_size_in_use: usize,
        size_allocated: usize,
    }
    unsafe extern "C" {
        fn malloc_zone_statistics(zone: *mut libc::c_void, stats: *mut Statistics);
    }
    let mut stats = Statistics::default();
    // SAFETY: null sums all zones; stats has the C layout and is writable for the call.
    unsafe { malloc_zone_statistics(std::ptr::null_mut(), &mut stats) };
    println!(
        "  malloc live {:.1} MiB; reserved {:.1} MiB; {} live blocks",
        stats.size_in_use as f64 / 1048576.,
        stats.size_allocated as f64 / 1048576.,
        stats.blocks_in_use
    );
}

fn memory(app: &App, hour: u32) {
    let mut sys = sysinfo::System::new().unwrap();
    let pid = sysinfo::get_current_pid().unwrap();
    sys.refresh_processes_specifics(
        sysinfo::ProcessesToUpdate::Some(&[pid]),
        true,
        sysinfo::ProcessRefreshKind::nothing().with_memory(),
    );
    println!(
        "hour {hour}: RSS {:.1} MiB; retained samples {}; process snapshots {}; extra snapshots {}",
        sys.process(pid).unwrap().memory() as f64 / 1024. / 1024.,
        app.history.len(),
        app.history
            .iter()
            .map(|s| Arc::as_ptr(&s.processes))
            .collect::<std::collections::HashSet<_>>()
            .len(),
        app.history
            .iter()
            .map(|s| Arc::as_ptr(&s.extra))
            .collect::<std::collections::HashSet<_>>()
            .len(),
    );
    #[cfg(target_os = "macos")]
    heap_usage();
}

fn measure(label: &str, app: &mut App, terminal: &mut Terminal<TestBackend>, frames: u32) {
    terminal.draw(|f| ui::draw(f, app, Duration::ZERO)).unwrap();
    let start = Instant::now();
    for _ in 0..frames {
        terminal
            .draw(|f| ui::draw(f, app, Duration::from_millis(33)))
            .unwrap();
    }
    println!(
        "{label}: {:.3} ms/frame",
        start.elapsed().as_secs_f64() * 1000. / frames as f64
    );
}

fn main() {
    let args = Args::parse();
    let mut cfg = Config::default();
    cfg.ui.onboarding_seen = true;
    cfg.ui.motion = "off".into();
    let mut app = App::new(cfg, Some("balanced".into()));
    app.motion = Motion::Off;
    let seed = metrics::demo("balanced", 0.);
    let mut procs = Vec::new();
    for i in 0..args.processes {
        let mut p = seed.processes[i as usize % seed.processes.len()].clone();
        p.pid = i + 100;
        p.start = i as u64;
        p.parent = 1;
        procs.push(p);
    }
    let mut shared = Arc::new(procs);
    let mut extra = Arc::new(extra::Snapshot::default());
    let start = Instant::now();
    for time in 180..=180 + 3600 * args.hours {
        if time % 5 == 0 {
            shared = Arc::new(shared.as_ref().clone());
            extra = Arc::new(extra::Snapshot {
                at: time as f64,
                traffic: Avail::Ok(
                    shared
                        .iter()
                        .map(|p| extra::Traffic {
                            pid: p.pid,
                            start: p.start,
                            read: p.pid as f64 * 10.,
                            write: p.pid as f64,
                        })
                        .collect(),
                ),
                ..extra::Snapshot::default()
            });
        }
        let mut s = metrics::demo("balanced", time as f64);
        s.processes = shared.clone();
        s.process_at = (time / 5 * 5) as f64;
        s.extra = extra.clone();
        app.ingest(s);
        if time > 180 && (time - 180) % 3600 == 0 {
            memory(&app, (time - 180) / 3600);
        }
    }
    println!(
        "synthetic ingest: {:.3} ms/sample (includes fixture generation)",
        start.elapsed().as_secs_f64() * 1000. / (3600 * args.hours + 1) as f64
    );
    println!(
        "{} processes; 200x55 cells; {} frames/case; motion off; TestBackend excludes terminal painting and collectors",
        args.processes, args.frames
    );
    app.toasts.clear();
    let mut terminal = Terminal::new(TestBackend::new(200, 55)).unwrap();
    app.screen = Screen::Teardown;
    measure("teardown", &mut app, &mut terminal, args.frames);
    for page in Page::ALL {
        app.act(Action::GoPage(page));
        measure(page.name(), &mut app, &mut terminal, args.frames);
        let start = Instant::now();
        for _ in 0..args.frames {
            app.refresh_rows(true);
        }
        println!(
            "{} / rebuild task rows: {:.3} ms/refresh",
            page.name(),
            start.elapsed().as_secs_f64() * 1000. / args.frames as f64
        );
        app.act(Action::SystemsInfo);
        for (tab, name) in ["stats", "history", "apps"].iter().enumerate() {
            if let Some(Layer::SystemsInfo { tab: current, .. }) = app.layers.last_mut() {
                *current = tab;
            }
            measure(
                &format!("{} / {name}", page.name()),
                &mut app,
                &mut terminal,
                args.frames,
            );
        }
        app.act(Action::Back);
    }
    app.act(Action::GoPage(Page::Cpu));
    app.search = Some("safari".into());
    let start = Instant::now();
    for _ in 0..args.frames {
        app.refresh_rows(true);
    }
    println!(
        "CPU / search rows: {:.3} ms/refresh",
        start.elapsed().as_secs_f64() * 1000. / args.frames as f64
    );
}
