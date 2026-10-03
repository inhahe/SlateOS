## 70. Raw `spin::Mutex` holder-preemption (Q24) — **proactive kernel-wide audit/conversion (option B)**, not reactive-only

**Date:** 2026-07-18

**Decided by:** Operator (Claude recommended **A**, reactive, with **C** as an
escalation; the operator **overruled** and chose **B** — "Let's not have
technical debt and do it the right way").

**Context.** The kernel had (at decision time) four confirmed single-CPU
deadlocks on raw `spin::Mutex` locks across two sub-variants — *holder-preemption*
(heap, `container::TABLE`) and *interrupt-reentrancy* (`sysctl::REGISTRY`,
completion-timer→`SCHED`). A raw `spin::Mutex` neither disables preemption on
acquire (so a holder can be preempted mid-section and a second task spins forever
on one CPU) nor is IRQ-safe by construction. The preempt-aware
`crate::sync::Mutex` prevents the holder-preemption class, but ~476 kernel files
import raw `spin::` locks. Claude had been fixing each caught instance reactively.

**Decision.** Do the **proactive audit/conversion (option B)** rather than
continuing reactive-only. Eliminate the whole deadlock class deliberately instead
of waiting for the soak to surface each latent instance. This is explicitly a
"no technical debt, do it right" call by the operator.

**Rationale (operator).** Two deadlock sub-variants and four instances already
found means the latent-instance tail is real; leaving it to chance (reactive-A)
is accepting known technical debt. A deliberate audit removes the class and can
add lockdep/owner-tracking where it pays.

**Execution guidance (to keep B safe — it "can't be a blind sed").**
- **Not a mechanical `use spin::Mutex` → `crate::sync::Mutex` sweep.** Some locks
  are deliberately raw and must stay raw + manual preempt discipline (e.g. the
  global heap lock — lockdep can't allocate under it). Triage each lock.
- Prefer a **preempt-aware, non-lockdep spinlock** (the `PreemptSpinMutex` idea
  from option C: `preempt_disable/enable` around the raw spin, no registry) for
  hot **leaf** locks where lockdep would be pure overhead; reserve
  `crate::sync::Mutex` (full lockdep + owner tracking) for **contended, non-leaf**
  locks where ordering bugs are plausible and the registration cost is
  affordable.
- Keep IRQ-context acquirers on `try_lock`/`without_interrupts` (the
  interrupt-reentrancy surface — timer hard-IRQ, softirq→`SCHED`, `#PF` — was
  already audited clean; don't regress it).
- Do it **incrementally and validated** — convert in reviewable batches, keep
  `scripts/wedge-soak.sh` green between batches, and expect a flood of
  newly-surfaced lock-ordering reports from lockdep to triage as locks are
  registered.

**Alternatives considered.** **A (reactive)** — rejected by the operator as
leaving known latent debt. **C (middle path, convert only contended non-leaf
locks)** — folded into B as the *execution technique* (add `PreemptSpinMutex`,
choose per-lock) rather than the whole scope.

**Where it lives.** `kernel/src/sync.rs` (`Mutex`; add `PreemptSpinMutex`); every
`use spin::Mutex` site (~476 files); already-fixed anchors `kernel/src/mm/heap.rs`,
`kernel/src/container.rs`, `sysctl` (B-SYSCTL-IRQ-DEADLOCK),
completion-timer→SCHED (B-COMPLETION-TIMER-IRQ-DEADLOCK). Detector:
`scripts/wedge-soak.sh`. Track the audit as a roadmap task.

**How to reverse.** Stop the sweep and fall back to reactive-A; already-converted
locks stay converted (no harm). Reversing is cheap since each conversion is
independently sound.

**Execution status / triage outcome (2026-07-18).** The sweep converted, in
reviewable per-subsystem batches (each boot-tested green before commit):
- **`PreemptSpinMutex`** (preempt-disabling, no lockdep) for hot/cold *leaf*
  locks held briefly in process/thread context: most of `fs/` (procfs stat/config
  stores), `ipc/` leaves (channel, completion, epoll, eventfd, inotify, memfd,
  pipe, semaphore, service_limits, shm, signalfd, stream_socket, timerfd,
  alsa_pcm), `mm/` service locks (mempool, page_cache, rmap, vmalloc),
  `proc/{exception,thread_clone}`, `cap/file_tags`, and driver/service leaves
  (blkdev, cnetwork, drvmon, initproc, ksyms, logpersist, netns, pidns, reslimit,
  scfilter, sockact, svcstart, syshealth, termsession, userns, volume,
  drm/{card_fd,dumb_mmap,mod,hotplug}, power, devhotplug, devpower, udriver,
  vmguest, acpi/mod, bench, eventlog, kshell, syscall/linux).
- **`crate::sync::Mutex`** (full lockdep + owner tracking + preempt-disable) for
  contended non-leaf/nested locks: core-FS contended locks, `ipc/{futex,io_ring,
  namespace,service}`, all of `net/` (28 files, uniform), `cap/groups`
  (GROUPS→NEXT_ID nesting), `kevent`.
- **Deliberately kept RAW** (holder-preemption does not apply — the lock is only
  taken with interrupts already off, or in panic/scheduler-core context where a
  preempt-aware wrapper is wrong or circular): `kernel/src/sync.rs` itself (the
  backing store — never convert); the scheduler core (`sched/{mod,priority_rr,
  waitqueue,kchannel}` — circular with `preempt_disable`); IRQ/panic-context
  primitives `console`, `klog`, `tty` (keyboard IRQ input), `rng`
  (`add_interrupt_entropy` runs in ISR), `sysctl` (reached from an ISR),
  `serial` (`lock_irqsave`), `hrtimer`/`workqueue` (acquired under
  `without_interrupts`), `proc/{itimer,signal}` (all sites under
  `without_interrupts`), and the hardware device drivers whose ISRs take their
  locks (`e1000`, `hda`, `xhci`, `virtio/{blk,net}`, `iommu_remap`). These
  acquire on `try_lock`/`without_interrupts` or run with IRQs disabled, so a
  timer preemption of the holder cannot occur.

One pre-existing **flaky self-test** surfaced (not a conversion bug): the
container port-forward Test-20 (`container.rs`) spawned an instantly-exiting
init and asserted the host-port NAT forwards were still live, but a container's
forwards are flushed by `notify_init_exit` the moment its init exits — a race the
new preemption timing made observable. Fixed by snapshotting the forwards inside
a `preempt_disable`/`enable` window straddling `run()` (the single-CPU boot test
then cannot schedule the init to flush in between).
