//! Build script: embed a Windows "asInvoker" application manifest.
//!
//! Windows' installer-detection heuristic demands elevation (UAC) for any
//! executable whose name contains an installer keyword such as "install" —
//! which includes both the real `install.exe` binary and the unit-test harness
//! exe that Cargo names `install-<hash>.exe`. Without a manifest declaring
//! `asInvoker`, `cargo test` cannot even launch the harness: it fails with
//! "The requested operation requires elevation." (os error 740).
//!
//! `embed-manifest` generates the COFF resource object in pure Rust (no
//! external tools such as `windres`, which is not installed on the dev
//! machine). Its `cargo:rustc-link-arg-bins` covers the bin's unit-test
//! harness. The ouros target is left untouched: its TARGET triple matches no
//! Windows ABI, so no manifest is linked.

fn main() {
    println!("cargo:rerun-if-changed=build.rs");

    let target = std::env::var("TARGET").unwrap_or_default();
    let is_windows = target.contains("windows-msvc")
        || target.contains("windows-gnu")
        || target.contains("windows-gnullvm");
    if !is_windows {
        return;
    }

    embed_manifest::embed_manifest(embed_manifest::new_manifest("OurOS.install"))
        .expect("failed to embed Windows manifest");
}
