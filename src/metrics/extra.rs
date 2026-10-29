//! Optional Systems telemetry. Kept separate from the teardown's existing measurements.
use super::{Avail, Proc};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Wifi {
    pub interface: String,
    pub rssi: i32,
    pub noise: Option<i32>,
    pub channel: u32,
    pub tx_mbps: Option<f64>,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Route {
    pub family: String,
    pub interface: String,
    pub gateway: String,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Assertion {
    pub pid: u32,
    pub kind: String,
    pub reason: String,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Adapter {
    pub watts: Option<f64>,
    pub milliamps: Option<f64>,
}
#[derive(Clone, Debug, Serialize)]
pub struct DiskActivity {
    pub name: String,
    pub reads: f64,
    pub writes: f64,
    pub read_size: Option<f64>,
    pub write_size: Option<f64>,
}
#[derive(Clone, Debug, Serialize)]
pub struct Traffic {
    pub pid: u32,
    pub start: u64,
    pub read: f64,
    pub write: f64,
}
#[derive(Clone, Debug, Default, Serialize)]
pub struct Snapshot {
    pub at: f64,
    pub wifi: Avail<Wifi>,
    pub routes: Avail<Vec<Route>>,
    pub assertions: Avail<Vec<Assertion>>,
    pub adapter: Avail<Adapter>,
    pub disks: Avail<Vec<DiskActivity>>,
    pub traffic: Avail<Vec<Traffic>>,
}
impl Snapshot {
    pub fn stale(&self, at: f64) -> bool {
        at - self.at > 15.
            && (self.wifi.value().is_some()
                || self.routes.value().is_some()
                || self.assertions.value().is_some()
                || self.adapter.value().is_some()
                || self.disks.value().is_some()
                || self.traffic.value().is_some())
    }
    pub fn traffic(&self, p: &Proc, at: f64) -> Option<&Traffic> {
        if at - self.at > 15. || at < self.at {
            return None;
        }
        self.traffic
            .value()?
            .iter()
            .find(|t| (t.pid, t.start) == p.key())
    }
}

#[cfg(target_os = "macos")]
mod macos {
    use super::*;
    use std::{
        collections::HashMap,
        io::Read,
        process::{Command, Stdio},
        time::{Duration, Instant},
    };

    #[derive(Deserialize)]
    struct RawDisk {
        id: u64,
        name: String,
        reads: u64,
        writes: u64,
        read_bytes: u64,
        write_bytes: u64,
    }
    #[derive(Default, Deserialize)]
    struct Native {
        wifi: Option<Wifi>,
        routes: Option<Vec<Route>>,
        assertions: Option<Vec<Assertion>>,
        adapter: Option<Adapter>,
        disks: Option<Vec<RawDisk>>,
    }
    pub fn install() -> Result<std::path::PathBuf, String> {
        use std::os::unix::fs::PermissionsExt;
        let path = crate::config::Config::path()
            .parent()
            .unwrap()
            .join("bin/overseer-stats");
        let binary = include_bytes!(concat!(env!("OUT_DIR"), "/overseer-stats"));
        std::fs::create_dir_all(path.parent().unwrap()).map_err(|e| e.to_string())?;
        if std::fs::read(&path).ok().as_deref() != Some(binary.as_slice()) {
            let tmp = path.with_extension(format!("{}.tmp", std::process::id()));
            std::fs::write(&tmp, binary).map_err(|e| e.to_string())?;
            std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o700))
                .map_err(|e| e.to_string())?;
            std::fs::rename(tmp, &path).map_err(|e| e.to_string())?;
        }
        Ok(path)
    }
    fn output(program: &std::path::Path, args: &[&str]) -> Result<String, String> {
        let mut child = Command::new(program)
            .args(args)
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| e.to_string())?;
        let mut stdout = child.stdout.take().unwrap();
        let reader = std::thread::spawn(move || {
            let mut data = Vec::new();
            let mut buffer = [0; 8192];
            while let Ok(n) = stdout.read(&mut buffer) {
                if n == 0 {
                    break;
                }
                if data.len() + n > 4_000_000 {
                    return Err("telemetry output too large".into());
                }
                data.extend_from_slice(&buffer[..n]);
            }
            String::from_utf8(data).map_err(|_| "invalid telemetry text".into())
        });
        let start = Instant::now();
        let status = loop {
            match child.try_wait() {
                Ok(Some(status)) => break Some(status),
                Ok(None) if start.elapsed() < Duration::from_secs(3) => {
                    std::thread::sleep(Duration::from_millis(25))
                }
                _ => {
                    let _ = child.kill();
                    let _ = child.wait();
                    break None;
                }
            }
        };
        let data = reader
            .join()
            .map_err(|_| "telemetry reader stopped".to_string())?;
        match status {
            Some(s) if s.success() => data,
            Some(_) => Err("telemetry command failed or access restricted".into()),
            None => Err("telemetry command timed out".into()),
        }
    }
    fn identities() -> HashMap<u32, u64> {
        let Ok(mut system) = sysinfo::System::new() else {
            return HashMap::new();
        };
        system.refresh_processes_specifics(
            sysinfo::ProcessesToUpdate::All,
            true,
            sysinfo::ProcessRefreshKind::nothing(),
        );
        system
            .processes()
            .iter()
            .map(|(pid, p)| (pid.as_u32(), p.start_time()))
            .collect()
    }
    type NetworkCounters = HashMap<(u32, u64), (u64, u64)>;
    #[derive(Default)]
    pub struct Reader {
        disks: Option<(Instant, Vec<RawDisk>)>,
        network: Option<(Instant, NetworkCounters)>,
    }
    impl Reader {
        pub fn sample(
            &mut self,
            path: &Result<std::path::PathBuf, String>,
            start: Instant,
        ) -> Snapshot {
            let mut s = Snapshot::default();
            let native = path
                .as_ref()
                .map_err(Clone::clone)
                .and_then(|p| output(p, &[]))
                .and_then(|json| serde_json::from_str::<Native>(&json).map_err(|e| e.to_string()));
            match native {
                Ok(n) => {
                    s.wifi = n.wifi.map(Avail::Ok).unwrap_or(Avail::Unsupported(
                        "Wi-Fi disconnected or reading restricted".into(),
                    ));
                    s.routes = n
                        .routes
                        .map(Avail::Ok)
                        .unwrap_or(Avail::Failed("route state unavailable".into()));
                    s.assertions = n
                        .assertions
                        .map(Avail::Ok)
                        .unwrap_or(Avail::Permission("sleep assertions unavailable".into()));
                    s.adapter = n
                        .adapter
                        .map(Avail::Ok)
                        .unwrap_or(Avail::NoHardware("no adapter details reported".into()));
                    if let Some(disks) = n.disks {
                        let now = Instant::now();
                        s.disks = if let Some((before, old)) = &self.disks {
                            let dt = now.duration_since(*before).as_secs_f64();
                            Avail::Ok(
                                disks
                                    .iter()
                                    .filter_map(|d| {
                                        let old = old.iter().find(|o| o.id == d.id)?;
                                        let reads = d.reads.checked_sub(old.reads)?;
                                        let writes = d.writes.checked_sub(old.writes)?;
                                        let read_bytes =
                                            d.read_bytes.checked_sub(old.read_bytes)?;
                                        let write_bytes =
                                            d.write_bytes.checked_sub(old.write_bytes)?;
                                        Some(DiskActivity {
                                            name: d.name.clone(),
                                            reads: reads as f64 / dt,
                                            writes: writes as f64 / dt,
                                            read_size: (reads > 0)
                                                .then(|| read_bytes as f64 / reads as f64),
                                            write_size: (writes > 0)
                                                .then(|| write_bytes as f64 / writes as f64),
                                        })
                                    })
                                    .collect(),
                            )
                        } else {
                            Avail::Acquiring
                        };
                        self.disks = Some((now, disks));
                    } else {
                        s.disks = Avail::Unsupported("disk operation counters unavailable".into());
                        self.disks = None;
                    }
                }
                Err(e) => {
                    s.wifi = Avail::Failed(e.clone());
                    s.routes = Avail::Failed(e.clone());
                    s.assertions = Avail::Failed(e.clone());
                    s.adapter = Avail::Failed(e.clone());
                    s.disks = Avail::Failed(e);
                    self.disks = None;
                }
            }
            let before = identities();
            let network = output(
                std::path::Path::new("/usr/bin/nettop"),
                &[
                    "-P",
                    "-L",
                    "1",
                    "-n",
                    "-x",
                    "-t",
                    "external",
                    "-J",
                    "bytes_in,bytes_out",
                ],
            );
            let now = Instant::now();
            let after = identities();
            s.traffic = match network.and_then(|text| super::parse_nettop(&text)) {
                Ok(counters) => {
                    let counters: HashMap<_, _> = counters
                        .into_iter()
                        .filter_map(|(pid, r, w)| {
                            let start = before.get(&pid)?;
                            (after.get(&pid) == Some(start)).then_some(((pid, *start), (r, w)))
                        })
                        .collect();
                    let values = if let Some((at, old)) = &self.network {
                        let dt = now.duration_since(*at).as_secs_f64();
                        Avail::Ok(
                            counters
                                .iter()
                                .filter_map(|(&(pid, start), &(r, w))| {
                                    let (old_r, old_w) = old.get(&(pid, start))?;
                                    Some(Traffic {
                                        pid,
                                        start,
                                        read: r.checked_sub(*old_r)? as f64 / dt,
                                        write: w.checked_sub(*old_w)? as f64 / dt,
                                    })
                                })
                                .collect(),
                        )
                    } else {
                        Avail::Acquiring
                    };
                    self.network = Some((now, counters));
                    values
                }
                Err(e) => {
                    self.network = None;
                    Avail::Failed(e)
                }
            };
            s.at = start.elapsed().as_secs_f64();
            s
        }
    }
}

