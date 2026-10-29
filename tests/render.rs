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
        let unselected = render(&mut a, w, h);
        assert_eq!(a.selection, None);
        assert!(!unselected.contains("SELECTED"));
        assert!(!unselected.contains("SYSTEM ON CHIP"));
        let mut frames = format!("TEARDOWN\n{unselected}");
        a.select(Part::Fan0);
        let selected = render(&mut a, w, h);
        if w >= 150 {
            assert!(selected.contains("SELECTED"));
            frames += &format!("\nSELECTED COMPONENT\n{selected}");
        }
        a.act(Action::Back);
        assert_eq!(a.selection, None);
        assert_eq!(render(&mut a, w, h), unselected);
        a.cells = true;
        frames += &format!("\nTELEMETRY CELLS\n{}", render(&mut a, w, h));
        a.act(Action::GoTo(Screen::Systems));
        for page in Page::ALL {
            a.change_page(page);
            frames += &format!("\n{}\n{}", page.name(), render(&mut a, w, h));
        }
        insta::assert_snapshot!(format!("layouts_{w}x{h}"), frames);
        if w >= 120 {
            a.act(Action::GoTo(Screen::Teardown));
            assert_eq!(a.selection, None);
            let mut numbered = None;
            for orientation in ["front", "back"] {
                a.cfg.teardown.orientation = orientation.into();
                let view = render(&mut a, w, h);
                assert!(view.contains(if orientation == "front" {
                    "╭HDMI ○"
                } else {
                    "○ HDMI╮"
                }));
                let fans = view.lines().find(|line| line.contains("FAN 0")).unwrap();
                assert!(fans.find("FAN 0").unwrap() < fans.find("FAN 1").unwrap());
                let rect = |part| a.parts.iter().find(|(p, _)| *p == part).unwrap().1;
                let positions = [Part::Fan0, Part::Fan1]
                    .into_iter()
                    .chain((0..6).map(Part::Cell))
                    .map(rect)
                    .collect::<Vec<_>>();
                assert!(positions[0].x < positions[1].x);
                // Cells run down the left pair, across the middle, then down the right pair.
                assert_eq!(positions[2].x, positions[3].x);
                assert_eq!(positions[6].x, positions[7].x);
                assert!(positions[2].x < positions[4].x);
                assert!(positions[4].x < positions[5].x);
                assert!(positions[5].x < positions[6].x);
                if let Some(front) = &numbered {
                    assert_eq!(front, &positions);
                }
                numbered = Some(positions);
            }
        }
    }
}
#[test]
fn pack_charge_fills_cells_in_order_from_bottom_to_top() {
    use overseer::metrics::Avail;
    use std::sync::Arc;
    for (w, h) in [(120, 36), (200, 55)] {
        for orientation in ["front", "back"] {
            let mut a = app("balanced");
            a.cfg.teardown.orientation = orientation.into();
            let mut term = Terminal::new(TestBackend::new(w, h)).unwrap();
            for (percent, expected) in [
                (0., [0.; 6]),
                (25., [1., 0.5, 0., 0., 0., 0.]),
                (50., [1., 1., 1., 0., 0., 0.]),
                (82., [1., 1., 1., 1., 0.92, 0.]),
                (100., [1.; 6]),
                (0., [0.; 6]), // Discharging clears previous fills on the same terminal.
            ] {
                if let Avail::Ok(battery) =
                    &mut Arc::make_mut(a.history.back_mut().unwrap()).battery
                {
                    battery.percent = percent;
                }
                term.draw(|f| ui::draw(f, &mut a, Duration::ZERO)).unwrap();
                let cells: Vec<_> = a
                    .hitmap
                    .0
                    .iter()
                    .filter(|(_, priority, hit)| *priority == 2 && *hit == Hit::Part(Part::Battery))
                    .map(|(r, _, _)| *r)
                    .collect();
                assert_eq!(cells.len(), 6);
                let bms = a.parts.iter().find(|(p, _)| *p == Part::Battery).unwrap().1;
                let green = (bms.x + 1..bms.right() - 1)
                    .map(|x| &term.backend().buffer()[(x, bms.y + 1)])
                    .find(|cell| cell.symbol().chars().any(|c| c.is_ascii_digit()))
                    .unwrap()
                    .fg;
                for (outer, expected) in cells.into_iter().zip(expected) {
                    let r = ui::kit::inner(outer, 1, 1);
                    let border = term.backend().buffer()[(outer.x, outer.y + 1)].fg;
                    assert_eq!(border == green, expected > 0.);
                    for x in outer.x..outer.right() {
                        let cell = &term.backend().buffer()[(x, outer.y)];
                        if cell.symbol().chars().any(|c| c.is_ascii_alphanumeric()) {
                            assert_eq!(cell.fg, border);
                        }
                    }
                    let mut rows = Vec::new();
                    for y in r.y..r.bottom() {
                        let symbol = term.backend().buffer()[(r.x, y)].symbol();
                        let units = match symbol.chars().next().unwrap() {
                            ' ' => 0,
                            ch @ '▁'..='█' => ch as u32 - 0x2580,
                            _ => panic!("Unexpected content in cell: {symbol}"),
                        };
                        for x in r.x..r.right() {
                            let cell = &term.backend().buffer()[(x, y)];
                            assert_eq!(cell.symbol(), symbol);
                            if units == 8 {
                                assert_eq!(cell.bg, cell.fg, "Full rows should have a solid fill");
                            }
                        }
                        rows.push(units);
                    }
                    assert!(rows.windows(2).all(|pair| pair[0] <= pair[1]));
                    let fraction = rows.iter().sum::<u32>() as f32 / (r.height * 8) as f32;
                    assert!((fraction - expected).abs() <= 1. / (r.height * 8) as f32 + 0.0001);
                }
            }
            Arc::make_mut(a.history.back_mut().unwrap()).battery =
                Avail::Failed("Unavailable".into());
            term.draw(|f| ui::draw(f, &mut a, Duration::ZERO)).unwrap();
            for (r, priority, hit) in &a.hitmap.0 {
                if *priority == 2 && *hit == Hit::Part(Part::Battery) {
                    let inside = ui::kit::inner(*r, 1, 1);
                    assert!(
                        term.backend().buffer()[(inside.x, inside.y + inside.height / 2)]
                            .symbol()
                            .chars()
                            .any(|c| c.is_ascii_alphabetic())
                    );
                }
            }
        }
    }
}
#[test]
fn speaker_grilles_follow_channels_and_respect_motion_settings() {
    use overseer::{audio::Frame, motion::Motion};
    use ratatui::{buffer::Buffer, layout::Rect};
    fn draw(a: &App, bounds: Rect, part: Part, audio: &Frame) -> (Rect, Buffer) {
        let mut buffer = Buffer::empty(bounds);
        let pill = ui::audio::grille(&mut buffer, bounds, part, a, audio);
        (pill, buffer)
    }
    let mut a = app("balanced");
    a.motion = Motion::Full;
    let silent = Frame {
        status: String::new(),
        channels: vec![vec![0.; 64]; 2],
    };
    let audible = Frame {
        status: String::new(),
        channels: vec![vec![0.01; 64], vec![0.; 64]],
    };
    assert!((audible.level(0) - 0.04).abs() < 0.000001);
    assert_eq!((audible.level(1), audible.level(2)), (0., 0.));
    for (w, h) in [(7, 7), (5, 7), (7, 5), (5, 5)] {
        let bounds = Rect::new(10, 8, w, h);
        let normal = draw(&a, bounds, Part::SpeakerLeft, &silent);
        let enlarged = draw(&a, bounds, Part::SpeakerLeft, &audible);
        assert_eq!((normal.0.width, normal.0.height), (5, 5));
        assert_eq!((enlarged.0.width, enlarged.0.height), (w, h));
        assert_eq!(
            normal.0.x * 2 + normal.0.width,
            enlarged.0.x * 2 + enlarged.0.width
        );
        assert_eq!(
            normal.0.y * 2 + normal.0.height,
            enlarged.0.y * 2 + enlarged.0.height
        );
        assert_eq!(draw(&a, bounds, Part::SpeakerRight, &audible), normal);
        assert_eq!(draw(&a, bounds, Part::SpeakerLeft, &silent), normal);
        // Advancing time cannot change a sprite when the waveform stays the same.
        let sample = a.sample();
        for _ in 0..16 {
            a.anim.tick(&sample, 1. / 60., Motion::Full, true, 60.);
            assert_eq!(draw(&a, bounds, Part::SpeakerLeft, &audible), enlarged);
        }
        for motion in [Motion::Off, Motion::Reduced] {
            a.motion = motion;
            assert_eq!(draw(&a, bounds, Part::SpeakerLeft, &audible), normal);
        }
        a.motion = Motion::Full;
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
