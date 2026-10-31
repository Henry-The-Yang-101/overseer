# Bluetooth feasibility

Investigated 2026-10-30 on macOS 27.0.1 (26A434), arm64, with the macOS 27.0 SDK. Research only; no TUI/backend implementation or connection changes.

## Verified on this Mac

`/usr/sbin/system_profiler SPBluetoothDataType -json -timeout 5` reported Bluetooth **on**, three connected devices (AirPods Pro, Keychron K8 Pro, MX Master 3S), and seven other device entries. Three subsequent queries took 50–65 ms each; this is a local observation, not a latency guarantee.

- Controller fields: power, address, chipset, firmware, transport, discoverability, vendor/product IDs, supported services.
- Device fields, when available: name, address, type, firmware/case firmware, vendor/product IDs, service/profile flags, RSSI, serial numbers, and separate AirPods left/right/case batteries.
- The keyboard and mouse had no battery field in that report. IORegistry checks of `AppleDeviceManagementHIDEventService` and `IOHIDDevice` did not yield their batteries either.
- Disconnected entries retained metadata and sometimes RSSI. These are cached OS records, not proof of current proximity or pairing. The report does not supply reliable measurement timestamps for those fields.
- A temporary Swift probe compiled successfully. `CBManager.authorization` returned `.notDetermined`; the probe deliberately stopped before creating a manager or scanning. Permission, scan coverage, on/off transitions, connect/disconnect, and pairing were **not** exercised.

No device addresses or serial-number values are recorded here.

## Capability map

| Capability | Practical source | Limits |
| --- | --- | --- |
| Bluetooth on/off | Profiler `controller_state`; public `IOBluetoothHostController.powerState`; `CBCentralManager.state` | Keep permission denied, resetting, unavailable, and unknown separate from off. SDK also exposes host power notifications. |
| Connected and known inventory | Profiler plus `IOBluetoothDevice.pairedDevices()`, `isPaired()`, `isConnected()` and connection notifications | Validate coverage of LE devices; do not turn every profiler entry into a confirmed paired device. |
| Nearby Classic devices | `IOBluetoothDeviceInquiry` | Finds discoverable devices; presence in a saved list does not mean nearby. |
| Nearby LE devices | `CBCentralManager.scanForPeripherals(withServices: nil)` | Advertising devices only. Names and metadata may be absent; scanning is not an inventory of every physical device nearby. |
| Metadata | Profiler, cached Classic SDP records, LE advertisement data | Availability varies. Advertisements can add service UUIDs/data, manufacturer data, connectability, TX power, and RSSI. |
| Additional LE metadata | Explicit GATT service/characteristic reads | Battery (`180F`) and Device Information (`180A`) can add details if exposed/readable. Requires an app connection, may require authentication, and is not passive inspection. |
| Connect/disconnect Classic | `IOBluetoothDevice.openConnection` / `closeConnection` | Baseband operations; confirm actual profile usability and observed system state. Connection success does not itself guarantee audio routing. |
| Connect/disconnect LE | Public Core Bluetooth for the app's own connection; investigate private IOBluetooth bridge for system actions | Public cancellation does **not** guarantee system disconnection while another client retains a connection. |
| Pair a new device | `IOBluetoothDevicePair` for the Classic path | Supports PIN requests, displayed passkeys, numeric confirmation, completion/error callbacks. LE bonding is device/security dependent; test separately. |

Core Bluetooth also supports some Classic communication; that does not make it a replacement for the system's complete device-management interface. Its connected-peripheral retrieval requires service UUIDs and is not a general paired-device list.

## Existing tool and private API findings

