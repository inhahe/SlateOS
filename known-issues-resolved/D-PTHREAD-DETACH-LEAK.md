### D-PTHREAD-DETACH-LEAK. Detached pthread stacks are never freed (64 KiB leaked per detached thread) — RESOLVED (2026-07-01)

**Resolved 2026-07-01.** Implemented the userspace self-unmap fix exactly
as prescribed below:

- `THREAD_TABLE` is now a **lock-free array of atomic `ThreadSlot`s**
  (`task_id: AtomicU64` doubling as occupancy flag with `SLOT_EMPTY`/
  `SLOT_RESERVED` sentinels; `stack_base`/`stack_size: AtomicUsize`;
  `state: AtomicU8`). The old `static mut` + "single-creator convention"
  data race is gone.
- Added the `__pthread_exit_unmap(stack_base, stack_size, retval)`
  `global_asm!` primitive (`target_os="none"` only): it does
  `SYS_MUNMAP` then `SYS_THREAD_EXIT`, carrying `retval` in **R12** (a
  callee-saved reg the kernel's SYSCALL entry stub preserves — verified
  against `kernel/src/syscall/entry.rs`, which pushes/pops rbx/rbp/r12-r15
  around the handler). No memory is touched between the two syscalls.
- The per-slot `state` arbitrates the detach-vs-exit race via
  `compare_exchange`: `JOINABLE --detach--> DETACHED` (thread self-unmaps
  on exit) vs `JOINABLE --exit--> EXITED` (a joiner, or a `pthread_detach`
  that observes `EXITED`, frees the stack after `SYS_THREAD_JOIN` confirms
  the thread is off it). **Exactly one party frees** — no use-after-free.
  `pthread_join` rejects a detached thread (`EINVAL`); double-detach
  returns `EINVAL`; detach-after-joinable-exit reaps.
- Covered by 5 host unit tests in `pthread::tests`
  (`test_thread_slot_store_find_release`, `test_detach_marks_state_detached`,
  `test_double_detach_is_einval`, `test_join_rejects_detached_thread`,
  `test_detach_after_joinable_exit_reaps`) exercising the arbitration
  state machine directly.

**Residual (smaller) follow-up — D-PTHREAD-DETACH-KERNEL-EXITVAL — RESOLVED (2026-07-01):**
the kernel previously retained a small `THREAD_EXIT_VALUES: BTreeMap<TaskId,i64>`
entry (~tens of bytes) for a never-joined *detached* thread, because only
`join` removed it. **Fixed** by threading a "detached" flag through
`SYS_THREAD_EXIT` (arg1): `sys_thread_exit` (`kernel/src/syscall/handlers.rs`)
now decodes `let detached = args.arg1 != 0;` and passes it to
`thread_exit_with_value(exit_value, detached)`. A new `record_exit_value`
helper in `kernel/src/proc/thread.rs` skips the map insert entirely when
`detached` is set (task IDs are not reused while a task is live, so there
is no stale entry to clear). The userspace `__pthread_exit_unmap` self-unmap
asm sets `esi = 1` (detached) before `SYS_THREAD_EXIT`; the joinable
`pthread_exit` path uses `syscall2(SYS_THREAD_EXIT, retval, 0)` so arg1 is a
*defined* 0 (a bare `syscall1` would leave RSI holding stale/undefined bits,
which the kernel could misread as detached). In-kernel self-test
`test_detached_exit_not_retained` (verified at boot: `[thread]   Detached
exit value not retained: OK`) confirms joinable exits are recorded and
detached exits are not. Combined with the userspace stack self-unmap fix,
a detached thread now leaks neither its 64 KiB stack, its table slot, nor a
kernel map entry.

*Note:* a native SlateOS-ABI userspace test harness that links `posix`
does not currently exist (the boot path's thread tests use real glibc via
`clone`, not our `SYS_THREAD_CREATE`), so the "boot self-test spawning N
detached threads" originally envisioned below is deferred until such a
harness exists; the host unit tests cover the (bug-prone) arbitration
logic in the meantime.

---

**Original entry (for reference):**

**Where:** `posix/src/pthread.rs`. `pthread_create` mmaps a
`DEFAULT_THREAD_STACK_SIZE` (64 KiB) user stack and records it in
`THREAD_TABLE`. `pthread_join` frees the stack after `SYS_THREAD_JOIN`
returns. But a **detached** thread is never joined, so nothing ever
munmaps its stack — the `ThreadInfo` slot and the 64 KiB mapping leak for
the life of the process. `pthread_detach` only flips `info.detached`.

**Effect:** A long-running process that repeatedly spawns detached
worker threads leaks 64 KiB per thread plus a `THREAD_TABLE` slot (only
64 slots), eventually exhausting the table and address space. Most
current userspace tools don't spawn many detached threads, so it's
low-frequency, but it is a genuine unbounded leak.

**Proper fix (userspace-only, no kernel change):** the exiting thread
must free its *own* stack, glibc-`__unmapself`-style. Add a small
bare-metal asm primitive `__pthread_exit_unmap(stack_base, stack_size,
retval)` that issues `SYS_MUNMAP(stack_base, stack_size)` then
`SYS_THREAD_EXIT(retval)` **without touching the stack between the two
syscalls** (stash `retval` in a callee-saved reg that `SYSCALL` doesn't
clobber — not the stack). In `pthread_exit`, after running TSD
destructors, look up the calling thread's `ThreadInfo`: if `detached`,
`take_thread_info()` and tail-call `__pthread_exit_unmap`; if joinable,
fall through to the normal `SYS_THREAD_EXIT` (the joiner frees the
stack). Threads created detached, or detached before exit, both work.

**Concurrency caveat that must be handled:** `pthread_detach` (called
from another thread) races the exiting thread's read of `info.detached`
+ `take_thread_info`. The `THREAD_TABLE` currently relies on a
"single-creator convention" with NO lock — that is unsafe for the
detach-vs-exit window. The proper fix must add a real lock (or an atomic
detached flag per slot, CAS'd by whichever of detach/exit gets there
first) so exactly one of {joiner, self-unmap} frees the stack and there
is no use-after-free. Model on glibc's `joinid`/`cancelhandling` atomic.

**Why deferred:** the asm self-unmap path is `target_os="none"`-only and
cannot be unit-tested on the host; combined with the detach/exit data
race it is too risky to land without a QEMU multithread stress test.
Landing it blind risks a use-after-free crash, which is far worse than a
slow leak. Do it as its own focused task with a boot self-test that
spawns N detached threads in a loop and asserts address-space / table
usage stays bounded.

**Discovered/documented:** 2026-06-30 (already noted as a `// Known
limitation` in `pthread_detach`'s doc comment; promoted to tracked tech
debt while implementing per-thread TSD).
