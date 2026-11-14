import Foundation
import AppKit
import CoreWLAN
import SystemConfiguration
import CoreAudio
import CoreGraphics
import IOKit
import Darwin

// Only audio actions are accepted. Wi-Fi, Bluetooth and display paths have no setters,
// scans, connection APIs, or power controls. All callers receive explicit failures.
func fail(_ message: String) throws -> Never {
    throw NSError(domain: message, code: 1)
}
func address(_ selector: AudioObjectPropertySelector, _ scope: AudioObjectPropertyScope = kAudioObjectPropertyScopeGlobal, _ element: UInt32 = kAudioObjectPropertyElementMain) -> AudioObjectPropertyAddress {
    AudioObjectPropertyAddress(mSelector: selector, mScope: scope, mElement: element)
}
func read<T: BitwiseCopyable>(_ id: AudioObjectID, _ selector: AudioObjectPropertySelector, _ initial: T, _ scope: AudioObjectPropertyScope = kAudioObjectPropertyScopeGlobal, _ element: UInt32 = kAudioObjectPropertyElementMain) -> T? {
    var key = address(selector, scope, element), value = initial
    var size = UInt32(MemoryLayout<T>.size)
    guard AudioObjectGetPropertyData(id, &key, 0, nil, &size, &value) == noErr else { return nil }
    return value
}
func string(_ id: AudioObjectID, _ selector: AudioObjectPropertySelector) -> String? {
    var key = address(selector), value: Unmanaged<CFString>? = nil
    var size = UInt32(MemoryLayout<Unmanaged<CFString>?>.size)
    guard AudioObjectGetPropertyData(id, &key, 0, nil, &size, &value) == noErr else { return nil }
    return value?.takeRetainedValue() as String?
}
func ids(_ id: AudioObjectID, _ selector: AudioObjectPropertySelector) -> [AudioObjectID] {
    var key = address(selector), size: UInt32 = 0
    guard AudioObjectGetPropertyDataSize(id, &key, 0, nil, &size) == noErr, size > 0, size < 65536 else { return [] }
    var values = [AudioObjectID](repeating: 0, count: Int(size) / MemoryLayout<AudioObjectID>.size)
    guard AudioObjectGetPropertyData(id, &key, 0, nil, &size, &values) == noErr else { return [] }
    return values
}
func channels(_ id: AudioObjectID, _ scope: AudioObjectPropertyScope) -> UInt32 {
    var key = address(kAudioDevicePropertyStreamConfiguration, scope), size: UInt32 = 0
    guard AudioObjectGetPropertyDataSize(id, &key, 0, nil, &size) == noErr, size >= MemoryLayout<AudioBufferList>.size, size < 65536 else { return 0 }
    let raw = UnsafeMutableRawPointer.allocate(byteCount: Int(size), alignment: MemoryLayout<AudioBufferList>.alignment)
    defer { raw.deallocate() }
    guard AudioObjectGetPropertyData(id, &key, 0, nil, &size, raw) == noErr else { return 0 }
    return UnsafeMutableAudioBufferListPointer(raw.assumingMemoryBound(to: AudioBufferList.self)).reduce(0) { $0 + $1.mNumberChannels }
}
func settable(_ id: AudioObjectID, _ selector: AudioObjectPropertySelector, _ element: UInt32 = 0) -> Bool {
    var key = address(selector, kAudioDevicePropertyScopeOutput, element), value: DarwinBoolean = false
    return AudioObjectIsPropertySettable(id, &key, &value) == noErr && value.boolValue
}
func volumeElements(_ id: AudioObjectID) -> [UInt32] {
    if settable(id, kAudioDevicePropertyVolumeScalar) { return [0] }
    return (1...max(1, channels(id, kAudioDevicePropertyScopeOutput))).filter { settable(id, kAudioDevicePropertyVolumeScalar, $0) }
}
func write<T: BitwiseCopyable>(_ id: AudioObjectID, _ selector: AudioObjectPropertySelector, _ value: T, _ scope: AudioObjectPropertyScope = kAudioObjectPropertyScopeGlobal, _ element: UInt32 = 0) throws {
    var key = address(selector, scope, element), value = value
    let status = AudioObjectSetPropertyData(id, &key, 0, nil, UInt32(MemoryLayout<T>.size), &value)
    if status != noErr { try fail("Audio control failed (\(status))") }
}
let system = AudioObjectID(kAudioObjectSystemObject)
func audio() -> [String: Any] {
    let output = read(system, kAudioHardwarePropertyDefaultOutputDevice, UInt32(0)) ?? 0
    let input = read(system, kAudioHardwarePropertyDefaultInputDevice, UInt32(0)) ?? 0
    var devices: [[String: Any]] = []
    for id in ids(system, kAudioHardwarePropertyDevices) {
        let ins = channels(id, kAudioDevicePropertyScopeInput), outs = channels(id, kAudioDevicePropertyScopeOutput)
        guard ins > 0 || outs > 0, read(id, kAudioDevicePropertyDeviceIsAlive, UInt32(0)) == 1 else { continue }
        // The private waveform tap is an implementation detail, not a route the user can select.
        let uid = string(id, kAudioDevicePropertyDeviceUID) ?? ""
        let name = string(id, kAudioObjectPropertyName) ?? "Audio device"
        if name == "Overseer Audio" { continue }
        var d: [String: Any] = ["id": id, "uid": uid, "name": name, "inputs": ins, "outputs": outs,
            "default_input": id == input, "default_output": id == output,
            "can_volume": !volumeElements(id).isEmpty, "can_mute": settable(id, kAudioDevicePropertyMute)]
        d["manufacturer"] = string(id, kAudioObjectPropertyManufacturer)
        d["sample_rate"] = read(id, kAudioDevicePropertyNominalSampleRate, Float64(0))
        let elements = volumeElements(id)
        let levels = (elements.isEmpty ? [0] : elements).compactMap { read(id, kAudioDevicePropertyVolumeScalar, Float32(0), kAudioDevicePropertyScopeOutput, $0) }
        if !levels.isEmpty { d["volume"] = levels.reduce(0, +) / Float32(levels.count) }
        if let muted = read(id, kAudioDevicePropertyMute, UInt32(0), kAudioDevicePropertyScopeOutput) { d["muted"] = muted != 0 }
        devices.append(d)
    }
    var sources: [[String: Any]] = []
    if #available(macOS 14.2, *) {
        for id in ids(system, kAudioHardwarePropertyProcessObjectList) {
            guard read(id, kAudioProcessPropertyIsRunningOutput, UInt32(0)) == 1 else { continue }
            let pid = read(id, kAudioProcessPropertyPID, Int32(0)) ?? 0
            let app = NSRunningApplication(processIdentifier: pid)
            let bundle = string(id, kAudioProcessPropertyBundleID) ?? app?.bundleIdentifier ?? ""
            sources.append(["pid": pid, "bundle": bundle, "name": app?.localizedName ?? bundle])
        }
    }
    return ["devices": devices, "sources": sources]
}
func controlAudio(_ args: [String]) throws -> [String: Any] {
    guard let command = args.first else { try fail("Missing audio action") }
    if command == "output" || command == "input" {
        guard args.count == 2, let id = UInt32(args[1]), ids(system, kAudioHardwarePropertyDevices).contains(id),
              channels(id, command == "output" ? kAudioDevicePropertyScopeOutput : kAudioDevicePropertyScopeInput) > 0,
              read(id, kAudioDevicePropertyDeviceIsAlive, UInt32(0)) == 1 else { try fail("Audio device is no longer available") }
        try write(system, command == "output" ? kAudioHardwarePropertyDefaultOutputDevice : kAudioHardwarePropertyDefaultInputDevice, id)
    } else {
        guard let id = read(system, kAudioHardwarePropertyDefaultOutputDevice, UInt32(0)), id != 0 else { try fail("No default output") }
        if command == "volume" {
            guard args.count == 2, let delta = Float32(args[1]), delta.isFinite, abs(delta) <= 0.1 else { try fail("Invalid volume step") }
            let elements = volumeElements(id)
            guard !elements.isEmpty else { try fail("This output has fixed volume; use its hardware control") }
            for element in elements {
                guard let level = read(id, kAudioDevicePropertyVolumeScalar, Float32(0), kAudioDevicePropertyScopeOutput, element) else { try fail("Volume unavailable") }
                try write(id, kAudioDevicePropertyVolumeScalar, max(0, min(1, level + delta)), kAudioDevicePropertyScopeOutput, element)
            }
        } else if command == "mute" {
            guard settable(id, kAudioDevicePropertyMute), let muted = read(id, kAudioDevicePropertyMute, UInt32(0), kAudioDevicePropertyScopeOutput) else { try fail("This output does not support mute") }
            try write(id, kAudioDevicePropertyMute, UInt32(muted == 0 ? 1 : 0), kAudioDevicePropertyScopeOutput)
        } else { try fail("Unknown audio action") }
    }
    return audio()
}
func wifi() -> [String: Any] {
    guard let wifi = CWWiFiClient.shared().interface() else { return ["status": "Wi-Fi interface unavailable"] }
    let connected = wifi.powerOn() && wifi.wlanChannel() != nil && wifi.rssiValue() != 0
    var result: [String: Any] = ["powered": wifi.powerOn(), "connected": connected,
        "interface": wifi.interfaceName ?? "", "status": connected ? "Connected" : (wifi.powerOn() ? "Not associated" : "Wi-Fi off")]
    result["ssid"] = wifi.ssid()
    result["bssid"] = wifi.bssid()
    result["country"] = wifi.countryCode()
    result["mac"] = wifi.hardwareAddress()
    if connected {
        result["rssi"] = wifi.rssiValue()
        result["noise"] = wifi.noiseMeasurement() == 0 ? nil : wifi.noiseMeasurement()
        result["tx_mbps"] = wifi.transmitRate() > 0 ? wifi.transmitRate() : nil
        result["phy"] = [0: "Unknown", 1: "802.11a", 2: "802.11b", 3: "802.11g", 4: "802.11n", 5: "802.11ac", 6: "802.11ax", 7: "802.11be"][Int(wifi.activePHYMode().rawValue)] ?? "Unknown"
        result["security"] = [0:"Open",1:"WEP",2:"WPA Personal",3:"WPA/WPA2 Personal",4:"WPA2 Personal",5:"Personal",6:"Dynamic WEP",7:"WPA Enterprise",8:"WPA/WPA2 Enterprise",9:"WPA2 Enterprise",10:"Enterprise",11:"WPA3 Personal",12:"WPA3 Enterprise",13:"WPA2/WPA3 Personal",14:"OWE",15:"OWE Transition"][Int(wifi.security().rawValue)] ?? "Unknown"
        if let channel = wifi.wlanChannel() {
            result["channel"] = channel.channelNumber
            result["band"] = [1:"2.4 GHz",2:"5 GHz",3:"6 GHz"][Int(channel.channelBand.rawValue)] ?? "Unknown"
            result["width"] = [1:20,2:40,3:80,4:160,5:320][Int(channel.channelWidth.rawValue)]
        }
    }
    if let store = SCDynamicStoreCreate(nil, "Overseer Control Center" as CFString, nil, nil) {
        let name = wifi.interfaceName ?? "en0"
        let v4 = SCDynamicStoreCopyValue(store, "State:/Network/Interface/\(name)/IPv4" as CFString) as? [String: Any] ?? [:]
        let v6 = SCDynamicStoreCopyValue(store, "State:/Network/Interface/\(name)/IPv6" as CFString) as? [String: Any] ?? [:]
        let dns = SCDynamicStoreCopyValue(store, "State:/Network/Global/DNS" as CFString) as? [String: Any] ?? [:]
        result["ipv4"] = v4["Addresses"]
        result["ipv6"] = v6["Addresses"]
        result["gateway"] = v4["Router"]
        result["dns"] = dns["ServerAddresses"]
        if let global = SCDynamicStoreCopyValue(store, "State:/Network/Global/IPv4" as CFString) as? [String: Any] {
            result["primary_interface"] = global["PrimaryInterface"]
            if global["PrimaryInterface"] as? String == name { result["gateway"] = global["Router"] }
        }
    }
    return result
}
func displays() -> [[String: Any]] {
    typealias Brightness = @convention(c) (UInt32, UnsafeMutablePointer<Float>) -> Int32
    let library = dlopen("/System/Library/PrivateFrameworks/DisplayServices.framework/DisplayServices", RTLD_LAZY | RTLD_LOCAL)
    defer { if let library { dlclose(library) } }
    let getter = library.flatMap { dlsym($0, "DisplayServicesGetBrightness") }.map { unsafeBitCast($0, to: Brightness.self) }
    var count: UInt32 = 0
    guard CGGetOnlineDisplayList(0, nil, &count) == .success, count > 0, count <= 64 else { return [] }
    var online = [CGDirectDisplayID](repeating: 0, count: Int(count))
    guard CGGetOnlineDisplayList(count, &online, &count) == .success else { return [] }
    return online.prefix(Int(count)).map { id in
        let screen = NSScreen.screens.first { ($0.deviceDescription[NSDeviceDescriptionKey("NSScreenNumber")] as? NSNumber)?.uint32Value == id }
        let bounds = CGDisplayBounds(id), mm = CGDisplayScreenSize(id), mode = CGDisplayCopyDisplayMode(id)
        let scale = screen?.backingScaleFactor ?? mode.map { Double($0.pixelWidth) / Double(max(1, $0.width)) } ?? 1
        var d: [String: Any] = ["id": id, "name": screen?.localizedName ?? "Display \(id)", "x": bounds.origin.x, "y": bounds.origin.y,
            "width": bounds.width, "height": bounds.height, "scale": scale,
            "builtin": CGDisplayIsBuiltin(id) != 0, "main": CGDisplayIsMain(id) != 0, "rotation": CGDisplayRotation(id),
            "mirrored": CGDisplayIsInMirrorSet(id) != 0, "mm_width": mm.width, "mm_height": mm.height,
            "max_refresh": screen?.maximumFramesPerSecond ?? 0, "edr": screen?.maximumPotentialExtendedDynamicRangeColorComponentValue ?? 1,
            "vendor": CGDisplayVendorNumber(id), "product": CGDisplayModelNumber(id),
            "color_space": screen?.colorSpace?.localizedName ?? "Unknown"]
        if let mode {
            d["pixel_width"] = mode.pixelWidth; d["pixel_height"] = mode.pixelHeight
            if mode.refreshRate > 0 { d["refresh"] = mode.refreshRate }
        }
        var brightness: Float = 0
        if let getter, getter(id, &brightness) == 0, brightness.isFinite, (0...1).contains(brightness) { d["brightness"] = brightness }
        return d
    }
}
do {
    let args = Array(CommandLine.arguments.dropFirst())
    let result: Any
    switch args.first {
    case "wifi": result = wifi()
    case "display": result = displays()
    case "sound": result = audio()
    case "audio": result = try controlAudio(Array(args.dropFirst()))
    default: try fail("Unknown Control Center request")
    }
    let data = try JSONSerialization.data(withJSONObject: result, options: [.sortedKeys])
    FileHandle.standardOutput.write(data)
} catch {
    let data = try! JSONSerialization.data(withJSONObject: ["error": error.localizedDescription])
    FileHandle.standardOutput.write(data)
    exit(1)
}
