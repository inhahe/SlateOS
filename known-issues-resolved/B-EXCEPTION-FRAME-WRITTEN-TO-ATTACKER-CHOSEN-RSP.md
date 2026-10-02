### B-EXCEPTION-FRAME-WRITTEN-TO-ATTACKER-CHOSEN-RSP. Arbitrary kernel write from any process with an exception handler — 2026-08-13 — ✅ FIXED 2026-08-13 (`kernel/src/idt.rs`)

**Symptom that found it.** Enabling CR4.SMAP wedged the boot at the
`spawn-test-seh-exit` self-test with a fatal kernel #PF: write to
`0x7ffffffeff50` (a *user* stack address) from `memcpy` inside
`idt::try_dispatch_user_exception`, error code `0x3` = present + write +
supervisor. That is the textbook SMAP violation signature: the page is there
and writable, the kernel just is not allowed to touch it outside a STAC window.

**The SMAP part.** `try_dispatch_user_exception` built the `ExceptionContext` on
the user stack with a raw `core::ptr::write(ctx_addr as *mut ExceptionContext,
ctx)` plus a second write for the null return address. Both are supervisor
writes to a user page.

This path was missed by the sweep done for
`D-SYSCALL-HANDLERS-HAND-RAW-USER-SLICES-TO-KERNEL-CODE` (below) for a reason
worth recording: that sweep enumerated the files that take a user address **out
of a syscall argument**. This one takes it out of the *ring-3 interrupt frame's
RSP*. It matched neither the greps nor the mental model, and only the hardware
found it. **Enabling the enforcement is the audit** — a "we converted everything"
claim is not checkable by reading, and this is the third time on this entry that
a syntax-shaped search missed a genuine user access (see also `futex.rs`'s
`&*(addr as *const AtomicU32)`).

**The much worse part, which has nothing to do with SMAP.** `rsp` is read out of
the interrupt frame the CPU pushed for a **ring-3** fault, so it is whatever the
faulting thread had in RSP — entirely attacker-chosen. `ctx_addr` is derived from
it by subtraction and alignment, with no check that the result is a user address.
CR3 is still the process's PML4, which maps the kernel. So:

1. An unprivileged process calls `SYS_SET_EXCEPTION_HANDLER` (no capability
   required).
2. It sets RSP to any kernel address and executes a faulting instruction.
3. The kernel writes a 168-byte `ExceptionContext` there — 15 of whose fields
   are that process's own general-purpose registers, i.e. fully chosen content.

That is an arbitrary kernel write with attacker-controlled data, available to
any process, with no capability and no privilege. It long predates the SMAP
work; SMAP is simply what made the code get read closely.

**Fix.** Both stores now go through `crate::mm::user::write_user_value`, which
validates the destination is below `USER_SPACE_END` and writable, faults in a
demand-paged page, breaks CoW (a freshly-forked process's stack is CoW, and the
kernel cannot rely on its own write triggering the CoW fault handler), and
brackets the copy in STAC/CLAC. On failure it returns `false`, so the caller
kills the process exactly as if no handler had been registered — the right
answer for an unwritable stack too, since there is nowhere to put the frame.
Linux reaches the same outcome via `force_sigsegv`.

**Why the STAC window could not have been opened by hand here.**
`validate_user_write` may fault a page in and may break CoW; both can block. An
AC window held across a reschedule leaves `AC = 1` in the task's saved RFLAGS,
disabling SMAP for that task *and* for the scheduler. The window has to live
inside `mm::user`, around the copy alone.

**Lesson.** Any kernel write whose address derives from a *saved ring-3
register* is as untrusted as one derived from a syscall argument. The
interrupt-frame fields (`rsp`, `rip`, `rflags`, and every `SavedRegisters` GPR)
are user input. Grep for uses of `frame.rsp` / `(*frame).rsp` before trusting
one as a destination.
