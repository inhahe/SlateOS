## 24. Cross-process memory introspection — keep channel/shared-memory IPC for *consensual* sharing; add a **debug-capability-gated** `process_vm_readv`/`writev` for *unilateral* introspection

**Date:** 2026-06-14

**Decided by:** Operator (this was `open-questions.md` Q6). The operator's words:
*"Q6: Yes, keep the existing IPC and add a debug-capability-gated ability to read
all of another process' memory."*

**The two-mechanism split (operator-confirmed):**
1. **Consensual** cross-process memory sharing → the **existing channel +
   shared-memory IPC** path, unchanged. Both parties opt in; no special right is
   needed because the owner of the memory chooses to share it.
2. **Unilateral** introspection (one process reading/writing another's memory
   *without the target's cooperation*, à la `process_vm_readv`/`writev` and, in
   future, `ptrace`) → gated by a **debug capability the caller holds over the
   specific target process**, never derived from ambient PID/uid authority.

**What was implemented this turn:**
- **`Rights::DEBUG`** (bit 17) added in `kernel/src/cap/rights.rs` — the
  unilateral-introspection authority, carried on a
  `ResourceType::Process` capability whose `resource_id` is the target PID.
  Delegation stays AND-mask (a holder can only pass on a subset), so debug
  authority can only flow parent→child or from a privileged debugger broker —
  never be conjured from PID/uid.
- **`process_vm_impl`** (`kernel/src/syscall/linux.rs`): the cross-address-space
  arm — previously a hard `ESRCH` rejection — now checks
  `pcb::has_capability_for(caller, Process, target_owner, Rights::DEBUG)`. No
  cap → **`EPERM`** (mirrors Linux `ptrace_may_access` denial); target gone /
  no PML4 → **`ESRCH`**. With the cap, the copy loop routes the remote side
  through `mm::user::copy_from_user_as` (read / `readv`) or
  `copy_to_user_as` (write / `writev`), preserving Linux's best-effort
  partial-copy contract.
- **`DEBUG` gates both read and write.** A debug capability is total
  introspection authority — real debuggers poke memory as well as read it — so a
  single right covers `readv` and `writev` rather than splitting them.
- Self-test `self_test_process_vm_cross_as` (registered in `main.rs`) covers the
  gate predicate (false with no cap / read-only cap / wrong pid; true after
  `DEBUG` granted) and the remote read/write transfer mechanism end-to-end via
  HHDM verification.

**Why a capability and not a PID/uid check.** Slate OS is capability-based with
no ambient authority (CLAUDE.md architectural rule). "Same uid may ptrace" is
exactly the ambient-authority model the design forbids. Routing unilateral
introspection through an explicit, delegable, AND-mask-narrowable `DEBUG` right
on a specific `Process` capability is the native-correct expression of "X may
debug Y."

**Deferred follow-up:** `ptrace` itself (breakpoints, single-step,
register access, signal-delivery interception) still returns `EPERM`/`ENOSYS`;
when it is built it will gate on the same `Process`+`DEBUG` capability. Logged in
`todo.txt`.

**Where it lives:** `kernel/src/cap/rights.rs` (`Rights::DEBUG`),
`kernel/src/syscall/linux.rs` (`process_vm_impl`, `self_test_process_vm_cross_as`,
`sys_process_vm_readv` doc), `kernel/src/main.rs` (self-test registration),
`kernel/src/mm/user.rs` (`copy_from_user_as`/`copy_to_user_as`, the purpose-built
cross-AS primitives).
