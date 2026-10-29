use overseer::{
    app::{Action, App, Screen},
    config::Config,
    metrics,
    motion::Motion,
    ui,
};
use ratatui::{Terminal, backend::TestBackend};
use std::{
    sync::Arc,
    time::{Duration, Instant},
};
fn main() {
    let mut cfg = Config::default();
    cfg.ui.onboarding_seen = true;
    cfg.ui.motion = "off".into();
    let mut app = App::new(cfg, Some("balanced".into()));
    app.motion = Motion::Off;
    let seed = metrics::demo("balanced", 0.);
    let mut procs = Vec::new();
    for i in 0..725 {
        let mut p = seed.processes[i % 20].clone();
        p.pid = i as u32 + 100;
        p.start = i as u64;
        p.parent = 1;
        procs.push(p);
    }
    let mut shared = Arc::new(procs);
    for time in 180..3781 {
        if time % 5 == 0 {
            shared = Arc::new(shared.as_ref().clone());
        }
        let mut s = metrics::demo("balanced", time as f64);
        s.processes = shared.clone();
        s.process_at = (time / 5 * 5) as f64;
        app.ingest(s);
    }
    app.toasts.clear();
    let mut terminal = Terminal::new(TestBackend::new(200, 55)).unwrap();
    for screen in [Screen::Teardown, Screen::Systems] {
        app.screen = screen;
        let start = Instant::now();
        for _ in 0..120 {
            terminal
                .draw(|f| ui::draw(f, &mut app, Duration::from_millis(33)))
                .unwrap();
        }
        println!(
            "{}: {:.3} ms/frame with one-hour history",
            screen.name(),
            start.elapsed().as_secs_f64() * 1000. / 120.
        );
    }
    app.move_row(1);
    app.act(Action::Inspect);
    terminal
        .draw(|f| ui::draw(f, &mut app, Duration::ZERO))
        .unwrap();
    std::fs::create_dir_all("artifacts").unwrap();
    std::fs::write(
        "artifacts/inspector.svg",
        ui::export_svg(terminal.backend().buffer()),
    )
    .unwrap();
    let mut sys = sysinfo::System::new().unwrap();
    let pid = sysinfo::get_current_pid().unwrap();
    sys.refresh_processes(sysinfo::ProcessesToUpdate::Some(&[pid]), true);
    println!(
        "RSS: {:.1} MiB; retained samples: {}; process snapshots: {}",
        sys.process(pid).unwrap().memory() as f64 / 1024. / 1024.,
        app.history.len(),
        app.history
            .iter()
            .map(|s| Arc::as_ptr(&s.processes))
            .collect::<std::collections::HashSet<_>>()
            .len()
    );
}
