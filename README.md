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
- In a task inspector, `←` / `→` switches tabs, `↑` / `↓` switches tasks, and `PgUp` / `PgDn` scrolls details. `Esc` closes one layer and never quits.
- `Space` pauses/plays; `,` / `.` step; `<` / `>` step ten; `{` / `}` jump to spikes; `End` returns live. Collectors keep recording while paused.
- `/` searches processes, including `user:root` and `pid:123`. `s` sorts, `S` reverses, `T` toggles the tree, `f` follows a process.
- `X` opens signal confirmation. Only `y` sends SIGTERM; `Y` sends SIGKILL. Signals are disabled in demos and replay. PID and start time are rechecked before sending, without privilege escalation.
- Teardown: `Tab` selects, arrows navigate, `g` opens the selected resource page, `Enter` inspects, `z` zooms; `m` telemetry slots, `L` labels, `H` activity highlights, `o` mirror.
- `t` cycles six themes; `:` finds commands; `?` opens controls; `q` quits.

Settings are saved in `$XDG_CONFIG_HOME/overseer/config.toml` (default `~/.config/overseer/config.toml`). Demo runs never save preferences. Hardware samples arrive approximately every second; process snapshots every five seconds; battery status every ten; visible socket ownership every fifteen. History is bounded to one hour and is not written to disk.

The M4 Pro 14-inch profile applies only to `Mac16,8`; other machines show a labelled generic schematic. Fan sides are assumed. Memory plates and power delivery are conceptual; NAND position is approximate. Core lanes, GPU tiles, and memory cells are symbolic aggregates. Physical battery silhouettes never imply per-cell telemetry. Σ power is exactly CPU + GPU + ANE; DRAM and GPU SRAM are separate. Zero, unavailable, failed, and estimated readings have distinct displays. Energy task rankings are explicitly estimated **CPU-only** power allocations. Network tasks rank visible socket ownership because per-task throughput is unavailable; the GPU task list shows an explicit unavailable state. Wi-Fi signal, SSD temperature, IOPS, and some protected process counters may be unavailable.

```sh
cargo test
cargo clippy --all-targets -- -D warnings
./run.sh --demo --snapshot artifacts/teardown.svg --size 160x46
./run.sh --demo --scenario missing-sensors --snapshot artifacts/fallback.ansi
```

Exports use the actual Ratatui buffer (`.svg`, `.ansi`, or `.txt`). Eight focused tests cover navigation, replay, accounting, signal safety, and rendering. Three snapshot files cover Teardown, telemetry cells, and the six Systems pages at 200×55, 120×36, and 80×24. `--sample` prints one real normalized snapshot as JSON for diagnostics; it includes process names and commands.
