# Request: Embed coreutils binaries in kernel

**From**: osb2 (shell zone)
**For**: os (kernel-core zone)

## What's needed

Add the 8 coreutils binaries to the kernel's embedded binaries and
deploy them to /bin/ on boot, alongside hello, ticker, and shell.

In `kernel/src/main.rs`, add include_bytes for each:

```rust
static ECHO_ELF: &[u8] = include_bytes!(
    "../../userspace/coreutils/target/x86_64-ouros/release/echo"
);
static CAT_ELF: &[u8] = include_bytes!(
    "../../userspace/coreutils/target/x86_64-ouros/release/cat"
);
static LS_ELF: &[u8] = include_bytes!(
    "../../userspace/coreutils/target/x86_64-ouros/release/ls"
);
static HEAD_ELF: &[u8] = include_bytes!(
    "../../userspace/coreutils/target/x86_64-ouros/release/head"
);
static WC_ELF: &[u8] = include_bytes!(
    "../../userspace/coreutils/target/x86_64-ouros/release/wc"
);
static MKDIR_ELF: &[u8] = include_bytes!(
    "../../userspace/coreutils/target/x86_64-ouros/release/mkdir"
);
static RM_ELF: &[u8] = include_bytes!(
    "../../userspace/coreutils/target/x86_64-ouros/release/rm"
);
static CP_ELF: &[u8] = include_bytes!(
    "../../userspace/coreutils/target/x86_64-ouros/release/cp"
);
```

And in VFS population, install each to /bin/:

```rust
for (name, elf) in [
    ("echo", ECHO_ELF), ("cat", CAT_ELF), ("ls", LS_ELF),
    ("head", HEAD_ELF), ("wc", WC_ELF), ("mkdir", MKDIR_ELF),
    ("rm", RM_ELF), ("cp", CP_ELF),
] {
    let path = format!("/bin/{name}");
    if let Err(e) = fs::Vfs::write_file(&path, elf) {
        serial_println!("[init] WARNING: failed to write {}: {:?}", path, e);
    } else {
        serial_println!("[init] Installed {} ({} bytes)", path, elf.len());
    }
}
```

## Why

These are standard Unix utilities needed for basic OS usability. They
exercise the full Rust std toolchain (file I/O, directory ops, formatted
output, environment access). Having them available at boot makes the OS
interactive — users can list files, copy them, read them, etc.

## Sizes

| Binary | Size |
|--------|------|
| echo   | 655 KiB |
| cat    | 681 KiB |
| ls     | 1.2 MiB |
| head   | 682 KiB |
| wc     | 680 KiB |
| mkdir  | 657 KiB |
| rm     | 1.2 MiB |
| cp     | 1.2 MiB |
| **Total** | **~6.0 MiB** |

This will increase the kernel image size by ~6 MiB. The debug build is
already ~9MB, so this is significant but manageable.

## Build dependency

Coreutils must be built before the kernel:
```powershell
# Build sysroot first (if not already done)
.\toolchain\build-sysroot.ps1

# Build coreutils
cd userspace/coreutils
cargo +nightly build --release

# Then build kernel
cd ../..
cargo build --release
```

## Priority

Medium-high — these utilities make the OS actually usable from a shell.
Can be batched with the shell embedding request.
