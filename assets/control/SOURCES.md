# Control Center assets

These meshes retain their original licenses, separately from Overseer's MIT source code.

| File | Original work / creator | License |
|---|---|---|
| `airpods.mesh` | [AirPods Pro](https://sketchfab.com/3d-models/airpods-pro-4478dddbfa244f6f9b93eebbd027a73b) by JY_JY_ | [CC BY 4.0](https://creativecommons.org/licenses/by/4.0/) |
| `sony-xm5.mesh` | [Sony Wh-1000xm5](https://sketchfab.com/3d-models/sony-wh-1000xm5-5d8aea0a780b49fa89c9c205912414e3) by [Zalaneke](https://sketchfab.com/Zalanekee) | [CC BY 4.0](https://creativecommons.org/licenses/by/4.0/) |
| `mx-master-3s.mesh` | [Mouse Logitech MX Master 3S white](https://sketchfab.com/3d-models/mouse-logitech-mx-master-3s-white-dc82de97cbe64c6a8deae3328208c984) by [Guibazilla](https://sketchfab.com/Guibazilla) | [CC BY 4.0](https://creativecommons.org/licenses/by/4.0/) |

Changes: normalized and simplified geometry, sampled monochrome materials, new terminal lighting. AirPods assemblies are separated into case, lid, left, and right earbuds, with original hinge/exploded motion. This is not Apple's proprietary iPhone animation. AirPods GLB obtained from [SceneView's credited mirror](https://sceneview.github.io/credits.html); Sony and mouse downloads obtained through Sketchfab. Conversion: `scripts/import_control_model.py`.

Other built-in avatars are original approximate meshes based on product form factors. Bluetooth names do not identify the iPhone/iPad generation or the generic mouse's exact model. AULA F99 Pro and Galaxy Buds FE use approximate geometry; no suitable freely redistributable exact mesh was found.

The [Keychron K8 Pro CAD](https://github.com/Keychron/Keychron-Keyboards-Hardware-Design/tree/main/K-Pro-Series/K8%20Pro) permits personal derivatives but [restricts redistribution](https://github.com/Keychron/Keychron-Keyboards-Hardware-Design/blob/main/LICENSE). A personal mesh may be installed at `$XDG_CONFIG_HOME/overseer/models/keychron-k8.mesh` (default `~/.config/overseer/models/`). It is not included in this repository.

# Implementation references

- [Raycast Speedtest](https://github.com/raycast/extensions/blob/main/extensions/speedtest/src/lib/speedtest.ts) wraps Ookla's CLI. [speedtest-rs](https://docs.rs/crate/speedtest-rs/latest) and [netrunner-core](https://docs.rs/netrunner-core/latest/netrunner_core/) were reviewed. Overseer instead supervises macOS `networkQuality -c -M 12 -I <wifi>` from Rust, avoiding another installed binary and automatic acceptance of Ookla terms. Like [Raycast Network Speed](https://www.raycast.com/koinzhang/network-speed), this uses Apple's measurement service. It tests the current Wi-Fi interface; `s` starts/cancels, and leaving Control Center cancels it.
- ICMP probes go to `1.1.1.1` over Wi-Fi, once per second while the Wi-Fi tab is open. The last 120 attempts determine loss. Jitter is mean absolute change between adjacent successful RTTs; gaps are not interpolated. Loaded probes are colored separately. This is end-to-end latency/loss, not radio-level retransmission statistics.
- Wi-Fi and Bluetooth have no mutating API, scanning, pairing, connection, or power controls. Bluetooth inventory and batteries are macOS's known-device cache; missing batteries stay unknown and disconnected readings are labeled cached. Location privacy can redact SSID/BSSID/country.
- [media-control](https://github.com/ungive/media-control) supplies the existing listener, artwork and transport commands. macOS exposes one selected Now Playing session through this API; other apps currently producing audio are listed through CoreAudio. Transport applies to that selected player, with an identity check before each command. It cannot provide independent track metadata/control for every audio-producing app.
- [artem](https://github.com/FineFindus/artem) was reviewed for colored ASCII conversion. The TUI uses `image` plus per-cell RGB/luminance mapping so Ratatui owns clipping and colors. Artwork is decoded under size/allocation limits, never saved by the app. The waveform and stereo phase plot reuse the existing optional system-output capture; `e` toggles capture.
- Display arrangement uses CoreGraphics coordinates; brightness is read when the display exposes it. Audio routes/volume/mute use CoreAudio. Missing capabilities remain unavailable.
