![Overseer terminal demo](assets/overseer-demo.gif)

# Overseer

A keyboard-first hardware observatory for the terminal. Rust, Ratatui, and real Apple Silicon telemetry from macmon. Everything stays on your machine.

```sh
./run.sh                    # live hardware
./run.sh --demo             # clearly labelled synthetic telemetry
./run.sh --page cpu         # open a Systems resource page
./run.sh --probe            # sensor and font check
```

Requires Rust 1.95+; `run.sh` also finds rustup in `~/.cargo/bin`. Apple Silicon macOS has the full collector. Other platforms use available system metrics and explicitly mark unsupported sensors. A terminal of at least 60×18 is required; 150×42 or larger shows the richest layout. JetBrains Mono, Iosevka, or Menlo are recommended. Use `--ascii` if glyphs misalign, `--no-animation` for static updates, or `a` for reduced motion.

- `1` Teardown (home), `2` Systems; `v` switches between them. Systems has CPU, Memory, Energy, Disk, Network, and GPU pages.
- Systems: `←` / `→` (or `Tab` / `Shift+Tab`) switches pages; `↑` / `↓` selects tasks; `Enter` opens details; `z` expands the visualizer. Each page remembers its task selection, scroll, and sort.
- Systems: `i` opens Stats, History, and Apps. Switch tabs with `←` / `→`; scroll with `↑` / `↓` or `PgUp` / `PgDn`. In Apps, arrows select an application and `Enter` expands its processes; `PgUp` / `PgDn` scroll expanded members. The panel pins its snapshot until closed. History uses the selected `[ ]` window and marks power, thermal, memory-pressure, and swap events on the Systems timeline. Memory Stats shows per-process footprint changes over 1/5/15 minutes and ranks growth; insufficient history stays unavailable. App memory totals sum RSS and may count shared memory more than once.
- In a task inspector, `←` / `→` switches tabs, `↑` / `↓` switches tasks, and `PgUp` / `PgDn` scrolls details. `Esc` closes one layer and never quits.
- `Space` pauses/plays; `,` / `.` step; `<` / `>` step ten; `{` / `}` jump to spikes; `End` returns live. Collectors keep recording while paused.
- `/` searches processes, including `user:root` and `pid:123`. `s` sorts, `S` reverses, `T` toggles the tree, `f` follows a process.
- `X` opens signal confirmation. Only `y` sends SIGTERM; `Y` sends SIGKILL. Signals are disabled in demos and replay. PID and start time are rechecked before sending, without privilege escalation.
- Teardown: `Tab` selects, arrows navigate, `Esc` clears selection and hides the sidebar, `g` opens the selected resource page, `Enter` inspects, `z` zooms; `m` telemetry slots, `L` labels, `H` activity highlights, `o` mirror; `p` shows connected physical ports.
- `t` cycles six themes; `:` finds commands; `?` opens controls; `q` quits.

Settings are saved in `$XDG_CONFIG_HOME/overseer/config.toml` (default `~/.config/overseer/config.toml`). Demo runs never save preferences. Hardware samples arrive approximately every second; process snapshots every five seconds; physical port connections every three seconds; battery status every ten; active macOS power mode every three; visible socket ownership every fifteen. Systems-only extended telemetry is collected on a separate worker approximately every five seconds: process RX/TX, disk IOPS and average operation size, Wi-Fi quality/link rate, primary routes, sleep assertions, and adapter capability. Network and disk rates need two samples; reset counters, reused PIDs, failures, and stale samples are not shown as zero. Tunnel interfaces are context, not proof of VPN routing; adapter ratings are not measured charging power. History is bounded to one hour and is not written to disk.

The M4 Pro 14-inch profile applies only to `Mac16,8`; other machines show a labelled generic schematic. The teardown faces the user by default, with MagSafe on the left. The chip block and CPU page show the active Low Power, Automatic, or High Power mode from `pmset -g`; unsupported readings stay unavailable. Its component spacing is approximate; faint wiring and idle signal pulses are symbolic. The highlighted port-to-system-rail power path is explicitly inferred only when AC power and exactly one eligible connection make the source unambiguous. The conceptual system rail connects to the BMS, which has separate leads to all six cell silhouettes. Fan sides are assumed; blower-shaped housings contain upright, zero-padded speed readings split into two pairs inside the hubs (7200 appears as 72 above 00). Memory plates and power delivery are conceptual; NAND position is approximate. Core lanes, GPU tiles, and memory cells are symbolic aggregates. The six battery silhouettes divide pack charge into equal shares and fill from bottom to top in numbered order; they do not show per-cell telemetry. One symbolic cell at the fill boundary receives a green directional lead, with a flashing top line while charging and not yet full; full cells shimmer over electrode ribs. BMS arrows carry a slow gap in the flow direction. These animations freeze with pause and become static with reduced/off motion. Σ power is exactly CPU + GPU + ANE; DRAM and GPU SRAM are separate. Zero, unavailable, failed, and estimated readings have distinct displays. Energy task rankings are explicitly estimated **CPU-only** power allocations. Network tasks rank measured external-interface RX + TX from `nettop`, excluding loopback; visible socket counts remain a separate column; the GPU task list shows an explicit unavailable state. Port detection reads macOS IORegistry, including display names and USB device trees linked to physical USB-C ports. Connected device names appear outside the chassis; unnamed analogue devices and unavailable model names are labelled honestly. Missing or stale connection flags stay unknown. Passive hover tips and notices use a reserved status row. Wi-Fi and disk operation counters depend on OS/device support; SSD temperature and some protected process counters may be unavailable. The teardown layout and its existing readings are unchanged by the Systems additions.

