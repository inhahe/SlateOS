## 18. TD8 membarrier per-mm registration — inherit `membarrier_state` across fork, do NOT reset on exec (yet)

**Date:** 2026-06-14

**Decided by:** Claude (autonomous). Reversible; the operator may overrule. Two
small judgment calls inside the TD8 fix (gating `membarrier(2)` expedited issues
on prior registration), neither of which has an obviously-correct answer given
Linux-source ambiguity and our missing exec-reset hook.

**Context:**
TD8 added a per-mm `Process::membarrier_state` READY bitmask so an unregistered
`PRIVATE_EXPEDITED*` issue returns `-EPERM` (matching Linux). Two lifecycle
questions arose:

1. **Fork inheritance.** Linux's `dup_mm` does `memcpy(mm, oldmm)` and `mm_init`
   does not clear `membarrier_state`, so a forked child inherits the parent's
   registrations. But intuition ("registration is per-mm, a new mm starts
   fresh") points the other way, and the kernel source is subtle.
2. **Exec reset.** Linux's `membarrier_exec_mmap` resets `membarrier_state` to 0
   on `execve`. We have no exec-time PCB-reset hook today (the same gap already
   documented for `linux_pdeathsig`, `linux_dumpable`, `linux_keepcaps`).

**Decision:**
1. **Inherit across fork** (child copies the parent's `membarrier_state`),
   matching the `dup_mm` memcpy. Rationale: it is what Linux actually does, and
   it is the *more permissive* choice — a child that relied on the parent's
   registration won't get a surprise `-EPERM`. Easy to flip to "reset to 0" if
   this proves wrong (one line in `fork_create`).
2. **Do NOT reset on exec yet** — consistent with the existing codebase, which
   already does not reset `pdeathsig`/`dumpable`/`keepcaps` on exec for the same
   reason (no shared exec-time PCB-reset block exists). Chasing a dedicated exec
   hook for one field would be a larger, separate change touching several fields
   at once. Documented as a residual in `known-issues.md` TD8 and `todo.txt`;
   the `todo.txt` entry says to add `membarrier_state = 0` when that shared
   exec-reset block is finally built.

**Alternatives considered:**
- *Reset membarrier_state to 0 on fork* — diverges from Linux's memcpy and is
  stricter (could EPERM a child that Linux permits). Rejected.
- *Build the exec-reset hook now, just for membarrier* — correct end-state but
  premature and narrow; the proper fix resets several fields together. Deferred.
- *Keep TD8 unresolved (per-task or no state)* — rejected: a per-task map would
  wrongly reject a cross-thread issue Linux accepts (threads share one mm); the
  per-mm field is the right home and is now testable via the pure
  `membarrier_decide` helper + direct `pcb` exercise without a userspace harness.

**Where:** `kernel/src/proc/pcb.rs` (`Process::membarrier_state`, fresh/fork
init, `membarrier_state`/`membarrier_register` accessors),
`kernel/src/syscall/linux.rs` (`membarrier_decide`,
`membarrier_registrations_mask`, `sys_membarrier`, self-test
`self_test_membarrier_registration`). Tracked as **known-issues.md TD8**
(now RESOLVED) with the exec-reset residual in `todo.txt`.

**How to reverse:** to reset on fork, set `membarrier_state: 0` in the
`fork_create` child literal instead of inheriting `parent.membarrier_state`. To
drop the whole feature, revert `sys_membarrier` to the unconditional
`fence`/`0` arms and remove the `Process` field + accessors.
