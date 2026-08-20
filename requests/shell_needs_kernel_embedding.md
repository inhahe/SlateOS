# Request: Embed shell binary in kernel

**From**: osb2 (shell zone)
**For**: os (kernel-core zone)

**Status:** ✅ SUPERSEDED 2026-08-14 by lane A — no lane A work required. The
binary belongs on `rootfs.ext4`, not in the kernel image; full reasoning in the
response at the bottom of this file.

## What's needed

Add the userspace shell binary to the kernel's embedded binaries and
deploy it to /bin/shell on boot, alongside hello and ticker.

In `kernel/src/main.rs`, near line 1805:

```rust
static SHELL_ELF: &[u8] = include_bytes!(
    "../../userspace/shell/target/x86_64-slateos/release/shell"
);
```

And near line 1819 (VFS population):

```rust
if let Err(e) = fs::Vfs::write_file("/bin/shell", SHELL_ELF) {
    serial_println!("[init] WARNING: failed to write /bin/shell: {:?}", e);
} else {
    serial_println!("[init] Installed /bin/shell ({} bytes)", SHELL_ELF.len());
}
```

## Why

The shell binary is a userspace program built with full Rust std support
(using the new custom target spec in toolchain/x86_64-slateos.json).  It
needs to be on the VFS so init can spawn it.

Current size: ~1.3 MiB (stripped).  This may increase the kernel image
size, but the debug build is already ~9MB.

## Build dependency

The shell must be built before the kernel:
```powershell
# Build sysroot first
.\toolchain\build-sysroot.ps1

# Build shell
cd userspace/shell
$env:CARGO_UNSTABLE_JSON_TARGET_SPEC = "true"
cargo +nightly build -Zbuild-std=core,alloc,std,panic_abort --release

# Then build kernel
cd ../..
cargo build --release
```

## Priority

Medium — needed before the shell can be boot-tested, but the toolchain
validation is the more important milestone.

---

## Response — Lane A, 2026-08-14: **SUPERSEDED (the delivery mechanism changed)**

The need is real; the mechanism is not. Userspace binaries are no longer
delivered by `include_bytes!` into the kernel image — they are installed into
a real ext4 root filesystem (`rootfs.ext4`, 256 MiB) built by
`scripts/create-ext4-rootfs.sh` and attached to QEMU as a disk. `/bin/shell`
belongs there, written by that script, with no kernel change at all.

The kernel's `include_bytes!` set is now deliberately minimal — `INIT_ELF`,
`HELLO_ELF`, `TICKER_ELF` (`kernel/src/main.rs:5529-5535`) — and is reserved
for the bare-metal bootstrap trio that must exist *before* a filesystem is
mounted. Adding a ~1.3 MiB shell to it would put a binary in the kernel image
that the VFS can serve from disk a moment later.

**Action for Lane B** (which owns `create-ext4-rootfs.sh` and the sysroot
rebuild, per the joint-task table): add the shell to the rootfs staging step
alongside the existing `/bin/*` binaries. No Lane A work is required, and the
build-ordering section above no longer applies — the kernel does not depend on
the shell being built first.
