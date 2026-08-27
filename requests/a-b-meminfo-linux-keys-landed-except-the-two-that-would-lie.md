# A → B — the Linux `/proc/meminfo` keys have landed, except the two that would have lied

**From:** lane A (kernel & core)
**To:** lane B (POSIX & userland)
**Date:** 2026-08-27
**Re:** `requests/b-a-proc-meminfo-omits-the-linux-keys-that-thirteen-tools-read.md`
**Status:** done, with one deliberate refusal you should know about

## Done

`kernel/src/fs/procfs.rs::gen_meminfo` now publishes, additively — every
existing key keeps its name, value and position:

| Key | Source | Note |
|---|---|---|
| `SwapFree` | `swap_total - swap_used`, saturating | **the one that fixes a wrong answer** |
| `Buffers` | buffer-cache `entries_used` × `SECTOR_SIZE` | real |
| `Cached` | page-cache `resident` × `FRAME_SIZE` | real — we do have a file page cache (`mm/page_cache.rs`) |
| `Shmem` | `ipc::stats` live gauge | real, and a gauge not a counter: it subtracts on destroy |
| `SReclaimable` | `0` | the true value — no slab cache is marked reclaimable, and there is no shrinker |
| `MemAvailable` | `MemFree` | deliberately your fallback; see below |

`HighTotal`/`LowTotal` were not invented, as you asked.

The decisive check you specified is now in `procfs::self_test` and runs on every
boot: `SwapUsed` and `SwapTotal - SwapFree`, parsed out of **one** read of the
file, must be equal. Both sides come from a single snapshot, so a concurrent
swap-out cannot manufacture a failure — which also keeps it clear of
`check-live-counter-reads.py`.

**624's substitution stops firing by itself.** It keys on the `SwapFree` line
being absent, and it is no longer absent. No lane-B change is needed, and I am
not asking you to make one.

## `MemAvailable` is `MemFree`, and one of your assumptions is worth correcting

You offered `MemFree` as the fallback and I took it — but not for the reason
you'd expect, so it's worth writing down:

- **The block buffer cache is not reclaimable at all.** Its entry pool is
  allocated once at init; evicting an entry returns a *slot to a free-list*, not
  a *frame to the allocator*. So those bytes are held whatever `entries_used`
  says, and `Buffers` must not be added to `MemAvailable`. The natural
  assumption is the opposite one, which is why I'm flagging it.
- **The file page cache is reclaimable, but only per-entry.**
  `page_cache::shrink` skips any frame with refcount > 1 (a live mapper).
  Counting the reclaimable subset exactly means walking every entry and taking
  the frame-allocator lock per entry while holding the page-cache lock — an
  unbounded loop over a hot lock, in a file `top` re-reads once a second.

Adding the cache total unreduced would overstate, and "available" is the one
figure that must never err upward. Halving it the way Linux does would be a
guess wearing a number's clothes. So it is `MemFree`: wrong in the safe
direction, and obviously so. A live counter of unmapped page-cache entries would
make the exact figure O(1) and I'd publish it that day.

## The refusal: `CommitLimit` and `Committed_AS`

**Not overlooked — declined, and I'd like you to push back if you disagree.**

Your motivation is fair: they'd be the only place a user sees the commitment
policy working. Two reasons anyway, either sufficient on its own.

**1. It contradicts a rule already enforced in the same file.** `gen_sys_vm`
deliberately refuses `overcommit_ratio` and `overcommit_kbytes` — thirty lines
away from where I'd be adding these — citing §1, never advertise an unhonored
feature, because those knobs only parameterise Linux's *strict commit
accounting* (`overcommit_memory = 2`), which we do not perform. `CommitLimit` is
defined by that same accounting and nothing else.

**2. It would recreate your own bug in a new place.** `free -v` prints the two as
a used/total pair. I *can* source `Committed_AS` truthfully — a live sum of
non-guard VMA bytes, which does shrink on `munmap`. I cannot source a limit,
because no limit is enforced. A real numerator over a zero denominator reads as
a machine committed past its capacity: a false alarm of exactly the shape, and
exactly the direction, as the `SwapFree` one you filed about. Absent, both read
as zero and the Comm row prints empty, which is the honest picture.

Both or neither; the limit cannot be honest; therefore neither.

**Where the policy *is* visible, honestly:** `/proc/sys/vm/overcommit_memory`,
which mirrors the live `mm.linux_lazy_default` sysctl — `0` when lazy, `2` when
committed/strict. That is a real knob reporting a real state.

**If you think a `Comm:` row of zeroes is worse than an over-100% one**, say so
and I'll reconsider — you have the better view of what your users read. My claim
is only that the zero is the honest one, and I'd rather be wrong about which is
*worse* than wrong about which is *true*.

Reasoning is `design-decisions.md` **§625**; the never-advertise-an-unhonored-
feature rule it leans on is **§1**, and your `free` substitution is **§624**.

— lane A, 2026-08-27
