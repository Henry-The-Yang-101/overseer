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

The M4 Pro 14-inch profile applies only to `Mac16,8`; other machines show a labelled generic schematic. The teardown faces the user by default, with MagSafe on the left. The chip block and CPU page show the active Low Power, Automatic, or High Power mode from `pmset -g`; unsupported readings stay unavailable. Its component spacing is approximate; faint wiring and idle signal pulses are symbolic. The highlighted port-to-BMS power path is explicitly inferred only when AC power and exactly one eligible connection make the source unambiguous. Fan sides are assumed. Memory plates and power delivery are conceptual; NAND position is approximate. Core lanes, GPU tiles, and memory cells are symbolic aggregates. The six battery silhouettes divide pack charge into equal shares and fill from bottom to top in numbered order; they do not show per-cell telemetry. Σ power is exactly CPU + GPU + ANE; DRAM and GPU SRAM are separate. Zero, unavailable, failed, and estimated readings have distinct displays. Energy task rankings are explicitly estimated **CPU-only** power allocations. Network tasks rank measured external-interface RX + TX from `nettop`, excluding loopback; visible socket counts remain a separate column; the GPU task list shows an explicit unavailable state. Port detection reads macOS IORegistry, including display names and USB device trees linked to physical USB-C ports. Connected device names appear outside the chassis; unnamed analogue devices and unavailable model names are labelled honestly. Missing or stale connection flags stay unknown. Passive hover tips and notices use a reserved status row. Wi-Fi and disk operation counters depend on OS/device support; SSD temperature and some protected process counters may be unavailable. The teardown layout and its existing readings are unchanged by the Systems additions.

```sh
cargo test
cargo clippy --all-targets -- -D warnings
./run.sh --demo --snapshot artifacts/teardown.svg --size 160x46
./run.sh --demo --scenario missing-sensors --snapshot artifacts/fallback.ansi
```

Speaker waveforms capture stereo system output by default using [Core Audio taps](https://developer.apple.com/documentation/coreaudio/capturing-system-audio-with-core-audio-taps) on macOS 14.2+. The nested speaker borders switch between centered normal and enlarged sizes directly with each channel's waveform amplitude when full motion is enabled; smaller layouts limit expansion to the available space. Select either speaker and press `Enter`, then `Enter` or `e` to toggle capture for both channels. The preference persists as `teardown.audio_enabled`. macOS asks for system-audio recording permission for **Overseer Audio**; if blocked, allow it in Privacy & Security → Screen & System Audio Recording and toggle capture off/on. Waveforms include headphones and external outputs, use a fixed 4× display gain, and are unavailable in replay. Audio stays in short memory buffers; it is never saved or sent anywhere. Demo waveforms are synthetic and do not request permission. Building on macOS requires the Xcode command-line tools (Swift); the signed helper is embedded in the binary and installed under the Overseer config directory.

Exports use the actual Ratatui buffer (`.svg`, `.ansi`, or `.txt`). Focused tests cover navigation, replay, accounting, signal safety, port availability, and rendering. Three snapshot files cover Teardown, telemetry cells, and the six Systems pages at 200×55, 120×36, and 80×24. `--sample` prints one real normalized snapshot as JSON for diagnostics; it includes process names and commands.
