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
fn caffeinate_options_and_custom_input_fit_and_support_clicks() {
    use crossterm::event::{
        KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
    };
    use overseer::app::{CAFFEINATE_OPTIONS, Layer};

    for (w, h) in [(60, 18), (80, 24), (120, 36), (200, 55)] {
        let mut a = app("balanced");
        assert!(render(&mut a, w, h).contains("c caffeinate"));
        let click = |a: &mut App, target: Hit| {
            let rect = a
                .hitmap
                .0
                .iter()
                .find(|(_, _, hit)| *hit == target)
                .unwrap()
                .0;
            a.on_mouse(MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: rect.x,
                row: rect.y,
                modifiers: KeyModifiers::NONE,
            });
        };
        click(&mut a, Hit::Caffeinate);
        let view = render(&mut a, w, h);
        let positions = CAFFEINATE_OPTIONS.map(|label| {
            view.find(label)
                .unwrap_or_else(|| panic!("{w}x{h}: missing {label}\n{view}"))
        });
        assert!(positions.windows(2).all(|pair| pair[0] < pair[1]));
        click(&mut a, Hit::CaffeinateOption(5));
        assert!(render(&mut a, w, h).contains("How many minutes?"));
        a.on_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert!(render(&mut a, w, h).contains("positive whole number"));
        assert!(matches!(
            a.layers.last(),
            Some(Layer::Caffeinate {
                custom: Some(_),
                ..
            })
        ));
        assert!(!a.caffeinate.is_active());
    }
}
#[test]
fn header_lists_device_name_and_hardware_in_order() {
    use std::sync::Arc;
    let mut a = app("balanced");
    let machine = Arc::make_mut(&mut Arc::make_mut(a.history.back_mut().unwrap()).machine);
    machine.name = "QA’s MacBook Pro".into();
    machine.storage_gb = Some(512);
    let wide = render(&mut a, 200, 55);
    let header = wide.lines().next().unwrap();
    assert!(header.contains(
        "QA’s MacBook Pro / M4 Pro / 12 Core CPU / 16 Core GPU / 24.0 GiB Memory / 512 GB Storage"
    ));
    assert!(!header.contains("Apple"));
    for (w, h) in [(60, 18), (80, 24), (120, 36)] {
        let view = render(&mut a, w, h);
        let header = view.lines().next().unwrap();
        assert!(header.contains("QA’s MacBook Pro"));
        assert!(header.contains("SYNTHETIC"));
        assert!(unicode_width::UnicodeWidthStr::width(header) <= w as usize);
    }
}
#[test]
fn representative_layouts() {
    for (w, h) in [(80, 24), (120, 36), (200, 55)] {
        let mut a = app("balanced");
        let unselected = render(&mut a, w, h);
        assert_eq!(a.selection, None);
        assert!(!unselected.contains("SELECTED"));
        assert!(!unselected.contains("SYSTEM ON CHIP"));
        assert!(!unselected.contains("ORIENTATION"));
        assert!(!unselected.contains("PORTS ·"));
        assert!(!unselected.contains("Layout approximate"));
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
                for (part, label) in [(Part::SpeakerLeft, 'L'), (Part::SpeakerRight, 'R')] {
                    let r = rect(part);
                    assert_eq!(
                        view.lines()
                            .nth(r.bottom() as usize)
                            .unwrap()
                            .chars()
                            .nth((r.x + r.width / 2) as usize),
                        Some(label)
                    );
                }
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
            // The chip and CPU page use the sampled mode.
            for mode in [
                overseer::metrics::PowerMode::LowPower,
                overseer::metrics::PowerMode::Automatic,
                overseer::metrics::PowerMode::HighPower,
            ] {
                std::sync::Arc::make_mut(a.history.back_mut().unwrap()).power_mode =
                    overseer::metrics::Avail::Ok(mode);
                let chip = render(&mut a, w, h);
                assert!(chip.contains(mode.label()));
                a.act(Action::GoPage(Page::Cpu));
                assert!(render(&mut a, w, h).contains(mode.label()));
                a.act(Action::GoTo(Screen::Teardown));
            }
        }
    }
}
#[test]
fn gpu_tasks_sort_and_render_measured_zero_warmup_and_missing_counters() {
    use overseer::metrics::Avail;
    use std::sync::Arc;

    let mut a = app("balanced");
    let s = Arc::make_mut(a.history.back_mut().unwrap());
    let processes = Arc::make_mut(&mut s.processes);
    processes.truncate(5);
    for (p, (name, gpu)) in processes.iter_mut().zip([
        ("gpu-zero", Avail::Ok(0.)),
        ("gpu-busy", Avail::Ok(150.)),
        ("gpu-wait", Avail::Acquiring),
        ("gpu-hidden", Avail::Unsupported("no counters".into())),
        (
            "gpu-restricted",
            Avail::Permission("GPU time restricted".into()),
        ),
    ]) {
        p.name = Arc::from(name);
        p.gpu = gpu;
        p.memory = if name == "gpu-zero" {
            (9.5 * 1_048_576.) as u64
        } else {
            (1234.5 * 1_048_576.) as u64
        };
        if matches!(name, "gpu-restricted" | "gpu-busy") {
            p.cpu = 0.;
            p.memory = 0;
            p.threads = Avail::Permission("task info restricted".into());
        }
    }
    s.process_gpu = Avail::Ok(3);
    a.act(Action::GoPage(Page::Gpu));
    assert_eq!(a.sort, 8);
    assert_eq!(a.rows.len(), 4);
    assert_eq!(a.rows[0].process.name.as_ref(), "gpu-busy");
    assert_eq!(a.rows[1].process.name.as_ref(), "gpu-zero");
    for (w, h) in [(80, 24), (120, 36), (200, 55)] {
        let view = render(&mut a, w, h);
        for label in [
            "TOP GPU TASKS",
            "GPU%",
            "150.0%",
            "0.0%",
            "ACQUIRING",
            "gpu-busy",
        ] {
            assert!(view.contains(label), "{w}x{h}: missing {label}\n{view}");
        }
        assert!(!view.contains("gpu-hidden"));
        let restricted = view
            .lines()
            .find(|line| line.contains("gpu-restricted"))
            .unwrap();
        assert!(restricted.contains("???%"));
        assert!(!restricted.contains("PERMISSION"));
        assert!(!restricted.contains("0.0%"));
        if w >= 120 {
            assert!(restricted.contains("??? MiB"));
            let unit_column = |row: &str| row[..row.find("MiB").unwrap()].chars().count();
            let zero = view.lines().find(|line| line.contains("gpu-zero")).unwrap();
            let wait = view.lines().find(|line| line.contains("gpu-wait")).unwrap();
            assert!(zero.contains("9.5 MiB"));
            assert!(wait.contains("1234.5 MiB"));
            assert_eq!(unit_column(restricted), unit_column(zero));
            assert_eq!(unit_column(zero), unit_column(wait));
        }
        let busy = view.lines().find(|line| line.contains("gpu-busy")).unwrap();
        assert!(busy.contains("150.0%") && busy.contains("???%"));
        if w >= 120 {
            assert!(busy.contains("??? MiB"));
        }
    }
    a.search = Some("gpu-busy".into());
    a.refresh_rows(true);
    assert_eq!(a.rows.len(), 1);
    a.search = None;
    let s = Arc::make_mut(a.history.back_mut().unwrap());
    s.process_gpu = Avail::Failed("GPU registry enumeration failed".into());
    for p in Arc::make_mut(&mut s.processes) {
        p.gpu = Avail::Failed("GPU registry enumeration failed".into());
    }
    a.refresh_rows(true);
    assert!(a.rows.is_empty());
    assert!(render(&mut a, 120, 36).contains("GPU registry enumeration failed"));
}
#[test]
fn gpu_windows_stay_square_and_fixed_across_rotation() {
    let mut a = app("balanced");
    a.act(Action::GoPage(Page::Gpu));
    a.motion = overseer::motion::Motion::Full;
    for (w, h) in [(80, 24), (120, 36), (200, 55)] {
        for expanded in [false, true] {
            a.expanded = expanded;
            let mut previous = None;
            for phase in [0., 0.37, 0.71] {
                a.anim.phases.insert("donut".into(), phase);
                let view = render(&mut a, w, h);
                let lines: Vec<Vec<char>> = view.lines().map(|s| s.chars().collect()).collect();
                let (top, header) = view
                    .lines()
                    .enumerate()
                    .find(|(_, line)| line.contains("╭ VISUAL"))
                    .unwrap();
                let title = header.find("╭ VISUAL").unwrap();
                let x = header[..title].chars().count();
                let right = (x + 1..lines[top].len())
                    .find(|&i| lines[top][i] == '╮')
                    .unwrap();
                let bottom = (top + 1..lines.len())
                    .find(|&y| lines[y].get(x) == Some(&'╰'))
                    .unwrap();
                let bounds = (x, top, right - x + 1, bottom - top + 1);
                assert!((bounds.2 as f32 / a.cell_aspect - bounds.3 as f32).abs() <= 0.5);
                if let Some(previous) = previous {
                    assert_eq!(bounds, previous, "donut window moved with its rotation");
                }
                previous = Some(bounds);
                let history = view.lines().find(|line| line.contains("UTIL 5m")).unwrap();
                assert!(
                    history
                        .chars()
                        .take(x)
                        .collect::<String>()
                        .contains("UTIL 5m")
                );
            }
        }
    }
}
#[test]
fn cpu_clusters_keep_channel_ids_and_size_performance_cores_larger() {
    let names = [
        "PCPU000", "PCPU010", "PCPU020", "PCPU030", "PCPU100", "PCPU110", "PCPU120", "PCPU130",
        "ECPU000", "ECPU010", "ECPU020", "ECPU030",
    ];
    let mut a = app("balanced");
    a.act(Action::GoPage(Page::Cpu));
    for (w, h) in [(80, 24), (120, 36), (160, 46), (200, 55)] {
        for expanded in [false, true] {
            a.expanded = expanded;
            let view = render(&mut a, w, h);
            for label in names.into_iter().chain([
                "PERFORMANCE 0 · ×4",
                "PERFORMANCE 1 · ×4",
                "EFFICIENCY 0 · ×4",
            ]) {
                assert!(
                    view.contains(label),
                    "{w}x{h}, expanded={expanded}: missing {label}"
                );
            }
        }
    }
    let bounds = ratatui::layout::Rect::new(0, 0, 120, 20);
    let mut buffer = ratatui::buffer::Buffer::empty(bounds);
    ui::widgets::content(&mut buffer, bounds, Part::Cpu, &a.sample(), &a);
    let card_size = |name: &str| {
        for y in 0..bounds.height {
            let row = (0..bounds.width)
                .map(|x| buffer[(x, y)].symbol())
                .collect::<String>();
            if let Some(start) = row.find(name) {
                let x = row[..start].chars().count() as u16 - 2;
                let right = (x + 1..bounds.width)
                    .find(|x| buffer[(*x, y)].symbol() == "╮")
                    .unwrap();
                let bottom = (y + 1..bounds.height)
                    .find(|y| buffer[(x, *y)].symbol() == "╰")
                    .unwrap();
                return (right - x + 1, bottom - y + 1);
            }
        }
        panic!("missing card {name}");
    };
    let p = card_size("PCPU000");
    let e = card_size("ECPU000");
    assert_eq!(p, card_size("PCPU100"));
    assert!(p.0 > e.0 && p.1 > e.1, "P {p:?}, E {e:?}");
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
                    battery.charging = false; // Inspect capacity independently of the charge front.
                    battery.watts = Avail::Ok(0.);
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
                            if units > 0 && units < 8 {
                                assert_eq!(
                                    cell.symbol(),
                                    symbol,
                                    "Partial rows preserve pack capacity"
                                );
                            } else if units == 0 {
                                assert_eq!(
                                    cell.bg,
                                    term.backend().buffer()[(outer.x, outer.y)].bg,
                                    "Empty rows stay unfilled"
                                );
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
fn teardown_flow_animations_follow_direction_and_motion_settings() {
    use overseer::{metrics::Avail, motion::Motion};
    use ratatui::buffer::Buffer;
    use std::sync::Arc;
    fn frame(a: &mut App, w: u16, h: u16, ambient: f32, step: f32) -> Buffer {
        a.anim.phases.insert("ambient".into(), ambient);
        a.anim.phases.insert("bms".into(), step);
        let mut term = Terminal::new(TestBackend::new(w, h)).unwrap();
        term.draw(|f| ui::draw(f, a, Duration::ZERO)).unwrap();
        term.backend().buffer().clone()
    }
    for (w, h) in [(120, 36), (200, 55)] {
        let mut a = app("balanced");
        a.motion = Motion::Full;
        for charging in [true, false] {
            if let Avail::Ok(battery) = &mut Arc::make_mut(a.history.back_mut().unwrap()).battery {
                battery.percent = 75.;
                battery.charging = charging;
                battery.ac = charging;
                battery.watts = Avail::Ok(if charging { 20. } else { -10. });
            }
            let first = frame(&mut a, w, h, 0., 0.);
            let bms = a.parts.iter().find(|(p, _)| *p == Part::Battery).unwrap().1;
            let x = bms.x + bms.width / 2;
            let arrows = bms.y + 3..bms.bottom() - 1;
            let gap = |b: &Buffer| arrows.clone().find(|y| b[(x, *y)].symbol() == " ").unwrap();
            let second = frame(&mut a, w, h, 0.2, 1.);
            assert_eq!(
                gap(&second) as i32 - gap(&first) as i32,
                if charging { 1 } else { -1 }
            );
            let cells: Vec<_> = a
                .hitmap
                .0
                .iter()
                .filter(|(_, priority, hit)| *priority == 2 && *hit == Hit::Part(Part::Battery))
                .map(|(r, _, _)| *r)
                .collect();
            for (i, cell) in cells.into_iter().enumerate() {
                let x = cell.x + 2;
                let capacity_y = cell.bottom() - 1 - (cell.height - 2).div_ceil(2);
                // Cell 5 is half full at 75% pack charge. Its pulse follows that edge.
                for y in cell.y + 1..cell.bottom() - 1 {
                    assert_eq!(
                        first[(x, y)].symbol() == "━",
                        charging && i == 4 && y == capacity_y
                    );
                    assert_ne!(second[(x, y)].symbol(), "━");
                }
            }
            let phases = a.anim.phases.clone();
            a.anim.tick(&a.sample(), 1., Motion::Full, false, 60.);
            assert_eq!(a.anim.phases, phases, "Pause freezes decorative motion");
            for motion in [Motion::Reduced, Motion::Off] {
                a.motion = motion;
                assert_eq!(frame(&mut a, w, h, 0., 0.), frame(&mut a, w, h, 0.7, 5.));
            }
            a.motion = Motion::Full;
        }
    }
}
#[test]
fn teardown_selects_both_fans_from_keyboard_and_mouse() {
    use crossterm::event::{
        KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
    };
    for cells in [false, true] {
        for orientation in ["front", "back"] {
            let mut a = app("balanced");
            a.cells = cells;
            a.cfg.teardown.orientation = orientation.into();
            let mut term = Terminal::new(TestBackend::new(200, 55)).unwrap();
            term.draw(|f| ui::draw(f, &mut a, Duration::ZERO)).unwrap();
            if cells {
                a.select(Part::Fan1);
            } else {
                a.on_key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE));
            }
            assert_eq!(a.selection, Some(Part::Fan0));
            assert_eq!(
                a.part_order()
                    .iter()
                    .filter(|p| p.selection_group() == Part::Fan0)
                    .count(),
                1
            );
            for mouse in [false, true] {
                term.draw(|f| ui::draw(f, &mut a, Duration::ZERO)).unwrap();
                if mouse {
                    let r = a.parts.iter().find(|(p, _)| *p == Part::Fan1).unwrap().1;
                    a.on_mouse(MouseEvent {
                        kind: MouseEventKind::Down(MouseButton::Left),
                        column: r.x + r.width / 2,
                        row: r.y + r.height / 2,
                        modifiers: KeyModifiers::NONE,
                    });
                    assert_eq!(a.selection, Some(Part::Fan0));
                    term.draw(|f| ui::draw(f, &mut a, Duration::ZERO)).unwrap();
                }
                for part in [Part::Fan0, Part::Fan1] {
                    let r = a.parts.iter().find(|(p, _)| *p == part).unwrap().1;
                    assert_eq!(term.backend().buffer()[(r.x, r.y)].symbol(), "┏");
                }
            }
            a.on_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
            assert_ne!(a.selection, Some(Part::Fan0));
        }
    }
}
#[test]
fn component_inspectors_have_no_sample_summary() {
    use ratatui::{buffer::Buffer, layout::Rect};
    let mut a = app("balanced");
    for (w, h, zoom) in [(60, 18, false), (112, 32, false), (200, 50, true)] {
        for p in [
            Part::Cpu,
            Part::Soc,
            Part::Gpu,
            Part::Ane,
            Part::Memory,
            Part::Power,
            Part::Disk,
            Part::Network,
            Part::Thermal,
            Part::Battery,
            Part::Fan0,
            Part::Fan1,
            Part::SpeakerLeft,
            Part::SpeakerRight,
        ] {
            for replay in [false, true] {
                a.cursor = replay.then_some(a.latest_time());
                let bounds = Rect::new(0, 0, w, h);
                let mut buffer = Buffer::empty(bounds);
                ui::overlays::detail(&mut buffer, bounds, &mut a, p, zoom);
                let text = buffer
                    .content
                    .iter()
                    .map(|cell| cell.symbol())
                    .collect::<String>();
                assert!(!text.contains("CURRENT SAMPLE"));
                assert!(!text.contains("T-SNAPSHOT"));
                assert!(!text.to_lowercase().contains("physical side assumed"));
                if !matches!(
                    p,
                    Part::Fan0 | Part::Fan1 | Part::SpeakerLeft | Part::SpeakerRight
                ) {
                    let lower = match p {
                        Part::Cpu | Part::Soc | Part::Gpu | Part::Memory | Part::Battery => "0% ┤",
                        Part::Ane | Part::Power => "0.0 W ┤",
                        Part::Disk | Part::Network => "0.0 B/s ┤",
                        Part::Thermal => "0°C ┤",
                        _ => unreachable!(),
                    };
                    assert!(text.contains(lower), "{p:?}: missing scale {lower}");
                }
                if matches!(p, Part::Fan0 | Part::Fan1) {
                    let mut other = Buffer::empty(bounds);
                    ui::overlays::detail(&mut other, bounds, &mut a, Part::Fan0, zoom);
                    assert_eq!(
                        buffer, other,
                        "Either fan opens the same combined inspector"
                    );
                    assert!(text.contains("FANS"));
                    if h >= 32 {
                        assert_eq!(text.matches("0 rpm ┤").count(), 2);
                    }
                    assert!(!text.contains("FAN 0") && !text.contains("FAN 1"));
                    // Expanded views have two impellers separated by a clear center gap.
                    for y in 2..h / 2 {
                        assert_eq!(buffer[(w / 2, y)].symbol(), " ");
                    }
                    for (start, end) in [(2, w / 2 - 2), (w / 2 + 2, w - 2)] {
                        assert!(
                            (2..h / 2).any(|y| (start..end).any(|x| {
                                buffer[(x, y)]
                                    .symbol()
                                    .chars()
                                    .any(|c| ('\u{2801}'..='\u{28ff}').contains(&c))
                            })),
                            "both fan visuals must be present"
                        );
                    }
                    let fans = a.sample();
                    let fans = fans.fans.value().unwrap();
                    let left = format!("LEFT {} rpm", fans[0].rpm);
                    let right = format!("RIGHT {} rpm", fans[1].rpm);
                    let rows = (1..h - 1)
                        .map(|y| {
                            (2..w - 2)
                                .map(|x| buffer[(x, y)].symbol())
                                .collect::<String>()
                        })
                        .collect::<Vec<_>>();
                    assert!(
                        rows.iter()
                            .any(|row| row.starts_with(&left) && row.ends_with(&right)),
                        "Fan speeds stay outside the art, aligned to the outer edges"
                    );
                }
            }
        }
    }
}
#[test]
fn soc_inspector_uses_total_cpu_and_ignores_core_navigation() {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use overseer::app::Layer;
    use std::sync::Arc;
    let mut a = app("balanced");
    for sample in &mut a.history {
        let sample = Arc::make_mut(sample);
        sample.cpu = 0.73;
        for tier in &mut sample.tiers {
            for core in &mut tier.cores {
                core.active = 0.11;
            }
        }
    }
    a.inspect_core = 3;
    a.select(Part::Soc);
    a.on_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert!(matches!(a.layers.last(), Some(Layer::Inspector(Part::Soc))));
    for (w, h) in [(60, 18), (120, 36), (200, 55)] {
        let before = render(&mut a, w, h);
        assert!(before.contains("% TOTAL CPU UTILIZATION"));
        assert!(before.contains("peak 73%"));
        assert!(!before.contains("← → core"));
        assert!(!before.contains(" / PCPU"));
        for key in [KeyCode::Left, KeyCode::Right, KeyCode::Up, KeyCode::Down] {
            a.on_key(KeyEvent::new(key, KeyModifiers::NONE));
            assert_eq!(a.inspect_core, 3);
            assert_eq!(render(&mut a, w, h), before);
        }
    }
}
#[test]
fn selected_graph_scales_follow_units_and_task_cpu_can_exceed_100_percent() {
    use overseer::{app::Layer, metrics::Avail};
    use std::sync::Arc;
    let mut a = app("balanced");
    a.select(Part::Soc);
    assert!(render(&mut a, 200, 55).contains("100% ┤"));
    a.select(Part::Disk);
    assert!(render(&mut a, 200, 55).contains("0.0 B/s ┤"));
    a.cfg.units.temp = "f".into();
    let bounds = ratatui::layout::Rect::new(0, 0, 112, 32);
    let mut buffer = ratatui::buffer::Buffer::empty(bounds);
    ui::overlays::detail(&mut buffer, bounds, &mut a, Part::Thermal, false);
    assert!(
        buffer
            .content
            .iter()
            .map(|c| c.symbol())
            .collect::<String>()
            .contains("32°F ┤")
    );
    for sample in &mut a.history {
        Arc::make_mut(&mut Arc::make_mut(sample).processes)[0].cpu = 250.;
    }
    a.act(Action::GoPage(Page::Cpu));
    let process = a.sample().processes[0].clone();
    a.layers.push(Layer::Process { process, tab: 1 });
    let view = render(&mut a, 200, 55);
    assert!(view.contains("250% ┤"));
    assert!(view.contains("0% ┤"));
    assert!(view.contains("0.0 B ┤"));
    assert!(view.contains("GiB ┤"));
    a.layers.clear();
    for page in [Page::Disk, Page::Network] {
        a.act(Action::GoPage(page));
        a.expanded = true;
        let view = render(&mut a, 200, 55);
        assert!(view.contains("0 · 5m/shared"));
        assert!(view.contains("MiB/s ┤"));
    }
    a.cfg.units.net = "bits".into();
    assert!(render(&mut a, 200, 55).contains("Mb/s ┤"));
    // A battery graph labels its existing observed range rather than pretending it reaches 100%.
    a.act(Action::GoTo(Screen::Teardown));
    a.select(Part::Battery);
    for sample in &mut a.history {
        if let Avail::Ok(battery) = &mut Arc::make_mut(sample).battery {
            battery.percent = 82.;
        }
    }
    assert!(render(&mut a, 200, 55).contains("82% ┤"));
}
#[test]
fn teardown_ssd_border_keeps_its_storage_color() {
    use overseer::metrics::Avail;
    use std::sync::Arc;
    let mut a = app("balanced");
    for read in [0., 1_000_000.] {
        if let Avail::Ok(disk) = &mut Arc::make_mut(a.history.back_mut().unwrap()).disk {
            disk.read = read;
            disk.write = 0.;
        }
        for highlight in [false, true] {
            a.cfg.teardown.highlight_active = highlight;
            for selected in [false, true] {
                a.selection = selected.then_some(Part::Disk);
                a.follow = a.sample().processes.first().cloned();
                let mut term = Terminal::new(TestBackend::new(200, 55)).unwrap();
                term.draw(|f| ui::draw(f, &mut a, Duration::ZERO)).unwrap();
                let r = a
                    .parts
                    .iter()
                    .find(|(part, _)| *part == Part::Disk)
                    .unwrap()
                    .1;
                let color = term.backend().buffer()[(r.x, r.y + 1)].fg;
                assert!(color == a.theme.disk || color == ui::theme::quantize(a.theme.disk));
                let capacity = (r.x + 1..r.right() - 1)
                    .map(|x| term.backend().buffer()[(x, r.y + 3)].symbol())
                    .collect::<String>();
                assert!(capacity.contains("383.7/925.7 GiB"));
            }
        }
    }
}
#[test]
fn teardown_memory_pressure_colors_both_readouts_without_a_notice() {
    use overseer::metrics::{self, Avail};
    use ratatui::style::Color;
    for theme in ["mono", "holo"] {
        let mut a = app("balanced");
        a.theme = ui::theme::Theme::get(theme);
        for (pressure, color) in [
            (Some("NORMAL"), a.theme.text),
            (Some("WARN"), Color::Rgb(255, 215, 95)),
            (Some("CRITICAL"), Color::Rgb(255, 95, 95)),
            (Some("NORMAL"), a.theme.text),
            (None, a.theme.text),
        ] {
            let mut sample = metrics::demo("balanced", a.latest_time() + 1.);
            sample.memory_used = (20.3 * 1024f64.powi(3)) as u64;
            sample.pressure = pressure.map_or_else(
                || Avail::Unsupported("Unavailable".into()),
                |value| Avail::Ok(value.into()),
            );
            a.ingest(sample);
            assert!(a.toasts.iter().all(|toast| toast.key != "memory"));
            let mut term = Terminal::new(TestBackend::new(200, 55)).unwrap();
            term.draw(|f| ui::draw(f, &mut a, Duration::ZERO)).unwrap();
            let mut modules = 0;
            for (_, r) in a.parts.iter().filter(|(part, _)| *part == Part::Memory) {
                let cells = (r.x + 1..r.right() - 1)
                    .map(|x| &term.backend().buffer()[(x, r.bottom() - 2)])
                    .filter(|cell| cell.symbol() != " ")
                    .collect::<Vec<_>>();
                assert_eq!(
                    cells.iter().map(|cell| cell.symbol()).collect::<String>(),
                    "20.3GiB"
                );
                assert!(
                    cells
                        .iter()
                        .all(|cell| { cell.fg == color || cell.fg == ui::theme::quantize(color) })
                );
                modules += 1;
            }
            assert_eq!(modules, 2);
        }
    }
}
#[test]
fn teardown_fan_hubs_center_readings_and_keep_them_upright() {
    use overseer::{metrics::Avail, motion::Motion};
    use std::sync::Arc;
    for (w, h) in [(100, 32), (120, 36), (160, 46), (200, 55), (240, 70)] {
        let mut a = app("balanced");
        a.motion = Motion::Full;
        for orientation in ["front", "back"] {
            a.cfg.teardown.orientation = orientation.into();
            for glyphs in ["unicode", "ascii"] {
                a.cfg.ui.glyphs = glyphs.into();
                for speeds in [[0, 7200], [273, 9999]] {
                    if let Avail::Ok(fans) = &mut Arc::make_mut(a.history.back_mut().unwrap()).fans
                    {
                        for (fan, rpm) in fans.iter_mut().zip(speeds) {
                            fan.rpm = rpm;
                        }
                    }
                    let mut positions = None;
                    for phase in [0., 0.21] {
                        for fan in a.sample().fans.value().unwrap() {
                            a.anim.phases.insert(fan.name.clone(), phase);
                        }
                        let mut term = Terminal::new(TestBackend::new(w, h)).unwrap();
                        term.draw(|f| ui::draw(f, &mut a, Duration::ZERO)).unwrap();
                        let mut hubs = Vec::new();
                        for (part, rpm) in [Part::Fan0, Part::Fan1].into_iter().zip(speeds) {
                            let r = a.parts.iter().find(|(p, _)| *p == part).unwrap().1;
                            let mut digits = Vec::new();
                            let mut value = String::new();
                            for y in r.y + 1..r.bottom() - 1 {
                                for x in r.x + 1..r.right() - 1 {
                                    let symbol = term.backend().buffer()[(x, y)].symbol();
                                    if symbol.chars().any(|c| c.is_ascii_digit()) {
                                        digits.push((x, y));
                                        value.push_str(symbol);
                                    }
                                }
                            }
                            let inline = w >= 160;
                            assert_eq!(
                                value,
                                if inline {
                                    rpm.to_string()
                                } else {
                                    format!("{rpm:04}")
                                }
                            );
                            let (x, y) = digits[0];
                            if inline {
                                let with_unit = w >= 200;
                                let label = if with_unit {
                                    format!("{value} rpm")
                                } else {
                                    value.clone()
                                };
                                let row = (r.x + 1..r.right() - 1)
                                    .map(|x| term.backend().buffer()[(x, y)].symbol())
                                    .collect::<String>();
                                assert_eq!(row.contains("rpm"), with_unit);
                                assert_eq!(
                                    (x..x + label.len() as u16)
                                        .map(|x| term.backend().buffer()[(x, y)].symbol())
                                        .collect::<String>(),
                                    label
                                );
                                assert_eq!(
                                    digits,
                                    (0..value.len() as u16)
                                        .map(|i| (x + i, y))
                                        .collect::<Vec<_>>()
                                );
                                for neighbor in [x - 1, x + label.len() as u16] {
                                    let symbol = term.backend().buffer()[(neighbor, y)].symbol();
                                    if with_unit {
                                        assert!(!symbol.chars().any(|c| c.is_ascii_alphanumeric()));
                                    } else {
                                        assert_eq!(symbol, " ");
                                    }
                                }
                                let inside = ui::kit::inner(r, 1, 1);
                                let left = (inside.width - 2) / 2;
                                let hub_x = inside.x
                                    + if part == Part::Fan1 {
                                        inside.width - left - 2
                                    } else {
                                        left
                                    };
                                let offset =
                                    x as i32 * 2 + label.len() as i32 - (hub_x as i32 * 2 + 2);
                                let expected = if label.len().is_multiple_of(2) {
                                    0
                                } else if inside.width.is_multiple_of(2) {
                                    -1
                                } else {
                                    1
                                };
                                assert_eq!(
                                    offset, expected,
                                    "Both labels use the same offset within their circles"
                                );
                            } else {
                                assert_eq!(
                                    digits,
                                    [(x, y), (x + 1, y), (x, y + 1), (x + 1, y + 1)]
                                );
                            }
                            hubs.push(digits);
                        }
                        if let Some(previous) = &positions {
                            assert_eq!(
                                previous, &hubs,
                                "The readout must not rotate with the vanes"
                            );
                        }
                        positions = Some(hubs);
                    }
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
fn speaker_bars_keep_a_mirrored_bell_envelope_at_high_levels() {
    use overseer::audio::Frame;
    use ratatui::{buffer::Buffer, layout::Rect};
    let mut a = app("balanced");
    for ascii in [false, true] {
        a.cfg.ui.glyphs = if ascii { "ascii" } else { "unicode" }.into();
        let glyph = if ascii { "=" } else { "▬" };
        for height in [1, 2, 9, 10] {
            let bounds = Rect::new(3, 5, 20, height);
            for amplitude in [0., 0.125, 0.25, 1.] {
                let audio = Frame {
                    status: String::new(),
                    channels: vec![vec![amplitude; 64], vec![0.; 64]],
                };
                for left in [false, true] {
                    let mut backdrop = Buffer::empty(bounds);
                    for cell in &mut backdrop.content {
                        cell.set_symbol(".").set_fg(a.theme.wire);
                    }
                    let mut buffer = backdrop.clone();
                    ui::audio::bars(&mut buffer, bounds, Part::SpeakerLeft, &a, left, &audio);
                    let widths = (bounds.y..bounds.bottom())
                        .map(|y| {
                            let width = (bounds.x..bounds.right())
                                .filter(|&x| buffer[(x, y)].symbol() == glyph)
                                .count() as u16;
                            for x in 0..bounds.width {
                                let filled = if left {
                                    x >= bounds.width - width
                                } else {
                                    x < width
                                };
                                assert_eq!(buffer[(bounds.x + x, y)].symbol() == glyph, filled);
                                if !filled {
                                    assert_eq!(
                                        buffer[(bounds.x + x, y)],
                                        backdrop[(bounds.x + x, y)]
                                    );
                                }
                            }
                            width
                        })
                        .collect::<Vec<_>>();
                    assert!(widths.iter().eq(widths.iter().rev()));
                    assert!(
                        widths[..=height as usize / 2]
                            .windows(2)
                            .all(|w| w[0] <= w[1])
                    );
                    let peak = (amplitude * 4.).clamp(0., 1.) * bounds.width as f32;
                    assert_eq!(widths[height as usize / 2], peak.round() as u16);
                    if height >= 3 {
                        assert_eq!(widths[0], (peak * 0.5).round() as u16);
                    }
                    let mut silent = backdrop.clone();
                    ui::audio::bars(&mut silent, bounds, Part::SpeakerRight, &a, left, &audio);
                    assert_eq!(silent, backdrop);
                }
            }
        }
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
            for fan in [Part::Fan0, Part::Fan1] {
                a.act(Action::InspectPart(fan));
                render(&mut a, w, h);
                a.act(Action::Back);
            }
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

#[test]
fn systems_details_pin_samples_and_support_small_layouts() {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use overseer::app::Layer;
    let key = |code| KeyEvent::new(code, KeyModifiers::NONE);
    let mut a = app("balanced");
    let teardown = render(&mut a, 200, 55);
    a.on_key(key(KeyCode::Char('i')));
    assert!(a.layers.is_empty());
    assert_eq!(teardown, render(&mut a, 200, 55));
    for scenario in ["balanced", "missing-sensors"] {
        for (w, h) in [(60, 18), (80, 24), (120, 36), (200, 55)] {
            let mut a = app(scenario);
            for page in Page::ALL {
                a.act(Action::GoPage(page));
                a.act(Action::SystemsInfo);
                assert!(render(&mut a, w, h).contains("SYSTEMS DETAILS"));
                let pinned = match a.layers.last().unwrap() {
                    Layer::SystemsInfo { sample, .. } => sample.at,
                    _ => unreachable!(),
                };
                a.ingest(overseer::metrics::demo(scenario, pinned + 1.));
                assert!(
                    matches!(a.layers.last(), Some(Layer::SystemsInfo { sample, .. }) if sample.at == pinned)
                );
                for _ in 0..2 {
                    a.on_key(key(KeyCode::Right));
                    render(&mut a, w, h);
                }
                a.on_key(key(KeyCode::Enter));
                assert!(
                    matches!(a.layers.last(), Some(Layer::SystemsInfo { expanded, .. }) if expanded.len() == 1)
                );
                for _ in 0..40 {
                    a.on_key(key(KeyCode::Down));
                }
                render(&mut a, w, h);
                a.on_key(key(KeyCode::Esc));
                assert!(a.layers.is_empty());
            }
        }
    }
}
