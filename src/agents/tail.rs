use super::{decode, *};
use std::{
    collections::BTreeMap,
    fs::{self, File},
    io::{BufRead, BufReader, Read, Seek, SeekFrom},
    path::{Path, PathBuf},
    time::UNIX_EPOCH,
};

const READ_BUDGET: usize = 4 * 1024 * 1024;
const RECORD_LIMIT: usize = 1024 * 1024;
const BASELINE_BYTES: u64 = 256 * 1024;
const FILE_LIMIT: usize = 128;
const ENTRY_LIMIT: usize = 20_000;

struct Cursor {
    identity: (u64, u64),
    offset: u64,
    baseline_until: u64,
    pending: Vec<u8>,
    discard_line: bool,
    key: SessionKey,
    turn: Option<String>,
    modified: Option<std::time::SystemTime>,
}

pub(crate) struct Tailer {
    homes: Vec<PathBuf>,
    cursors: BTreeMap<PathBuf, Cursor>,
    started_ms: u64,
    first_scan: bool,
    next_file: usize,
    titles: BTreeMap<SessionKey, String>,
}

impl Tailer {
    pub fn new(homes: Vec<PathBuf>) -> Self {
        Self {
            homes,
            cursors: BTreeMap::new(),
            started_ms: now_ms(),
            first_scan: true,
            next_file: 0,
            titles: BTreeMap::new(),
        }
    }

    pub fn discover(&mut self, health: &mut Health) -> Vec<Event> {
        let mut candidates = Vec::new();
        let mut entries = 0;
        health.scan_limited = false;
        for home in &self.homes {
            let mut dirs = vec![(home.join("sessions"), 0)];
            while let Some((dir, depth)) = dirs.pop() {
                if entries >= ENTRY_LIMIT {
                    health.scan_limited = true;
                    break;
                }
                let listing = match fs::read_dir(&dir) {
                    Ok(v) => v,
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
                    Err(_) => {
                        error(health, format!("Cannot scan {}", dir.display()));
                        continue;
                    }
                };
                let mut children = Vec::new();
                for item in listing {
                    entries += 1;
                    if entries > ENTRY_LIMIT {
                        health.scan_limited = true;
                        break;
                    }
                    let Ok(item) = item else {
                        continue;
                    };
                    let Ok(kind) = item.file_type() else {
                        continue;
                    };
                    // Do not traverse symlinks into unrelated homes or files.
                    if kind.is_dir() && depth < 5 {
                        children.push((item.path(), depth + 1));
                    } else if kind.is_file()
                        && item.file_name().to_string_lossy().starts_with("rollout-")
                        && item.path().extension().is_some_and(|x| x == "jsonl")
                        && let Ok(meta) = item.metadata()
                    {
                        let modified = modified_ms(&meta);
                        if now_ms().saturating_sub(modified) <= 24 * 3600 * 1000
                            || self.cursors.contains_key(&item.path())
                        {
                            candidates.push((modified, item.path(), home.clone()));
                        }
                    }
                }
                children.sort(); // LIFO visits newest date partitions first.
                dirs.extend(children);
            }
        }
        candidates.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
        if candidates.len() > FILE_LIMIT {
            health.scan_limited = true;
            candidates.truncate(FILE_LIMIT);
        }
        self.cursors
            .retain(|path, _| candidates.iter().any(|(_, p, _)| p == path));
        let mut events = Vec::new();
        for (_, path, home) in candidates {
            if !self.cursors.contains_key(&path)
                && let Some((cursor, event)) = open(&path, &home, self.first_scan, health)
            {
                self.cursors.insert(path, cursor);
                events.push(event);
            }
        }
        self.first_scan = false;
        events.extend(self.read_titles(health));
        events
    }

