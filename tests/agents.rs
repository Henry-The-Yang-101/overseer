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
        echo.transport = Transport::Jsonl;
        echo.turn_id = None;
        r.apply(echo, 3);
        r.apply(event(3, EventKind::ApprovalRequested { id: id.into() }), 3);
    }
    // A transcript completion still matters when the post-tool hook was lost.
    let mut end = event(
        4,
        EventKind::ToolFinished {
            id: "a".into(),
            failed: None,
        },
    );
    end.transport = Transport::Jsonl;
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
    assert_eq!(s.approval_requests, ["b"]);
    assert_eq!(s.phase, Phase::ApprovalRequested);
    assert_eq!(s.tools.len(), 1);
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
    let mut turn = None;
    let usage = json!({"type":"event_msg","payload":{"type":"token_count","info":{
        "total_token_usage":{"input_tokens":100,"output_tokens":20}, "last_token_usage":{"total_tokens":60}, "model_context_window":128000}}});
    apply(
        &mut r,
        decode::codex_line(&key(), &usage, &mut turn, 10, true),
    );
    apply(
        &mut r,
        decode::codex_line(&key(), &usage, &mut turn, 10, true),
    );
    let s = snapshot(&r)
        .sessions
        .into_iter()
        .find(|s| s.key == key())
        .unwrap();
    assert_eq!(s.usage.input, Some(100));
    assert_eq!(s.usage.context_used, Some(60));
    assert_eq!(s.freshness, Freshness::Historical);
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
