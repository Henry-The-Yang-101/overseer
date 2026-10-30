use overseer::agents::{decode, reducer::Reducer, *};
use serde_json::json;

fn key() -> SessionKey {
    SessionKey {
        provider: Provider::Codex,
        scope: "/home/one/.codex".into(),
        id: "session".into(),
    }
}
fn event(at_ms: u64, kind: EventKind) -> Event {
    Event {
        session: key(),
        transport: Transport::Hook,
        at_ms,
        turn_id: Some("turn".into()),
        replayed: false,
        kind,
    }
}
fn apply(r: &mut Reducer, events: Vec<Event>) {
    for e in events {
        let at = e.at_ms;
        r.apply(e, at);
    }
}
fn snapshot(r: &Reducer) -> Snapshot {
    r.snapshot(1000, 5000, Health::default())
}

#[test]
fn duplicate_and_reordered_transports_preserve_parallel_calls_and_terminal_turns() {
    let mut r = Reducer::default();
    r.apply(event(1, EventKind::TurnStarted), 1);
    for id in ["a", "b"] {
        r.apply(
            event(
                2,
                EventKind::ToolRequested {
                    id: id.into(),
                    name: "Bash".into(),
                },
            ),
            2,
        );
        let mut echo = event(
            2,
            EventKind::ToolRequested {
                id: id.into(),
                name: "exec_command".into(),
            },
        );
        echo.transport = Transport::Hook;
        echo.turn_id = None;
        r.apply(echo, 3);
        r.apply(event(3, EventKind::ApprovalRequested { id: id.into() }), 3);
    }
    let mut prompt_echo = event(3, EventKind::TurnStarted);
    prompt_echo.turn_id = None;
    r.apply(prompt_echo, 3);
    assert_eq!(snapshot(&r).sessions[0].turn_id.as_deref(), Some("turn"));
    // Repeated and delayed hook deliveries must preserve completion.
    let mut end = event(
        4,
        EventKind::ToolFinished {
            id: "a".into(),
            failed: None,
        },
    );
    end.transport = Transport::Hook;
    r.apply(end.clone(), 4);
    r.apply(end, 5);
    r.apply(
        event(
            3,
            EventKind::ToolRequested {
                id: "a".into(),
                name: "Bash".into(),
            },
        ),
        6,
    );
    let s = &snapshot(&r).sessions[0];
    assert_eq!(s.observed_tool_calls, 2);
    assert_eq!(
        snapshot(&r)
            .events
            .iter()
            .filter(|e| matches!(e.kind, EventKind::ToolRequested { .. }))
            .count(),
        2
    );
    assert_eq!(s.approval_requests, ["b"]);
    assert_eq!(s.phase, Phase::ApprovalRequested);
    assert_eq!(s.tools.len(), 1);
    r.apply(
        event(
            7,
            EventKind::ToolFinished {
                id: "a".into(),
                failed: Some(true),
            },
        ),
        7,
    );
    assert!(snapshot(&r).events.iter().any(|e| matches!(
        e.kind,
        EventKind::ToolFinished {
            failed: Some(true),
            ..
        }
    )));
    r.apply(
        event(
            8,
            EventKind::TurnEnded {
                outcome: Outcome::Interrupted,
            },
        ),
        8,
    );
    r.apply(
        event(
            10,
            EventKind::ToolRequested {
                id: "late".into(),
                name: "Bash".into(),
            },
        ),
        10,
    );
    assert_eq!(snapshot(&r).sessions[0].phase, Phase::Idle);
    assert!(snapshot(&r).sessions[0].tools.is_empty());
    let mut next = event(12, EventKind::TurnStarted);
    next.turn_id = Some("next".into());
    r.apply(next, 12);
    assert_eq!(snapshot(&r).sessions[0].phase, Phase::Working);
    r.apply(
        event(
            15,
            EventKind::TurnEnded {
                outcome: Outcome::Completed,
            },
        ),
        15,
    );
    assert_eq!(
        snapshot(&r).sessions[0].phase,
        Phase::Working,
        "late old-turn completion cannot stop the next turn"
    );
}