[blueutil](https://github.com/toy/blueutil) is a useful Classic reference: inspect, inquire, connect, disconnect, pair, and JSON output. Its power/discoverability controls use private APIs. Its profiler alternative addresses some connection-status discrepancies, but does not supply comprehensive LE discovery/actions. It is not installed on this Mac.

[Issue #110](https://github.com/toy/blueutil/issues/110) reports an MX Master 3S discovery gap on the same macOS build and includes a [proposed patch](https://github.com/user-attachments/files/33121552/patch.diff.txt) combining Core Bluetooth discovery with private `+[IOBluetoothDevice deviceWithCBPeripheral:]`. The author reports pairing/connection success; this is an unmerged proposal, not independently validated behavior here. Runtime introspection confirmed that exact class method exists locally (`@24@0:8@16`), without invoking it. It is the most focused candidate for bridging LE UUIDs into IOBluetooth actions; disconnect still needs its own verification.

Local introspection also found private `CBController`, `CBDiscovery`, `CBDevice`, and `CBConnection` classes, with inventory/discovery handlers, component battery/firmware/serial fields, and connection/pairing selectors. Their presence proves neither permission nor entitlement access nor Settings parity. Treat these as optional research paths, guarded by runtime capability checks.

## Recommended Overseer design

1. Add a dedicated Bluetooth view with connected, confirmed saved, and nearby groups, plus a selected-device details pane. Preserve unknown fields and show source/freshness. Nearby entries need actual scan sightings and expiry; RSSI is signal strength, not a reliable distance estimate.
2. Use a native Swift helper with a small Objective-C shim if the private LE bridge works. This fits the existing `build.rs`/native-helper pattern. Exchange structured commands/events with Rust, including pairing prompts and action results; keep all work off rendering and telemetry collection threads.
3. Use bounded profiler snapshots for broad inventory/metadata, initially every 5–10 seconds while the view is open and after actions. Add native notifications for responsiveness. Keep OS metadata caching distinct from our snapshot time. Scan Classic/LE only on an explicit, finite scan request; retain LE peripheral objects for selected actions.
4. Start with public Classic actions. Capability-check the LE bridge, then enable each LE action only after real-device validation. Match addresses or resolved identities; never merge devices solely by display name or assume a Core Bluetooth UUID is a MAC address. Confirm outcomes against a fresh inventory, with deadlines and meaningful errors.
5. Provide connect/disconnect only where supported, and a separate Pair flow for new devices with PIN/passkey/confirmation handling. Keep a Settings fallback for unsupported pairing. Power is informational in this scope.

The helper needs `NSBluetoothAlwaysUsageDescription` and a deliberate permission flow. Start it when the user enters Bluetooth functionality, expose denied/restricted states, and test permission attribution/signing in the actual distributed executable. Existing audio-helper packaging is a useful starting point, not proof that Bluetooth permission behaves identically.

Before promising Settings-like actions, validate AirPods audio/profile reconnection, Keychron HID reconnection, MX Master LE discovery and system disconnect/reconnect, pairing of a new Classic and LE device, cancellation/timeouts, radio-off behavior, denied permission, and devices auto-reconnecting. Until then, broad read coverage is demonstrated; action and discovery coverage remains provisional.

## Primary references

- [Apple Bluetooth Settings scope](https://support.apple.com/en-gb/guide/mac-help/-blth8111/mac)
- [IOBluetoothDevice](https://developer.apple.com/documentation/iobluetooth/iobluetoothdevice), [host controller](https://developer.apple.com/documentation/iobluetooth/iobluetoothhostcontroller), [inquiry](https://developer.apple.com/documentation/iobluetooth/iobluetoothdeviceinquiry), [pairing](https://developer.apple.com/documentation/iobluetooth/iobluetoothdevicepair)
- [Core Bluetooth scanning](https://developer.apple.com/documentation/corebluetooth/cbcentralmanager/scanforperipherals(withservices:options:)), [local disconnect semantics](https://developer.apple.com/documentation/corebluetooth/cbcentralmanager/cancelperipheralconnection(_:)), [Classic support](https://developer.apple.com/documentation/corebluetooth/using-core-bluetooth-classic)
- [Bluetooth permission purpose string](https://developer.apple.com/documentation/bundleresources/information-property-list/nsbluetoothalwaysusagedescription), [Bluetooth SIG assigned service numbers](https://www.bluetooth.com/wp-content/uploads/Files/Specification/HTML/Assigned_Numbers/out/en/index-en.html)
- Installed SDK headers: `IOBluetooth.framework/Versions/A/Headers/objc/{IOBluetoothHostController,IOBluetoothDevice,IOBluetoothDeviceInquiry,IOBluetoothDevicePair}.h` and `CoreBluetooth.framework/Headers/{CBManager,CBCentralManager,CBCentralManagerConstants}.h` beneath the Xcode macOS SDK.
