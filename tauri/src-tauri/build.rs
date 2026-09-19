fn main() {
    // Keep the JNI entry points alive through LTO and stripping.
    //
    // The JVM calls these by symbol lookup; nothing in Rust references them, so
    // a release build treats them as dead code and drops them. The result is an
    // app that works in debug and dies with `UnsatisfiedLinkError` in release.
    //
    // `#[used]` on each function stops the compiler from eliminating them, but
    // that is not sufficient on its own: the linker still discards sections that
    // nothing appears to reference. `--undefined` forces each symbol to be
    // treated as a root, so it survives to the final `.so`.
    //
    // Passed via `cargo:rustc-link-arg` (not `rustflags`) so it only applies to
    // the final link, and only for Android targets — the host build has no JNI
    // entry points to preserve.
    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();

    if target_os == "android" {
        // Must match the exact mangled names exported in camera/mobile/android.rs.
        // A typo here fails silently: the flag is accepted, the symbol is simply
        // not kept, and the crash only appears on a release device build.
        const JNI_SYMBOLS: [&str; 3] = [
            "Java_org_antitimpa_antitimpa_CameraBridge_antitimpa_1push_1frame",
            "Java_org_antitimpa_antitimpa_CameraBridge_antitimpa_1set_1stream_1active",
            "Java_org_antitimpa_antitimpa_CameraBridge_antitimpa_1frames_1received",
        ];

        for symbol in JNI_SYMBOLS {
            println!("cargo:rustc-link-arg=-Wl,--undefined={symbol}");
        }
    }

    tauri_build::build()
}