#[test]
fn hook_identity_privacy_and_usage_are_shared_without_mixing_homes_or_children() {
    let mut r = Reducer::default();
    let hook = json!({"hook_event_name":"PreToolUse","session_id":"root","agent_id":"child", "turn_id":"t",
        "cwd":"/repo", "tool_use_id":"call", "tool_name":"Bash", "tool_input":{"command":"SECRET_COMMAND"}, "prompt":"SECRET_PROMPT"});
    for home in ["/one", "/two"] {
        apply(&mut r, decode::hook(Provider::Codex, home, &hook, 5));
    }
    let snap = snapshot(&r);
    assert_eq!(snap.sessions.len(), 2);
    assert!(
        snap.sessions
            .iter()
            .all(|s| s.key.id == "child" && s.metadata.parent_id.as_deref() == Some("root"))
    );
    assert!(!serde_json::to_string(&snap).unwrap().contains("SECRET"));
    for _ in 0..2 {
        let mut observation = event(
            10,
            EventKind::Usage(Usage {
                input: Some(100),
                context_used: Some(60),
                ..Usage::default()
            }),
        );
        observation.replayed = true;
        r.apply(observation, 10);
    }
    let s = snapshot(&r)
        .sessions
        .into_iter()
        .find(|s| s.key == key())
        .unwrap();
    assert_eq!(s.usage.input, Some(100));
    assert_eq!(s.usage.context_used, Some(60));
    assert_eq!(s.freshness, Freshness::Historical);
    assert_eq!(
        s.phase,
        Phase::Unknown,
        "usage alone is not lifecycle evidence"
    );
    assert_eq!(
        decode::hook(
            Provider::Claude,
            "/claude",
            &json!({"hook_event_name":"PostToolUseFailure","session_id":"c","tool_use_id":"x"}),
            10
        )
        .last()
        .unwrap()
        .kind,
        EventKind::ToolFinished {
            id: "x".into(),
            failed: Some(true)
        }
    );
}

#[test]
fn app_server_and_cloud_keep_distinct_evidence_and_staleness_does_not_claim_completion() {
    let collaboration = decode::app_server(
        "/one",
        &json!({"method":"item/completed","params":{"threadId":"t","turnId":"turn","item":{"type":"collabAgentToolCall","id":"child-call","status":"failed","prompt":"SECRET"}}}),
        10,
    );
    assert!(matches!(
        collaboration[0].kind,
        EventKind::ToolFinished {
            failed: Some(true),
            ..
        }
    ));
    assert!(
        !serde_json::to_string(&collaboration)
            .unwrap()
            .contains("SECRET")
    );
    let mut r = Reducer::default();
    apply(
        &mut r,
        decode::app_server(
            "/one",
            &json!({"method":"turn/started","params":{"threadId":"t","turn":{"id":"turn"}}}),
            10,
        ),
    );
    apply(
        &mut r,
        decode::app_server(
            "/one",
            &json!({"method":"item/started","params":{"threadId":"t","turnId":"turn","item":{"type":"commandExecution","id":"call"}}}),
            12,
        ),
    );
    apply(
        &mut r,
        decode::cloud(
            "/one",
            &json!({"tasks":[{"id":"t","title":"Cloud task","status":"pending"}]}),
            15,
        ),
    );
    let snap = r.snapshot(10000, 100, Health::default());
    let local = snap
        .sessions
        .iter()
        .find(|s| s.key.scope == "/one")
        .unwrap();
    assert_eq!(local.phase, Phase::Working);
    assert_eq!(local.freshness, Freshness::Stale);
    assert!(local.tools["call"].running);
    let cloud = snap
        .sessions
        .iter()
        .find(|s| s.key.scope == "cloud:/one")
        .unwrap();
    assert_eq!(cloud.phase, Phase::Unknown);
    assert_eq!(cloud.cloud_status.as_deref(), Some("pending"));
}

