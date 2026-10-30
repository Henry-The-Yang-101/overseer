use super::*;
use std::collections::{BTreeMap, VecDeque};

const MAX_SESSIONS: usize = 256;
const MAX_CALLS: usize = 2048;
const MAX_HISTORY: usize = 2048;
const MAX_ACTIVE_TOOLS: usize = 128;

struct Entry {
    session: Session,
    in_turn: bool,
    compacting: bool,
    input_requested: bool,
    ended: bool,
    boundary_ms: u64,
    closed_turns: VecDeque<String>,
    calls: BTreeMap<String, (bool, Option<bool>)>,
    call_order: VecDeque<String>,
}

/// Single-writer reducer: adapters supply facts; the renderer only reads snapshots.
#[derive(Default)]
pub struct Reducer {
    entries: BTreeMap<SessionKey, Entry>,
    events: VecDeque<Event>,
    revision: u64,
}

impl Reducer {
    pub fn apply(&mut self, event: Event, received_ms: u64) {
        if !self.entries.contains_key(&event.session)
            && self.entries.len() >= MAX_SESSIONS
            && let Some(oldest) = self
                .entries
                .iter()
                .min_by_key(|(_, e)| e.session.last_event_at_ms)
                .map(|(k, _)| k.clone())
        {
            self.entries.remove(&oldest);
        }
        let e = self
            .entries
            .entry(event.session.clone())
            .or_insert_with(|| Entry {
                session: Session {
                    key: event.session.clone(),
                    metadata: Metadata::default(),
                    phase: Phase::Unknown,
                    freshness: Freshness::Historical,
                    turn_id: None,
                    last_outcome: None,
                    last_event_at_ms: 0,
                    last_live_at_ms: None,
                    transports: Vec::new(),
                    tools: BTreeMap::new(),
                    approval_requests: Vec::new(),
                    observed_tool_calls: 0,
                    usage: Usage::default(),
                    cloud_status: None,
                },
                in_turn: false,
                compacting: false,
                input_requested: false,
                ended: false,
                boundary_ms: 0,
                closed_turns: VecDeque::new(),
                calls: BTreeMap::new(),
                call_order: VecDeque::new(),
            });
        if !e.session.transports.contains(&event.transport) {
            e.session.transports.push(event.transport);
        }
        // Metadata and counters may enrich an older observation without rewinding lifecycle.
        let enrichment = matches!(event.kind, EventKind::Metadata(_) | EventKind::Usage(_));
        let out_of_order_safe = enrichment || matches!(event.kind, EventKind::ToolFinished { .. });
        if !out_of_order_safe
            && (event.at_ms < e.boundary_ms
                || event
                    .turn_id
                    .as_ref()
                    .is_some_and(|id| e.closed_turns.contains(id)))
        {
            return;
        }
        if !out_of_order_safe
            && !matches!(
                event.kind,
                EventKind::TurnStarted | EventKind::SessionStarted
            )
            && event
                .turn_id
                .as_ref()
                .zip(e.session.turn_id.as_ref())
                .is_some_and(|(incoming, current)| incoming != current)
        {
            return;
        }
        let before = e.session.clone();
        let mut completion_evidence = false;
        match &event.kind {
            EventKind::Metadata(m) => {
                if m.cwd.is_some() {
                    e.session.metadata.cwd.clone_from(&m.cwd);
                }
                if m.title.is_some() {
                    e.session.metadata.title.clone_from(&m.title);
                }
                if m.model.is_some() {
                    e.session.metadata.model.clone_from(&m.model);
                }
                if m.version.is_some() {
                    e.session.metadata.version.clone_from(&m.version);
                }
                if m.parent_id.is_some() {
                    e.session.metadata.parent_id.clone_from(&m.parent_id);
                }
            }
            EventKind::Usage(u) => {
                if event.at_ms >= e.session.last_event_at_ms {
                    if u.input.is_some() {
                        e.session.usage.input = u.input;
                    }
                    if u.cached_input.is_some() {
                        e.session.usage.cached_input = u.cached_input;
                    }
                    if u.output.is_some() {
                        e.session.usage.output = u.output;
                    }
                    if u.context_used.is_some() {
                        e.session.usage.context_used = u.context_used;
                    }
                    if u.context_limit.is_some() {
                        e.session.usage.context_limit = u.context_limit;
                    }
                }
            }
            EventKind::SessionStarted => {
                e.ended = false;
            }
            EventKind::TurnStarted => {
                // A duplicate prompt/task_started must not clear tools or approval waits.
                if !e.in_turn || (event.turn_id.is_some() && event.turn_id != e.session.turn_id) {
                    if let Some(previous) = &e.session.turn_id
                        && event.turn_id.as_ref().is_some_and(|id| id != previous)
                    {
                        e.closed_turns.push_back(previous.clone());
                        while e.closed_turns.len() > 128 {
                            e.closed_turns.pop_front();
                        }
                    }
                    e.session.tools.clear();
                    e.session.approval_requests.clear();
                    e.compacting = false;
                    e.input_requested = false;
                }
                if event.turn_id.is_some() || !e.in_turn {
                    e.session.turn_id.clone_from(&event.turn_id);
                }
                e.in_turn = true;
                e.ended = false;
                e.boundary_ms = event.at_ms;
            }
            EventKind::TurnEnded { outcome } => {
                e.in_turn = false;
                e.compacting = false;
                e.input_requested = false;
                e.session.tools.clear();
                e.session.approval_requests.clear();
                e.session.last_outcome = Some(*outcome);
                e.boundary_ms = event.at_ms;
                if let Some(id) = event.turn_id.as_ref().or(e.session.turn_id.as_ref()) {
                    e.closed_turns.push_back(id.clone());
                    if e.closed_turns.len() > 128 {
                        e.closed_turns.pop_front();
                    }
                }
            }
            EventKind::SessionEnded => {
                e.ended = true;
                e.in_turn = false;
                e.compacting = false;
                e.input_requested = false;
                e.session.tools.clear();
                e.session.approval_requests.clear();
                e.boundary_ms = event.at_ms;
            }
            EventKind::ToolRequested { id, name } | EventKind::ToolStarted { id, name } => {
                let key = call_key(&event, id);
                if e.calls.get(&key).is_some_and(|(done, _)| *done) || e.ended {
                    return;
                }
                if !e.calls.contains_key(&key) {
                    e.remember_call(key, false, None);
                    e.session.observed_tool_calls += 1;
                }
                if e.session.tools.len() < MAX_ACTIVE_TOOLS || e.session.tools.contains_key(id) {
                    let t = e.session.tools.entry(id.clone()).or_insert_with(|| Tool {
                        name: name.clone(),
                        started_at_ms: event.at_ms,
                        running: false,
                    });
                    if t.name == "tool" && !name.is_empty() {
                        t.name.clone_from(name);
                    }
                    t.running |= matches!(event.kind, EventKind::ToolStarted { .. });
                }
                e.in_turn = true;
                if e.session.turn_id.is_none() {
                    e.session.turn_id.clone_from(&event.turn_id);
                }
            }
            EventKind::ToolFinished { id, failed } => {
                let key = call_key(&event, id);
                if e.calls
                    .get(&key)
                    .is_some_and(|(done, known)| *done && (known.is_some() || failed.is_none()))
                {
                    return;
                }
                if !e.calls.contains_key(&key) {
                    e.session.observed_tool_calls += 1;
                }
                e.remember_call(key, true, *failed);
                completion_evidence = true;
                e.session.tools.remove(id);
                e.session.approval_requests.retain(|v| v != id);
            }
            EventKind::ApprovalRequested { id } => {
                if e.calls
                    .get(&call_key(&event, id))
                    .is_some_and(|(done, _)| *done)
                {
                    return;
                }
                if !e.session.approval_requests.contains(id)
                    && e.session.approval_requests.len() < MAX_ACTIVE_TOOLS
                {
                    e.session.approval_requests.push(id.clone());
                }
            }
            EventKind::InputRequested => e.input_requested = true,
            EventKind::RequestResolved { id } => {
                e.session.approval_requests.retain(|v| v != id);
                e.input_requested = false;
            }
            EventKind::CompactStarted => e.compacting = true,
            EventKind::CompactFinished => e.compacting = false,
            EventKind::CloudStatus { status } => {
                e.session.cloud_status = Some(status.clone());
                // Cloud 'pending' does not distinguish queued work from running work.
            }
        }
        e.session.phase = if enrichment {
            // A title, model or usage record says nothing about whether a turn is active.
            before.phase
        } else if e.session.cloud_status.is_some() {
            Phase::Unknown
        } else if e.ended {
            Phase::Ended
        } else if !e.session.approval_requests.is_empty() {
            Phase::ApprovalRequested
        } else if e.input_requested {
            Phase::InputRequested
        } else if e.compacting {
            Phase::Compacting
        } else if e.in_turn {
            Phase::Working
        } else {
            Phase::Idle
        };
        let changed = before != e.session;
        e.session.last_event_at_ms = e.session.last_event_at_ms.max(event.at_ms);
        if !event.replayed && !enrichment {
            e.session.last_live_at_ms = Some(received_ms);
        }
        if (changed || completion_evidence) && !event.replayed {
            self.events.push_back(event);
            while self.events.len() > MAX_HISTORY {
                self.events.pop_front();
            }
        }
        self.revision += 1;
    }

    pub fn snapshot(&self, now: u64, stale_after_ms: u64, health: Health) -> Snapshot {
        Snapshot {
            revision: self.revision,
            captured_at_ms: now,
            sessions: self
                .entries
                .values()
                .map(|e| {
                    let mut s = e.session.clone();
                    s.freshness = match s.last_live_at_ms {
                        None => Freshness::Historical,
                        Some(at) if now.saturating_sub(at) > stale_after_ms => Freshness::Stale,
                        Some(_) => Freshness::Recent,
                    };
                    s
                })
                .collect(),
            events: self.events.iter().cloned().collect(),
            health,
        }
    }
}

impl Entry {
    fn remember_call(&mut self, key: String, complete: bool, failed: Option<bool>) {
        if !self.calls.contains_key(&key) {
            self.call_order.push_back(key.clone());
        }
        self.calls.insert(key, (complete, failed));
        while self.call_order.len() > MAX_CALLS {
            if let Some(k) = self.call_order.pop_front() {
                self.calls.remove(&k);
            }
        }
    }
}

fn call_key(event: &Event, id: &str) -> String {
    // Codex and Claude call ids are session-scoped. JSONL echoes can omit turn ids.
    format!("{}:{id}", event.session.id)
}
