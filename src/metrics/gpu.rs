use super::{Avail, Proc};
use std::{collections::HashMap, time::Instant};

#[derive(Clone)]
struct Client {
    id: u64,
    pid: u32,
    counters: Vec<(String, u64)>,
}

#[derive(Default)]
pub(super) struct Collector {
    // Registry entry + process lifetime: neither a reused PID nor a new queue inherits work.
    previous: HashMap<(u64, u32, u64), Client>,
    at: Option<Instant>,
}
impl Collector {
    pub fn sample(&mut self, processes: &mut [Proc]) -> Avail<usize> {
        let clients = read();
        self.update(processes, clients, Instant::now())
    }

    fn update(
        &mut self,
        processes: &mut [Proc],
        clients: Avail<Vec<Client>>,
        at: Instant,
    ) -> Avail<usize> {
        let clients = match clients {
            Avail::Ok(clients) => clients,
            other => {
                self.previous.clear();
                self.at = None;
                let reason = other.reason().to_owned();
                let failed = matches!(other, Avail::Failed(_));
                for p in processes {
                    p.gpu = if failed {
                        Avail::Failed(reason.clone())
                    } else {
                        Avail::Unsupported(reason.clone())
                    };
                }
                return if failed {
                    Avail::Failed(reason)
                } else {
                    Avail::Unsupported(reason)
                };
            }
        };
        let dt = self.at.map(|old| at.duration_since(old).as_secs_f64());
        let live: HashMap<_, _> = processes.iter().map(|p| (p.pid, p.start)).collect();
        let current: HashMap<_, _> = clients
            .into_iter()
            .filter_map(|c| {
                let start = *live.get(&c.pid)?;
                Some(((c.id, c.pid, start), c))
            })
            .collect();
        let mut rates = HashMap::<u32, Option<f64>>::new();
        for (key, client) in &current {
            let delta = (|| {
                let dt = dt.filter(|dt| *dt > 0. && *dt <= 30.)?;
                let old = self.previous.get(key)?;
                if old.counters.len() != client.counters.len() {
                    return None;
                }
                let mut delta = 0.;
                for ((api, time), (old_api, old_time)) in client.counters.iter().zip(&old.counters)
                {
                    if api != old_api {
                        return None;
                    }
                    delta += time.checked_sub(*old_time)? as f64;
                }
                Some(delta / 1e9 / dt * 100.)
            })();
            rates
                .entry(client.pid)
                .and_modify(|rate| {
                    // A new/reset client makes this process wait rather than report a partial rate.
                    *rate = rate.zip(delta).map(|(a, b)| a + b);
                })
                .or_insert(delta);
        }
        let count = rates.len();
        for p in processes {
            p.gpu = match rates.get(&p.pid) {
                Some(Some(rate)) => Avail::Ok(*rate as f32),
                Some(None) => Avail::Acquiring,
                None => Avail::Unsupported("no readable GPU time counters".into()),
            };
        }
        self.previous = current;
        self.at = Some(at);
        Avail::Ok(count)
    }
}

#[cfg(not(target_os = "macos"))]
fn read() -> Avail<Vec<Client>> {
    Avail::Unsupported("process GPU counters require macOS".into())
}
#[cfg(target_os = "macos")]
fn read() -> Avail<Vec<Client>> {
    native::read()
}

#[cfg(target_os = "macos")]
mod native {
    use super::*;
    use core_foundation::{
        array::CFArray,
        base::{CFAllocatorRef, CFType, CFTypeRef, TCFType, kCFAllocatorDefault},
        dictionary::{CFDictionary, CFMutableDictionaryRef},
        number::CFNumber,
        string::{CFString, CFStringRef},
    };
    use std::ffi::c_char;