    fn read_titles(&mut self, health: &mut Health) -> Vec<Event> {
        let mut events = Vec::new();
        self.titles
            .retain(|key, _| self.cursors.values().any(|c| &c.key == key));
        for home in &self.homes {
            let path = home.join("session_index.jsonl");
            if !fs::symlink_metadata(&path).is_ok_and(|m| m.is_file()) {
                continue;
            }
            let Ok(mut file) = File::open(path) else {
                continue;
            };
            let len = file.metadata().map(|m| m.len()).unwrap_or(0);
            let start = len.saturating_sub(BASELINE_BYTES);
            if file.seek(SeekFrom::Start(start)).is_err() {
                continue;
            }
            let mut bytes = Vec::new();
            if file.take(BASELINE_BYTES).read_to_end(&mut bytes).is_err() {
                continue;
            }
            health.bytes_read += bytes.len() as u64;
            let mut latest = BTreeMap::new();
            for (i, line) in bytes.split_inclusive(|b| *b == b'\n').enumerate() {
                if (start > 0 && i == 0) || line.last() != Some(&b'\n') {
                    continue;
                }
                let Ok(v) = serde_json::from_slice::<serde_json::Value>(line) else {
                    continue;
                };
                let (Some(id), Some(title)) = (text(&v, "id"), text(&v, "thread_name")) else {
                    continue;
                };
                let key = SessionKey {
                    provider: Provider::Codex,
                    scope: home.to_string_lossy().into_owned(),
                    id,
                };
                if self.cursors.values().any(|c| c.key == key) {
                    latest.insert(key, title);
                }
            }
            for (key, title) in latest {
                if self.titles.get(&key) != Some(&title) {
                    self.titles.insert(key.clone(), title.clone());
                    events.push(Event {
                        session: key,
                        transport: Transport::Jsonl,
                        at_ms: 0,
                        turn_id: None,
                        replayed: true,
                        kind: EventKind::Metadata(Metadata {
                            title: Some(title),
                            ..Metadata::default()
                        }),
                    });
                }
            }
        }
        events
    }

    pub fn poll(&mut self, health: &mut Health) -> Vec<Event> {
        let mut events = Vec::new();
        let mut budget = READ_BUDGET;
        let mut cursors: Vec<_> = self.cursors.iter_mut().collect();
        let count = cursors.len();
        if count > 0 {
            cursors.rotate_left(self.next_file % count);
        }
        for (path, c) in cursors {
            if budget == 0 {
                break;
            }
            self.next_file = (self.next_file + 1) % count;
            let Ok(meta) = fs::symlink_metadata(path) else {
                continue;
            };
            if !meta.is_file() {
                continue;
            }
            let modified = meta.modified().ok();
            if identity(&meta) != c.identity
                || meta.len() < c.offset
                || (meta.len() == c.offset && modified != c.modified)
            {
                let home = PathBuf::from(&c.key.scope);
                if let Some((replacement, event)) = open(path, &home, true, health) {
                    *c = replacement;
                    events.push(event);
                } else {
                    continue;
                }
            }
            c.modified = modified;
            if meta.len() <= c.offset {
                continue;
            }
            let Ok(mut file) = File::open(path) else {
                error(health, format!("Cannot read {}", path.display()));
                continue;
            };
            if file.seek(SeekFrom::Start(c.offset)).is_err() {
                continue;
            }
            let amount = budget
                .min(512 * 1024)
                .min(meta.len().saturating_sub(c.offset) as usize);
            let mut bytes = vec![0; amount];
            let Ok(n) = file.read(&mut bytes) else {
                continue;
            };
            bytes.truncate(n);
            budget -= n;
            health.bytes_read += n as u64;
            let start = c.offset;
            c.offset += n as u64;
            for (i, byte) in bytes.into_iter().enumerate() {
                if byte == b'\n' {
                    if c.discard_line {
                        c.discard_line = false;
                        c.pending.clear();
                        continue;
                    }
                    if !c.pending.is_empty() {
                        match serde_json::from_slice::<serde_json::Value>(&c.pending) {
                            Ok(v) => {
                                let replayed = start + (i as u64) < c.baseline_until
                                    || decode::timestamp(&v["timestamp"], self.started_ms)
                                        < self.started_ms;
                                events.extend(decode::codex_line(
                                    &c.key,
                                    &v,
                                    &mut c.turn,
                                    now_ms(),
                                    replayed,
                                ));
                            }
                            Err(_) => health.malformed_records += 1,
                        }
                    }
                    c.pending.clear();
                } else if c.discard_line {
                    health.skipped_bytes += 1;
                } else if c.pending.len() < RECORD_LIMIT {
                    c.pending.push(byte);
                } else {
                    health.skipped_bytes += c.pending.len() as u64 + 1;
                    health.malformed_records += 1;
                    c.pending.clear();
                    c.discard_line = true;
                }
            }
        }
        events
    }
}

