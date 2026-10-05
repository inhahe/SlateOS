### BUG-BOOT-SPINLOCK-STALL-UNNAMED -- an intermittent boot hang reports `lock '?'` and `cpu 0 holds 0 lock(s)`, because lockdep is not yet enabled when the self-test battery runs -- 2026-08-17 -- OPEN (flaky; the diagnostic gap is the actionable part)

**Observed.** Boot #10 of the lane-A tree (2026-08-17, tree at `8fd6813e1`)
never reached `BOOT_OK`; `run-timeout` killed it at 1444 s, past the kernel's
own 827 s deadline. Serial froze after
`[spawn] Running link()/linkat no-follow symlink test (kernel, ext4 /mnt)...`
with:

```
[sync] *** SPINLOCK STALL *** lock '?' still not acquired after ~30s of spinning
       (cpu 0, task 0, 46170112 iters).
[sync]   lock '?' holder: task 0 == spinner -- RECURSIVE self-deadlock
[lockdep]   cpu 0 holds 0 lock(s):
[liveness] cpu0: ... preempt_disable_depth=4 last_rip=0xffffffff804ca584
```

then three `SYSTEM HANG` reports and silence. **Not a regression, and not that
test:** the immediately following boot (#11, identical kernel source plus the
`mm::pat` work) passed `link()/linkat no-follow: OK` and went on to reach
`BOOT_OK` in 493 s. Boot history at the time: 42 boots, 37 clean.

**The actionable finding is the diagnostic, not the deadlock.** Two lines of
that dump contradict each other, and working out which one lies is the whole
content of this entry:

* `sync::report_spin_stall` compares the lock's recorded `owner` against the
  spinning task id. Both were 0, so it printed "RECURSIVE self-deadlock".
* `lockdep::dump_held_locks(0)` printed **zero** held locks -- which, if lockdep
  were running, would mean cpu 0 does *not* hold the lock, making the line above
  false.

lockdep was **not** running. `lockdep::lock_acquire` returns immediately unless
`ENABLED` is set, and `ENABLED` is set only by `lockdep::init()`, which is called
at `kernel/src/main.rs:5374` -- whereas the ring-3 self-test battery begins at
`main.rs:1510` ("Step 21: Enable hardware interrupts -- BEFORE the ring-3
self-test battery") and runs to roughly 4790. Confirmation from the serial log:
`lockdep::init()`'s banner (`Lock order validator enabled`) appears **zero**
times in all 1.29 MB of boot #10's output, and the only `[lockdep]` line in the
entire log is the misleading "holds 0 lock(s)" from the stall report itself.

So the most deadlock-prone phase of boot -- several thousand lines of self-tests
exercising ext4, spawn, signals and the VFS -- runs with the lock-order
validator switched off, and the phase after it, which is mostly idle, runs with
it on. Worse, `dump_held_locks` prints a confident "0 lock(s)" when disabled
rather than "lockdep disabled", so the dump actively misleads a reader into
ruling out the correct explanation.

**What was lost by that.** `lockdep::lock_acquire` already contains a precise
detector for exactly this failure: it walks the per-CPU held stack and, on
finding the same lock class already held, reports a re-entrant acquisition of the
*same instance* immediately -- with the class name, and *before* the 30 s stall
detector would fire (its own comment says as much). Had it been enabled, boot
#10 would have produced a named lock and a located report instead of `lock '?'`
and a 29-frame unsymbolized backtrace.

**Proper fix, in two parts.**

1. **Move `lockdep::init()` ahead of the self-test battery** (before
   `main.rs:1510`). Its stated prerequisite -- "Must be after SMP init so
   `current_cpu_index()` works on all CPUs" -- does not hold:
   `smp::current_cpu_index` returns `BSP_CPU_INDEX` whenever `SMP_INITIALIZED`
   is clear, which is the correct answer while only the BSP is running. Expect
   this to surface real lock-order findings from paths that have never been
   validated; those are the point, not an objection.
2. **Make `dump_held_locks` say when it is disabled**, so a dump can never again
   claim "0 lock(s)" on the strength of not having looked.

Also worth doing, and cheaper: the stalled lock printed `'?'` because
`sync::Mutex::new` defaults its name to `b"?"`. Naming the mutexes reachable from
the self-test battery (`Mutex::named`) would make the stall detector's output
usable on its own, without lockdep.

**Reproducing.** Loop `python scripts/run-timeout.py --poll 60 1800
./scripts/boot-test.sh`; roughly 1 boot in 8 was non-clean at the time of
writing. Before treating a single failure at any one self-test as a regression,
re-run -- and check whether the freeze point moves, which is what distinguishes
this from a deterministic deadlock in the test the log happens to stop at.