/// Parse from the right because process names may contain commas and dots.
#[cfg(any(target_os = "macos", test))]
fn parse_nettop(text: &str) -> Result<Vec<(u32, u64, u64)>, String> {
    if !text
        .lines()
        .next()
        .is_some_and(|l| l.trim() == ",bytes_in,bytes_out,")
    {
        return Err("unrecognized nettop columns".into());
    }
    let mut out = Vec::new();
    for line in text.lines().skip(1).filter(|l| !l.trim().is_empty()) {
        let mut fields = line.trim_end_matches(',').rsplitn(3, ',');
        let row = (|| {
            let write = fields.next()?.parse().ok()?;
            let read = fields.next()?.parse().ok()?;
            let pid = fields.next()?.rsplit_once('.')?.1.parse().ok()?;
            Some((pid, read, write))
        })();
        out.push(row.ok_or_else(|| "unrecognized nettop record".to_string())?);
    }
    Ok(out)
}

pub fn spawn(start: std::time::Instant) -> std::sync::mpsc::Receiver<Arc<Snapshot>> {
    let (tx, rx) = std::sync::mpsc::sync_channel(1);
    std::thread::spawn(move || {
        #[cfg(target_os = "macos")]
        {
            let path = macos::install();
            let mut reader = macos::Reader::default();
            loop {
                let began = std::time::Instant::now();
                if tx.send(Arc::new(reader.sample(&path, start))).is_err() {
                    break;
                }
                std::thread::sleep(
                    std::time::Duration::from_secs(5).saturating_sub(began.elapsed()),
                );
            }
        }
        #[cfg(not(target_os = "macos"))]
        {
            let _ = start;
            let why = "extended telemetry requires macOS".to_string();
            let _ = tx.send(Arc::new(Snapshot {
                wifi: Avail::Unsupported(why.clone()),
                routes: Avail::Unsupported(why.clone()),
                assertions: Avail::Unsupported(why.clone()),
                adapter: Avail::Unsupported(why.clone()),
                disks: Avail::Unsupported(why.clone()),
                traffic: Avail::Unsupported(why),
                ..Snapshot::default()
            }));
        }
    });
    rx
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn traffic_parser_and_identity_guard() {
        let rows = parse_nettop(",bytes_in,bytes_out,\nApp, Helper.thing.42,120,250,\n").unwrap();
        assert_eq!(rows, vec![(42, 120, 250)]);
        assert!(parse_nettop("permission denied").is_err());
        assert!(parse_nettop(",bytes_in,bytes_out,\napp.42,no,5,").is_err());
        let s = Snapshot {
            at: 10.,
            traffic: Avail::Ok(vec![Traffic {
                pid: 42,
                start: 5,
                read: 0.,
                write: 20.,
            }]),
            ..Snapshot::default()
        };
        let mut p = Proc {
            pid: 42,
            start: 5,
            ..Proc::default()
        };
        assert!(s.traffic(&p, 12.).is_some());
        assert!(s.traffic(&p, 26.).is_none());
        assert!(s.stale(26.));
        assert!(!Snapshot::default().stale(26.));
        assert!(s.traffic(&p, 9.).is_none());
        p.start = 6;
        assert!(s.traffic(&p, 12.).is_none());
    }
}
