### BUG-SINGLE-SHOT-PARK-FABRICATES-EVENT. `sys_irq_wait` and thread `join()` parked with a bare `block_current()` and then *assumed* their event had happened, so a spurious wake made them report an interrupt that never fired / an exit value from a thread still running — FIXED 2026-07-27

**Where:** `kernel/src/syscall/handlers.rs` — `sys_irq_wait()` (~423); and
`kernel/src/proc/thread.rs` — `join()` (~406).

**Bug.** Both were single-shot parks: register, `block_current()` once, then
treat the return as proof of the event.

* `sys_irq_wait` consumed the pending counter after waking and, when it was
  zero, **fabricated a count of 1** (`let result = if count > 0 { count }
  else { 1 };`) — literally reporting an interrupt that no ISR ever
  recorded. A driver would then go poll a device that had nothing for it.
* `join()` retrieved the exit value after waking and, when there was none,
  returned `Ok(0)` with a "shouldn't happen" warning — handing the caller a
  fake exit status for a thread that was still running, and dropping the
  registration on the floor so the *real* exit later woke nothing.

This is the residual-risk class left by
`BUG-TRYWAKE-FALSE-CONFLATES-CONTENTION`: a stale `pending_wake` token makes
the next `block_current()` return immediately. An audit of all 61
`block_current()` call sites (18 files) found every other one already inside
a condition re-check `loop {}` — the 9 in `syscall/linux.rs`, both in
`container.rs`, both `wait4` arms in `handlers.rs`, and all of the `ipc/*`
ones (rewritten during the `WaiterSet` sweep). These two were the only
exceptions.

**Second, independent defect found in the same audit.** The join wake lived
in `thread_exit_with_value()`, but that is only *one* of the ways a thread
dies. `on_thread_exit()` is the universal death hook — it is also reached
from `idt.rs` (unhandled exception kills the task), `sys_exit_group`, and
process teardown. A thread that died by any of those routes left its joiner
blocked **forever**, because nothing ever removed the `THREAD_JOIN_WAITERS`
entry or woke the waiter.

**Fix.**

* `sys_irq_wait` now loops: consume → return if non-zero → (re)register →
  park. It can only ever return a count an ISR actually recorded; the
  fabricated `1` is gone. Re-registering each iteration is an idempotent
  atomic store.
* `join()` now loops on the real condition — *its own registration*.
  `on_thread_exit` removes the entry under the `THREAD_JOIN_WAITERS` lock
  immediately before waking, so while the entry is still ours nothing has
  happened and we park again.
* The join wake **moved** from `thread_exit_with_value` into
  `on_thread_exit`, placed *before* the `THREAD_OWNERS` lookup (which
  early-returns for tasks never registered as process threads — a joiner
  must be released regardless). Every death path now releases the joiner.
  `thread_exit_with_value` still records the exit value first, so the
  ordering the joiner depends on is unchanged.
* A thread that died without recording an exit value (detached, or killed)
  now yields `Ok(0)` with an explanatory message instead of a
  "shouldn't happen" warning — that case is now expected, not anomalous.

**Regression test:** `test_blocking_join()` in `kernel/src/proc/thread.rs`
(wired into `thread::self_test()`), two phases: target records an exit value
(→ 77) and target dies *without* recording one, the crash shape (→ 0). The
target task itself asserts the joiner stays parked for 256 yields after its
registration appears — i.e. `join()` does not return while the target is
still alive. Boot-verified:
`[thread]   Blocking join (exit value recorded: true/false): OK`.

**Boot-phase note for future tests here.** The handshake is deliberately
*between the two spawned tasks*, never with the task running the self-test.
Both spawned tasks run at `DEFAULT_PRIORITY`, which outranks the boot task,
so a target gated on a flag set by the boot task **livelocks**: the target
spins `yield_now()` and the boot task is never scheduled to release it. The
first version of this test did exactly that and hung the boot (955 s
timeout, serial stops right after "Spawned task 77").

**Known remaining limitation (by design, unchanged):** `IRQ_WAIT_TASK` holds
one task per IRQ line and `THREAD_JOIN_WAITERS` one joiner per target. A
second waiter on either overwrites/`WouldBlock`s rather than queueing. Both
are documented single-waiter contracts, unlike the four IPC objects fixed in
`BUG-PIPE-SINGLE-WAITER-SLOT` (which promised multi-waiter semantics and did
not deliver them).
