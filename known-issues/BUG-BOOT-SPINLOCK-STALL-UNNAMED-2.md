### BUG-BOOT-SPINLOCK-STALL-UNNAMED -- RESOLUTION 2026-08-17 -- both diagnostic fixes landed; the third is superseded by a better one

**In short:** the two diagnostic defects described above are fixed and verified
by boot #12. The intermittent hang itself is **not** fixed -- it has not
reproduced since, so there is nothing to debug against -- but the next
occurrence will print a lock the reader can actually identify, and a held-lock
stack that is either true or says it is unavailable. The point of this work was
never to fix the hang; it was to make sure the *next* one is diagnosable.

**Part 1 -- move `lockdep::init()` ahead of the self-test battery: done.**
It now runs as "Step 20z" at `kernel/src/main.rs:1530-1531`, immediately before
`// Step 21: Enable hardware interrupts`, instead of at the old `main.rs:5374`.
The stated prerequisite was checked against the code rather than taken on trust
and does not hold: `smp::current_cpu_index` returns `BSP_CPU_INDEX` whenever
`SMP_INITIALIZED` is clear, which is the correct answer while only the BSP runs.

Verified: `Lock order validator enabled` appears at serial line **1931** of boot
#12, where in boot #11 it appeared at line **26693** of ~27150 -- it now precedes
essentially the entire self-test battery instead of trailing it.

The lock-order findings this entry predicted did **not** materialise. Boot #12
logged 28 `[lockdep]` lines in total, and every warning among them belongs to
`lockdep::self_test`'s own deliberate `test-A`/`test-B`/`test-C` inversions
(which the self-test labels as expected). Zero real violations across the
newly-covered phase. That is a result rather than a non-event: the validator now
watches the ext4 / spawn / signal / VFS self-tests and finds their lock ordering
correct, so a violation reported there in future is a genuine signal and not a
gap in coverage.

**Part 2 -- make `dump_held_locks` say when it is disabled: done.**
`kernel/src/lockdep.rs` now returns early with `validator DISABLED -- held-lock
stack is not maintained, so nothing can be concluded from it` when `ENABLED` is
clear, instead of printing a confident `0 lock(s)` on the strength of not having
looked.

**Part 3, as this entry proposed it, was the wrong fix, and has been replaced by
a better one.** The entry suggested naming the mutexes reachable from the
self-test battery with `Mutex::named`. The scope of that was measured first:
**627** `Mutex::new` call sites against **28** `Mutex::named` (worst offenders
`kshell.rs` 17, `net/httpd.rs` 8, `net/firewall.rs` 7, `sched/kmutex.rs` 6,
`ipc/namespace.rs` 6, `sync.rs` 5). Naming them is 627 edits, 627 chances to
miss one, does nothing for a lock allocated on the heap or created after the
edit, and -- decisively -- only helps the locks somebody already suspected.

What landed instead: **both `sync::report_spin_stall` and
`lockdep::dump_held_locks` now print the lock's address.** The stall report
reads `lock '?' @ 0xffff...` and the held stack reads `[0] ? @ 0xffff...`.

Why this is the better fix rather than merely the cheaper one:

* It is **one change that covers every lock in the tree**, including the ones
  nobody has thought about, the heap-allocated ones no symbol covers, and every
  lock added after today.
* The address is **already** lockdep's class identity -- `find_or_register_class`
  keys on `lock_addr`, and `Mutex::addr()` is the address of the inner
  `spin::Mutex`. So the two reports now share one key and can be
  cross-referenced directly.
* It answers the question the dump exists to answer. With most locks named `?`,
  a held stack rendered by name alone reads `[0] ?` / `[1] ?` -- entries
  indistinguishable from each other, let alone matchable against the stalled
  lock. "Am I already holding the lock I am stalled on?" is the entire question a
  recursive self-deadlock turns on, and it is now a comparison instead of a
  guess. That is precisely the question boot #10's dump left unanswerable.

`PreemptSpinMutex` gained an `addr()` accessor for this, deliberately returning
the address of its inner `spin::Mutex` so it agrees with `Mutex::addr()` and
needs no offset correction when compared against a lockdep class ID.

**Still open:** the hang itself, and it stays open. It is flaky (~1 boot in 8 was
non-clean when first seen), it has not recurred, and the reproduction advice
above still stands -- in particular, check whether the freeze point *moves*
between runs, which is what distinguishes this from a deterministic deadlock in
whichever self-test the log happens to stop at.
