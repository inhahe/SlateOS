## B-A-FORTY-ONE-SELF-TESTS-HAD-NEVER-RUN (lane A, 2026-08-22) — FIXED 2026-08-22

**In short:** 41 `self_test` functions in `kernel/src` could not be called by
any code path in the kernel. They compiled, they were counted as coverage in
commit messages, and none had ever executed. All 41 are now wired into the boot
path, and `scripts/check-self-tests-wired.py` runs before every build to keep
the number at zero.

**How it was found.** `evdev::self_test` was committed without a caller. One
commit later it was wired in fatally, and the *first* boot that ran it failed
on `B-A-EVDEV-SYN-DROPPED-ARRIVES-ONE-RECORD-LATE` — a real input-stack
ordering bug that had been in the tree unnoticed. A test that finds a genuine
defect the instant it first runs is an argument that the other unrun tests are
worth running, so the tree was audited.

**The 41.** 38 under `fs/` (`archive`, `backup`, `batch`, `changetrack`,
`contextmenu`, `cpufreq`, `cputopo`, `dedup`, `deskicons`, `dirsync`, `diskio`,
`encrypt`, `fcompress`, `fileselect`, `filetype`, `fswalk`, `health`, `ioprio`,
`iso9660`, `linkcheck`, `openwith`, `policy`, `powerwake`, `properties`,
`readdir_plus`, `reclaim`, `search`, `sidebar`, `snapshot`, `splice`,
`statusbar`, `sysctlfs`, `sysuptime`, `tags`, `thermal`, `transaction`,
`undelete`, `usage`), plus `sockact`, `sync` and `virtio::blk`.

**`virtio::blk::self_test` was a different shape of dead.** It takes
`&mut VirtioBlkDevice` and was written against `virtio::blk::with_device`,
which reads a global populated only by `virtio::blk::init()` — the
single-device probe path that `blkdev::init_multi`/`probe_all` superseded and
that nothing has called since. Wiring it via `with_device` would have produced
a test that silently skips on every boot, which is the same disease with better
manners. It is now called from `blkdev::init_multi`, the one point on the live
boot path where a concrete `VirtioBlkDevice` is owned — one line later it is
boxed as `dyn BlockDevice` and the driver-specific test is unreachable by
construction. It runs once per registered disk and is non-fatal, so a dead
virtqueue is reported without turning one bad disk into an unbootable kernel.

**Why the compiler could not have caught it.** An uncalled `pub fn` in a
library crate is not dead code as far as `rustc` is concerned — something
outside the crate might call it. Several also carried `#[allow(dead_code)]`.
Nothing in the toolchain separates "exported for callers" from "exported and
forgotten", so the predicate had to be written down: see
`scripts/check-self-tests-wired.py` and design-decisions.md §273
(*a self-test the build cannot reach is a build failure*).

**A caveat that is not fixed, and is not this bug.** A further **258**
self-tests are reachable *only* from `kshell.rs`, as interactive `test`
subcommands of the kernel shell. They are runnable but the boot test never runs
them, so a regression in one is caught only if a human happens to type the
command. The checker reports that count on every run rather than failing on it
— see §273 (*unreachable self-tests*) for why. Shrinking it is open work, not a
defect with a fix.
