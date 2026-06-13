# Request: Embed shell binary in kernel

**From**: osb2 (shell zone)
**For**: os (kernel-core zone)

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
