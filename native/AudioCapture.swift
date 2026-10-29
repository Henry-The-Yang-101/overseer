import Foundation
import CoreAudio

func emit(_ object: [String: Any]) {
    if let data = try? JSONSerialization.data(withJSONObject: object) {
        FileHandle.standardOutput.write(data + Data([10]))
    }
}
func check(_ status: OSStatus, _ operation: String) throws {
    if status != noErr { throw NSError(domain: "\(operation) failed (\(status)). Allow Overseer Audio in System Settings → Privacy & Security → Screen & System Audio Recording, then toggle capture off/on.", code: Int(status)) }
}
if #available(macOS 14.2, *) {
    var tap: AudioObjectID = 0
    var device: AudioObjectID = 0
    var io: AudioDeviceIOProcID?
    func cleanup() {
        if device != 0 {
            if let io { AudioDeviceStop(device, io); AudioDeviceDestroyIOProcID(device, io) }
            AudioHardwareDestroyAggregateDevice(device)
        }
        if tap != 0 { AudioHardwareDestroyProcessTap(tap) }
    }
    do {
        let desc = CATapDescription(stereoGlobalTapButExcludeProcesses: [])
        desc.name = "Overseer Audio"
        desc.isPrivate = true
        desc.muteBehavior = .unmuted
        try check(AudioHardwareCreateProcessTap(desc, &tap), "Create audio tap")
        var format = AudioStreamBasicDescription()
        var size = UInt32(MemoryLayout.size(ofValue: format))
        var address = AudioObjectPropertyAddress(mSelector: kAudioTapPropertyFormat, mScope: kAudioObjectPropertyScopeGlobal, mElement: kAudioObjectPropertyElementMain)
        try check(AudioObjectGetPropertyData(tap, &address, 0, nil, &size, &format), "Read audio format")
        guard format.mFormatID == kAudioFormatLinearPCM, format.mBitsPerChannel == 32, format.mFormatFlags & kAudioFormatFlagIsFloat != 0 else {
            throw NSError(domain: "Unsupported audio sample format", code: 1)
        }
        let config: [String: Any] = [
            kAudioAggregateDeviceNameKey: "Overseer Audio",
            kAudioAggregateDeviceUIDKey: UUID().uuidString,
            kAudioAggregateDeviceIsPrivateKey: true,
            kAudioAggregateDeviceTapAutoStartKey: true,
            kAudioAggregateDeviceTapListKey: [[kAudioSubTapUIDKey: desc.uuid.uuidString, kAudioSubTapDriftCompensationKey: true]]
        ]
        try check(AudioHardwareCreateAggregateDevice(config as CFDictionary, &device), "Create capture device")
        let lock = NSLock()
        var latest = [[Float]](repeating: [Float](repeating: 0, count: 64), count: 2)
        var received = false
        try check(AudioDeviceCreateIOProcIDWithBlock(&io, device, nil) { _, input, _, _, _ in
            // No allocation, I/O or waiting on the real-time audio thread.
            guard lock.try() else { return }
            defer { lock.unlock() }
            let buffers = UnsafeMutableAudioBufferListPointer(UnsafeMutablePointer(mutating: input))
            var sampled = false
            for channel in 0..<2 {
                guard !buffers.isEmpty else { continue }
                let buffer = buffers[min(channel, buffers.count - 1)]
                let channels = max(1, Int(buffer.mNumberChannels))
                let offset = buffers.count > 1 ? 0 : min(channel, channels - 1)
                let frames = Int(buffer.mDataByteSize) / (4 * channels)
                guard frames > 0, let raw = buffer.mData else { continue }
                sampled = true
                let samples = raw.assumingMemoryBound(to: Float.self)
                for bucket in 0..<64 {
                    var peak: Float = 0
                    for frame in (bucket * frames / 64)..<max(bucket * frames / 64 + 1, (bucket + 1) * frames / 64) {
                        let v = samples[min(frame, frames - 1) * channels + offset]
                        if v.isFinite && abs(v) > abs(peak) { peak = min(1, max(-1, v)) }
                    }
                    latest[channel][bucket] = peak
                }
            }
            received = sampled
        }, "Create audio callback")
        try check(AudioDeviceStart(device, io), "Start capture")
        emit(["status": "Waiting for audio / macOS permission"])
        let timer = DispatchSource.makeTimerSource(queue: .main)
        timer.schedule(deadline: .now(), repeating: .milliseconds(50))
        timer.setEventHandler {
            lock.lock()
            let frame = latest.map { $0.map { $0 } }
            let ready = received
            received = false
            lock.unlock()
            if ready { emit(["status": "Live system output · stereo", "channels": frame]) }
        }
        timer.resume()
        DispatchQueue.global().async {
            while !FileHandle.standardInput.availableData.isEmpty {}
            DispatchQueue.main.async { timer.cancel(); cleanup(); exit(0) }
        }
        RunLoop.main.run()
        cleanup()
    } catch {
        emit(["status": (error as NSError).domain])
        cleanup()
        exit(1)
    }
} else {
    emit(["status": "Audio capture requires macOS 14.2 or later"])
}
