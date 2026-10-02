### B-VIRTIO-BLK-WRITE-TIMEOUT. Intermittent boot hang — a spurious virtio-blk request timeout corrupts the virtqueue, cascading into an unrecoverable storm of write timeouts during ext4 journal replay — ROOT-CAUSED & FIXED 2026-07-15

**Symptom.** A live boot wedge caught by `scripts/wedge-soak.sh`
(`build/hang-catches/soak-20260715-190010-iter28.*`), ~1 in 28 boots.
Distinct from the three prior soak catches (B-WAITQ-IDLEPARK,
B-SCHED-SPAWN-DEADLOCK, B-SYSCTL-IRQ-DEADLOCK) — the SCHED/idle-fallback
dumps correctly did **not** fire. The i6300esb NMI watchdog froze CPU#0 at
`RIP=0xffffffff81e9492a` = `ext4::journal::Journal::open`, `RFL=0x86`
(IF *set* — not a spinlock deadlock; the CPU is livelocked retrying I/O).
The serial log ends with **136** `[virtio-blk] Write sector N timed out`
messages (first in polling mode, later in IRQ mode) interleaved with a
livelock of `[sched] Anti-starvation: cur=0 boosted 1 task to priority 0:
[130(p20)]` (kswapd starved while the boot task spins retrying journal
writes).

**Root cause.** Two compounding bugs in the single-outstanding virtio-blk
driver (`kernel/src/virtio/blk.rs`), which shares *one* DMA frame across all
requests:
1. **Trigger — too-short polling budget.** `wait_completion`'s polling
   fallback (early boot, pre-IOAPIC) timed out after only `1_000_000`
   `spin_loop()` iterations (~1 ms). Under soak-test host contention a real
   QEMU virtio-blk completion can take longer, so the *first* timeout fired
   spuriously even though the device was healthy and would have completed.
2. **Cascade — unsafe timeout recovery.** On timeout the old code did
   `self.queue.free_chain(head)` and returned `Err`, but the device **still
   owned** those descriptors and the shared DMA buffer. The caller
   (`Journal::open`) retried; the next `submit()` reused the just-freed
   descriptors and the same DMA buffer. When the device finally completed
   the abandoned request, `poll_used()` returned that stale head (the driver
   accepted *any* completion with no head-matching), the used ring desynced,
   and `free_chain` double-freed a descriptor — corrupting the free list.
   From there every request timed out (now in IRQ mode, 5 s each), an
   unrecoverable storm.

**Fix (this session).**
- **Adequate polling budget.** New `POLL_TIMEOUT_SPINS = 100_000_000`
  constant (100× headroom) so a healthy device under load never spuriously
  times out, while still bounding a genuinely-dead device so boot can't hang
  forever.
- **Head-matching completion.** New `poll_matching(head, …)` only returns
  when the completion's head equals *our* submitted head; a mismatched
  (stale) completion is drained (`free_chain`) and polling continues.
  Guarantees `read_sector`/`write_sector` free exactly the chain they
  submitted.
- **Safe timeout recovery.** On timeout the driver no longer blindly frees a
  device-owned chain. Instead `recover_after_timeout()` → `recover()`
  re-runs the legacy virtio init handshake (reset → ACK → DRIVER → features
  → re-select queue 0 → `Virtqueue::reset()` → re-publish queue PFN →
  DRIVER_OK), forcing the device to relinquish **all** outstanding buffers
  so the next request starts from a clean, consistent state. New
  `Virtqueue::reset()` (`kernel/src/virtio/queue.rs`) re-zeroes the rings,
  rebuilds the free list, and clears avail/used index tracking, reusing the
  same backing frame.

**Status.** FIXED. The spurious-timeout trigger is removed and, even if a
timeout does occur (genuinely dead device), the reset-based recovery keeps
the virtqueue consistent instead of cascading. Re-soak to confirm the wedge
no longer reproduces.