    #[link(name = "IOKit", kind = "framework")]
    unsafe extern "C" {
        fn IOServiceMatching(name: *const c_char) -> CFMutableDictionaryRef;
        fn IOServiceGetMatchingServices(
            port: u32,
            matching: CFMutableDictionaryRef,
            iterator: *mut u32,
        ) -> i32;
        fn IORegistryEntryCreateIterator(
            entry: u32,
            plane: *const c_char,
            options: u32,
            iterator: *mut u32,
        ) -> i32;
        fn IOIteratorNext(iterator: u32) -> u32;
        fn IOIteratorIsValid(iterator: u32) -> u32;
        fn IOObjectRelease(object: u32) -> i32;
        fn IORegistryEntryGetRegistryEntryID(entry: u32, id: *mut u64) -> i32;
        fn IORegistryEntryCreateCFProperty(
            entry: u32,
            key: CFStringRef,
            allocator: CFAllocatorRef,
            options: u32,
        ) -> CFTypeRef;
    }
    // All owned Mach/IOKit handles, including iterators, are released on every exit path.
    struct Object(u32);
    impl Drop for Object {
        fn drop(&mut self) {
            // SAFETY: this wrapper exclusively owns the returned IOKit reference.
            unsafe {
                IOObjectRelease(self.0);
            }
        }
    }
    impl Object {
        fn next(&self) -> Option<Self> {
            // SAFETY: self is a live iterator; a nonzero result transfers ownership.
            let next = unsafe { IOIteratorNext(self.0) };
            (next != 0).then(|| Self(next))
        }
        fn valid(&self) -> bool {
            // SAFETY: self is a live iterator.
            unsafe { IOIteratorIsValid(self.0) != 0 }
        }
    }
    fn property(entry: u32, key: &str) -> Option<CFType> {
        let key = CFString::new(key);
        // SAFETY: entry and key are live. Create returns an owned CF object or null.
        unsafe {
            let value = IORegistryEntryCreateCFProperty(
                entry,
                key.as_concrete_TypeRef(),
                kCFAllocatorDefault,
                0,
            );
            (!value.is_null()).then(|| CFType::wrap_under_create_rule(value))
        }
    }
    fn field(dict: &CFDictionary, key: &str) -> Option<CFType> {
        let key = CFString::new(key);
        let value = *dict.find(key.as_CFTypeRef())?;
        // SAFETY: dictionary values are CF objects; the get rule retains the borrowed value.
        Some(unsafe { CFType::wrap_under_get_rule(value) })
    }
    fn number(value: &CFType) -> Option<u64> {
        let number = value.downcast::<CFNumber>()?;
        // SAFETY: downcast checked that this is a live CFNumber.
        if unsafe { core_foundation::number::CFNumberIsFloatType(number.as_concrete_TypeRef()) }
            != 0
        {
            return None;
        }
        u64::try_from(number.to_i64()?).ok()
    }
    fn counters(direct: Option<CFType>, usage: Option<CFType>) -> Option<Vec<(String, u64)>> {
        // Drivers exposing both forms report the same work; never sum both.
        if let Some(direct) = direct {
            return Some(vec![("total".into(), number(&direct)?)]);
        }
        let usage = usage?.downcast::<CFArray>()?;
        if usage.is_empty() {
            return None;
        }
        usage
            .iter()
            .map(|value| {
                // SAFETY: a CFArray holds CF objects, retained before downcasting.
                let value = unsafe { CFType::wrap_under_get_rule(*value) };
                let dict = value.downcast::<CFDictionary>()?;
                let api = field(&dict, "API")?.downcast::<CFString>()?.to_string();
                let time = number(&field(&dict, "accumulatedGPUTime")?)?;
                Some((api, time))
            })
            .collect()
    }
    pub(super) fn read() -> Avail<Vec<Client>> {
        // SAFETY: matching consumes the created dictionary; initialized out parameters receive
        // owned IOKit references. Only public read-only registry APIs are used.
        unsafe {
            let matching = IOServiceMatching(c"IOAccelerator".as_ptr());
            let mut handle = 0;
            if matching.is_null() || IOServiceGetMatchingServices(0, matching, &mut handle) != 0 {
                return Avail::Failed("GPU registry enumeration failed".into());
            }
            if handle == 0 {
                return Avail::Unsupported("no GPU accelerator counters".into());
            }
            let accelerators = Object(handle);
            let mut found = false;
            let mut clients = vec![];
            while let Some(accelerator) = accelerators.next() {
                found = true;
                let mut handle = 0;
                // Recursive IOService traversal includes unregistered AGXDeviceUserClient nodes,
                // which direct IOServiceMatching on the client class cannot find.
                if IORegistryEntryCreateIterator(
                    accelerator.0,
                    c"IOService".as_ptr(),
                    1,
                    &mut handle,
                ) != 0
                {
                    return Avail::Failed("GPU client enumeration failed".into());
                }
                if handle == 0 {
                    continue;
                }
                let children = Object(handle);
                while let Some(entry) = children.next() {
                    let Some(creator) = property(entry.0, "IOUserClientCreator")
                        .and_then(|v| v.downcast::<CFString>())
                        .map(|v| v.to_string())
                    else {
                        continue;
                    };
                    let Some(pid) = creator
                        .strip_prefix("pid ")
                        .and_then(|v| v.split_once(','))
                        .and_then(|(pid, _)| pid.parse::<u32>().ok())
                        .filter(|pid| *pid > 0)
                    else {
                        continue;
                    };
                    let Some(counters) = counters(
                        property(entry.0, "accumulatedGPUTime"),
                        property(entry.0, "AppUsage"),
                    ) else {
                        continue;
                    };
                    let mut id = 0;
                    if IORegistryEntryGetRegistryEntryID(entry.0, &mut id) == 0 {
                        clients.push(Client { id, pid, counters });
                    }
                }
                if !children.valid() {
                    return Avail::Failed("GPU registry changed during enumeration".into());
                }
            }
            if !accelerators.valid() {
                return Avail::Failed("GPU registry changed during enumeration".into());
            }
            if found {
                Avail::Ok(clients)
            } else {
                Avail::Unsupported("no GPU accelerator counters".into())
            }
        }
    }

