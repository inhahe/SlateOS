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
    println!("cargo:rerun-if-changed=linker.ld");
    // `TARGET` is set by cargo to the target triple (or the JSON spec's file
    // stem, `x86_64-slateos`, for our custom target). Only the slateos target
    // wants the bare-metal linker script.
    let target = std::env::var("TARGET").unwrap_or_default();
    if !target.contains("slateos") {
        return;
    }
    let manifest = std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR set by cargo");
    let script = format!("{manifest}/linker.ld");
    println!("cargo:rustc-link-arg=-T{script}");
}
