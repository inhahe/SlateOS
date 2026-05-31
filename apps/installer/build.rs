//! Build script: embed a Windows application manifest declaring an `asInvoker`
//! execution level.
//!
//! The host test harnesses for this crate are named `installer-<hash>.exe`.
//! Windows installer-detection heuristics force a UAC elevation prompt for
//! executables whose name contains installer keywords ("setup", "install",
//! "update", "patch") — "installer" matches "install" — and the elevation
//! request makes the cargo test runner fail with "os error 740: requires
//! elevation". Embedding an explicit `asInvoker` manifest opts the binaries
//! out of that heuristic.
//!
//! Unlike single-binary crates, this package has BOTH a `[lib]` and a `[[bin]]`
//! target, so `cargo test` produces two unit-test harnesses (one per target),
//! both named `installer-<hash>.exe`. `embed_manifest()` only emits
//! `cargo:rustc-link-arg-bins`, which covers the bin (and its unit-test
//! harness) but NOT the lib's unit-test harness. We therefore additionally
//! emit the broad `cargo:rustc-link-arg` scope — which Cargo applies to the
//! lib unit-test build — pointing at the COFF object that `embed_manifest`
//! generated, so the lib harness is opted out as well.
//!
//! The manifest is only relevant on Windows hosts; the OurOS target triple
//! matches no Windows ABI, so this is a no-op when cross-compiling.

use std::path::PathBuf;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    let target = std::env::var("TARGET").unwrap_or_default();
    let is_windows = target.contains("windows-msvc")
        || target.contains("windows-gnu")
        || target.contains("windows-gnullvm");
    if !is_windows {
        return;
    }

    embed_manifest::embed_manifest(embed_manifest::new_manifest("OurOS.installer"))
        .expect("failed to embed Windows manifest");

    // embed_manifest() generates the COFF object at OUT_DIR/embed-manifest.o and
    // links it via `rustc-link-arg-bins`. That scope misses the lib unit-test
    // harness, so add the broad scope (which Cargo applies to the lib unit-test
    // build as well) pointing at the same object file. The broad scope also
    // covers bins, so the bin and bin unit-test link the object twice; that is
    // benign (the harnesses launch without UAC, proving Windows parsed a valid
    // asInvoker manifest) and in any case only affects the host test artifacts
    // — the shipped OurOS binary takes the early `return` above and has no
    // embedded manifest at all.
    if (target.contains("windows-gnu") || target.contains("windows-gnullvm"))
        && let Some(out_dir) = std::env::var_os("OUT_DIR")
    {
        let obj = PathBuf::from(out_dir).join("embed-manifest.o");
        println!("cargo:rustc-link-arg={}", obj.display());
    }
}
