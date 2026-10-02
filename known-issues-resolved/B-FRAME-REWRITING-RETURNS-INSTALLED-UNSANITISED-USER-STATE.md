### B-FRAME-REWRITING-RETURNS-INSTALLED-UNSANITISED-USER-STATE. Every syscall that rewrites the return frame took RIP/RSP/RFLAGS from userspace unchecked — 2026-08-13 — ✅ FIXED 2026-08-13 (`kernel/src/syscall/entry.rs`, `kernel/src/syscall/handlers.rs`, `kernel/src/syscall/linux.rs`)

**Context.** Three syscalls do not return to their caller — they overwrite the
saved return frame so the SYSRET path resumes somewhere else entirely:

| Syscall | File | Source of the new state |
|---|---|---|
| `sys_exception_return_with_frame` | `handlers.rs` | user `ExceptionContext` |
| `sys_signal_return_with_frame` | `handlers.rs` | user `SignalContext` |
| `linux_rt_sigreturn` (Linux ABI) | `linux.rs` | user `ucontext.uc_mcontext` |

All three read RIP, RSP and RFLAGS out of a userspace structure and stored them
straight into `frame.user_rip` / `frame.user_rsp` / `frame.user_rflags`. Nothing
between the copy-in and `sysretq` looked at the values. This was found while
converting the two `handlers.rs` entries away from raw user pointers for
`D-SYSCALL-HANDLERS-HAND-RAW-USER-SLICES-TO-KERNEL-CODE` (below) — the pointer
conversion is unrelated, but writing an honest justification comment for it
required checking what *did* sanitise the frame, and the answer was "nothing".

**Bug 1 — non-canonical RIP faults at CPL 0 (CVE-2012-0217 shape).** `sysretq`
loads RIP from RCX *while still at ring 0* and only then drops privilege. If
RCX is non-canonical, the #GP is raised in ring 0, not ring 3. This kernel makes
that strictly worse than the classic bug: `syscall_entry_stub` in `entry.rs`
does `mov rsp, gs:[8]` and `swapgs` **before** `sysretq`, so at the moment of
the fault RSP is attacker-influenced and the GS base is the user's. The #GP
handler then runs at kernel privilege on a stack and per-CPU base the attacker
chose. A userspace thread could trigger this with a one-line `ExceptionContext`
whose `rip` was `0xFFFF_8000_0000_0000`.

**Bug 2 — unsanitised RFLAGS.** The restored RFLAGS went to ring 3 verbatim, so
a caller could set:

- **IOPL = 3** — direct `in`/`out` to every I/O port from an unprivileged
  process. That is a full privilege escalation on its own: PCI config space,
  the PS/2 controller, the PIT, the CMOS/RTC, ATA PIO.
- **NT** (nested task) — corrupts a subsequent `iret` into a task switch.
- **VM** (virtual-8086) — puts the CPU in a mode the rest of the kernel has no
  handling for.
- **VIF/VIP** — meaningless without VME but sets up confusing state.
- **IF cleared** — returns to ring 3 with interrupts disabled, which wedges that
  CPU until something re-enables them (nothing does).

`linux.rs` was the one path that *had* a mask (`SIGRETURN_RFLAGS_USER_MASK`),
but it was a local constant covering only that function, and it did not check
RIP/RSP at all.

**Bug 3 — the registration side accepted kernel addresses.** Even with the
return paths fixed, `sys_signal_register`, `sys_set_exception_handler` and
Linux `sys_rt_sigaction` would accept a *kernel* address as the handler /
trampoline. Delivery builds a frame targeting that address, so the check has to
exist at registration too or the same kernel RIP arrives by a different route.

**Fix.** One shared policy in `kernel/src/syscall/entry.rs`, applied at every
path rather than reimplemented per-caller:

```rust
pub const USER_RFLAGS_MASK: u64   = 0x0024_0DD5; // CF PF AF ZF SF TF DF OF AC ID
pub const USER_RFLAGS_FORCED: u64 = 0x0000_0202; // IF + reserved bit 1

pub const fn sanitize_user_rflags(raw: u64) -> u64 {
    (raw & USER_RFLAGS_MASK) | USER_RFLAGS_FORCED
}

pub fn user_return_state_ok(rip: u64, rsp: u64) -> bool {
    rip < crate::mm::page_table::USER_SPACE_END && rsp < crate::mm::page_table::USER_SPACE_END
}
```

`user_return_state_ok` is deliberately stronger than a canonicality test:
`rip < 0x0000_8000_0000_0000` rejects the whole upper half, so a canonical
*kernel* address is refused as well as a non-canonical one. All three return
paths now reject a failing pair with `EFAULT`/`InvalidAddress` **before**
touching the frame, and pass RFLAGS through `sanitize_user_rflags`.
`linux.rs`'s `SIGRETURN_RFLAGS_USER_MASK` was deleted and its
`SIGRETURN_RFLAGS_FORCED` re-pointed at `entry::USER_RFLAGS_FORCED` so the
kernel-built signal frame and the user-supplied one cannot drift apart. The
three registration syscalls now reject a handler `>= USER_SPACE_END` up front
(0 still means unregister / `SIG_DFL` / `SIG_IGN`).

**Why reject rather than Linux's lazy approach.** Linux lets a bad
`rt_sigaction` handler through and only kills the process when delivery
faults (`force_sigsegv`). Rejecting at registration gives the caller a
diagnosable `EFAULT` at the point of the mistake instead of an unexplained
death later, and it means the delivery path has one fewer failure mode to
unwind. Nothing in-tree registers a kernel address, so there is no
compatibility cost.
