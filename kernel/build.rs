// Emit the kernel's linker script as a `-T` link argument, anchored to
// this crate's manifest dir so the path is correct no matter what cwd
// cargo was invoked from.
//
// This used to live in the workspace-root `.cargo/config.toml` as
// `link-arg=-Tkernel/linker.ld`, but that flag is merged into every
// crate targeting `x86_64-unknown-none` — including the bare-metal
// services in `services/`, which then failed to link because
// `kernel/linker.ld` doesn't exist relative to their build cwd. Keeping
// it in a build.rs scopes it to the kernel crate only.

fn main() {
    let manifest = std::env::var("CARGO_MANIFEST_DIR")
        .expect("CARGO_MANIFEST_DIR is set by cargo");
    // Linker script anchored to this crate's directory. Lives here
    // rather than in `.cargo/config.toml` because cargo merges rustflags
    // into every crate sharing the target triple — a workspace-level
    // `link-arg=-T<path>` would also be passed when building bare-metal
    // services, which need their own linker scripts.
    println!("cargo:rustc-link-arg=-T{manifest}/linker.ld");
    println!("cargo:rerun-if-changed=linker.ld");
}
