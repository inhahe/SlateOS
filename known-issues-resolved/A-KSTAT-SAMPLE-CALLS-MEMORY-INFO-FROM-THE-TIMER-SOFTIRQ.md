## `A-KSTAT-SAMPLE-CALLS-MEMORY-INFO-FROM-THE-TIMER-SOFTIRQ` (lane A's code, filed by lane B, 2026-08-29) — ✅ FIXED 2026-08-29 (`07e5d617b`), and the reported lock list was wrong in both directions

**In short:** the periodic metrics sampler runs inside the timer interrupt and
calls a heavyweight memory query that takes ordinary blocking locks. When the
timer lands on a CPU that already holds one of those locks, the interrupt
handler spins for a lock only the code it just suspended could release. The
kernel detects the recursive acquire and panics instead of hanging. Observed on
`mm::swap::SWAP`, but `SWAP` is only the lock that lost the race -- the same
path takes at least three others.

**Where:** `kernel/src/kstat.rs:140` (`sample()` -> `mm::memory_pressure()`),
`kernel/src/mm/mod.rs:428` (`memory_pressure()` -> `memory_info()`),
`kernel/src/mm/mod.rs:286` (`memory_info()` -> `swap::summary()`),
`kernel/src/mm/swap.rs:1149` (`SWAP.lock()`). Panic raised at
`kernel/src/sync.rs:969`.

**The path:**

```
sync::Mutex::lock        <- blocks forever
mm::swap::summary        <- SWAP.lock()        swap.rs:1149
mm::memory_info          <- swap::summary()    mm/mod.rs:286
mm::memory_pressure      <- memory_info()      mm/mod.rs:428
kstat::sample            <- memory_pressure()  kstat.rs:140
softirq::handle_timer / process_pending / handle_timer_irq / idt::dispatch_vector
```

The interrupted context was inside `lockdep::lock_release`, i.e. mid-release of
`SWAP` when the timer fired. That few-instruction window is why this is
intermittent rather than every boot.

**It is a real deadlock, not a lockdep false positive.** `fail_if_recursive` is
reached only from `lock_contended` -- *after* `try_acquire` has already failed --
so the lock word was genuinely still held, not merely still recorded as held.
The owner check compares against `sched::current_task_id()`, which in softirq
context is the interrupted task (task 0, idle). Hardware state and ownership
bookkeeping agree.

**The tell.** `kstat::sample()` already knows it must not block, and applies
that discipline inconsistently fifteen lines apart:

| Line | Call | Locking |
|---|---|---|
| `kstat.rs:125` | `mm::frame::try_stats()` | try-lock; comment reads `--- Memory stats (lock-free query) ---` |
| `kstat.rs:140` | `mm::memory_pressure()` | blocking, and reaches four different locks |

Blocking locks reachable from `memory_info()` — **as reported by lane B, and
wrong in both directions; see the corrected list under "Fixed" below**:
`frame::stats()` (`mm/mod.rs:274` *and* `:296` -- the blocking `stats()`, though
`try_stats()` exists at `frame.rs:2692`), `swap::summary()` (`:286`),
`heap::stats()` (`:283`), and
`kswapd::is_running()`/`reclaim_cycles()`/`total_reclaimed()` (`:290-292`).
Fixing only `SWAP` moves the panic rather than removing it.

**Proper fix:** give `memory_info` a non-blocking twin -- `swap::try_summary()`,
`try_memory_info()`, `try_memory_pressure()`, each returning `None` when any
lock is busy -- and have `kstat::sample()` skip the sample rather than block. A
dropped metrics sample costs nothing; the next tick gets it. This is exactly the
shape `frame::try_stats()` already has. Alternatives (defer sampling to a kernel
thread; make the locks IRQ-safe) are set out in the request file.

**Reproduce:** not deterministic. Seen on `./scripts/boot-test.sh` against
`lane-b` @ `8340fe48b`, debug profile, QEMU TCG; panic at serial log line 43907,
during the `sysdiag` self-tests. `boot-history` reports 508 boots, 387 clean --
this class of intermittent failure is not new, though how many of the 121 are
this specific bug is unknown.

