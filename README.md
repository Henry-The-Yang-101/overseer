![Overseer terminal demo](assets/overseer-demo.gif)

# Overseer

activity monitor but its in terminal and looks cool 👍

hardcoded to match my own laptop

Press `c` in Teardown to keep the Mac and display awake. Choose until turned off or quit, 30 minutes, 1, 2, or 4 hours, or a custom number of minutes; press `c` again to stop. This uses native process-owned power assertions matching `caffeinate -sudi`, with no helper process. macOS releases them when Overseer exits, crashes, or is killed; timed sessions also expire in macOS. The `-s` behavior follows macOS's AC-power rules.

The GPU tab includes per-process GPU-time rates from the macOS driver's I/O Registry counters, without sudo. Rates refresh about every five seconds, need two samples, and can exceed 100% when work overlaps. Only reporting processes appear; these rates are distinct from whole-GPU utilization. The driver fields are undocumented and may be unavailable on other hardware or macOS versions.
