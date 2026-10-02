## TD-B-WHO-S-W-MODE-CANNOT-BE-REACHED-BY-ANY-INVOCATION (lane B, 2026-09-10)

**In short:** `who` contains a complete `w` — the header line with uptime and
load averages, the USER/TTY/FROM/LOGIN@/IDLE/JCPU/PCPU/WHAT columns, and the
per-process CPU accounting behind them. None of it can run. The mode is turned
on in exactly one place, by checking whether the program was invoked under the
name `w`, and nothing installs it under that name.

**How it got here, which is not a mistake anybody made.** `w` was provided by
`userspace/w` until today, so the name had a producer and the alias ledger was
satisfied. Removing that crate's `w` personality (it printed a fixed `-` for
WHAT and a fabricated idle time for everyone, where `who` measures both) left
`who` as the only implementation — and revealed that the only way to reach it
was a name nothing produces.

**Why it is not being fixed by staging.** `scripts/create-ext4-rootfs.sh`
stages seven C test programs and NO userspace Rust crate at all. All 51
entries in `multicall-aliases-baseline.txt` are unreachable for the same
reason. Staging `who` alone would be deciding what ships, which is a separate
decision affecting 474 other crates equally.

**Why not a flag instead.** `-w` is GNU `who`'s documented synonym for `-T`
(message status), so taking it would break a compatibility this crate is
otherwise careful about. Inventing `--activity` or similar is a user-visible
naming choice rather than a repair.

**The proper fix** is whichever of these the rootfs question settles: stage
`who` under both names once userspace crates are staged at all, or give `w` a
long option of its own if the operator would rather not have argv[0]-dependent
behaviour. Until then the code is correct, tested (the print path has unit
tests) and unreachable, which is worth knowing before someone deletes it as
dead.