#[cfg(unix)]
#[test]
fn real_hook_transport_is_fail_open_exclusive_and_publishes_immutable_snapshots() {
    use std::{
        io::Write,
        process::{Command, Stdio},
        time::{Duration, Instant},
    };
    let directory = tempfile::Builder::new()
        .prefix("overseer-test-")
        .tempdir_in("/tmp")
        .unwrap();
    let socket = directory.path().join("agent.sock");
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let options = Options {
        socket: Some(socket.clone()),
        ..Options::default()
    };
    let monitor = Monitor::start(options.clone()).unwrap();
    assert!(
        Monitor::start(options).is_err(),
        "second reader must not unlink a live socket"
    );
    let mut child = Command::new(env!("CARGO_BIN_EXE_overseer"))
        .args(["agents", "hook", "--provider", "codex", "--socket"])
        .arg(&socket)
        .args(["--source-home", "/tmp"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(br#"{"hook_event_name":"PreToolUse","session_id":"live","turn_id":"t","tool_use_id":"call","tool_name":"Bash","tool_input":{"command":"SECRET"}}"#).unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success());
    assert!(output.stdout.is_empty() && output.stderr.is_empty());
    let until = Instant::now() + Duration::from_secs(3);
    while monitor.latest().sessions.is_empty() && Instant::now() < until {
        std::thread::sleep(Duration::from_millis(20));
    }
    let frozen = monitor.latest();
    assert_eq!(frozen.sessions.len(), 1);
    assert_eq!(frozen.sessions[0].freshness, Freshness::Recent);
    assert!(!serde_json::to_string(&frozen).unwrap().contains("SECRET"));
    drop(monitor);
    assert!(!socket.exists());
    assert_eq!(frozen.sessions[0].phase, Phase::Working);
    // Stalled stdin is bounded, too, and a missing receiver is harmless.
    let start = Instant::now();
    let mut unavailable = Command::new(env!("CARGO_BIN_EXE_overseer"))
        .args(["agents", "hook", "--provider", "claude", "--socket"])
        .arg(&socket)
        .stdin(Stdio::piped())
        .spawn()
        .unwrap();
    let _held_stdin = unavailable.stdin.take().unwrap();
    assert!(unavailable.wait().unwrap().success());
    assert!(start.elapsed() < Duration::from_secs(2));
    let output = Command::new(env!("CARGO_BIN_EXE_overseer"))
        .args(["agents", "hook-config", "--provider", "codex"])
        .output()
        .unwrap();
    let config: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!(config["hooks"]["PreToolUse"].is_array());
}

#[cfg(unix)]
#[test]
fn cloud_adapter_paginates_structured_statuses_without_blocking_or_leaving_logs() {
    use std::{
        fs,
        os::unix::fs::PermissionsExt,
        time::{Duration, Instant},
    };
    let directory = tempfile::tempdir().unwrap();
    let binary = directory.path().join("codex-fixture");
    fs::write(&binary, "#!/bin/sh\nprintf 'private diagnostics' > error.log\ncase \"$*\" in\n  *--cursor*) printf '%s' '{\"tasks\":[{\"id\":\"second\",\"status\":\"ready\"}],\"cursor\":null}' ;;\n  *) printf '%s' '{\"tasks\":[{\"id\":\"first\",\"status\":\"pending\"}],\"cursor\":\"next\"}' ;;\nesac\n").unwrap();
    fs::set_permissions(&binary, fs::Permissions::from_mode(0o700)).unwrap();
    let monitor = Monitor::start(Options {
        socket: None,
        cloud: true,
        cloud_executable: binary,
        cloud_home: directory.path().into(),
        ..Options::default()
    })
    .unwrap();
    let until = Instant::now() + Duration::from_secs(3);
    while monitor.latest().sessions.len() < 2 && Instant::now() < until {
        std::thread::sleep(Duration::from_millis(20));
    }
    let snap = monitor.latest();
    assert_eq!(snap.sessions.len(), 2);
    assert!(snap.health.last_cloud_poll_ms.is_some());
    assert!(
        snap.sessions
            .iter()
            .all(|s| s.phase == Phase::Unknown && s.key.scope.starts_with("cloud:"))
    );
    let start = Instant::now();
    drop(monitor);
    assert!(start.elapsed() < Duration::from_secs(1));
    assert!(!directory.path().join("error.log").exists());
}
