//! Build script: embed a Windows "asInvoker" application manifest.
//!
//! Windows' installer-detection heuristic demands elevation (UAC) for any
//! executable whose name contains "setup" — which includes both the real
//! `dmsetup.exe` binary and, crucially, the unit-test harness exe that Cargo
//! names `dmsetup-<hash>.exe`. Without a manifest declaring `asInvoker`,
//! `cargo test` cannot even launch the harness: it fails with
//! "The requested operation requires elevation." (os error 740).
//!
//! We previously tried to embed the manifest via `windres`, but `windres` is
//! not installed on the dev machine (the windows-gnu toolchain ships no
//! resource compiler), so that path silently no-op'd and the manifest was
//! never embedded on this host. The proper fix is `embed-manifest`, which
//! generates the COFF resource object in pure Rust — no external tools.
//!
//! `embed_manifest()` links the manifest into the `bins` target kind via
//! `cargo:rustc-link-arg-bins`. The unit-test harness compiled from a binary's
//! `src/main.rs` is part of that same bin target, so the `-bins` link arg
//! covers it — which is exactly what lets `cargo test` launch the harness. The
//! slateos target is left untouched: its TARGET triple matches neither Windows
//! ABI below, so no manifest is linked (the userspace ELF needs none).

fn main() {
    println!("cargo:rerun-if-changed=build.rs");

    let target = std::env::var("TARGET").unwrap_or_default();
    let is_windows = target.contains("windows-msvc")
        || target.contains("windows-gnu")
        || target.contains("windows-gnullvm");
    if !is_windows {
        return;
    }

    // new_manifest defaults to an asInvoker execution level, which is exactly
    // what disables Windows' installer-detection heuristic. embed-manifest
    // generates the COFF resource object in pure Rust (no windres needed).
    embed_manifest::embed_manifest(embed_manifest::new_manifest("SlateOS.dmsetup"))
        .expect("failed to embed Windows manifest");
}
