//! Make `toolchain/sysroot/lib/libc.a` a build input.
//!
//! Without this, rebuilding the libc leaves every binary in this crate stale:
//! cargo does not know the archive exists, so nothing that links it is out of
//! date and the build reports `Finished` having done nothing. See
//! `userspace/sysroot-dep` for the measurement.
fn main() {
    sysroot_dep::emit();
}
