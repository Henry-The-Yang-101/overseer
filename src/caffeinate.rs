//! Process-owned macOS assertions matching `caffeinate -sudi`, without a helper.
use std::time::{Duration, Instant};

#[derive(Default)]
pub struct Caffeinate {
    session: Option<Session>,
}

struct Session {
    deadline: Option<Instant>,
    _assertions: platform::Assertions,
}

pub fn minutes(input: &str) -> Result<Duration, &'static str> {
    let minutes: u64 = input
        .trim()
        .parse()
        .map_err(|_| "Enter a positive whole number of minutes.")?;
    if minutes == 0 {
        return Err("Enter at least one minute.");
    }
    // powerd stores assertion timeouts in seconds as a uint32_t.
    let seconds = minutes
        .checked_mul(60)
        .filter(|seconds| *seconds <= u32::MAX as u64)
        .ok_or("That duration is too large.")?;
    Ok(Duration::from_secs(seconds))
}

impl Caffeinate {
    pub fn is_active(&self) -> bool {
        self.session.is_some()
    }

    pub fn remaining(&self) -> Option<Duration> {
        self.session
            .as_ref()?
            .deadline
            .map(|deadline| deadline.saturating_duration_since(Instant::now()))
    }

    pub fn start(&mut self, duration: Option<Duration>) -> Result<(), String> {
        let deadline = duration
            .map(|duration| {
                if duration.is_zero() || duration.as_secs_f64() > u32::MAX as f64 {
                    return Err("Invalid caffeinate duration.");
                }
                Instant::now()
                    .checked_add(duration)
                    .ok_or("That duration is too large.")
            })
            .transpose()?;
        let assertions = platform::Assertions::new(duration)?;
        self.session = Some(Session {
            deadline,
            _assertions: assertions,
        });
        Ok(())
    }

    pub fn stop(&mut self) {
        self.session = None;
    }

    pub fn expire(&mut self) -> bool {
        if self.remaining().is_some_and(|time| time.is_zero()) {
            self.stop();
            true
        } else {
            false
        }
    }
}

#[cfg(target_os = "macos")]
mod platform {
    use core_foundation::{
        base::TCFType,
        string::{CFString, CFStringRef},
    };
    use std::{ptr, time::Duration};

    #[link(name = "IOKit", kind = "framework")]
    unsafe extern "C" {
        fn IOPMAssertionCreateWithDescription(
            assertion_type: CFStringRef,
            name: CFStringRef,
            details: CFStringRef,
            reason: CFStringRef,
            localization: CFStringRef,
            timeout: f64,
            timeout_action: CFStringRef,
            assertion_id: *mut u32,
        ) -> i32;
        fn IOPMAssertionRelease(assertion_id: u32) -> i32;
    }

    pub struct Assertions(Vec<u32>);

    impl Assertions {
        pub fn new(duration: Option<Duration>) -> Result<Self, String> {
            let name = CFString::new("Overseer caffeinate");
            let details = CFString::new("Keep awake enabled from Overseer's Teardown view");
            let release = CFString::new("TimeoutActionRelease");
            let mut assertions = Self(Vec::with_capacity(4));
            // These are the same four types and order used by Apple's caffeinate.
            // -s remains subject to macOS's AC-power rules. Without -t, -u is a
            // five-second user-activity assertion; the other three stay active.
            for kind in [
                "PreventUserIdleSystemSleep",
                "PreventUserIdleDisplaySleep",
                "PreventSystemSleep",
                "UserIsActive",
            ] {
                let timeout = duration.map_or_else(
                    || if kind == "UserIsActive" { 5. } else { 0. },
                    |duration| duration.as_secs_f64(),
                );
                let assertion_type = CFString::new(kind);
                let mut id = 0;
                // SAFETY: all CFStrings and the output pointer live through the
                // synchronous call. Each successful ID is owned by this guard.
                let result = unsafe {
                    IOPMAssertionCreateWithDescription(
                        assertion_type.as_concrete_TypeRef(),
                        name.as_concrete_TypeRef(),
                        details.as_concrete_TypeRef(),
                        ptr::null(),
                        ptr::null(),
                        timeout,
                        release.as_concrete_TypeRef(),
                        &mut id,
                    )
                };
                if result != 0 {
                    // Dropping the guard rolls back any earlier successes.
                    return Err(format!("Could not enable {kind} (IOKit {result:#x})."));
                }
                assertions.0.push(id);
            }
            Ok(assertions)
        }
    }

    impl Drop for Assertions {
        fn drop(&mut self) {
            for &id in &self.0 {
                // SAFETY: these IDs came from successful creates in this process.
                // Native timeouts may already have released an ID. macOS also
                // releases all process-owned assertions on death, even SIGKILL;
                // that guarantee does not depend on this destructor running.
                unsafe { IOPMAssertionRelease(id) };
            }
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use core_foundation::base::{CFRelease, CFTypeRef};

        unsafe extern "C" {
            fn IOPMAssertionCopyProperties(id: u32) -> CFTypeRef;
        }

        fn exists(id: u32) -> bool {
            // SAFETY: a valid assertion ID may return a retained dictionary or null.
            unsafe {
                let properties = IOPMAssertionCopyProperties(id);
                if properties.is_null() {
                    false
                } else {
                    CFRelease(properties);
                    true
                }
            }
        }

        #[test]
        #[ignore = "live macOS power assertions; run explicitly"]
        fn native_assertions_release_on_drop_and_timeout() {
            let assertions = Assertions::new(None).unwrap();
            let ids = assertions.0.clone();
            assert_eq!(ids.len(), 4);
            assert!(ids.iter().all(|&id| exists(id)));
            drop(assertions);
            assert!(ids.iter().all(|&id| !exists(id)));

            let assertions = Assertions::new(Some(Duration::from_secs(1))).unwrap();
            assert!(assertions.0.iter().all(|&id| exists(id)));
            // Do not tick or drop the owner: powerd itself must enforce the timeout.
            std::thread::sleep(Duration::from_secs(2));
            assert!(assertions.0.iter().all(|&id| !exists(id)));
        }
    }
}

#[cfg(not(target_os = "macos"))]
mod platform {
    use std::time::Duration;

    pub struct Assertions;
    impl Assertions {
        pub fn new(_: Option<Duration>) -> Result<Self, String> {
            Err("Caffeinate is only available on macOS.".into())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn custom_minutes_require_positive_whole_minutes_without_overflow() {
        assert_eq!(minutes(" 90 "), Ok(Duration::from_secs(5400)));
        for input in [
            "",
            "0",
            "-1",
            "1.5",
            "abc",
            "71582789",
            "18446744073709551615",
        ] {
            assert!(minutes(input).is_err(), "accepted {input}");
        }
    }
}
