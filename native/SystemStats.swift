import Foundation
import CoreWLAN
import SystemConfiguration
import IOKit
import IOKit.pwr_mgt
import IOKit.ps

// A short-lived, read-only helper. No network probes, scanning, or permission prompts.
var result: [String: Any] = [:]
if let wifi = CWWiFiClient.shared().interface(), wifi.powerOn(),
   let channel = wifi.wlanChannel(), wifi.rssiValue() != 0 {
    var value: [String: Any] = ["interface": wifi.interfaceName ?? "",
        "rssi": wifi.rssiValue(), "channel": channel.channelNumber]
    if wifi.transmitRate() > 0 { value["tx_mbps"] = wifi.transmitRate() }
    if wifi.noiseMeasurement() != 0 { value["noise"] = wifi.noiseMeasurement() }
    result["wifi"] = value
}

if let store = SCDynamicStoreCreate(nil, "Overseer" as CFString, nil, nil) {
    var routes: [[String: String]] = []
    for family in ["IPv4", "IPv6"] {
        if let global = SCDynamicStoreCopyValue(store, "State:/Network/Global/\(family)" as CFString) as? [String: Any],
           let interface = global["PrimaryInterface"] as? String {
            routes.append(["family": family, "interface": interface,
                "gateway": global["Router"] as? String ?? "unavailable"])
        }
    }
    result["routes"] = routes
}

var assertions: Unmanaged<CFDictionary>?
if IOPMCopyAssertionsByProcess(&assertions) == kIOReturnSuccess,
   let dict = assertions?.takeRetainedValue() as? [AnyHashable: Any] {
    var records: [[String: Any]] = []
    for (pid, items) in dict {
        for item in items as? [[String: Any]] ?? [] {
            guard (item[kIOPMAssertionLevelKey] as? NSNumber)?.intValue != 0,
                  let type = item[kIOPMAssertionTypeKey] as? String,
                  ["PreventUserIdleSystemSleep", "PreventUserIdleDisplaySleep", "PreventSystemSleep", "NoIdleSleepAssertion"].contains(type)
            else { continue }
            records.append(["pid": UInt32(String(describing: pid)) ?? 0, "kind": type,
                "reason": item[kIOPMAssertionNameKey] as? String ?? "Unnamed assertion"])
        }
    }
    result["assertions"] = records
}

if let adapter = IOPSCopyExternalPowerAdapterDetails()?.takeRetainedValue() as? [String: Any] {
    var value: [String: Any] = [:]
    if let watts = adapter[kIOPSPowerAdapterWattsKey] as? NSNumber, watts.doubleValue > 0 { value["watts"] = watts }
    if let current = adapter[kIOPSPowerAdapterCurrentKey] as? NSNumber { value["milliamps"] = current }
    result["adapter"] = value
}

var iterator: io_iterator_t = 0
if IOServiceGetMatchingServices(kIOMainPortDefault, IOServiceMatching("IOBlockStorageDriver"), &iterator) == KERN_SUCCESS {
    defer { IOObjectRelease(iterator) }
    var disks: [[String: Any]] = []
    while true {
        let driver = IOIteratorNext(iterator)
        if driver == 0 { break }
        defer { IOObjectRelease(driver) }
        guard let stats = IORegistryEntryCreateCFProperty(driver, "Statistics" as CFString, nil, 0)?.takeRetainedValue() as? [String: Any],
              let reads = stats["Operations (Read)"], let writes = stats["Operations (Write)"],
              let readBytes = stats["Bytes (Read)"], let writeBytes = stats["Bytes (Write)"] else { continue }
        var child: io_registry_entry_t = 0
        guard IORegistryEntryGetChildEntry(driver, kIOServicePlane, &child) == KERN_SUCCESS else { continue }
        defer { IOObjectRelease(child) }
        guard let bsd = IORegistryEntryCreateCFProperty(child, "BSD Name" as CFString, nil, 0)?.takeRetainedValue() as? String else { continue }
        var id: UInt64 = 0
        IORegistryEntryGetRegistryEntryID(driver, &id)
        disks.append(["id": id, "name": bsd, "reads": reads, "writes": writes,
            "read_bytes": readBytes, "write_bytes": writeBytes])
    }
    result["disks"] = disks
}
if let data = try? JSONSerialization.data(withJSONObject: result, options: [.sortedKeys]) {
    FileHandle.standardOutput.write(data)
}
