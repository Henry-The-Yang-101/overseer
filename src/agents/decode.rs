//! Small projections of official hook/app-server fields. Unknown fields are ignored.
//! Rollout decoding is deliberately isolated: persisted JSONL is not a stable API.
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

pub fn timestamp(v: &Value, fallback: u64) -> u64 {
    v.as_str()
        .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
        .and_then(|d| u64::try_from(d.timestamp_millis()).ok())
        .unwrap_or(fallback)
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

pub fn metadata(v: &Value) -> Metadata {
    Metadata {
        cwd: text(v, "cwd"),
        title: text(v, "thread_name"),
        model: text(v, "model"),
        version: text(v, "cli_version"),
        parent_id: text(v, "parent_thread_id").or_else(|| {
            v.pointer("/source/subagent/thread_spawn/parent_thread_id")
                .and_then(Value::as_str)
                .map(clean)
        }),
    }
}

fn usage(v: &Value) -> Usage {
    Usage {
        input: v.get("input_tokens").and_then(Value::as_u64),
        cached_input: v.get("cached_input_tokens").and_then(Value::as_u64),
        output: v.get("output_tokens").and_then(Value::as_u64),
        ..Usage::default()
    }
}

/// A rollout's session identity comes from its session_meta header, never its working directory.
pub fn codex_line(
    key: &SessionKey,
    v: &Value,
    current_turn: &mut Option<String>,
    received: u64,
    replayed: bool,
) -> Vec<Event> {
    let p = &v["payload"];
    let typ = v["type"].as_str().unwrap_or_default();
    let at = timestamp(&v["timestamp"], received).min(received.saturating_add(5000));
    if typ == "turn_context" || (typ == "event_msg" && p["type"] == "task_started") {
        *current_turn = text(p, "turn_id");
    }
    let turn = text(p, "turn_id").or_else(|| current_turn.clone());
    let make = |kind| event(key, Transport::Jsonl, at, turn.clone(), replayed, kind);
    let kind = match typ {
        "session_meta" | "turn_context" => EventKind::Metadata(metadata(p)),
        "token_usage_record" => EventKind::Usage(usage(&p["thread_token_usage"])),
        "event_msg" => match p["type"].as_str().unwrap_or_default() {
            "task_started" => EventKind::TurnStarted,
            "task_complete" => EventKind::TurnEnded {
                outcome: Outcome::Completed,
            },
            "turn_aborted" => EventKind::TurnEnded {
                outcome: Outcome::Interrupted,
            },
            "token_count" if p["info"].is_object() => {
                let info = &p["info"];
                let mut u = usage(&info["total_token_usage"]);
                u.context_used = info["last_token_usage"]["total_tokens"].as_u64();
                u.context_limit = info["model_context_window"].as_u64();
                EventKind::Usage(u)
            }
            "context_compacted" => EventKind::CompactFinished,
            _ => return vec![],
        },
        "response_item" => {
            let id = text(p, "call_id").or_else(|| text(p, "id"));
            match (p["type"].as_str().unwrap_or_default(), id) {
                ("function_call" | "custom_tool_call", Some(id)) => EventKind::ToolRequested {
                    id,
                    name: text(p, "name").unwrap_or_else(|| "tool".into()),
                },
                ("function_call_output" | "custom_tool_call_output", Some(id)) => {
                    EventKind::ToolFinished { id, failed: None }
                }
                // Hosted web search bypasses command hooks, but has a structured rollout item.
                ("web_search_call", Some(id)) if p["status"] == "completed" => {
                    EventKind::ToolFinished {
                        id,
                        failed: Some(false),
                    }
                }
                ("web_search_call", Some(id)) => EventKind::ToolStarted {
                    id,
                    name: "web_search".into(),
                },
                _ => return vec![],
            }
        }
        _ => return vec![],
    };
    vec![make(kind)]
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
