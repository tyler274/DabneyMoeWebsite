fn main() {
    // Google Play requires 16 KB ELF LOAD-segment alignment on 64-bit Android
    // ABIs (Android 15+ / API 35+). Emit linker flags here because Tauri's
    // Android CLI overrides `CARGO_TARGET_*_RUSTFLAGS` in the Gradle environment.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("android") {
        let arch = std::env::var("CARGO_CFG_TARGET_ARCH").unwrap_or_default();
        if arch == "aarch64" || arch == "x86_64" {
            println!("cargo:rustc-link-arg=-Wl,-z,max-page-size=16384");
            println!("cargo:rustc-link-arg=-Wl,-z,common-page-size=16384");
        }
    }
    tauri_build::build();
}
