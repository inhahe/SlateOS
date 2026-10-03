//! Emit the per-crate linker-script reference so the workspace
//! `.cargo/config.toml` doesn't need to know about each crate's own
//! `linker.ld`. CARGO_MANIFEST_DIR is set by cargo to this crate's
//! directory regardless of where cargo was invoked.
//!
//! The custom `linker.ld` is only valid for the bare-metal `x86_64-slateos`
//! target. Emitting it for host builds (e.g. the `x86_64-pc-windows-gnu`
//! target used to compile and run the unit tests) breaks linking: the host
//! std's Windows import libraries don't fit the script's layout, producing
//! "relocation truncated to fit" errors. So we gate the link-arg on the
//! target triple and skip it for anything that isn't slateos.
fn main() {
    // The hand-built sysroot `libc.a` is an input cargo cannot infer. Without
    // this, rebuilding the libc leaves every binary in this crate stale: the
    // build reports `Finished` having relinked nothing. See
    // `userspace/sysroot-dep` for the measurement.
    sysroot_dep::emit();

    println!("cargo:rerun-if-changed=linker.ld");
    // `TARGET` is set by cargo to the target triple (or the JSON spec's file
    // stem, `x86_64-slateos`, for our custom target). Only the slateos target
    // wants the bare-metal linker script.
    let target = std::env::var("TARGET").unwrap_or_default();
    if target.contains("windows") {
        embed_as_invoker_manifest();
    }
    if !target.contains("slateos") {
        return;
    }
    // `env!`, read when this script is compiled -- cargo sets the variable for
    // that too -- rather than `env::var` at run time, which can only fail by
    // panicking, and a build script that panics says less than one that cannot.
    let manifest = env!("CARGO_MANIFEST_DIR");
    let script = format!("{manifest}/linker.ld");
    println!("cargo:rustc-link-arg=-T{script}");
}

/// On a Windows host, give every binary an `asInvoker` application manifest.
///
/// Windows' installer detection demands elevation for an executable whose
/// name contains an installer keyword -- `install`, `setup`, `update`,
/// `patch` -- and this crate builds `install.exe` and `patch.exe`, and
/// `cargo test` runs them as `install-<hash>.exe` and `patch-<hash>.exe`.
/// Where that detection is on, the harness cannot even start: "The requested
/// operation requires elevation" (os error 740). `userspace/install` carried
/// this manifest for its one binary before `install` moved here; the
/// manifest is harmless for the rest, since `asInvoker` is what any of them
/// would be run as anyway. `embed-manifest` writes the resource in pure Rust,
/// so no `windres` is needed.
fn embed_as_invoker_manifest() {
    if let Err(e) =
        embed_manifest::embed_manifest(embed_manifest::new_manifest("SlateOS.coreutils"))
    {
        // A build that cannot embed it still builds; it is a host-only
        // convenience, and the warning says why a test may then need elevation.
        println!("cargo:warning=could not embed the asInvoker manifest: {e}");
    }
}
