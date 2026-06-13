# Request: Embed coreutils binaries in kernel

**From**: osb2 (shell zone)
**For**: os (kernel-core zone)

## What's needed

Add the 60 coreutils binaries to the kernel's embedded binaries and
deploy them to /bin/ on boot, alongside hello, ticker, and shell.

Recommend using a loop over the binary directory or a build script that
scans `userspace/coreutils/target/x86_64-slateos/release/` for all
binaries and generates `include_bytes!` statics automatically.

Example pattern in `kernel/src/main.rs`:

```rust
// One include_bytes per utility. Generate with a build script or
// maintain a list. Key binaries (sorted):
const COREUTILS: &[(&str, &[u8])] = &[
    ("basename", include_bytes!("../../userspace/coreutils/target/x86_64-slateos/release/basename")),
    ("cat", include_bytes!("../../userspace/coreutils/target/x86_64-slateos/release/cat")),
    ("chmod", include_bytes!("../../userspace/coreutils/target/x86_64-slateos/release/chmod")),
    // ... etc for all 60 binaries
];

// At VFS init time:
for (name, elf) in COREUTILS {
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

60 binaries, ranging from ~650 KiB to ~1.3 MiB each.
Estimated total: ~45 MiB embedded in kernel image.

This is large — consider either:
1. Embedding only essential utilities (echo, cat, ls, cp, mv, rm, mkdir,
   chmod, stat, kill, ps, test, which, find) and loading the rest from
   a disk image.
2. Using a compressed initramfs instead of include_bytes!.
3. Stripping with `opt-level = "z"` (currently using `"s"`).

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

## Full binary list (60 total)

basename, cat, chmod, chown, comm, cp, cut, date, dd, df, dirname,
du, echo, env, expand, expr, false, find, fold, grep, head, hostname,
id, kill, ln, ls, md5sum, mkdir, mkfifo, mv, nice, nl, nohup, paste,
printf, ps, pwd, readlink, realpath, rm, rmdir, seq, sha256sum, sleep,
sort, stat, tail, tee, test, touch, tr, true, tty, uname, uniq, wc,
which, whoami, xargs, yes

## Priority

High — these utilities make the OS actually usable from a shell.
Can be batched with the shell embedding request.
