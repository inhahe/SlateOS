### `BUG-HRTIMER-EVICTS-AN-ARMED-TIMER` - fixed

`kernel/src/hrtimer.rs`, `schedule_absolute`.

On reaching `MAX_TIMERS_PER_CPU` (256) the code did `state.timers.pop()` -
discarding the furthest-out pending timer to make room. That timer is armed,
someone is blocked waiting for it, and its owner is never told. It is a silent
lost wakeup, and it happened **1541 times in one boot**, to subsystems that had
done nothing wrong. `channel_recv_timeout`, `futex_wait_timeout`,
`eventfd`/`pipe`/`stream_socket` timeouts and `timerfd` are all reachable this
way; the victim is whoever happened to have the longest deadline.

The warning made it worse rather than better: it was `serial_println!` from
inside `without_interrupts` with the per-CPU timer lock held, unconditionally,
once per overflowing schedule. Serial I/O with interrupts disabled delays the
APIC tick that drains the queue, so the flood deepened the queue it was
reporting on.

**Fix.** Never evict. `MAX_TIMERS_PER_CPU` becomes a soft threshold that warns
once and keeps accepting; a new `MAX_TIMERS_HARD_CEILING` (4096) refuses instead
of evicting, on the grounds that concentrating the harm on the caller that is
actually asking is the only version of this that is diagnosable. Both
diagnostics moved outside the IRQ-disabled critical section and are one-shot,
and `TOTAL_REFUSED` is now reported by the self-test alongside a
`scheduled - fired - cancelled - pending` tripwire that would have made the
original bug visible on the first boot that hit it.

**Correction, 2026-08-21 (lane A): that last claim was false — the tripwire
could never have fired.** `TOTAL_FIRED` counted every firing of a repeating
timer, but `TOTAL_SCHEDULED` counted only its *first* arming, so within a
second or two of boot `fired` permanently exceeded `scheduled` and the
`saturating_sub` chain floored the difference at 0 no matter how many wakeups
were being destroyed. The boot logs said so plainly and nobody read them:
`scheduled=388, fired=75833` — a 195x gap that is arithmetically impossible if
the two counters measure the same events. A guard that reports 0 on a healthy
boot and 0 on a broken one is not a guard; it is a green light wired to nothing,
and it sat in the tree as the designated detector for the very bug it was
written to catch. Fixed by counting each re-arm in `process_expired` into
`TOTAL_SCHEDULED`, which makes the two counters commensurable: every firing of
a repeating timer is now preceded by exactly one arming. The self-test carries
a comment forbidding the "simplification" that would undo it.

**The general lesson, which is not about timers.** A derived quantity that is
clamped at one end (`saturating_sub`, `max(0, …)`, an unsigned subtraction) can
be *structurally* incapable of reporting a fault, and it will look completely
healthy while being so. When adding a tripwire, check that its two inputs count
the same events — and prefer to prove it by making the tripwire fire once on
purpose rather than by reading the code.

**The same asymmetry was latent in `ktimer`, and was fixed in the same pass
(2026-08-21, lane A).** `ktimer::process_expirations` re-armed a periodic timer
without touching `TIMERS_SCHEDULED` while `TIMERS_FIRED` counted every firing,
so its self-test printed `scheduled=5, fired=8` — three wakeups apparently
conjured from nowhere. Nothing was *false* there yet, because ktimer had no
tripwire over those counters; the danger was that adding one later would have
inherited a guard that could never fire, exactly as hrtimer's did. Re-arms are
now counted, and the self-test carries the conservation check
`scheduled >= fired + cancelled`, which the pre-fix numbers **would have
failed** — that failure is the only evidence worth having that the guard can
fire at all. Two details of that check are deliberate and are commented at the
site: it is phrased as a **comparison, not a clamped subtraction** (a
`saturating_sub` would have reproduced hrtimer's failure mode verbatim), and it
samples `fired`/`cancelled` *before* `scheduled` — a firing increments
`scheduled` (the re-arm) before `fired` (the workqueue submit), so reading the
consequence first and the cause last makes a concurrent expiry on another CPU
bias the check safe rather than manufacture a spurious failure.

### Not fixed, and deliberately so

- ~~**`IRQ 10` storming at ~500 kHz.** IRQ 10 is a shared level-triggered PCI
  line in this QEMU config (virtio-net, virtio-blk dev 0, ATI VGA, AC97, NVMe,
  xHCI, AHCI, SMBus). The dispatcher at `kernel/src/ioapic.rs:735-770` acks
  virtio-blk, virtio-net and rtl8139 on every IRQ; the other five devices have
  no handler, so if one of them asserts, nothing deasserts it and the line stays
  low. **A single storm-mask-cooldown cycle happens on passing boots too** - it
  is only fatal when the scheduler is too wedged to service the device during
  the cooldown. So the storm is a real and separate defect, but it was a
  passenger here, not the driver. Needs its own investigation: identify which
  of the five unhandled devices asserts, and either give it a stub handler that
  acks or mask it at the PCI command register.~~
  **FIXED 2026-08-22 (lane A).** See
  `A-IRQ10-STORM-IS-AN-UNHANDLED-DEVICE-ALLOWED-TO-ASSERT` below and
  `design-decisions.md` §281.
- ~~**`hrtimer`'s sorted `Vec`.** O(n) insert under a lock with interrupts
  disabled. At the new ceiling that is a 160 KiB memmove worst case. Logged in
  `todo.txt`; wants a real min-heap with lazy cancel-by-id, or a timer wheel.~~
  **FIXED 2026-08-21 (lane A, commit `520634ccc`).** Replaced by a binary
  min-heap over a slot slab: `schedule` and `process_expired` are O(log n),
  `cancel` is an O(1) slot lookup plus an O(log n) sift (handles carry
  `(slot, generation)`, so a stale handle cannot evict the timer that inherited
  a recycled slot). The heap key is `(expiry_ns, id)` rather than `expiry_ns`
  alone, because a binary heap is not stable and a bare-deadline key lets a
  timer be passed over indefinitely by arrivals sharing its deadline. Four new
  self-tests (heap order under a shuffled permutation, equal-deadline FIFO,
  interior cancel, stale-handle rejection) — the five pre-existing ones all
  passed against the `Vec` and *cannot* observe a mis-ordering, since the only
  multi-timer case sums its arguments and a sum is order-independent.
