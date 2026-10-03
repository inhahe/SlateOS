## 7. waitpid(pid <= 0) — collapse all "any child" cases; single any-child waiter slot; reaped-pid via arg1

**Date:** 2026-05-31 (predates this file; recorded retroactively 2026-06-12)

**Decided by:** Claude (autonomous) — implementation choices made while
building the wait-for-any-child path.

**Context:**
POSIX `waitpid` distinguishes four pid forms: `> 0` (that specific child),
`== 0` (any child in the **caller's process group**), `< -1` (any child in
process group `|pid|`), and `== -1` (any child whatsoever). The first
"wait for any child" implementation had to choose how faithfully to model the
process-group cases when there is **no process-group subsystem yet**, how to
register the any-child waiter, and how to return the reaped pid to userspace
without breaking the existing specific-pid syscall ABI.

**Decision (three sub-decisions):**
- **(a) Collapse all `pid <= 0` to "any child."** With no process groups,
  `pid == 0` and `pid < -1` are treated identically to `pid == -1`. Correct for
  the common case (shells/make use `-1`) and for any single-process-group
  workload.
- **(b) One any-child waiter slot on the parent PCB** (`Process::wait_any_task`),
  unlike the specific-pid waiter which lives on the *child* PCB. If two threads
  of the same process both call `waitpid(-1)` concurrently, the second
  registration clobbers the first; only one thread reliably gets the child-exit
  wake (the other relies on its own `try_reap_any` at block entry or a later
  wake). The clobber is **safe**: `clear_wait_any_task` only clears the slot if
  it still holds the caller's own `TaskId`, so a thread never clears another's
  registration.
- **(c) Reaped pid returned via the `arg1` pointer.** The any-child path writes
  the reaped child's pid as an `i32` to the user `arg1` slot (posix `waitpid`
  passes a real `&mut` via `syscall2`), while `rax` still carries the exit code.
  The kernel writes `arg1` **only** in the any-child branch — specific-pid
  callers (init/services using `syscall1`) leave a stale pointer in `rsi/arg1`,
  so writing it for them would corrupt memory.

**Rationale:**
- Ships a working `wait(-1)` (the form shells and `make` actually use) without
  blocking on a process-group subsystem that isn't needed yet.
- The single-slot waiter matches typical single-threaded-waiter usage and
  avoids a per-process waiter list before any caller needs one.
- The `arg1` ABI extends wait without breaking the established specific-pid
  calling convention.

**Cons / cost accepted:**
- `pid == 0` / `pid < -1` do **not** filter by process group — once process
  groups land, a caller that means "my group only" would over-match. Acceptable
  while there is exactly one group.
- Concurrent multi-thread `waitpid(-1)` in one process is not fully reliable
  (only one waiter is registered at a time).

**Alternatives considered:**
- *Implement process-group filtering now* — rejected: requires a process-group
  subsystem that doesn't exist; premature.
- *A list/set of any-child waiters woken together* — deferred: no current
  caller does concurrent multi-thread `wait(-1)`; the proper fix when one
  appears is to make `wait_any_task` a small `TaskId` list and wake all.
- *Return the reaped pid in `rax` and the exit code elsewhere* — rejected:
  would break the existing specific-pid ABI (`rax` = exit code) that init and
  services already depend on.

**Where it lives:**
- `kernel/src/syscall/handlers.rs`: `sys_process_wait` / `sys_process_try_wait`
  (`pid_arg <= 0` branch ~line 3162+), `write_reaped_pid` (~line 3147),
  `set_wait_any_task` / `clear_wait_any_task` usage (~line 3238+).
- `kernel/src/proc/pcb.rs`: `Process::wait_any_task`, `set_/clear_wait_any_task`.
- `posix/src/process.rs::waitpid`: passes the `&mut` reaped-pid slot.

**How to reverse:**
- **(a)** When process groups land, subdivide the `pid_arg <= 0` branch on the
  exact value (`0` → caller's group, `< -1` → group `|pid|`, `-1` → any).
- **(b)** Replace `wait_any_task` with a `TaskId` list/set and wake all
  registered waiters on child exit.
- **(c)** Only revisit if the wait ABI is redesigned; the split (exit code in
  `rax`, pid in `arg1`) is intentional and back-compatible.
