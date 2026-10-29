use overseer::{
    app::{Action, App, Page, Part, Screen},
    config::Config,
    ui::{self, kit::Hit},
};
use ratatui::{Terminal, backend::TestBackend};
use std::time::{Duration, Instant};
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
fn speaker_grilles_follow_channels_and_respect_motion_settings() {
    use overseer::{audio::Frame, motion::Motion};
    fn speakers(a: &mut App, w: u16, h: u16) -> Vec<(ratatui::layout::Rect, String)> {
        let mut term = Terminal::new(TestBackend::new(w, h)).unwrap();
        term.draw(|f| ui::draw(f, a, Duration::ZERO)).unwrap();
        [Part::SpeakerLeft, Part::SpeakerRight]
            .into_iter()
            .map(|part| {
                let r = a.parts.iter().find(|(p, _)| *p == part).unwrap().1;
                let cells = (r.y..r.bottom())
                    .flat_map(|y| (r.x..r.right()).map(move |x| (x, y)))
                    .map(|pos| term.backend().buffer()[pos].symbol())
                    .collect();
                (r, cells)
            })
            .collect()
    }
    let mut a = app("balanced");
    a.motion = Motion::Full;
    let quiet = speakers(&mut a, 200, 55);
    assert!(quiet[0].1.contains("╭─╮"));
    assert!(!quiet[0].1.contains('▪'));
    let audio = Frame {
        status: String::new(),
        channels: vec![vec![0.25; 64], vec![0.; 64]],
    };
    assert_eq!(
        (audio.level(0), audio.level(1), audio.level(2)),
        (1., 0., 0.)
    );
    a.anim.speakers[0].snap(audio.level(0));
    a.anim.speaker_phase = 0.25;
    let loud = speakers(&mut a, 200, 55);
    assert_eq!(quiet[0].0, loud[0].0); // Hit targets stay still while the grille moves.
    assert!(loud[0].1.contains("╭───╮"));
    assert_ne!(quiet[0].1, loud[0].1);
    assert_eq!(quiet[1], loud[1]);
    for motion in [Motion::Off, Motion::Reduced] {
        a.motion = motion;
        assert_eq!(speakers(&mut a, 200, 55), quiet);
    }
    // Steady, quiet audio must still visibly vibrate, even with no expansion room.
    for (w, h) in [(120, 36), (160, 46), (200, 55)] {
        a.motion = Motion::Full;
        a.anim.speakers[0].snap(0.01);
        let sample = a.sample();
        let mut frames = std::collections::HashSet::new();
        let mut other_channel = std::collections::HashSet::new();
        for _ in 0..32 {
            a.anim.tick(&sample, 1. / 64., Motion::Full, true, 60.);
            let grilles = speakers(&mut a, w, h);
            frames.insert(grilles[0].1.clone());
            other_channel.insert(grilles[1].1.clone());
        }
        assert!(frames.len() > 1, "Grille stayed still at {w}x{h}");
        assert_eq!(other_channel.len(), 1, "Silent channel should stay still");
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
            a.act(Action::Ports);
            let ports = render(&mut a, w, h);
            if w >= 60 && h >= 18 {
                assert!(ports.contains("PHYSICAL PORTS"));
                assert!(ports.lines().last().unwrap().contains("Esc close ports"));
            }
            a.act(Action::Back);
            a.act(Action::InspectPart(Part::SpeakerLeft));
            render(&mut a, w, h);
            a.act(Action::Back);
        }
    }
    let mut a = app("balanced");
    let before = render(&mut a, 120, 36);
    a.hover = Some((
        Hit::Part(Part::Memory),
        Instant::now() - Duration::from_secs(1),
    ));
    let hover = render(&mut a, 120, 36);
    a.info("Motion · reduced".into());
    let notice = render(&mut a, 120, 36);
    for after in [&hover, &notice] {
        assert_eq!(
            before.lines().take(34).collect::<Vec<_>>(),
            after.lines().take(34).collect::<Vec<_>>()
        );
        assert_eq!(before.lines().last(), after.lines().last());
    }
    assert!(hover.lines().nth(34).unwrap().contains("Enter inspect"));
    assert!(notice.lines().nth(34).unwrap().contains("Motion · reduced"));
}
