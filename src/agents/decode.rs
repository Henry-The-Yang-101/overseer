//! Small projections of official hook/app-server fields. Unknown fields are ignored.
use super::*;
use serde_json::Value;

fn event(
    key: &SessionKey,
    transport: Transport,
    at_ms: u64,
    turn_id: Option<String>,
    replayed: bool,
    kind: EventKind,
) -> Event {
    Event {
        session: key.clone(),
        transport,
        at_ms,
        turn_id,
        replayed,
        kind,
    }
}

/// Official command-hook JSON input. Raw prompts, arguments and responses never leave this function.
pub fn hook(provider: Provider, scope: &str, v: &Value, received: u64) -> Vec<Event> {
    let Some(session_id) = text(v, "session_id").filter(|s| !s.is_empty()) else {
        return vec![];
    };
    let Some(name) = v.get("hook_event_name").and_then(Value::as_str) else {
        return vec![];
    };
    let child = text(v, "agent_id");
    let key = SessionKey {
        provider,
        scope: scope.into(),
        id: child.clone().unwrap_or_else(|| session_id.clone()),
    };
    // These lifecycle hooks are delivered on the parent's turn, not the child's.
    let turn = if matches!(name, "SubagentStart" | "SubagentStop") {
        None
    } else {
        text(v, "turn_id")
    };
    let make = |kind| event(&key, Transport::Hook, received, turn.clone(), false, kind);
    let mut events = vec![make(EventKind::Metadata(Metadata {
        cwd: text(v, "cwd"),
        model: text(v, "model"),
        parent_id: child.map(|_| session_id),
        ..Metadata::default()
    }))];
    let call = text(v, "tool_use_id");
    let tool = text(v, "tool_name").unwrap_or_else(|| "tool".into());
    let kind = match name {
        "SessionStart" | "SubagentStart" => EventKind::SessionStarted,
        "SessionEnd" => EventKind::SessionEnded,
        "UserPromptSubmit" => EventKind::TurnStarted,
        "Stop" | "SubagentStop" => EventKind::TurnEnded {
            outcome: Outcome::Completed,
        },
        "StopFailure" => EventKind::TurnEnded {
            outcome: Outcome::Failed,
        },
        "Interrupt" => EventKind::TurnEnded {
            outcome: Outcome::Interrupted,
        },
        "PreToolUse" => match call {
            Some(id) => EventKind::ToolRequested { id, name: tool },
            None => return events,
        },
        "PostToolUse" | "PostToolUseFailure" => match call {
            Some(id) => EventKind::ToolFinished {
                id,
                failed: if name == "PostToolUseFailure" {
                    Some(true)
                } else {
                    v.pointer("/tool_response/exit_code")
                        .and_then(Value::as_i64)
                        .map(|n| n != 0)
                        .or_else(|| v.pointer("/tool_response/isError").and_then(Value::as_bool))
                },
            },
            None => return events,
        },
        "PermissionRequest" => match call {
            Some(id) => EventKind::ApprovalRequested { id },
            None => EventKind::InputRequested,
        },
        "PreCompact" => EventKind::CompactStarted,
        "PostCompact" => EventKind::CompactFinished,
        "Notification"
            if matches!(
                v.get("notification_type").and_then(Value::as_str),
                Some("permission_prompt" | "elicitation_dialog")
            ) =>
        {
            EventKind::InputRequested
        }
        _ => return events,
    };
    events.push(make(kind));
    events
}

/// Adapter seam for a future connection to an explicitly selected app-server.
/// This function only projects notifications; it does not resume or subscribe to threads.
pub fn app_server(scope: &str, v: &Value, received: u64) -> Vec<Event> {
    let p = &v["params"];
    let Some(id) = text(p, "threadId").or_else(|| text(&p["thread"], "id")) else {
        return vec![];
    };
    let key = SessionKey {
        provider: Provider::Codex,
        scope: scope.into(),
        id,
    };
    let turn = text(p, "turnId").or_else(|| text(&p["turn"], "id"));
    let make = |kind| {
        event(
            &key,
            Transport::AppServer,
            received,
            turn.clone(),
            false,
            kind,
        )
    };
    let method = v["method"].as_str().unwrap_or_default();
    let kind = match method {
        "thread/started" => EventKind::SessionStarted,
        "thread/closed" => EventKind::SessionEnded,
        "turn/started" => EventKind::TurnStarted,
        "turn/completed" => EventKind::TurnEnded {
            outcome: match p["turn"]["status"].as_str() {
                Some("completed") => Outcome::Completed,
                Some("interrupted") => Outcome::Interrupted,
                Some("failed") => Outcome::Failed,
                _ => return vec![],
            },
        },
        "item/started" | "item/completed" => {
            let item = &p["item"];
            let kind = item["type"].as_str().unwrap_or_default();
            if kind == "contextCompaction" {
                if method == "item/started" {
                    EventKind::CompactStarted
                } else {
                    EventKind::CompactFinished
                }
            } else if matches!(
                kind,
                "commandExecution"
                    | "fileChange"
                    | "mcpToolCall"
                    | "dynamicToolCall"
                    | "collabAgentToolCall"
                    | "webSearch"
            ) {
                let Some(id) = text(item, "id") else {
                    return vec![];
                };
                if method == "item/started" {
                    EventKind::ToolStarted {
                        id,
                        name: kind.into(),
                    }
                } else {
                    EventKind::ToolFinished {
                        id,
                        failed: item
                            .get("exitCode")
                            .and_then(Value::as_i64)
                            .map(|n| n != 0)
                            .or_else(|| item.get("success").and_then(Value::as_bool).map(|ok| !ok))
                            .or_else(|| match item["status"].as_str() {
                                Some("failed" | "declined") => Some(true),
                                Some("completed") => Some(false),
                                _ => None,
                            }),
                    }
                }
            } else {
                return vec![];
            }
        }
        "item/commandExecution/requestApproval" | "item/fileChange/requestApproval" => {
            let Some(id) = text(p, "itemId") else {
                return vec![];
            };
            EventKind::ApprovalRequested { id }
        }
        _ => return vec![],
    };
    vec![make(kind)]
}

pub fn cloud(scope: &str, v: &Value, received: u64) -> Vec<Event> {
    let Some(tasks) = v["tasks"].as_array() else {
        return vec![];
    };
    tasks
        .iter()
        .filter_map(|t| {
            let id = text(t, "id")?;
            let status = text(t, "status")?;
            let key = SessionKey {
                provider: Provider::Codex,
                scope: format!("cloud:{scope}"),
                id,
            };
            let make = |kind| event(&key, Transport::Cloud, received, None, false, kind);
            Some([
                make(EventKind::Metadata(Metadata {
                    title: text(t, "title"),
                    ..Metadata::default()
                })),
                make(EventKind::CloudStatus { status }),
            ])
        })
        .flatten()
        .collect()
}