    #[cfg(test)]
    #[test]
    fn parses_driver_arrays_and_direct_totals_without_double_counting() {
        let usage = |time: CFType| {
            let row = CFDictionary::from_CFType_pairs(&[
                (CFString::new("API"), CFString::new("Metal").as_CFType()),
                (CFString::new("accumulatedGPUTime"), time),
            ]);
            CFArray::from_CFTypes(&[row.as_CFType()]).as_CFType()
        };
        assert_eq!(
            counters(None, Some(usage(CFNumber::from(123_i64).as_CFType()))),
            Some(vec![("Metal".into(), 123)])
        );
        assert_eq!(
            counters(
                Some(CFNumber::from(0_i64).as_CFType()),
                Some(usage(CFNumber::from(123_i64).as_CFType()))
            ),
            Some(vec![("total".into(), 0)])
        );
        for time in [
            CFNumber::from(-1_i64).as_CFType(),
            CFNumber::from(1.5_f64).as_CFType(),
            CFString::new("123").as_CFType(),
        ] {
            assert!(counters(None, Some(usage(time))).is_none());
        }
        assert!(counters(None, Some(CFArray::<CFType>::from_CFTypes(&[]).as_CFType())).is_none());
        assert!(counters(None, Some(CFString::new("malformed").as_CFType())).is_none());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn rates_rebaseline_on_reset_queue_changes_pid_reuse_and_gaps() {
        let mut collector = Collector::default();
        let mut processes = [
            Proc {
                pid: 42,
                start: 1,
                ..Proc::default()
            },
            Proc {
                pid: 99,
                ..Proc::default()
            },
        ];
        let start = Instant::now();
        let client = |id, counters: &[u64]| Client {
            id,
            pid: 42,
            counters: counters.iter().map(|v| ("Metal".into(), *v)).collect(),
        };
        let mut step = |seconds, clients, processes: &mut [Proc]| {
            collector.update(
                processes,
                Avail::Ok(clients),
                start + Duration::from_secs(seconds),
            );
            processes[0].gpu.clone()
        };
        assert_eq!(
            step(0, vec![client(1, &[1_000_000_000])], &mut processes),
            Avail::Acquiring
        );
        // Duplicate registry entries are deduplicated; rates are not capped at 100%.
        assert_eq!(
            step(
                5,
                vec![client(1, &[8_500_000_000]), client(1, &[8_500_000_000])],
                &mut processes
            ),
            Avail::Ok(150.)
        );
        assert!(matches!(processes[1].gpu, Avail::Unsupported(_)));
        assert_eq!(
            step(10, vec![client(1, &[8_500_000_000])], &mut processes),
            Avail::Ok(0.)
        );
        assert_eq!(
            step(15, vec![client(1, &[0])], &mut processes),
            Avail::Acquiring
        );
        assert_eq!(
            step(20, vec![client(1, &[0, 0])], &mut processes),
            Avail::Acquiring
        );
        assert_eq!(
            step(
                25,
                vec![client(1, &[1_000_000_000, 0]), client(2, &[0])],
                &mut processes
            ),
            Avail::Acquiring
        );
        assert_eq!(
            step(
                30,
                vec![client(1, &[2_000_000_000, 0]), client(2, &[1_000_000_000])],
                &mut processes
            ),
            Avail::Ok(40.)
        );
        processes[0].start = 2;
        assert_eq!(
            step(35, vec![client(1, &[3_000_000_000, 0])], &mut processes),
            Avail::Acquiring
        );
        assert_eq!(
            step(70, vec![client(1, &[4_000_000_000, 0])], &mut processes),
            Avail::Acquiring
        );
        assert!(matches!(
            step(75, vec![], &mut processes),
            Avail::Unsupported(_)
        ));
        assert_eq!(
            step(80, vec![client(1, &[5_000_000_000, 0])], &mut processes),
            Avail::Acquiring
        );
        drop(step);
        collector.update(
            &mut processes,
            Avail::Failed("test read failure".into()),
            start + Duration::from_secs(85),
        );
        assert!(matches!(processes[0].gpu, Avail::Failed(_)));
        assert!(collector.previous.is_empty());
    }
}
