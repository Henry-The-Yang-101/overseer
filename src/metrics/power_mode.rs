use super::Avail;
use serde::Serialize;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub enum PowerMode {
    LowPower,
    Automatic,
    HighPower,
}
impl PowerMode {
    pub fn label(self) -> &'static str {
        match self {
            Self::LowPower => "LOW POWER",
            Self::Automatic => "AUTOMATIC",
            Self::HighPower => "HIGH POWER",
        }
    }
    pub fn compact_label(self) -> &'static str {
        match self {
            Self::LowPower => "LOW PWR",
            Self::Automatic => "AUTO",
            Self::HighPower => "HIGH PWR",
        }
    }
}

pub fn read() -> Avail<PowerMode> {
    #[cfg(target_os = "macos")]
    {
        let Ok(output) = std::process::Command::new("/usr/bin/pmset")
            .arg("-g")
            .env("LC_ALL", "C")
            .output()
        else {
            return Avail::Failed("power mode query failed".into());
        };
        if !output.status.success() {
            return Avail::Failed("power mode query failed".into());
        }
        parse(&String::from_utf8_lossy(&output.stdout))
    }
    #[cfg(not(target_os = "macos"))]
    Avail::Unsupported("macOS power mode unavailable".into())
}

#[cfg(any(target_os = "macos", test))]
fn parse(output: &str) -> Avail<PowerMode> {
    let Some((_, active)) = output.split_once("Currently in use:") else {
        return Avail::Failed("active power settings missing".into());
    };
    let value = |key: &str| {
        active.lines().find_map(|line| {
            let mut words = line.split_whitespace();
            (words.next() == Some(key)).then(|| words.next()).flatten()
        })
    };
    // Apple's pmset uses 0/1/2 for Automatic/Low/High, or a lowpowermode flag
    // on models that do not support High Power. Read the active source, not -g custom.
    if let Some(mode) = value("powermode") {
        return match mode {
            "0" => Avail::Ok(PowerMode::Automatic),
            "1" => Avail::Ok(PowerMode::LowPower),
            "2" => Avail::Ok(PowerMode::HighPower),
            _ => Avail::Failed("unrecognized power mode".into()),
        };
    }
    match (value("lowpowermode"), value("highpowermode")) {
        (Some("1"), None | Some("0")) => Avail::Ok(PowerMode::LowPower),
        (Some("0"), Some("1")) => Avail::Ok(PowerMode::HighPower),
        (Some("0"), None | Some("0")) => Avail::Ok(PowerMode::Automatic),
        (None, None) => Avail::Unsupported("power mode not exposed".into()),
        _ => Avail::Failed("unrecognized power mode flags".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reads_current_power_mode_and_rejects_unknown_settings() {
        for (settings, expected) in [
            ("powermode 0", PowerMode::Automatic),
            ("powermode 1", PowerMode::LowPower),
            ("powermode 2", PowerMode::HighPower),
            ("lowpowermode 1", PowerMode::LowPower),
            ("lowpowermode 0", PowerMode::Automatic),
            ("lowpowermode 0\nhighpowermode 1", PowerMode::HighPower),
        ] {
            assert_eq!(
                parse(&format!(
                    "System-wide power settings:\nCurrently in use:\n {settings}\n"
                )),
                Avail::Ok(expected)
            );
        }
        for output in [
            "",
            "Battery Power:\n powermode 1\nAC Power:\n powermode 2",
            "Currently in use:\n sleep 1",
            "Currently in use:\n powermode 3",
            "Currently in use:\n lowpowermode 1\n highpowermode 1",
        ] {
            assert!(parse(output).value().is_none());
        }
    }
}
