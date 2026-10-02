## A-SYSFS-KEEPS-A-THIRD-HOSTNAME-THAT-NOTHING-ELSE-READS (found by lane B, 2026-09-10; `kernel/**`, lane A owns the fix) — **fixed 2026-09-10; there were FOUR stores, not three** (see the resolution note at the end of this file)

**In short:** the system has two hostnames. Writing `/sys/kernel/hostname`
changes one of them; every program that asks the system its name reads the
other. The write reports success.

`kernel/src/fs/sysfs.rs:77` declares

```rust
static HOSTNAME: Mutex<String> = Mutex::new(String::new());
```

private to that file, defaulting to `"mintos"` -- the project's former name.
`hostname()` (line 80) reads it and `set_hostname()` (line 90) writes it, and
they are reached from `read_file`/`write_file` for `SysPath::KernelFile("hostname")`.
Nothing else in the tree touches that static.

Meanwhile `/proc/sys/kernel/hostname` (`kernel/src/fs/procfs.rs:14557`) reads
`crate::fs::nameservice::get_hostname()`, and so does `uname`, and so does
`posix`'s `gethostname` via `/proc`. So:

| Path | Store | Writable |
|---|---|---|
| `/proc/sys/kernel/hostname` | `fs::nameservice` | no -- procfs `write_file` serves only `/proc/<pid>/oom_score_adj` |
| `/sys/kernel/hostname` | `sysfs.rs`'s own static | yes, mode 0644 |

The two agree only until somebody writes the second one. After that the
machine answers `hostname` one way and `uname -n` the other, and the file that
accepted the change is the one nobody reads.

**The self-test cannot fail.** `sysfs.rs:1283-1296` writes `/kernel/hostname`,
reads it back, and asserts they match. Both ends are the same private static,
so it passes whatever the rest of the system believes. This is the same shape
as `posix`'s `test_setdomainname_roundtrip`, fixed on 2026-09-09: a round trip
through one buffer is evidence about the buffer, not about the system, and it
reads exactly like evidence about the system.

**Why this surfaced now.** Lane A is adding `SYS_HOSTNAME_SET` (1072), gated on
a new `Rights::SET_HOSTNAME`, precisely so that setting the hostname is a
checked operation against the real store. That work is right, and this entry is
not an argument against it -- but while it lands, an unguarded file write sets a
decoy. Lane A also declined to add a hostname *getter* syscall, on the ground
(which lane B proposed and lane A agreed with) that one value with two sources
is the defect shape to avoid. That defect already exists here.

Three bounds are also in play and disagree: `sysfs::set_hostname` caps at 253,
`nameservice` at 253, and the new syscall at 64 (Linux's `__NEW_UTS_LEN`). A
name of 100 bytes is settable by one route and refused by another.

**What lane B did about it:** nothing to `kernel/**`, which is not ours.
`userspace/dhcpcd`'s `set_hostname` was changed to go through
`posix::unistd::sethostname` and to report its failure, and its doc records
explicitly that `/sys/kernel/hostname` was **not** chosen despite being the one
path that accepts a write -- a write that appears to work and sets a value
nobody reads is worse than one that fails. Reported to lane A by notice the
same day.

**The fix, when lane A takes it:** point `sysfs.rs`'s `hostname()` and
`set_hostname()` at `crate::fs::nameservice` and delete the static, so both
paths are views of one value; then make the write go through the same
capability check as `SYS_HOSTNAME_SET`, or make the file read-only and let the
syscall be the only writer. The self-test should compare against procfs's
bytes, the way `sysfs.rs`'s `version`/`ostype`/`osrelease` tests already do
after the 2026-08-22 fix in that same file -- the pattern is present two
hundred lines above the bug.
