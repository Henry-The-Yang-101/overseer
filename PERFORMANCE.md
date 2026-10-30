# Efficiency audit — 2026-10-29

Audited production revision `5a68a81` on an Apple M4 Pro (12 CPU cores, 24 GiB RAM), macOS 27.0.1. Production code was not changed. A live collector probe saw 673 processes.

## Live measurements

Release binary in a 160×46 PTY, four separate 35-second runs. Main/audio CPU uses cumulative `ps` CPU-time deltas after a five-second warmup; memory is median RSS during that interval. **100% CPU means one fully occupied core.** Settings were isolated in temporary directories; the user's running app and preferences were left alone.

| Mode | Main CPU | Main RSS | Audio CPU | Audio RSS |
| --- | ---: | ---: | ---: | ---: |
| Teardown, full motion + audio | 9.35% | 32.2 MiB | 1.64% | 17.0 MiB |
| Systems CPU, full motion + audio | 6.42% | 21.7 MiB | 1.51% | 14.6 MiB |
| Teardown, animation off + audio | 3.82% | 21.8 MiB | 1.25% | 14.9 MiB |
| Teardown, full motion, audio disabled | 6.44% | 25.3 MiB | — | — |

Short-lived telemetry commands add CPU beyond those columns. Whole-run CPU including startup and reaped descendants was approximately 14.45%, 11.25%, 8.63%, and 9.40%, respectively (`wait4` user+system time / 35 seconds). These totals can omit children still running at parent exit. Audio-disabled Teardown also selects a lower frame rate, so its difference does not isolate audio-processing cost.

These are short observations on a working machine, not controlled audio-playback comparisons. They exclude the terminal emulator's painting/compositing cost and do not measure battery drain. Startup RSS does not represent a full history buffer.

## Retention and rendering

Expanded `examples/benchmark.rs` to cover every Systems page, all three details tabs, row rebuilds/search, and configurable process counts/hours. Fixtures retain 1 Hz system samples, 5-second traffic snapshots, and the existing 15-second archived process tables. Static metadata is shared; real process churn and collector allocations are outside this test.

Twelve **simulated** hours were ingested rapidly, not observed over twelve wall-clock hours:

| Processes | RSS at hour 1 | Peak hourly RSS | RSS at hour 12 | Live malloc bytes, hours 1–12 |
| --- | ---: | ---: | ---: | ---: |
| 725 | 94.7 MiB | 140.2 MiB | 135.9 MiB | 81.1 MiB, constant |
| 2,000 | 216.0 MiB | 389.7 MiB | 369.9 MiB | 196.2 MiB, constant |

Both retained exactly 3,601 samples, 241 process tables, and 721 extra snapshots after each hour. Allocator-reserved memory reached 160/412 MiB, respectively. This fixture shows stable live allocations with allocator retention/fragmentation increasing RSS; it does **not** establish absence of leaks in the live native collectors or under process churn.

Release render benchmarks at 200×55, motion off, with a full history buffer (120/60 frames per case at 725/2,000 processes):

| Work | 725 processes | 2,000 processes |
| --- | ---: | ---: |
| Teardown | 0.49 ms/frame | 0.50 ms/frame |
| Systems CPU | 0.49 ms/frame | 0.50 ms/frame |
| Energy | 0.69 ms/frame | 2.47 ms/frame |
| Network | 0.68 ms/frame | 1.27 ms/frame |
| Memory / Stats | 0.87 ms/frame | 1.77 ms/frame |
| Energy / Apps | 1.36 ms/frame | 4.54 ms/frame |
| Network row rebuild | 0.30 ms/refresh | 1.82 ms/refresh |
| CPU search for `safari` | 0.85 ms/refresh | 2.73 ms/refresh |

TestBackend includes buffer construction/diffing, but excludes collectors, audio, terminal I/O and terminal painting. At 30 FPS, 4.54 ms/frame alone would consume roughly 13.6% of one core; this is a projection, not a live measurement of that panel.

## Improvements worth making

1. **Cache snapshot-derived work.** `src/ui/processes.rs:13,158` recomputes the all-process CPU denominator for each Energy row, giving quadratic work per frame. Compute the denominator once per sample, preserving the current formula. `src/metrics/extra.rs:66` linearly searches traffic for each lookup; index by `(pid, start)` once per extra snapshot, retaining stale/missing-data guards. Apps grouping and formatting in `src/ui/insights.rs:389` also run every frame on a pinned snapshot. Cache them with page/tab, selection, expansion, width, units, and relevant history changes included in invalidation. These changes can preserve every reading and animation frame.
2. **Avoid rebuilding unchanged process rows while exit ghosts are visible.** `src/app.rs:1805` bypasses its cache whenever any row is exited. `tick()` can consequently clone/filter/sort the whole table each frame for two seconds. Rebuild when the snapshot, controls, or ghost membership changes; use a set of live process identities for exit detection instead of the nested scan at line 1823. Preserve the two-second exit indication and selection behavior.
3. **Reduce history allocation churn.** The bounded history is working, but allocator overhead is material. Consider reusing expired process/traffic vector capacity when its `Arc` has no other owners, and sharing immutable application metadata (currently rebuilt in `src/metrics/collector.rs:385`). Pinned/replay snapshots must remain immutable. Benchmark RSS and live heap separately; do not shorten retention or remove fields to claim an equivalent improvement.
4. **Reuse fuzzy-search machinery.** `src/app.rs:1940` constructs a matcher, pattern and scratch buffers per process, even when filtering only needs a match result. Reuse these per query/refresh and preserve `user:`/`pid:` behavior and Unicode matching.
5. **Profile collector subprocess costs before a larger rewrite.** The collector still costs CPU with animation off. Ports launch 3–4 `ioreg` commands per scan; power mode invokes `pmset`; extended telemetry launches a Swift helper and `nettop` and enumerates process identities twice. Batching native reads may reduce overhead while preserving existing cadence and PID-reuse checks. Individual savings are not yet measured. Keeping background recording is necessary for complete replay/history.

Start with snapshot-derived caches and row invalidation; normal views already render quickly. Keep the existing frame rates, sensor intervals, history duration, and waveform quality while measuring each change.

## Reproduction and validation

```sh
cargo build --release --examples --bin overseer
cargo run --release --example benchmark
cargo run --release --example benchmark -- --processes 2000 --frames 60
cargo run --release --example benchmark -- --hours 12 --processes 2000 --frames 1
cargo test
cargo clippy --all-targets -- -D warnings
```

The 15 existing tests (including unchanged layout snapshots), release build, and all-target Clippy passed. No tests or production behavior were added. Local raw results and the ad hoc PTY measurement script are in ignored `artifacts/performance/`; the script expects `/tmp/overseer-audit` as its output directory and a release binary path argument.
