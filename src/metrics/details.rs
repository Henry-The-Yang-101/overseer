use super::{Avail, Proc};
use std::{
    io::Read,
    process::{Command, Stdio},
    sync::mpsc::{self, Receiver},
    time::{Duration, Instant},
};
#[derive(Clone, Debug)]
pub struct ProcessDetails {
    pub key: (u32, u64),
    pub tab: usize,
    pub lines: Avail<Vec<String>>,
}
/// Collect on demand, away from the render loop. Results are never substituted for historical data.
pub fn request(p: Proc, tab: usize) -> Receiver<ProcessDetails> {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let result = query(&p, tab);
        let _ = tx.send(ProcessDetails {
            key: p.key(),
            tab,
            lines: result,
        });
    });
    rx
}
fn query(p: &Proc, tab: usize) -> Avail<Vec<String>> {
    let Ok(mut sys) = sysinfo::System::new() else {
        return Avail::Failed("process access failed".into());
    };
    let pid = sysinfo::Pid::from_u32(p.pid);
    sys.refresh_processes(sysinfo::ProcessesToUpdate::Some(&[pid]), true);
    if !sys
        .process(pid)
        .is_some_and(|live| live.start_time() == p.start)
    {
        return Avail::Failed("process exited or PID was reused".into());
    }
    #[cfg(target_os = "macos")]
    let program = "/usr/sbin/lsof";
    #[cfg(not(target_os = "macos"))]
    let program = "lsof";
    let mut cmd = Command::new(program);
    cmd.args(["-nP", "-a", "-p", &p.pid.to_string()]);
    if tab == 4 {
        cmd.arg("-i");
    }
    let Ok(mut child) = cmd.stdout(Stdio::piped()).stderr(Stdio::null()).spawn() else {
        return Avail::Unsupported("lsof is not installed".into());
    };
    let mut stdout = child.stdout.take().unwrap();
    let reader = std::thread::spawn(move || {
        let mut data = String::new();
        let _ = stdout.by_ref().take(1_000_000).read_to_string(&mut data);
        data
    });
    let start = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(s)) => break Some(s),
            Ok(None) if start.elapsed() < Duration::from_secs(4) => {
                std::thread::sleep(Duration::from_millis(25))
            }
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                break None;
            }
        }
    };
    let text = reader.join().unwrap_or_default();
    if status.is_none() {
        return Avail::Failed("inspection timed out".into());
    }
    let lines: Vec<_> = text
        .lines()
        .map(|s| s.chars().filter(|c| !c.is_control()).collect::<String>())
        .take(2000)
        .collect();
    if lines.is_empty() {
        return if tab == 4 {
            Avail::Ok(vec!["No network sockets reported for this process.".into()])
        } else {
            Avail::Permission("No readable file records. macOS may restrict this process.".into())
        };
    }
    Avail::Ok(lines)
}
