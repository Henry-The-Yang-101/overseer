import Foundation
import CoreWLAN
import SystemConfiguration
import IOKit
import IOKit.pwr_mgt
import IOKit.ps
import ObjectiveC
import Darwin

// PowerUI's manual-limit getter is distinct from its legacy currentChargeLimit getter.
// Resolve only read methods, and omit unknown signatures/results instead of reporting OFF.
func chargingPolicy() -> [String: Any] {
    guard dlopen("/System/Library/PrivateFrameworks/PowerUI.framework/PowerUI", RTLD_LAZY | RTLD_LOCAL) != nil,
          let cls = NSClassFromString("PowerUISmartChargeClient") as? NSObject.Type else { return [:] }
    let initializer = NSSelectorFromString("initWithClientName:")
    guard let method = class_getInstanceMethod(cls, initializer),
          let encoding = method_getTypeEncoding(method), String(cString: encoding) == "@24@0:8@16",
          let allocation = cls.perform(NSSelectorFromString("alloc")) else { return [:] }
    let allocated = allocation.takeUnretainedValue() as! NSObject
    guard let client = allocated.perform(initializer, with: "Overseer" as NSString)?.takeRetainedValue() as? NSObject else { return [:] }
    func read(_ name: String, byte: Bool = false) -> UInt? {
        let selector = NSSelectorFromString(name)
        guard let method = class_getInstanceMethod(cls, selector),
              let encoding = method_getTypeEncoding(method),
              String(cString: encoding) == (byte ? "C24@0:8^@16" : "Q24@0:8^@16") else { return nil }
        var error: AnyObject?
        let value: UInt
        if byte {
            typealias Getter = @convention(c) (AnyObject, Selector, AutoreleasingUnsafeMutablePointer<AnyObject?>) -> UInt8
            value = UInt(unsafeBitCast(method_getImplementation(method), to: Getter.self)(client, selector, &error))
        } else {
            typealias Getter = @convention(c) (AnyObject, Selector, AutoreleasingUnsafeMutablePointer<AnyObject?>) -> UInt
            value = unsafeBitCast(method_getImplementation(method), to: Getter.self)(client, selector, &error)
        }
        return error == nil ? value : nil
    }
    var result: [String: Any] = [:]
    if let limit = read("getMCLLimitWithError:", byte: true), (1...100).contains(limit) {
        result["limit"] = limit
    }
    for (key, method) in [("limit_enabled", "isMCLCurrentlyEnabled:"), ("optimized", "isSmartChargingCurrentlyEnabled:")] {
        if let value = read(method), value <= 1 { result[key] = value == 1 }
    }
    return result
}

// A short-lived, read-only helper. No network probes, scanning, or permission prompts.
var result: [String: Any] = [:]
result["charging"] = chargingPolicy()
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
