use overseer::{
    app::{Action, App, Page, Screen},
    config::Config,
    ui,
};
use ratatui::{Terminal, backend::TestBackend};
use std::time::Duration;
fn app(scenario: &str) -> App {
    let mut c = Config::default();
    c.ui.onboarding_seen = true;
    c.ui.motion = "off".into();
    App::new(c, Some(scenario.into()))
}
fn render(a: &mut App, w: u16, h: u16) -> String {
    let mut term = Terminal::new(TestBackend::new(w, h)).unwrap();
    term.draw(|f| ui::draw(f, a, Duration::ZERO)).unwrap();
    (0..h)
        .map(|y| {
            (0..w)
                .map(|x| term.backend().buffer()[(x, y)].symbol())
                .collect::<String>()
                .trim_end()
                .to_owned()
        })
        .collect::<Vec<_>>()
        .join("\n")
}
#[test]
fn representative_layouts() {
    for (w, h) in [(80, 24), (120, 36), (200, 55)] {
        let mut a = app("balanced");
        let mut frames = format!("TEARDOWN\n{}", render(&mut a, w, h));
        a.cells = true;
        frames += &format!("\nTELEMETRY CELLS\n{}", render(&mut a, w, h));
        a.act(Action::GoTo(Screen::Systems));
        for page in Page::ALL {
            a.change_page(page);
            frames += &format!("\n{}\n{}", page.name(), render(&mut a, w, h));
        }
        insta::assert_snapshot!(format!("layouts_{w}x{h}"), frames);
    }
}
#[test]
fn unavailable_sensors_and_small_resizes_stay_usable() {
    for scenario in [
        "missing-sensors",
        "fanless",
        "no-battery",
        "memory-pressure",
        "three-tier-cpu",
    ] {
        let mut a = app(scenario);
        for (w, h) in [(59, 17), (60, 18), (80, 24), (120, 36)] {
            render(&mut a, w, h);
            a.act(Action::GoTo(Screen::Systems));
            for page in Page::ALL {
                a.change_page(page);
                render(&mut a, w, h);
            }
            a.change_page(Page::Cpu);
            a.move_row(1);
            a.act(Action::Inspect);
            render(&mut a, w, h);
            a.act(Action::Back);
            a.act(Action::GoTo(Screen::Teardown));
        }
    }
}
