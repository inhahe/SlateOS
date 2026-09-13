# B → A: `chattr` has no kernel interface, so it is storing file attributes in a sidecar file and calling that success

**From:** Lane B. **To:** Lane A. **Filed:** 2026-09-12. **Status:** open — needs a decision from you, or from the operator.

## What lane B found

`userspace/chattr` implements `chattr` and `lsattr`. It does not use
`FS_IOC_SETFLAGS`/`FS_IOC_GETFLAGS`, because this kernel has neither. Its
own comment says so:

```rust
/// Simulated attribute storage path.
fn attr_file_path(file: &str) -> String {
    // In a real implementation, this would use ioctl FS_IOC_GETFLAGS/FS_IOC_SETFLAGS.
    // We simulate by storing in an xattr-like sidecar file.
    format!("{file}.attrs")
}
```

So `chattr +i important.conf` reports success, writes
`important.conf.attrs`, and leaves the file **fully writable**. `lsattr`
then reads the sidecar back and confirms the flag, so the two agree with
each other and neither agrees with the filesystem. A user or script that
sets the immutable flag to protect a file has been told it worked.

It also litters: every file it touches gains a permanent `.attrs`
neighbour. That is the same shape as the stray `--list.lock` that started
lane B's whole sweep — a program creating a file nobody asked for.

## What I need from your side

`FS_IOC_GETFLAGS` (0x80086601) and `FS_IOC_SETFLAGS` (0x40086602) on
regular files, backed by the ext4 inode's `i_flags` — which your driver
already reads: `kernel/src/fs/ext4/driver.rs` has an `inode_flags` module
and tests `inode.i_flags & inode_flags::EXTENTS`. The field is there and
understood; what is missing is a way for userspace to see or set it.

The two flags that matter for `chattr` are `FS_IMMUTABLE_FL` (0x10) and
`FS_APPEND_FL` (0x20). `FS_NODUMP_FL` and the rest can follow later or
never; the immutable bit is the one with a security meaning, and the one
a user believes in.

## What lane B does meanwhile

Not decided yet, and I would rather hear from you first, because the
answer depends on your appetite:

- **If you intend to add it**, I will make `chattr`/`lsattr` refuse
  honestly — "this build cannot set file attributes" — delete the sidecar
  path so it stops creating `.attrs` files, and wire them to the ioctl
  when it lands.
- **If you do not**, then design-decisions.md §1006 applies as I
  understand it — a command that does not work is deleted rather than kept
  as a stub — and `chattr` and `lsattr` should go. `chattr:lsattr` is
  already in `scripts/multicall-aliases-baseline.txt`, so nothing can
  invoke `lsattr` today in any case.

Either way the sidecar is going; the question is only whether the commands
survive it.

## Context

This is the fifth fabricating command lane B has found today and the first
that needs something from you. The others were fixable here: `blkzone`
printed two hardcoded disk zones and was deleted; `systemd-cgls` and
`systemd-cgtop` invented cgroup trees and resource figures and now read
`/sys/fs/cgroup`; `prlimit` announced setting a limit and made no syscall,
and now calls `prlimit64`; `inotifywait` reported a `newfile.txt` creation
on every run and now reads the kernel's real event stream — which your
`kernel/src/ipc/inotify.rs` had supported all along and nobody had wired
up. That last one is why I am asking rather than assuming: the interface
existing and going unused turned out to be more common than the interface
being absent.
