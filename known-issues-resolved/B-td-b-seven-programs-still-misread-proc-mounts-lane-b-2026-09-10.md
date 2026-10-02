## ~~TD-B-SEVEN-PROGRAMS-STILL-MISREAD-PROC-MOUNTS~~ (lane B, 2026-09-10) -- CLOSED the same day

**In short:** `/proc/mounts` escapes a space in a device or mount-point name as
`\040`, and ten programs parse the file by hand. Nine of the ten do not undo
that escaping, so a device or mount point whose name contains a space does not
match the name the user typed. Worse, all ten read the file with
`read_to_string`, which **fails outright** if any single line holds a byte that
is not UTF-8 -- taking every other line with it.

`mkfs` and `fsck` were fixed on 2026-09-10 because in those two the failure had
teeth; the remaining seven are listed below.

**Why `mkfs` and `fsck` came first.** Both call `is_mounted(device)` to decide
whether it is safe to write to a device, and both were written

```rust
let content = match fs::read_to_string("/proc/mounts") {
    Ok(c) => c,
    Err(_) => return false,          // "not mounted"
};
```

So **one mount anywhere on the system with a non-UTF-8 path made every device
report as unmounted**, and `mkfs` would go on to format a live filesystem. The
error path answered the safety question in the dangerous direction: "I could
not read the file" became "nothing is mounted". Both now answer `true` when
they cannot tell, which is the only defensible default for a check that guards
a destructive write.

**The escaping half bit the same two functions.** They compared
`split_whitespace`'s first field against the caller's argument, so a device
called `/dev/my disk` -- listed as `/dev/my\040disk` -- never matched, and a
mounted device reported as free. `procinfo::Mount` undoes the escaping; the
crate's module doc names it as one of the three reasons the crate exists.

**Still to convert** (`/proc/mounts` by hand, no unescaping, `read_to_string`):

| Program | What it uses the file for |
|---|---|
| ~~`userspace/df`~~ | ~~which filesystem a path is on, and its usage~~ -- **done 2026-09-10** |
| ~~`userspace/mount`~~ | ~~whether a target is already mounted~~ -- **done 2026-09-10** |
| ~~`userspace/findmnt`~~ | ~~the whole of its output~~ -- **done 2026-09-10** |
| ~~`userspace/lsblk`~~ | ~~mount points beside each block device~~ -- **done 2026-09-10** |
| ~~`userspace/eject`~~ | ~~whether the device must be unmounted first~~ -- **done** |
| ~~`userspace/grub2`~~ | ~~locating the boot filesystem~~ -- **done** |
| ~~`userspace/udisks`~~ | ~~mount state per device~~ -- **done** |

**`df` and `mount` keep `String` fields and escape at the boundary** rather
than carrying bytes through their table-formatting code. That is deliberate and
worth stating, because it looks like a half-measure: a space is *printable*, so
`escape_unprintable` leaves it alone and a path a user can type compares
exactly as it did before. Only a byte they could not have typed is escaped --
and the alternative for such a byte was taking the whole table down with it.

`userspace/diskutil` already unescaped and was the exception; it is converted
too, because **undoing the escaping was only half the problem**. It still read
the file with `read_to_string`, so one awkward mount left it showing no mount
points at all and the careful unescaping never ran. Half a correct parser is
not a correct parser, and the half that was missing was the one that fails
silently.

**Closed 2026-09-10, derived rather than decremented:** no file under
`userspace/` or `apps/` parses `/proc/mounts` by hand any more -- checked by
grepping for the literal path in files that do not use `procinfo`. 72 files
still open some other `/proc` path without it, down from 95.

**If you re-verify this, grep for bare `procinfo`, not for a method name.**
Re-checking the closure later the same day with `procinfo::(Mount|mounts)`
reported five programs as unconverted -- `coreutils/df`, `diskutil`, `findmnt`,
`lsblk`, `sysinfo` -- and all five were false positives. The call is
`procinfo::ProcFs::new().mounts()`, which contains `procinfo::ProcFs` and not
`procinfo::mounts`; `sysinfo` imports through a braced `use procinfo::{Mount,
...}`, which contains `procinfo::{Mount` and not `procinfo::Mount`; and
`coreutils/df`'s only hit is a comment, since it reads the mount table through
a syscall rather than the file. A regex written around one spelling of an API
answers a narrower question than the one being asked, and its answer is a list
of names that looks exactly like a real finding.

**`grub2`'s was the one with consequences.** It picks the device carrying a
path by longest-prefix match over mount points, so an escaped mount point --
`/mnt/my backup`, which no real path starts with -- simply never matched,
the next-longest won, and the bootloader was told the wrong device with
nothing to indicate it. `eject` and `udisks` used `unwrap_or_default()`, so
their whole-file failure produced an empty mount table rather than an error:
"nothing is mounted". In `eject` that is inert today only because its unmount
is still a `would call umount(...)` stub.

**A related limitation, pinned rather than fixed.** A device whose *name* is
not UTF-8 cannot be named on the command line at all: `mkfs` and `fsck` read
argv through `env::args()`, which panics on such an argument. That is
B-COREUTILS-PANIC-ON-A-NON-UTF-8-ARGUMENT, not this entry, and the tests say so
where a reader would otherwise take it for a parsing failure.
