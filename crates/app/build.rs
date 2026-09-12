//! One custom cfg: `sandboxed`, set for the targets that can host neither
//! subprocesses nor dlopen nor a JIT -- the browser (wasm32) and iOS --
//! and for Android, which could host all three but has nothing to host:
//! the plug-in helpers are desktop binaries, the agent CLIs are desktop
//! installs, and the store updates the app. The plug-in hosts, the AI
//! sidebar's agent CLIs, the self-updater and the desktop drag-out are
//! compiled out there; everything else that is "native" (files, threads,
//! sockets, the GPU compositor) still is on iOS and Android, so those
//! sites keep testing for wasm32 alone.
fn main() {
    println!("cargo::rustc-check-cfg=cfg(sandboxed)");
    let os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let arch = std::env::var("CARGO_CFG_TARGET_ARCH").unwrap_or_default();
    if arch == "wasm32" || os == "ios" || os == "android" {
        println!("cargo::rustc-cfg=sandboxed");
    }
    println!("cargo::rerun-if-changed=build.rs");
}
