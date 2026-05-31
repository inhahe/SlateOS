//! Emit the per-crate linker-script reference so the workspace
//! `.cargo/config.toml` doesn't need to know about each crate's own
//! `linker.ld`. CARGO_MANIFEST_DIR is set by cargo to this crate's
//! directory regardless of where cargo was invoked.
fn main() {
    let manifest = std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR set by cargo");
    let script = format!("{manifest}/linker.ld");
    println!("cargo:rustc-link-arg=-T{script}");
    println!("cargo:rerun-if-changed=linker.ld");
}
