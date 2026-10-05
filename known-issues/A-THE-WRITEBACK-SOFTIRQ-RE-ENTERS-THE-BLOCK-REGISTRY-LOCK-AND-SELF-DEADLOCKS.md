## A-THE-WRITEBACK-SOFTIRQ-RE-ENTERS-THE-BLOCK-REGISTRY-LOCK-AND-SELF-DEADLOCKS (lane A, 2026-09-15) — **Status: FIXED**, pending a boot

**Observed, not theorised.** A FAT/virtio boot panicked:

```
[sync] *** SELF-DEADLOCK *** lock '?' @ 0xffffffff82af3728 is already held by
       task 0 on cpu 0 -- the same task that is now taking it
panicked at kernel\src\sync.rs:981:5
```

The backtrace names the whole mechanism:

```
#19 blkdev::with_device            <- takes REGISTRY
#18 fs::cache::read_sector_uncached
#16 VirtioBlkDevice::read_sector   <- spins for the device, interrupts ENABLED
#14 irq_common_dispatch            <- a timer tick lands mid-read
#10 softirq::process_pending
#8  fs::cache::try_flush_expired
#7  BufferCacheInner::writeback_entry
#6  blkdev::with_device            <- takes REGISTRY AGAIN
#5  PreemptSpinMutex::lock -> spin -> stall -> panic
```

Process-context code holds the block registry across a real device read with
interrupts enabled. A timer softirq lands, the buffer cache flushes an expired
entry, and the writeback path takes the same lock on the same CPU.

**The near miss is the interesting part: the file already knew.**
`try_flush_expired` opens with `CACHE.try_lock()?` — deliberately non-blocking,
precisely because it runs in interrupt context. The defensive treatment was
applied to **one** of the two locks that path ends up taking. The second,
`blkdev`'s `REGISTRY`, is reached *indirectly* through `writeback_entry`, three
frames away and in another file, and nothing about the call site says
"interrupt context". The author of the `try_lock` was right about the hazard and
the analysis simply did not follow the call chain out of the function.

**Fix.** `blkdev::try_with_device`, used by a `writeback_entry_try` that
`try_flush_expired` calls instead of the blocking form. Backing off is free
here: the entry stays dirty and the next tick retries it, and
`try_flush_expired` already counts a failed writeback as "not flushed".

`try_with_device` keeps three outcomes distinct — `Err(WouldBlock)`, `Ok(None)`
(no such device) and `Ok(Some(r))` — rather than folding busy into `None`.
Collapsing them would make a transient lock conflict indistinguishable from a
missing disk, and in interrupt context those call for opposite responses.

**Why it survived, and it is the same reason as the two before it.**
`scripts/boot-test.sh` attaches no disk **[WRONG -- see the CORRECTION below:
it attaches `rootfs.ext4` on every boot, so the deadlock IS reachable there]**,
so the root is `memfs`, which has no
block device, no buffer cache writeback and no registry to re-enter. The
deadlock cannot occur under the only harness that gates the tree **[also wrong;
see below]**. It is the
**third** defect this configuration has surfaced today, and each became
reachable only when the previous one was fixed:

| # | defect | unblocked by |
|---|---|---|
| 1 | `openat2` create-then-report-failure on a permissionless fs | running a FAT root at all |
| 2 | case (e) asserts a mode round-trip FAT cannot do, and misdiagnoses it as §639 | fixing 1 |
| 3 | this self-deadlock | fixing 2 |

That ordering is the argument for the second boot configuration, stated better
than the original entry managed: a fixture does not reveal its defects in
parallel. Each one hides the next, so the value of running it is not one bug —
it is a queue of unknown length that nothing else will ever drain.

**CORRECTION, and it makes this WORSE rather than better. Second time today I
asserted a negative from too narrow a grep.**

The claim above that the deadlock "cannot occur under the only harness that
gates the tree" is false. I ran `grep -c 'disk\.img' scripts/boot-test.sh`, got 0,
and concluded it attaches no disk. It attaches `rootfs.ext4`, by default, on
every run -- line 1442 of that script says so in as many words (*"every boot
attaches rootfs.ext4"*), and a `--no-rootfs` flag exists whose very presence
should have been the tell. One image name is not the set of images.

So the gated harness **does** register a block device and **does** drive the
buffer cache, the page cache and `blkdev`'s registry. The deadlock is not
unreachable there -- it is a **race that had not yet fired**. A timer landing
inside a device read with an expired dirty entry waiting: all three ingredients
are present on every boot the tree gates.

What the FAT root changed is **rate, not reach**. With FAT serving `/`, every
file read goes to disk through the buffer cache instead of to `memfs` in RAM, so
the window is entered orders of magnitude more often and the race becomes
near-deterministic. That reclassifies all three:

| defect | under the gated harness |
|---|---|
| 1, `openat2` create-then-fail | genuinely unreachable: needs a filesystem with no permission model, and none is mounted |
| 2, mode round-trip asserted unconditionally | genuinely unreachable, same reason |
| 3, this self-deadlock | **reachable on every boot**, and had simply never fired |

A rare race that a heavier fixture makes deterministic is more useful than an
unreachable arm, and more dangerous: it can fire on the operator's machine,
under load, having passed every gate. The queue framing above still holds for 1
and 2; for 3 the lesson is sharper. The value of a second configuration is not
only reaching new code, it is **changing the timing of code already reached**.

**Which corrects the proposal too.** This entry asked for a FAT-root boot. That
is the wrong shape and the run proved it: with FAT at `/`, four ring-3 POSIX
tests fail inherently and permanently -- `symlink()`, `chmod`/`chown`,
`utimensat`, and a directory-handle setup -- because FAT stores no symlinks, no
ownership, no permission bits and no nanosecond timestamps. No real system roots
on vfat, a POSIX suite can never pass there, and such a gate would ship with
four permanent failures and teach everyone to ignore it.

The correct ask is smaller, and is what this entry now requests: **mount a FAT
volume somewhere other than `/`** and point the mode-sensitive cases at that
path. That reaches defects 1 and 2 with no POSIX breakage. The coverage that
exposed defect 3 needs no new fixture at all -- it is already attached on every
boot. What it needs is I/O pressure, which is a load question, not a fixture
one.