```sh
cargo test
cargo clippy --all-targets -- -D warnings
./run.sh --demo --snapshot artifacts/teardown.svg --size 160x46
./run.sh --demo --scenario missing-sensors --snapshot artifacts/fallback.ansi
```

Speaker waveforms capture stereo system output by default using [Core Audio taps](https://developer.apple.com/documentation/coreaudio/capturing-system-audio-with-core-audio-taps) on macOS 14.2+. The nested speaker borders switch between centered normal and enlarged sizes directly with each channel's waveform amplitude when full motion is enabled; smaller layouts limit expansion to the available space. Select either speaker and press `Enter`, then `Enter` or `e` to toggle capture for both channels. The preference persists as `teardown.audio_enabled`. macOS asks for system-audio recording permission for **Overseer Audio**; if blocked, allow it in Privacy & Security → Screen & System Audio Recording and toggle capture off/on. Waveforms include headphones and external outputs, use a fixed 4× display gain, and are unavailable in replay. Audio stays in short memory buffers; it is never saved or sent anywhere. Demo waveforms are synthetic and do not request permission. Building on macOS requires the Xcode command-line tools (Swift); the signed helper is embedded in the binary and installed under the Overseer config directory.

The GPU page divides Shader Fabric equally between utilization tiles and a spinning ASCII donut. The Rust renderer samples a torus, rotates it on two axes, and uses a depth buffer plus the `.,-~:;=!*#$@` luminance ramp, inspired by [donut.c](https://www.a1k0n.net/2011/07/20/donut-math.html). It preserves terminal character proportions and fits the pane with a small border gap. On macOS, Metal accelerates projection, lighting, and depth resolution using the geometry generated in Rust; other platforms and Metal failures use the Rust renderer. Paused frames are cached; reduced/off motion and exports use a static pose. Motion is decorative, and its rendering workload contributes to the system's measured usage. Use `cargo test --lib donut::tests::metal -- --ignored --nocapture` on a Metal-capable Mac to compare GPU output with the Rust renderer.

Exports use the actual Ratatui buffer (`.svg`, `.ansi`, or `.txt`). Focused tests cover navigation, replay, accounting, signal safety, port availability, and rendering. Three snapshot files cover Teardown, telemetry cells, and the six Systems pages at 200×55, 120×36, and 80×24. `--sample` prints one real normalized snapshot as JSON for diagnostics; it includes process names and commands.

## Agent collection (no panel yet)

The agent backend runs independently of the hardware TUI. Local collection uses Codex/Claude command hooks only; Codex Cloud status polling remains a separate, optional adapter. It does not install hooks, start agents, or change their settings.

```sh
./run.sh agents snapshot --seconds 10               # collect hooks for ten seconds
./run.sh agents watch                               # JSONL snapshots + hook receiver
./run.sh agents watch --cloud                       # explicitly enable cloud polling
./run.sh agents hook-config --provider codex         # print a configuration fragment
./run.sh agents hook-config --provider claude
```

Merge the generated `hooks` entries with existing agent settings, then review/trust them in that agent. Codex uses `hooks.json`; Claude uses its settings JSON. Generate the fragment from the release binary you intend to keep installed. Hook availability varies by agent version. The helper reads JSON on stdin, sends only normalized metadata to a private same-user Unix socket, and exits successfully within a 250 ms watchdog even if the monitor is absent. One monitor owns a socket; `--socket /private/directory/agents.sock` selects another (the directory must be owned by you with mode 700). `--no-hooks` disables the receiver, for example for cloud-only polling. `--seconds N` bounds a watch; snapshot defaults to two seconds. Hooks currently require macOS/Linux/BSD Unix sockets.

No rollout or session-index files are scanned or read. Start the collector before the activity you want to observe. Hooks received while it is stopped are dropped, and restarting starts an empty in-memory view. Saved titles, historical sessions, and transcript-derived token usage are unavailable. Coverage is limited to the hooks emitted by each agent version. Limits are 256 sessions and 2,048 normalized history events. `freshness` becomes `stale` after five minutes without a live observation; this does **not** mean completed, crashed, or idle. `phase` is the last observed state, and `last_outcome` describes a turn, not project completion. Tool counts cover observed calls, not lifetime totals. Approval hooks report requests, not proof that a human is still blocking execution.

Snapshots retain identifiers, project paths, titles, tool names, lifecycle events, and reported usage; prompts, tool arguments/output, credentials, and transcript bodies are not retained. History stays in memory. Both local providers use hooks only. Cloud runs on a separate worker with bounded execution and pagination every 30 seconds; its coarse status is kept separate from local execution phases. `--cloud-home` selects its Codex account home. No cloud request is made without `--cloud`.

For the future panel, `agents::Monitor::start(Options)` owns the workers, `latest()` returns an immutable `Arc<Snapshot>`, and dropping the monitor shuts down its workers and releases its socket. `try_ingest(Event)` and `decode::app_server` expose a bounded adapter seam; automatic attachment to existing app-server instances is intentionally not implemented. The decoders use [official Codex hooks](https://learn.chatgpt.com/docs/hooks), [app-server events](https://learn.chatgpt.com/docs/app-server), and [Claude hooks](https://code.claude.com/docs/en/hooks). The producer/reducer split and hook transport were informed by [pixtuoid](https://github.com/pixtuoid/pixtuoid/blob/main/docs/ARCHITECTURE.md).
