//! UI-independent agent observations. No prompts, arguments, or tool output are retained.
pub mod cli;
pub mod decode;
pub mod reducer;
mod runtime;
mod transport;
pub use runtime::{Monitor, Options};

use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    time::{SystemTime, UNIX_EPOCH},
};

pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

#[derive(
    Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize, clap::ValueEnum,
)]
#[serde(rename_all = "snake_case")]
pub enum Provider {
    Codex,
    Claude,
}

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
pub struct SessionKey {
    pub provider: Provider,
    /// Canonical local agent home, or a separately namespaced cloud account scope.
    pub scope: String,
    pub id: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Transport {
    Hook,
    AppServer,
    Cloud,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Metadata {
    pub cwd: Option<String>,
    pub title: Option<String>,
    pub model: Option<String>,
    pub version: Option<String>,
    pub parent_id: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Usage {
    pub input: Option<u64>,
    pub cached_input: Option<u64>,
    pub output: Option<u64>,
    pub context_used: Option<u64>,
    pub context_limit: Option<u64>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    Completed,
    Interrupted,
    Failed,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum EventKind {
    Metadata(Metadata),
    SessionStarted,
    SessionEnded,
    TurnStarted,
    TurnEnded { outcome: Outcome },
    ToolRequested { id: String, name: String },
    ToolStarted { id: String, name: String },
    ToolFinished { id: String, failed: Option<bool> },
    ApprovalRequested { id: String },
    InputRequested,
    RequestResolved { id: String },
    CompactStarted,
    CompactFinished,
    Usage(Usage),
    CloudStatus { status: String },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Event {
    pub session: SessionKey,
    pub transport: Transport,
    pub at_ms: u64,
    pub turn_id: Option<String>,
    /// Baseline/recovery reads must never masquerade as newly observed activity.
    pub replayed: bool,
    pub kind: EventKind,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    Unknown,
    Idle,
    Working,
    ApprovalRequested,
    InputRequested,
    Compacting,
    Ended,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Freshness {
    Historical,
    Recent,
    Stale,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Tool {
    pub name: String,
    pub started_at_ms: u64,
    /// A pre-tool hook reports intent, not proof that execution passed approval.
    pub running: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Session {
    pub key: SessionKey,
    pub metadata: Metadata,
    pub phase: Phase,
    pub freshness: Freshness,
    pub turn_id: Option<String>,
    pub last_outcome: Option<Outcome>,
    pub last_event_at_ms: u64,
    pub last_live_at_ms: Option<u64>,
    pub transports: Vec<Transport>,
    pub tools: BTreeMap<String, Tool>,
    pub approval_requests: Vec<String>,
    pub observed_tool_calls: u64,
    pub usage: Usage,
    pub cloud_status: Option<String>,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct Health {
    pub malformed_records: u64,
    pub cloud_truncated: bool,
    pub last_cloud_poll_ms: Option<u64>,
    pub errors: Vec<String>,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct Snapshot {
    pub revision: u64,
    pub captured_at_ms: u64,
    pub sessions: Vec<Session>,
    /// Bounded, normalized recent activity for a future timeline; excludes baseline replay.
    pub events: Vec<Event>,
    pub health: Health,
}

pub(crate) fn text(value: &serde_json::Value, field: &str) -> Option<String> {
    value.get(field)?.as_str().map(clean)
}

pub(crate) fn clean(s: &str) -> String {
    s.chars().filter(|c| !c.is_control()).take(512).collect()
}