**Frequency: 1 in 2 so far.** The next boot test, same debug/TCG configuration
on `ca0a25d96` (same tree plus lane A's two kshell commits), passed clean in
897 s. The window is narrow enough to miss on a retry and wide enough to hit on
a first try -- the worst shape for a race. The passing re-run is not evidence
against the bug; it was run only to establish intermittency.

**Not a lane-B regression:** the lane-B diff for that merge touches no
`kernel/`, `drivers/`, `fs/` or `net/` file (`design-decisions.md`,
`requests/`, `scripts/argv-utf8-baseline.txt`, `scripts/df-diff.sh`,
`userspace/coreutils/{df,du,strings}.rs`, `userspace/coreutils/src/human.rs`).
Filed to lane A as
`requests/b-a-kstat-sample-calls-memory-info-from-the-timer-softirq-and-self-deadlocks-on-swap.md`.

### ✅ Fixed 2026-08-29 — `07e5d617b`

The diagnosis was right and the proposed fix was the right one; the *audit*
underneath it was not, in both directions, so taking the list on trust would
have left the panic reachable.

**Two of the four reported blocking leaves take no lock at all.** `heap::stats()`
(`mm/heap.rs:1473`) reads `PCPU_SLAB_CACHES` plainly and loads its counters
`Ordering::Relaxed`; `kswapd::is_running()`/`reclaim_cycles()`/
`total_reclaimed()` are relaxed atomic loads. Neither can block anything.

**Two that do block were missed.** `frame::zero_pool_count()` takes
`ZERO_POOL.lock()`, and `accounting::tracked_count()` takes
`ACCOUNTING.lock_irqsave()`. Lane B's own sentence applies to lane B's report:
*fixing only `SWAP` would move the panic, not remove it* — and a fix built from
their list would have done exactly that, on `ZERO_POOL`.

The corrected list of what `memory_info()` reaches:

| Call | Lock | Softirq-safe? |
|---|---|---|
| `frame::stats()` | `ALLOCATOR` | no → `try_stats()` (already existed) |
| `frame::zero_pool_count()` | `ZERO_POOL` | no → **new** `try_zero_pool_count()` |
| `swap::summary()` | `SWAP` | no → **new** `try_summary()` |
| `accounting::tracked_count()` | `ACCOUNTING` (`lock_irqsave`) | cannot self-deadlock; **new** `try_tracked_count()` anyway, see below |
| `heap::stats()` | none (relaxed atomics) | yes |
| `kswapd::*` | none (relaxed atomics) | yes |

**`ACCOUNTING` is a latency fix, not a correctness one, and the entry should
not blur the two.** `lock_irqsave` masks interrupts for the duration, so a
same-CPU timer cannot land inside the critical section and there is no
self-deadlock to have. It got a try-variant because the body is an O(n) scan of
the whole 256-entry address-space table, and a softirq that *waits* for another
CPU to finish one is a latency spike on every tick.

**`try_lock` on each leaf was not sufficient on its own.** It stops *us*
blocking; it does not stop a nested interrupt landing while we hold one of these
locks and re-entering it — deadlocking one level further in. So the whole
try-chain in `mm::try_memory_info()` runs inside `crate::cpu::without_interrupts`,
which is the construct `frame::stats()` already uses for the same reason. That
was preferred over adding a `Mutex::try_lock_irqsave` primitive: one
clearly-correct construct at one site beats a new primitive used at four.

**Shipped:** `swap::try_summary()`, `frame::try_zero_pool_count()`,
`accounting::try_tracked_count()`, `mm::try_memory_info()`,
`mm::try_memory_pressure()`, and `mm::pressure_from_info()` (public, so a caller
holding a snapshot scores *that* rather than taking every lock a second time —
which is what `kstat::sample()` now does, halving its lock acquisitions rather
than merely making them non-blocking). `sample()` abandons the whole sample and
returns when any lock is busy: a record of zeros is indistinguishable from an
idle system, and a *missing* record is not.

**A silently-skipping sampler looks identical to one with nothing to report,**
so the skips are counted (`kstat::skipped_samples()`), surfaced by `kshell`'s
`kstat` command, and asserted by a new invariant (`invariant.rs`,
`check_metrics_sampling`). The invariant fails **only** on `total == 0 &&
skipped > 0` — every attempt refused, i.e. a try-chain that can no longer
succeed. `total == 0 && skipped == 0` is "the sampler has not ticked yet", which
is not a fault and depends only on where in boot the battery runs.

**Fixed in passing:** `memory_info()` called `frame::stats()` twice, paying for
`ALLOCATOR` twice and letting the order histogram come from a different instant
than the free count printed beside it. Both now come from one snapshot, via a
shared `assemble_memory_info()` body that `memory_info()` and `try_memory_info()`
both call — so the blocking and non-blocking twins cannot drift into reporting
different numbers from the same state.

**Regression test:** `swap::self_test()` holds `SWAP` and asserts the whole
chain refuses — `try_summary()`, `try_memory_info()` *and* `try_memory_pressure()`.
Asserting only about the leaf would pass a fix that made `try_summary()`
non-blocking while some caller in the middle still took a blocking lock, which
is the exact shape of the bug. The positive direction (releases and succeeds) is
a bounded 1000-iteration retry, not a single assert: the negative direction is
deterministic because we hold the lock ourselves, but the positive one races
every other CPU and `ALLOCATOR` is hot — a single-shot assert there would be an
intermittent boot failure, i.e. this bug's own failure mode reintroduced by its
own test. A bounded retry still fails a chain that is *permanently* `None`.

**Verified:** boot test PASSED in 1132 s; `[swap]   Softirq-safe try_* chain: OK
(refuses while SWAP held)` at serial line 490. The only `SELF-DEADLOCK` strings
in the log are lockdep's own intentional self-test. All static gates green.
