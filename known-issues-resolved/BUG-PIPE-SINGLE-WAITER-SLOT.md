### BUG-PIPE-SINGLE-WAITER-SLOT. A pipe remembered only ONE blocked reader and ONE blocked writer, so a second blocker on the same pipe end was silently forgotten and never woken — FIXED 2026-07-27

**Where:** `kernel/src/ipc/pipe.rs` — the per-pipe fields `reader_waiter:
Option<TaskId>` / `writer_waiter: Option<TaskId>`, written in `read()`,
`write()`, `wait_readable()`, `read_timeout()`, `write_timeout()` and
consumed with `.take()` by the peer operation and by `close()`.

**Bug:** the fields were *single slots*, not wait queues. If task A parked on
an empty pipe (`reader_waiter = Some(A)`) and task B then parked on the same
pipe, B's assignment **overwrote** the slot. The subsequent write/close
`.take()`d only B and woke only B; **A was never woken by anything** and
parked forever. The same applied symmetrically to `writer_waiter` when two
writers blocked on a full pipe. Several waiters per end is not exotic:
`dup()` and process spawn hand the same end to multiple processes, and
`wait_readable()` (the `tee` primitive) parks on the read end alongside a
real reader.

A second, subtler defect rode along: only the *signal* exit path cleared the
waiter slot (`if pipe.writer_waiter == Some(task) { … = None }`). The
timeout path left a stale task id behind, which a later state change would
then "wake" — mis-waking whatever task had since recycled that id.

**Impact when found:** latent — every self-test and the shell plumbing used a
pipe with exactly one reader and one writer, so the slot was never contended.
It would have become real for any legitimate multi-reader/multi-writer pipe
(a worker pool where N children share one pipe end — a standard POSIX
pattern, since reads of ≤PIPE_BUF are atomic precisely so this works).

**Found by:** the BUG-DASH-CMDSUB-INTERMITTENT-HANG audit (2026-07-27), which
ruled out the lost-wakeup hypothesis on that path but surfaced this adjacent
defect.

**Fix:** both `Option<TaskId>` slots were replaced by a `WaiterSet` — a
`Vec<TaskId>` with `insert`/`remove`/`take_all` — embedded in `Pipe` and
mutated under the existing `PIPES` lock, so the documented `PIPES → SCHED`
lock order and the enqueue-inside-the-same-critical-section guarantee are
both preserved unchanged. (`sched::waitqueue::WaitQueue` was evaluated and
rejected: it owns an internal `Mutex<[u64; 32]>` that would have introduced
a *second* lock inside the `PIPES`-held critical sections, plus a new
lock-order obligation and a spin-yield-when-full failure mode.)

Three semantic changes came with it:

* **Wake-all, always.** Every state change wakes *all* waiters on the
  affected end, matching Linux (`fs/pipe.c` parks on a non-exclusive wait
  queue, so `wake_up_interruptible_sync_poll()` wakes every sleeper). This
  is required for correctness on EOF/EPIPE, which are permanent broadcast
  conditions — a waiter missed there can never be woken by anything else.
* **Deregister-first.** Every park loop now removes itself from the set at
  the top of each iteration (inside the lock), so no exit path — success,
  timeout, signal, or error — can leave a stale task id behind.
* **`close()` broadcasts.** Full closure of one end takes the whole waiter
  set of the far end and wakes all of it after dropping the table lock.

**Regression test:** `test_multi_waiter_wake()` in `kernel/src/ipc/pipe.rs`
(wired into `pipe::self_test()`). Phase 1 parks two kernel tasks on one
empty read end and does a single 2-byte write, asserting *both* readers wake
and each consumes one byte; phase 2 parks two more on a fresh empty pipe and
closes the write end, asserting both observe `Ok(0)` EOF. Under the old
single-slot code exactly one reader would have completed in each phase.

**Sweep: the same defect existed in all four blocking IPC objects.** Pipes
were only where it was noticed. `eventfd`, `stream_socket` (`socketpair`) and
`timerfd` each carried the *identical* `Option<TaskId>`-per-end
representation, with the identical overwrite and identical stale-entry-on-
timeout behaviour. All four were converted, and the representation was
factored into one shared module so they cannot drift apart again:

* **`kernel/src/ipc/waiters.rs` (new).** Home of `WaiterSet` +
  `wake_all(Vec<TaskId>)`, with the usage contract in the module docs:
  mutate the set inside the owning object's lock, `take_all()`, drop the
  lock, *then* wake (the IPC lock hierarchy always puts the object lock
  before `SCHED`); and deregister at the top of every park-loop iteration.
* **`eventfd.rs`** — `reader_waiters`/`writer_waiters`; `write`,
  `try_write`, `write_timeout`, `read`, `try_read`, `read_timeout`, `close`.
  Multi-waiter is the *design* here: `EFD_SEMAPHORE` exists precisely so N
  consumers can share one counter.
* **`stream_socket.rs`** — per-`Endpoint` sets; `send`/`recv` (+ `try_`/
  `_timeout` variants), `close`, `shutdown`. `socketpair` endpoints are
  routinely inherited by several processes.
* **`timerfd.rs`** — `reader_waiters`; `settime`, `clock_was_set`,
  `read_expirations_blocking`, `close`. The *armed* case partly survived the
  old slot (each blocked reader also arms its own expiry `hrtimer`), but the
  *disarmed* case depended entirely on the registration that `settime`
  broadcasts to, so an overwritten reader slept forever even as the timer
  fired periodically.

**Two further latent hangs found during the sweep and fixed:**

1. `stream_socket::close()` woke only the *peer* endpoint's waiters before
   removing the pair from the table. Anything still parked on the *local*
   endpoint (a caller closing the last reference out from under a blocked
   task) was then unreachable forever. It now drains all four sets.
2. `timerfd::close()` removed the table entry without waking anyone at all.
   A reader parked on a *disarmed* timerfd has no `hrtimer` of its own, so
   the final `close()` left it parked with no wake source in existence. It
   now takes the waiter set and wakes it after dropping the table lock.

**Regression tests for the sweep** (all boot-verified): `test_multi_waiter_
wake()` in `eventfd.rs` (counter wake + close wake) and in
`stream_socket.rs` (data wake + EOF wake), plus
`timerfd::self_test_blocking_multi_waiter()`.

**Boot-phase note (why the timerfd test is not in `timerfd::self_test()`).**
`ipc::timerfd::self_test()` runs in the early deterministic-init phase of
`kmain`, which is *before* `hrtimer::init()` and well before `sti()`. There
is no APIC timer ISR yet, so `hrtimer::process_expired()` is never called and
no hrtimer callback can fire — a reader that re-parks with an expiry timer
there sleeps forever. (This was confirmed empirically: a one-reader version
of the test failed at that point with `hrtimers_pending=1` after 200 ms of
elapsed monotonic time.) The blocking test therefore runs from `kmain`
immediately after interrupts are enabled and `apic::self_test()` has
confirmed the tick is live. **Any future self-test that depends on an
hrtimer callback firing must be placed after that point.**
Verified on target: `[pipe]   Multi-waiter wake (data + EOF): OK`.
