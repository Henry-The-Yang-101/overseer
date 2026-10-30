use super::{
    runtime::{agent_home, canonical_home},
    transport, *,
};
use clap::{Args, Subcommand};
use std::{
    io::{self, Read, Write},
    path::PathBuf,
    time::{Duration, Instant},
};

#[derive(Debug, Args)]
pub struct Cli {
    #[command(subcommand)]
    pub command: AgentCommand,
}

#[derive(Debug, Subcommand)]
pub enum AgentCommand {
    /// Observe local agents and emit immutable JSON snapshots (no TUI).
    Watch(Watch),
    /// Collect a short observation and print one JSON snapshot.
    Snapshot(Sample),
    /// Best-effort hook helper: reads agent JSON on stdin; always exits successfully.
    Hook(Hook),
    /// Print a hooks configuration fragment; does not edit agent settings.
    HookConfig(Hook),
}

#[derive(Clone, Debug, Args)]
pub struct Sources {
    /// Codex home directories to observe. Repeat for alternate homes.
    #[arg(long)]
    pub codex_home: Vec<PathBuf>,
    #[arg(long)]
    pub socket: Option<PathBuf>,
    #[arg(long)]
    pub no_hooks: bool,
    #[arg(long)]
    pub no_logs: bool,
    /// Opt in to lower-fidelity Codex Cloud status polling.
    #[arg(long)]
    pub cloud: bool,
    #[arg(long)]
    pub cloud_home: Option<PathBuf>,
}

impl Sources {
    fn options(self) -> Options {
        let mut options = Options::default();
        if !self.codex_home.is_empty() {
            options.codex_homes = self.codex_home;
        }
        if self.no_logs {
            options.codex_homes.clear();
        }
        if self.no_hooks {
            options.socket = None;
        } else if self.socket.is_some() {
            options.socket = self.socket;
        }
        options.cloud = self.cloud;
        if let Some(home) = self.cloud_home {
            options.cloud_home = home;
        }
        options
    }
}

#[derive(Debug, Args)]
pub struct Watch {
    #[command(flatten)]
    sources: Sources,
    /// Stop after this many seconds; otherwise run until interrupted.
    #[arg(long, value_parser = clap::value_parser!(u64).range(1..))]
    seconds: Option<u64>,
}

#[derive(Debug, Args)]
pub struct Sample {
    #[command(flatten)]
    sources: Sources,
    #[arg(long, default_value_t = 2, value_parser = clap::value_parser!(u64).range(1..=60))]
    seconds: u64,
}

#[derive(Debug, Args)]
pub struct Hook {
    #[arg(long, value_enum)]
    provider: Provider,
    #[arg(long)]
    socket: Option<PathBuf>,
    /// Defaults to the provider's environment/default home in the calling agent.
    #[arg(long)]
    source_home: Option<PathBuf>,
}

pub fn run(cli: Cli) -> anyhow::Result<()> {
    match cli.command {
        AgentCommand::Hook(args) => {
            // Covers stdin, connect and write; an unavailable monitor never holds up an agent.
            if std::thread::Builder::new()
                .name("overseer-hook-deadline".into())
                .spawn(|| {
                    std::thread::sleep(Duration::from_millis(250));
                    std::process::exit(0);
                })
                .is_err()
            {
                return Ok(());
            }
            let _ = forward_hook(args);
            // This is a one-shot process. Also prevents the watchdog leaking into another command.
            std::process::exit(0);
        }
        AgentCommand::HookConfig(args) => {
            let binary = std::env::current_exe()?;
            let provider = match args.provider {
                Provider::Codex => "codex",
                Provider::Claude => "claude",
            };
            let mut command = format!(
                "{} agents hook --provider {provider} --socket {}",
                quote(&binary.to_string_lossy()),
                quote(
                    &args
                        .socket
                        .unwrap_or_else(transport::default_socket)
                        .to_string_lossy()
                )
            );
            if let Some(home) = args.source_home {
                command.push_str(&format!(
                    " --source-home {}",
                    quote(&canonical_home(&home)?.to_string_lossy())
                ));
            }
            let mut names = vec![
                "SessionStart",
                "SessionEnd",
                "UserPromptSubmit",
                "PreToolUse",
                "PostToolUse",
                "PermissionRequest",
                "SubagentStart",
                "SubagentStop",
                "PreCompact",
                "PostCompact",
                "Stop",
            ];
            if args.provider == Provider::Codex {
                names.push("Interrupt");
            } else {
                names.extend(["PostToolUseFailure", "StopFailure", "Notification"]);
            }
            let hooks: serde_json::Map<_, _> = names.into_iter().map(|name| (name.into(), serde_json::json!([{ "hooks": [{"type":"command", "command":command, "timeout":1}] }]))).collect();
            println!(
                "{}",
                serde_json::to_string_pretty(&serde_json::json!({"hooks":hooks}))?
            );
        }
        AgentCommand::Snapshot(args) => {
            let monitor = Monitor::start(args.sources.options())?;
            std::thread::sleep(Duration::from_secs(args.seconds));
            println!(
                "{}",
                serde_json::to_string_pretty(monitor.latest().as_ref())?
            );
        }
        AgentCommand::Watch(args) => {
            let monitor = Monitor::start(args.sources.options())?;
            let start = Instant::now();
            let mut revision = 0;
            let stdout = io::stdout();
            let mut out = stdout.lock();
            while args
                .seconds
                .is_none_or(|s| start.elapsed() < Duration::from_secs(s))
            {
                let snapshot = monitor.latest();
                if snapshot.revision != revision {
                    revision = snapshot.revision;
                    let line = serde_json::to_vec(snapshot.as_ref())?;
                    if let Err(e) = out
                        .write_all(&line)
                        .and_then(|_| out.write_all(b"\n"))
                        .and_then(|_| out.flush())
                    {
                        if e.kind() == io::ErrorKind::BrokenPipe {
                            return Ok(());
                        }
                        return Err(e.into());
                    }
                }
                std::thread::sleep(Duration::from_millis(100));
            }
        }
    }
    Ok(())
}

fn forward_hook(args: Hook) -> anyhow::Result<()> {
    let mut bytes = Vec::new();
    io::stdin()
        .take(transport::INPUT_LIMIT + 1)
        .read_to_end(&mut bytes)?;
    anyhow::ensure!(
        bytes.len() as u64 <= transport::INPUT_LIMIT,
        "hook input too large"
    );
    let value = serde_json::from_slice(&bytes)?;
    let home = canonical_home(
        &args
            .source_home
            .unwrap_or_else(|| agent_home(args.provider)),
    )?;
    let events = decode::hook(args.provider, &home.to_string_lossy(), &value, now_ms());
    transport::send(
        &args.socket.unwrap_or_else(transport::default_socket),
        events,
    );
    Ok(())
}

fn quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}
