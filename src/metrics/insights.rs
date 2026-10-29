use super::{Proc, Sample};
use std::{
    collections::{BTreeMap, HashMap, HashSet, VecDeque},
    sync::Arc,
};

#[derive(Clone, Debug)]
pub struct Event {
    pub at: f64,
    pub text: String,
}
pub fn events(before: &Sample, now: &Sample) -> Vec<Event> {
    let mut text = vec![];
    if now.at <= before.at || now.at - before.at > 10. {
        return vec![];
    }
    if let (Some(a), Some(b)) = (before.battery.value(), now.battery.value())
        && a.ac != b.ac
    {
        text.push(
            if b.ac {
                "AC connected"
            } else {
                "AC disconnected"
            }
            .into(),
        );
    }
    if let (Some(a), Some(b)) = (before.power_mode.value(), now.power_mode.value())
        && a != b
    {
        text.push(format!("Power mode: {}", b.label()));
    }
    if let (Some(a), Some(b)) = (before.thermal.value(), now.thermal.value())
        && a != b
    {
        text.push(format!("Thermal: {a} → {b}"));
    }
    if let (Some(a), Some(b)) = (before.pressure.value(), now.pressure.value())
        && a != b
    {
        text.push(format!("Memory pressure: {a} → {b}"));
    }
    if let (Some(a), Some(b)) = (before.vm_rates.value(), now.vm_rates.value())
        && a[2] + a[3] < 8_388_608.
        && b[2] + b[3] >= 8_388_608.
    {
        text.push("Swap traffic crossed 8 MiB/s".into());
    }
    text.into_iter()
        .map(|text| Event { at: now.at, text })
        .collect()
}

#[derive(Debug, Default)]
pub struct Summary {
    pub average: f64,
    pub peak: f64,
    pub above: f64,
    pub coverage: f64,
}
/// Time-weighted, bounded to the displayed cursor. Missing data and long gaps are excluded.
pub fn summary(
    history: &VecDeque<Arc<Sample>>,
    end: f64,
    span: f64,
    value: impl Fn(&Sample) -> Option<f64>,
    threshold: f64,
) -> Summary {
    let mut out = Summary::default();
    let mut weighted = 0.;
    for (before, after) in history.iter().zip(history.iter().skip(1)) {
        if after.at > end || after.at <= end - span || after.at - before.at > 3. {
            continue;
        }
        let dt = after.at - before.at.max(end - span);
        if dt <= 0. {
            continue;
        }
        if let Some(v) = value(after).filter(|v| v.is_finite()) {
            out.coverage += dt;
            weighted += v * dt;
            out.peak = out.peak.max(v);
            if v >= threshold {
                out.above += dt;
            }
        }
    }
    if out.coverage > 0. {
        out.average = weighted / out.coverage;
    }
    out
}

/// A real retained footprint at the requested lookback, never a different lifetime of the PID.
pub fn memory_growth(
    history: &VecDeque<Arc<Sample>>,
    now: &Sample,
    p: &Proc,
    seconds: f64,
) -> Option<i64> {
    let current = *p.footprint.value()?;
    let target = now.process_at - seconds;
    let old = history
        .iter()
        .rev()
        .find(|s| s.at <= now.at && s.process_at <= target)?;
    if target - old.process_at > 20. || now.at - now.process_at > 20. {
        return None;
    }
    let old = old
        .processes
        .iter()
        .find(|v| v.key() == p.key())?
        .footprint
        .value()?;
    Some((current as i128 - *old as i128).clamp(i64::MIN as i128, i64::MAX as i128) as i64)
}

pub struct AppGroup<'a> {
    pub key: String,
    pub name: String,
    pub processes: Vec<&'a Proc>,
    pub cpu: f32,
    pub memory: u64,
    pub read: f64,
    pub write: f64,
    pub download: f64,
    pub upload: f64,
    pub traffic_count: usize,
}
/// Group by outer application bundle, inheriting it through a bounded, cycle-safe parent walk.
/// Standalone processes keep their own identity; launchd never becomes an all-system group.
pub fn groups(s: &Sample) -> Vec<AppGroup<'_>> {
    let by_pid: HashMap<_, _> = s.processes.iter().map(|p| (p.pid, p)).collect();
    let mut groups = BTreeMap::<String, AppGroup<'_>>::new();
    for p in s.processes.iter() {
        let mut owner = p;
        let mut seen = HashSet::new();
        for _ in 0..32 {
            if !owner.application.is_empty() || owner.parent <= 1 || !seen.insert(owner.pid) {
                break;
            }
            let Some(parent) = by_pid.get(&owner.parent) else {
                break;
            };
            if parent.start > owner.start {
                break;
            }
            owner = parent;
        }
        let (key, name) = if owner.application.is_empty() {
            (format!("pid:{}:{}", p.pid, p.start), p.name.to_string())
        } else {
            (
                owner.application.to_string(),
                owner
                    .application
                    .rsplit('/')
                    .next()
                    .unwrap_or(&owner.application)
                    .trim_end_matches(".app")
                    .to_string(),
            )
        };
        let g = groups.entry(key.clone()).or_insert_with(|| AppGroup {
            key,
            name,
            processes: vec![],
            cpu: 0.,
            memory: 0,
            read: 0.,
            write: 0.,
            download: 0.,
            upload: 0.,
            traffic_count: 0,
        });
        g.processes.push(p);
        g.cpu += p.cpu;
        g.memory = g.memory.saturating_add(p.memory);
        g.read += p.read;
        g.write += p.write;
        if let Some(t) = s.extra.traffic(p, s.at) {
            g.download += t.read;
            g.upload += t.write;
            g.traffic_count += 1;
        }
    }
    let mut out: Vec<_> = groups.into_values().collect();
    out.sort_by(|a, b| b.cpu.total_cmp(&a.cpu).then(a.key.cmp(&b.key)));
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::metrics::{Avail, demo};
    #[test]
    fn history_is_time_weighted_and_lifetime_safe() {
        let mut a = demo("balanced", 0.);
        let mut b = demo("balanced", 2.);
        let mut c = demo("balanced", 60.);
        a.cpu = 0.;
        b.cpu = 0.5;
        c.cpu = 1.;
        let mut current = c.processes[0].clone();
        current.footprint = Avail::Ok(1000);
        Arc::make_mut(&mut a.processes)[0].footprint = Avail::Ok(100);
        let history = VecDeque::from([Arc::new(a), Arc::new(b), Arc::new(c.clone())]);
        let sum = summary(&history, 60., 60., |s| Some(s.cpu as f64), 0.4);
        assert_eq!((sum.average, sum.coverage, sum.above), (0.5, 2., 2.));
        assert_eq!(memory_growth(&history, &c, &current, 60.), Some(900));
        current.start += 1;
        assert_eq!(memory_growth(&history, &c, &current, 60.), None);
        assert_eq!(memory_growth(&history, &c, &current, 900.), None);
        if let Avail::Ok(b) = &mut c.battery {
            b.ac = false;
        }
        let before = demo("balanced", 59.);
        assert!(
            events(&before, &c)
                .iter()
                .any(|e| e.text == "AC disconnected")
        );
        assert!(events(&history[0], &c).is_empty());
        let groups = groups(&c);
        assert_eq!(
            groups.iter().map(|g| g.processes.len()).sum::<usize>(),
            c.processes.len()
        );
        assert!(
            groups
                .iter()
                .any(|g| g.name == "Safari" && g.processes.len() == 2)
        );
    }
}
