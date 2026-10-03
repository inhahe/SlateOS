## §124 — Untrusted return state is rejected at the point it is supplied, and the check lives in one shared place

**Date:** 2026-08-13
**Decided by:** Claude (autonomous)

**Context.** Three syscalls do not return to their caller — they overwrite the
saved return frame so the SYSRET path resumes elsewhere:
`sys_exception_return_with_frame`, `sys_signal_return_with_frame`, and Linux
`rt_sigreturn`. All three took RIP, RSP and RFLAGS out of a userspace structure
and installed them unchecked (`B-FRAME-REWRITING-RETURNS-INSTALLED-UNSANITISED-USER-STATE`).
Two further paths — `idt::try_dispatch_user_exception` and the signal-delivery
builder — write a frame *to* a user address taken from the saved ring-3 RSP,
which had the same missing check
(`B-EXCEPTION-FRAME-WRITTEN-TO-ATTACKER-CHOSEN-RSP`). Fixing them raised three
questions with real arguments on both sides.

### 1. Reject a bad handler at registration, or let delivery fail like Linux

Linux accepts any `sa_handler` from `rt_sigaction` and only kills the process
when delivery faults (`force_sigsegv`). We reject a handler at or above
`USER_SPACE_END` in `sys_signal_register`, `sys_set_exception_handler` and
`sys_rt_sigaction`.

*For the Linux behaviour:* it is the compatible one, and it needs no check on
the registration path at all — delivery has to handle a bad address anyway
(the page can be unmapped after registration), so the registration check is
strictly redundant for *safety*. Two checks where one suffices is the kind of
duplication that later drifts.

*For rejecting early (chosen):* the two checks are not the same check. Delivery
can only answer "this address did not work"; registration can answer "this
address can never work", and it can say so at the point the caller made the
mistake, with an `EFAULT` the caller can act on. A process that registers a
kernel address is not going to be helped by dying silently thousands of
instructions later. And the delivery-side check remains, so this adds a
diagnostic, not a dependency — deleting the registration check would leave the
system safe, merely less debuggable. Nothing in-tree registers a kernel address,
so the compatibility cost is hypothetical.

### 2. Test `< USER_SPACE_END`, not canonicality

The classic bug here (CVE-2012-0217) is a *non-canonical* RIP reaching `sysretq`,
which loads RIP from RCX while still at CPL 0 — so the #GP lands in ring 0. Our
entry stub makes that worse than the original: it does `mov rsp, gs:[8]` and
`swapgs` before `sysretq`, so the ring-0 fault handler would run on an
attacker-influenced stack with the user's GS base.

*For a canonicality test:* it is the precise statement of the hardware hazard,
and it is what the CVE is about.

*For `< USER_SPACE_END` (chosen):* it is strictly stronger — it rejects every
non-canonical address *and* every canonical kernel one. A canonical kernel RIP
is not a `sysretq` hazard, but installing one is never a legitimate request from
ring 3, and the same predicate then serves the frame-*writing* paths, where a
canonical kernel address is exactly the dangerous case. One predicate that is
right for both directions beats two that each cover half.

### 3. Mask RFLAGS to an allowlist, not a denylist of known-bad bits

`USER_RFLAGS_MASK = 0x0024_0DD5` keeps CF/PF/AF/ZF/SF/TF/DF/OF/AC/ID and drops
everything else; `USER_RFLAGS_FORCED = 0x0000_0202` puts IF and the reserved
bit-1 back. This is the same allowlist-over-denylist call as §122's FMASK
widening, reached for the same reason: the bits that hurt (IOPL=3 gives ring 3
direct I/O-port access; NT corrupts a later `iret`; VM enters a mode nothing
handles; a cleared IF wedges the CPU) were found by enumeration, and enumeration
is a standing obligation to re-audit. An allowlist is a one-time decision.

`linux.rs` already had a local `SIGRETURN_RFLAGS_USER_MASK` doing this correctly
for one of the three paths. It was deleted rather than copied: three private
copies of a security predicate drift, and the one that drifts is the one nobody
is looking at. The constants and both predicates now live in
`kernel/src/syscall/entry.rs`, next to the SYSRET path they protect.

**How to reverse.** The predicates are two small functions in `syscall/entry.rs`;
loosening the policy means widening `USER_RFLAGS_MASK` or relaxing
`user_return_state_ok`, in one place. Do not delete the registration-side checks
and the return-side checks together — the return-side ones are load-bearing.
