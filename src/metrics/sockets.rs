use super::{Avail, Proc};
use std::{
    collections::{BTreeMap, HashMap},
    io::Read,
    process::{Command, Stdio},
    sync::{
        Arc,
        mpsc::{self, Receiver},
    },
    time::{Duration, Instant},
};
pub struct Snapshot {
    pub counts: HashMap<(u32, u64), u32>,
    pub states: Avail<Vec<(String, u32)>>,
}
/// Socket ownership only, on a separate worker; no process traffic is inferred.
pub fn request(processes: Arc<Vec<Proc>>) -> Receiver<Snapshot> {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(collect(&processes));
    });
    rx
}
fn collect(processes: &[Proc]) -> Snapshot {
    let mut out = Snapshot {
        counts: HashMap::new(),
        states: Avail::Acquiring,
    };
    #[cfg(target_os = "macos")]
    let program = "/usr/sbin/lsof";
    #[cfg(not(target_os = "macos"))]
    let program = "lsof";
    let Ok(mut child) = Command::new(program)
        .args(["-nP", "-i", "-FpfT"])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
    else {
        out.states = Avail::Unsupported("socket ownership unavailable".into());
        return out;
    };
    let stdout = child.stdout.take().unwrap();
    let reader = std::thread::spawn(move || {
        let mut s = String::new();
        let _ = stdout.take(2_000_000).read_to_string(&mut s);
        s
    });
    let began = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if began.elapsed() < Duration::from_secs(4) => {
                std::thread::sleep(Duration::from_millis(50))
            }
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                out.states = Avail::Failed("socket scan timed out".into());
                break;
            }
        }
    }
    let text = reader.join().unwrap_or_default();
    if matches!(out.states, Avail::Failed(_)) {
        return out;
    }
    let ids: HashMap<_, _> = processes.iter().map(|p| (p.pid, p.key())).collect();
    let mut current = None;
    let mut states: BTreeMap<String, u32> = BTreeMap::new();
    for line in text.lines() {
        if let Some(pid) = line.strip_prefix('p') {
            current = pid.parse::<u32>().ok().and_then(|p| ids.get(&p).copied());
        } else if line.starts_with('f') {
            if let Some(key) = current {
                *out.counts.entry(key).or_default() += 1;
            }
        } else if let Some(state) = line.strip_prefix("TST=") {
            *states.entry(state.into()).or_default() += 1;
        }
    }
    out.states = if text.is_empty() {
        Avail::Permission("no visible sockets; macOS may restrict access".into())
    } else {
        Avail::Ok(states.into_iter().collect())
    };
    out
}
