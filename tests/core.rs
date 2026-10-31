use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use overseer::{
    app::{Action, App, Layer, Page, Part, Screen},
    config::Config,
    metrics::{self, Avail, allocate_cells},
    ui::{
        processes::power_estimate,
        teardown::{CELLS, COMPONENTS, fit_chassis, to_cells},
    },
};
use ratatui::layout::Rect;
fn app() -> App {
    let mut c = Config::default();
    c.ui.onboarding_seen = true;
    c.ui.motion = "off".into();
    App::new(c, Some("balanced".into()))
}
fn key(a: &mut App, k: KeyCode) {
    a.on_key(KeyEvent::new(k, KeyModifiers::NONE));
}
#[test]
fn caffeinate_menu_owns_input_and_c_is_scoped_to_teardown() {
    let mut a = app();
    key(&mut a, KeyCode::Char('c'));
    for expected in 0..6 {
        assert!(matches!(
            a.layers.last(),
            Some(Layer::Caffeinate { index, custom: None, .. }) if *index == expected
        ));
        if expected < 5 {
            key(&mut a, KeyCode::Down);
        }
    }
    key(&mut a, KeyCode::Enter);
    key(&mut a, KeyCode::Enter);
    assert!(matches!(
        a.layers.last(),
        Some(Layer::Caffeinate {
            custom: Some(_),
            error: Some(_),
            ..
        })
    ));
    key(&mut a, KeyCode::Char('0'));
    key(&mut a, KeyCode::Enter);
    assert!(!a.caffeinate.is_active());
    key(&mut a, KeyCode::Backspace);
    for c in "90".chars() {
        key(&mut a, KeyCode::Char(c));
    }
    assert!(matches!(
        a.layers.last(),
        Some(Layer::Caffeinate { custom: Some(input), error: None, .. }) if input == "90"
    ));
    key(&mut a, KeyCode::Esc);
    assert!(matches!(
        a.layers.last(),
        Some(Layer::Caffeinate {
            index: 5,
            custom: None,
            ..
        })
    ));
    key(&mut a, KeyCode::Char('c'));
    assert!(a.layers.is_empty() && !a.caffeinate.is_active());
    key(&mut a, KeyCode::Char('2'));
    key(&mut a, KeyCode::Char('c'));
    assert!(a.layers.is_empty());
    key(&mut a, KeyCode::Char('1'));
    key(&mut a, KeyCode::Char('c'));
    a.on_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL));
    assert!(a.quit && !a.caffeinate.is_active());
}
#[test]
fn navigation_restores_page_state_and_gives_drawer_arrow_ownership() {
    let mut a = app();
    assert_eq!(a.screen, Screen::Teardown);
    assert_eq!(a.selection, None);
    for code in [KeyCode::Enter, KeyCode::Char('z'), KeyCode::Char('g')] {
        key(&mut a, code);
        assert_eq!(a.selection, None);
        assert!(a.layers.is_empty() && a.zoom.is_none());
        assert_eq!(a.screen, Screen::Teardown);
    }
    key(&mut a, KeyCode::Tab);
    assert!(a.selection.is_some());
    key(&mut a, KeyCode::Esc);
    assert_eq!(a.selection, None);
    assert!(a.cfg.teardown.audio_enabled);
    a.act(Action::InspectPart(Part::SpeakerLeft));
    key(&mut a, KeyCode::Enter);
    assert!(!a.cfg.teardown.audio_enabled);
    assert!(overseer::ui::audio::frame(&a).channels.is_empty());
    key(&mut a, KeyCode::Char('e'));
    assert!(a.cfg.teardown.audio_enabled);
    assert_eq!(overseer::ui::audio::frame(&a).channels.len(), 2);
    key(&mut a, KeyCode::Esc);
    assert_eq!(a.cfg.teardown.orientation, "front");
    key(&mut a, KeyCode::Char('p'));
    assert!(matches!(a.layers.last(), Some(Layer::Ports)));
    key(&mut a, KeyCode::Right);
    assert_eq!(a.screen, Screen::Teardown);
    key(&mut a, KeyCode::Esc);
    key(&mut a, KeyCode::Char('o'));
    assert_eq!(a.cfg.teardown.orientation, "back");
    key(&mut a, KeyCode::Char('o'));
    assert_eq!(a.cfg.teardown.orientation, "front");
    key(&mut a, KeyCode::Char('2'));
    assert_eq!(a.screen, Screen::Systems);
    assert!(a.selected_process.is_none());
    key(&mut a, KeyCode::Down);
    assert_eq!(a.selected_process, Some(a.rows[0].process.key()));
    let selected = a.selected_process;
    key(&mut a, KeyCode::Enter);
    key(&mut a, KeyCode::Right);
    assert_eq!(a.page, Page::Cpu);
    assert!(matches!(
        a.layers.last(),
        Some(Layer::Process { tab: 1, .. })
    ));
    key(&mut a, KeyCode::Down);
    assert_ne!(a.selected_process, selected);
    if let Some(Layer::Process { process, .. }) = a.layers.last() {
        assert_eq!(Some(process.key()), a.selected_process);
    }
    key(&mut a, KeyCode::Esc);
    let selected = a.selected_process;
    key(&mut a, KeyCode::Right);
    assert_eq!(a.page, Page::Gpu);
    assert_eq!(a.sort, 8);
    key(&mut a, KeyCode::Right);
    assert_eq!(a.page, Page::Memory);
    assert!(a.selected_process.is_none());
    assert_eq!(a.sort, 1);
    key(&mut a, KeyCode::Left);
    assert_eq!(a.page, Page::Gpu);
    key(&mut a, KeyCode::Left);
    assert_eq!(a.selected_process, selected);
    key(&mut a, KeyCode::Left);
    assert_eq!(a.page, Page::Network);
    key(&mut a, KeyCode::Right);
    assert_eq!(a.page, Page::Cpu);
    let selected = a.selected_process;
    key(&mut a, KeyCode::Char('s'));
    assert_eq!(a.sort, 1);
    key(&mut a, KeyCode::Char('s'));
    assert_eq!(a.sort, 4);
    assert_eq!(a.selected_process, selected);
    a.act(Action::GoTo(Screen::Teardown));
    a.select(Part::Memory);
    key(&mut a, KeyCode::Char('g'));
    assert_eq!(a.page, Page::Memory);
}
#[test]
fn filters_expansion_and_escape_are_safe() {
    let mut a = app();
    a.act(Action::Search);
    for c in "qtam".chars() {
        key(&mut a, KeyCode::Char(c));
    }
    assert!(!a.quit);
    assert_eq!(a.search.as_deref(), Some("qtam"));
    key(&mut a, KeyCode::Enter);
    key(&mut a, KeyCode::Right);
    assert_eq!(a.search.as_deref(), Some("qtam"));
    key(&mut a, KeyCode::Esc);
    assert_eq!(a.search.as_deref(), Some(""));
    key(&mut a, KeyCode::Esc);
    assert!(a.search.is_none());
    a.act(Action::Zoom);
    assert!(a.expanded);
    key(&mut a, KeyCode::Esc);
    assert!(!a.expanded);
    for _ in 0..10 {
        key(&mut a, KeyCode::Esc);
    }
    assert!(!a.quit);
}
#[test]
fn paused_replay_pins_telemetry_task_values_and_energy() {
    let mut a = app();
    a.follow = Some(a.rows[0].process.clone());
    a.act(Action::PlayPause);
    let before = a.sample();
    for time in 180..200 {
        a.ingest(metrics::demo("gpu-burst", time as f64));
    }
    let after = a.sample();
    assert_eq!(before.at, after.at);
    assert_eq!(before.energy_wh, after.energy_wh);
    assert_eq!(before.processes[0].cpu, after.processes[0].cpu);
    a.act(Action::Step(-10));
    let snapshot = a.sample();
    assert_eq!(snapshot.at, 169.);
    assert!(snapshot.process_at <= snapshot.at);
    let followed = a.displayed_follow().unwrap();
    assert_eq!(
        followed.cpu,
        snapshot
            .processes
            .iter()
            .find(|p| p.key() == followed.key())
            .unwrap()
            .cpu
    );
    a.act(Action::Live);
    assert_eq!(a.sample().at, 199.);
    let s = metrics::demo("idle", 1.);
    let mut anim = overseer::motion::Anim::default();
    anim.tick(&s, 0.1, overseer::motion::Motion::Full, true, 30.);
    let phases = anim.phases.clone();
    assert!(phases["ambient"] > 0.);
    anim.tick(&s, 0.1, overseer::motion::Motion::Full, false, 30.);
    assert_eq!(
        anim.phases, phases,
        "paused telemetry must freeze animation"
    );
}
#[test]
fn signals_require_explicit_confirmation_and_are_disabled_in_demo_and_replay() {
    let mut a = app();
    a.move_row(1);
    a.act(Action::Signal);
    assert!(a.layers.is_empty());
    a.demo = None;
    a.cursor = Some(40.);
    a.act(Action::Signal);
    assert!(a.layers.is_empty());
    let p = a.rows[0].process.clone();
    a.layers.push(Layer::Signal {
        process: p,
        force: true,
    });
    for code in [KeyCode::Enter, KeyCode::Char('y'), KeyCode::Char('q')] {
        key(&mut a, code);
        assert!(matches!(a.layers.last(), Some(Layer::Signal { .. })));
        assert!(!a.quit);
    }
    key(&mut a, KeyCode::Esc);
    assert!(a.layers.is_empty());
    let mut protected = metrics::demo("balanced", 0.).processes[0].clone();
    protected.pid = 1;
    assert!(metrics::send_signal(&protected, false).is_err());
}
#[test]
fn accounting_never_fabricates_sensor_values_or_exceeds_totals() {
    assert!(matches!(
        metrics::normalize_system_power(0.),
        Avail::Failed(_)
    ));
    let mut s = metrics::demo("balanced", 1.);
    use metrics::ports::{PortKind, power_input};
    if let Avail::Ok(b) = &mut s.battery {
        b.ac = true;
    }
    assert_eq!(power_input(&s), Avail::Estimated(PortKind::UsbC(3)));
    if let Avail::Ok(p) = &mut s.ports {
        std::sync::Arc::make_mut(p)
            .iter_mut()
            .find(|p| p.kind == PortKind::MagSafe)
            .unwrap()
            .connected = Avail::Ok(true);
    }
    assert!(
        power_input(&s).value().is_none(),
        "ambiguous inputs must not be highlighted"
    );
    let sum = s
        .processes
        .iter()
        .map(|p| *power_estimate(p, &s).value().unwrap())
        .sum::<f32>();
    assert!(sum <= s.power[0].value().unwrap() + 0.001);
    let mut a = app();
    a.act(Action::Demo("missing-sensors".into()));
    assert!(a.history_values(Part::Gpu).is_empty());
    assert!(a.history_values(Part::Power).is_empty());
    for values in [[0; 5], [1, 2, 3, 4, 5], [u64::MAX; 5], [3, 9, 2, 5, 5]] {
        for cells in [0, 1, 37, 1000] {
            assert_eq!(allocate_cells(values, cells).iter().sum::<usize>(), cells);
        }
    }
}
#[test]
fn chassis_fits_terminal_sizes() {
    for (w, h) in [(60, 18), (80, 24), (120, 36), (200, 55), (320, 100)] {
        for aspect in [1.5, 2.05, 2.8] {
            let r = Rect::new(1, 3, w, h);
            let c = fit_chassis(r, 1.413, aspect);
            assert!(c.x >= r.x && c.y >= r.y && c.right() <= r.right() && c.bottom() <= r.bottom());
            assert_eq!(c.width % 2, 1, "chassis must have a center column");
            for n in COMPONENTS.iter().map(|p| p.rect).chain(CELLS) {
                let left = to_cells(n, c, false);
                let right = to_cells(n, c, true);
                assert_eq!(left.width, right.width);
                assert_eq!(left.x + right.right(), c.x + c.right());
                if (n.x + n.w / 2. - 0.5).abs() < 0.0001 {
                    assert_eq!(left, right);
                    assert_eq!(left.width % 2, 1);
                    assert_eq!(left.x + left.width / 2, c.x + c.width / 2);
                }
            }
            for (l, r) in [(1, 2), (4, 5), (6, 7)] {
                assert_eq!(
                    to_cells(COMPONENTS[l].rect, c, true),
                    to_cells(COMPONENTS[r].rect, c, false)
                );
            }
        }
    }
}
