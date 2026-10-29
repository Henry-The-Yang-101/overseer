fn main() {
    println!("cargo:rerun-if-changed=native/AudioCapture.swift");
    println!("cargo:rerun-if-changed=native/AudioCapture.plist");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("macos") {
        return;
    }
    let out = std::path::PathBuf::from(std::env::var_os("OUT_DIR").unwrap()).join("overseer-audio");
    let plist = std::env::current_dir()
        .unwrap()
        .join("native/AudioCapture.plist");
    let status = std::process::Command::new("xcrun")
        .args(["swiftc", "-O", "native/AudioCapture.swift", "-o"])
        .arg(&out)
        .args([
            "-Xlinker",
            "-sectcreate",
            "-Xlinker",
            "__TEXT",
            "-Xlinker",
            "__info_plist",
            "-Xlinker",
        ])
        .arg(plist)
        .status()
        .expect("Swift compiler required for macOS audio capture");
    assert!(status.success(), "Audio helper compilation failed");
    assert!(
        std::process::Command::new("codesign")
            .args([
                "--force",
                "--sign",
                "-",
                "--identifier",
                "local.overseer.audio"
            ])
            .arg(&out)
            .status()
            .unwrap()
            .success()
    );
}
