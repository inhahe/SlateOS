//! Build script: embed a Windows application manifest declaring an `asInvoker`
//! execution level.
//!
//! The host test harness for this crate is named `losetup-<hash>.exe`. Windows
//! installer-detection heuristics force a UAC elevation prompt for executables
//! whose name contains installer keywords ("setup", "install", "update",
//! "patch") — "losetup" matches "setup" — and the elevation request makes the
//! cargo test runner fail with "os error 740: requires elevation". Embedding an
//! explicit `asInvoker` manifest opts the binary out of that heuristic.
//!
//! The manifest is only relevant on Windows hosts; the Slate OS target triple
//! matches no Windows ABI, so this is a no-op when cross-compiling.

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    let target = std::env::var("TARGET").unwrap_or_default();
    let is_windows = target.contains("windows-msvc")
        || target.contains("windows-gnu")
        || target.contains("windows-gnullvm");
    if !is_windows {
        return;
    }
    embed_manifest::embed_manifest(embed_manifest::new_manifest("SlateOS.losetup"))
        .expect("failed to embed Windows manifest");
}
