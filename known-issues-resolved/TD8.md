### TD8. `membarrier` PRIVATE_EXPEDITED issue without prior REGISTER returns 0 where Linux returns `-EPERM` — RESOLVED 2026-06-14

**What it was:** `sys_membarrier()` (`kernel/src/syscall/linux.rs`) accepted every
issue command (`MEMBARRIER_CMD_PRIVATE_EXPEDITED`,
`…_PRIVATE_EXPEDITED_SYNC_CORE`, `…_PRIVATE_EXPEDITED_RSEQ`) and returned 0
unconditionally. Linux v6.6's `membarrier_private_expedited()` first checks the
issuing mm's `membarrier_state` and returns **`-EPERM`** when the matching
`MEMBARRIER_STATE_*_READY` bit is not set — i.e. when the process never issued
the corresponding `…_REGISTER_*` command. That EPERM check runs **before** the
single-CPU `return 0` shortcut, so even on our uniprocessor an unregistered
`PRIVATE_EXPEDITED` issue should be `-EPERM`, not 0. Symmetrically, our
`…_REGISTER_*` commands were no-ops and `MEMBARRIER_CMD_GET_REGISTRATIONS`
always reported 0. (Note: `GLOBAL_EXPEDITED` *issue* is NOT gated on Linux —
only the three `PRIVATE_EXPEDITED*` issues are; the original note overstated
this.)

**Fix (implemented):** added a per-mm `membarrier_state: u32` READY bitmask to
`Process` (`kernel/src/proc/pcb.rs`), shared across the process's threads (so a
thread may register and a sibling issue), inherited verbatim across `fork`
(Linux's `dup_mm` memcpy) via `pcb::membarrier_register` / `membarrier_state`
accessors. `sys_membarrier` now resolves the issuing mm's state and routes
through the pure, unit-tested `membarrier_decide(cmd, state)`: `REGISTER_*` OR
in their READY bit; the three `PRIVATE_EXPEDITED*` issues return `-EPERM`
unless their bit is set; `GET_REGISTRATIONS` reports the registered-command
bitmask via `membarrier_registrations_mask`; `GLOBAL`/`GLOBAL_EXPEDITED` issue
need no registration. The boot self-test (`self_test_membarrier_registration`,
"membarrier per-mm registration gating (TD8): OK") exercises `membarrier_decide`
exhaustively and drives the per-mm READY-bit store (register/idempotency/
cross-command isolation/GET mask) through a throwaway `pcb::create` process —
solving the original "no owner mm at boot" testability blocker by testing the
pure helper and the pcb layer directly rather than through the syscall caller's
(absent) mm.

**Residual divergence — RESOLVED 2026-06-14:** Linux resets `membarrier_state`
to 0 on `execve` (`membarrier_exec_mmap`); we previously lacked an exec-time
PCB-reset hook (the same gap noted for `linux_dumpable`/`linux_keepcaps`/
`linux_thp_disable`), so a registration survived exec. Now fixed: added
`pcb::reset_linux_state_for_exec(pid)`, called from `spawn::exec_process` after
`reset_vmas_for_exec`, which clears (under one `PROCESS_TABLE` lock) exactly the
fields Linux unconditionally resets on every exec — `membarrier_state` → 0
(`exec_mmap`→`membarrier_exec_mmap`), `linux_dumpable` → 1 (`SUID_DUMP_USER`;
explicit `set_dumpable` in `begin_new_exec`), and the `linux_securebits`
`SECBIT_KEEP_CAPS` bit (bit 4 only — `cap_bprm_creds_from_file` clears it on
every exec, preserving the lock bit and every other securebit). That bit 4 is
now the **single source of truth** for `prctl(PR_SET_KEEPCAPS)` (see the
follow-up note below), so clearing it on exec resets keepcaps too. Fields Linux
preserves across a normal (non-privileged)
exec are left untouched: `linux_thp_disable` and `linux_memory_merge` (both
`MMF_INIT_MASK` mm-flags that the new mm inherits via
`mm->flags = current->mm->flags & MMF_INIT_MASK` — `begin_new_exec` has no
explicit THP/KSM override, so they survive exec), `linux_pdeathsig` (cleared
only on set-uid/caps exec, otherwise preserved per prctl(2)),
`linux_personality` (x86_64 `set_personality_64bit` only clears the unmodelled
`READ_IMPLIES_EXEC`; `ADDR_NO_RANDOMIZE` survives), `linux_no_new_privs`
(sticky), `linux_child_subreaper`, timer-slack. (An initial version of the hook
wrongly reset `linux_thp_disable`, repeating entry 98's mistaken "cleared on
execve" claim; corrected same session.) Self-test
`pcb::test_reset_linux_state_for_exec` asserts the cleared state (membarrier,
dumpable, keepcaps, securebits KEEP_CAPS bit with lock+other bits kept) and the
five preserved fields ("[proc]   exec Linux-state reset: OK"). The in-kernel
`membarrier` self-test
caller (no owner mm) keeps the "fence/0" behaviour by feeding `u32::MAX` to the
gating helper — there is no registration model for a kernel thread with no
sibling userspace threads.

**Follow-up — keepcaps/securebits single source of truth (2026-06-14):** the
exec-reset audit surfaced a real ABI incoherence: `prctl(PR_SET_KEEPCAPS)` was
backed by a standalone `linux_keepcaps` field while `SECBIT_KEEP_CAPS` lived in
`linux_securebits`, even though Linux stores both in the *same*
`cred->securebits` bit 4. `PR_SET_KEEPCAPS`/`PR_SET_SECUREBITS` wrote different
storage, so `PR_GET_KEEPCAPS` and `PR_GET_SECUREBITS` could disagree where Linux
keeps them identical. Fixed by removing the `linux_keepcaps` field and making
`pcb::get_keepcaps`/`set_keepcaps` thin views over `linux_securebits` bit 4
(set/clear only bit 4, leaving every other securebit intact). Also added the
missing Linux gate to the `PR_SET_KEEPCAPS` handler: once
`SECBIT_KEEP_CAPS_LOCKED` (bit 5) is engaged the flag is frozen and the call
returns `-EPERM` (`cap_task_prctl`, verified against torvalds/linux v6.6
`security/commoncap.c`). The gate is the pure helper
`keepcaps_change_allowed(securebits)` so it is unit-testable without a caller
PCB. Tests: `self_test_prctl_dispatch`'s keepcaps block now asserts get/set
coherence in both directions (keepcaps↔securebits bit 4) and the lock-gate
truth table; `pcb::test_reset_linux_state_for_exec` proves `set_keepcaps`
coherently drives bit 4 and the exec reset clears only it.

**Companion fix — PR_SET_SECUREBITS lock enforcement now unit-tested
(2026-06-14):** the same audit found the `PR_SET_SECUREBITS` lock-bit
enforcement (a set lock can't be cleared; a locked flag can't flip) was
inline in the handler and so its `-EPERM` path was unreachable from the
kernel-context boot self-test (no `caller_pid` PCB to seed locked bits) —
the test only covered value validation. Extracted the decision into the pure
`securebits_change_allowed(cur, new_val)` (mirrors `cap_task_prctl`) and added
a truth-table test to `self_test_prctl_dispatch` covering: no-locks→allowed,
new-lock→allowed, clear-set-lock→denied, flip-locked-flag (both
set→clear and clear→set)→denied, and locked-flag-kept-while-flipping-an-
unlocked-flag→allowed ("PR_SET_SECUREBITS lock-bit enforcement … : OK").
