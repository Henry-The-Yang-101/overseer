//! Read-only diagnostic; the TUI does not start this listener.
use anyhow::{Context, Result, bail};
use clap::Parser;
use overseer::now_playing::{Listener, State};
use std::{
    io::{self, Write},
    path::PathBuf,
    sync::atomic::{AtomicBool, Ordering},
    thread,
    time::{Duration, Instant},
};

static INTERRUPTED: AtomicBool = AtomicBool::new(false);

#[cfg(unix)]
extern "C" fn interrupt(_: libc::c_int) {
    INTERRUPTED.store(true, Ordering::Relaxed);
}

#[derive(Parser)]
#[command(about = "Print local macOS Now Playing metadata changes as JSON lines")]
struct Args {
    /// Stop after this many seconds (keeps listening when omitted).
    #[arg(long, value_parser = clap::value_parser!(u64).range(1..))]
    seconds: Option<u64>,
    /// Exit after the first session with a title, or after --seconds.
    #[arg(long)]
    once: bool,
    /// Path to media-control, if it is not on PATH.
    #[arg(long, default_value = "media-control")]
    helper: PathBuf,
}

fn main() -> Result<()> {
    let args = Args::parse();
    #[cfg(unix)]
    for signal in [libc::SIGINT, libc::SIGTERM] {
        // SAFETY: the handler only stores a lock-free atomic flag, allowing the
        // main thread to drop the listener and reap its child outside the handler.
        if unsafe { libc::signal(signal, interrupt as *const () as libc::sighandler_t) }
            == libc::SIG_ERR
        {
            return Err(io::Error::last_os_error().into());
        }
    }
    let listener = Listener::start_with(&args.helper)
        .context("Could not start Now Playing. Install the helper with `brew install media-control`, or pass --helper /path/to/media-control")?;
    let start = Instant::now();
    let mut previous = State::Waiting;
    let stdout = io::stdout();
    let mut output = stdout.lock();
    while !INTERRUPTED.load(Ordering::Relaxed) {
        let state = listener.snapshot();
        if state != previous {
            serde_json::to_writer(&mut output, &state)?;
            writeln!(output)?;
            output.flush()?;
            if let State::Failed(error) = &state {
                bail!("{error}");
            }
            if args.once && matches!(&state, State::Active(metadata) if metadata.title.is_some()) {
                break;
            }
            previous = state;
        }
        if args
            .seconds
            .is_some_and(|seconds| start.elapsed() >= Duration::from_secs(seconds))
        {
            if args.once {
                bail!("No titled Now Playing session observed before timeout");
            }
            break;
        }
        thread::sleep(Duration::from_millis(50));
    }
    Ok(())
}