fn open(path: &Path, home: &Path, baseline: bool, health: &mut Health) -> Option<(Cursor, Event)> {
    let meta = fs::symlink_metadata(path).ok()?;
    if !meta.is_file() {
        return None;
    }
    let file = File::open(path).ok()?;
    let mut head = Vec::new();
    BufReader::new(file)
        .take(RECORD_LIMIT as u64 + 1)
        .read_until(b'\n', &mut head)
        .ok()?;
    if head.last() != Some(&b'\n') || head.len() > RECORD_LIMIT {
        return None;
    }
    health.bytes_read += head.len() as u64;
    let v: serde_json::Value = serde_json::from_slice(&head).ok()?;
    if v["type"] != "session_meta" {
        return None;
    }
    let p = &v["payload"];
    let id = text(p, "id").filter(|s| !s.is_empty())?;
    let key = SessionKey {
        provider: Provider::Codex,
        scope: home.to_string_lossy().into_owned(),
        id,
    };
    let offset = meta
        .len()
        .saturating_sub(BASELINE_BYTES)
        .max(head.len() as u64);
    health.skipped_bytes += offset.saturating_sub(head.len() as u64);
    let event = Event {
        session: key.clone(),
        transport: Transport::Jsonl,
        at_ms: decode::timestamp(&v["timestamp"], now_ms()),
        turn_id: None,
        replayed: true,
        kind: EventKind::Metadata(decode::metadata(p)),
    };
    Some((
        Cursor {
            identity: identity(&meta),
            offset,
            baseline_until: if baseline { meta.len() } else { 0 },
            pending: Vec::new(),
            discard_line: offset > head.len() as u64,
            key,
            turn: None,
            modified: meta.modified().ok(),
        },
        event,
    ))
}

fn modified_ms(meta: &fs::Metadata) -> u64 {
    meta.modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn identity(meta: &fs::Metadata) -> (u64, u64) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        (meta.dev(), meta.ino())
    }
    #[cfg(not(unix))]
    {
        (
            0,
            meta.created()
                .ok()
                .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                .map(|d| d.as_nanos() as u64)
                .unwrap_or(0),
        )
    }
}

pub(crate) fn error(health: &mut Health, message: String) {
    if !health.errors.contains(&message) {
        health.errors.push(message);
        if health.errors.len() > 16 {
            health.errors.remove(0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn baseline_partial_lines_replacement_and_truncation_do_not_resurrect_activity() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir(dir.path().join("sessions")).unwrap();
        let path = dir.path().join("sessions/rollout-test.jsonl");
        let header = "{\"type\":\"session_meta\",\"payload\":{\"id\":\"s\",\"cwd\":\"/repo\"}}\n";
        let start =
            "{\"type\":\"event_msg\",\"payload\":{\"type\":\"task_started\",\"turn_id\":\"t\"}}\n";
        fs::write(&path, format!("{header}{start}")).unwrap();
        let mut t = Tailer::new(vec![dir.path().to_owned()]);
        let mut h = Health::default();
        t.discover(&mut h);
        let baseline = t.poll(&mut h);
        assert!(baseline.iter().all(|e| e.replayed));
        let mut f = fs::OpenOptions::new().append(true).open(&path).unwrap();
        write!(f, "{{\"type\":\"event_msg\",\"payload\":").unwrap();
        assert!(t.poll(&mut h).is_empty());
        writeln!(f, "{{\"type\":\"task_complete\",\"turn_id\":\"t\"}}}}").unwrap();
        let end = t.poll(&mut h);
        assert_eq!(end.len(), 1);
        assert!(!end[0].replayed);
        assert!(t.poll(&mut h).is_empty());
        drop(f);
        fs::write(&path, header).unwrap();
        t.poll(&mut h);
        let replacement = dir.path().join("replacement");
        fs::write(&replacement, format!("{header}{start}")).unwrap();
        fs::rename(replacement, &path).unwrap();
        let recovered = t.poll(&mut h);
        assert!(recovered.iter().all(|e| e.replayed));
        assert_eq!(h.malformed_records, 0);
    }
}
