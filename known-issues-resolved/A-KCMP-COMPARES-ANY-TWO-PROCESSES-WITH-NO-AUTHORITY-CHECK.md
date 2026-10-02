### A-KCMP-COMPARES-ANY-TWO-PROCESSES-WITH-NO-AUTHORITY-CHECK — 2026-08-21 — **Status: ✅ FIXED 2026-08-21** in `e62931fb7`; **this heading said `OPEN` for 12 days after the fix landed** (corrected 2026-09-02, lane A)

> **Resolution — the fix took the first of the two proposed branches, and the
> entry below describes the code as it was, not as it is.**
>
> `e62931fb7` ("kcmp: gate cross-process introspection on a DEBUG capability")
> added `kcmp_may_access` / `kcmp_may_compare` (`kernel/src/syscall/linux.rs`
> :34751, :34775) and calls the conjunction at :34637, refusing with `EPERM`
> **between** the `ESRCH` liveness gate and the `type` range gate — exactly the
> placement this entry argued for, so the errno discriminator still matches
> Linux: `(pid=-1, type=99)` sees `ESRCH`, and an unauthorised probe with a bad
> type sees `EPERM` rather than `EINVAL`. The predicate is a single
> undifferentiated conjunction over both targets, so the gate does not itself
> become an oracle for which of the two was refused. The `kernel_ctx` escape is
> preserved, so the boot self-test still drives the comparator paths.
>
> Covered by a self-test at :50530 that runs `kcmp_may_compare` against real
> PCBs and real capability tokens, and — the part that matters — asserts the
> **negative** cases too: prober→victim without `DEBUG` is refused, victim→
> bystander is refused in both argument orders, and an unresolvable owner
> (`None`) is refused rather than defaulted. A gate tested only on its allow
> path is a gate you have not tested.
>
> **What this entry is now worth reading for.** Not the defect — that is gone —
> but the two findings that outlive it: the `KCMP_FILE` `EBADF` flip is an
> fd-*presence* oracle and not merely an identity leak, and this comparator is
> **not** a KASLR concern (it orders `handle_kind_ord` and a handle-table index,
> never a kernel address), so Linux's `kptr_obfuscate()` cookie has no analogue
> to add here. Both are now recorded in the handler's own comment so the next
> reader does not re-derive them.
>
> **Why it sat stale.** The same failure `A-CREATE-MODE-SYSCALLS-SILENTLY-DROP-
> SETUID-SETGID-STICKY` records about itself: the fix landed, the heading did
> not move, and nothing in the tree compares a `Status:` line against the code
> it describes. Found by re-reading the entry while picking a task, not by any
> gate. A stale `OPEN` is not harmless — it spends a later session's time
> re-deriving a fix that already exists, which is exactly what happened here.

**In short.** A "syscall" is a request a program makes of the kernel. One of
them, `kcmp`, asks the kernel to compare two *other* programs — "are these two
things part of the same running program?", "does program 41 have file number 7
open?". Linux only answers if the asker would be allowed to debug both targets.
Ours answers anybody, about anybody. Nothing in SlateOS can reach it today, for
an accidental reason (the syscall table our own programs use has no number for
it), so this is a latent hole rather than a live one.

**Where.** `kernel/src/syscall/linux.rs`, `sys_kcmp`, 33398–33546 — lane A's
tree, so lane B has not touched it. Filed as
`requests/b-a-kcmp-compares-any-two-processes-with-no-authority-check.md`.

**The gap.** The handler's own header comment (33402–33410) writes Linux's gate
order down correctly, including `ptrace_may_access(both tasks) -> -EPERM` as
step 2. Step 2 is in the comment and not in the code: grepping the entire
function body for `EPERM`, `capabilit`, `Rights` or `may_access` matches only
that comment line. There is no path on which any caller is refused.

This is inconsistent with lane A's own established policy rather than with some
external standard, which is what makes it look like an oversight. Eleven lines
away in the same file, `sys_process_vm_readv` (26529) gates cross-process access
on a `Process` capability carrying `Rights::DEBUG` and states the principle
outright — *"never by ambient PID authority"* (26702). `kcmp` is the same shape
of introspection and got no gate.

**What actually leaks, having checked rather than assumed.**

- *Thread membership.* `KCMP_VM`/`FILES`/`FS`/`SIGHAND`/`IO`/`SYSVSEM` all
  collapse to one `same_proc` predicate — identical owning `ProcessId` — so the
  call reports whether any two TIDs on the system are threads of one process.
  Narrower than Linux (we have no separable `files_struct` to share) but still
  another user's private thread layout.
- *Cross-process fd probing — the sharper one.* `KCMP_FILE` calls
  `pcb::linux_fd_lookup(proc_pid, fd)` against **the target's** fd table and
  answers `EBADF` when a descriptor is absent (33507–33527). Walking `idx1` and
  watching `EBADF` flip enumerates exactly which fds another process holds open;
  when both resolve, the returned ordering leaks `handle_kind_ord(kind)`, i.e.
  whether the target's fd 7 is a socket or a file.

**What does *not* leak, recorded so nobody re-derives it.** `kcmp` is usually
also a KASLR concern, because Linux's comparator orders raw kernel pointers and
must launder them through a per-boot cookie (`kptr_obfuscate()`). Ours does not:
it orders `handle_kind_ord(kind)` and `raw_handle` — a handle-table index — and
TIDs in the cross-process case. No kernel address is exposed and the cookie has
no analogue to add. This is an authority bug only.

**Why it is not urgent.** The native syscall table has no number for `kcmp`, so
no binary linked against our libc can call it at all, and `posix/src/process.rs`
`kcmp` is an argument validator that terminates in `ENOSYS`. But "unreachable
because the other ABI happens not to expose it" is not a security property, and
it interacts directly with the open question in
`B-THE-NATIVE-LIBC-AND-THE-LINUX-ABI-DISAGREE-ABOUT-WHAT-EXISTS` step 2 — one of
the calls being considered for a native number is this one. It should not get a
number before it gets a gate.

**Proper fix (lane A's call; the request argues for the first).** Either
require a `Process` capability with `DEBUG` over each target, refusing with
`EPERM` between the `ESRCH` liveness gate and the `type` range gate so the errno
discriminator still matches Linux — noting the predicate is a conjunction over
both targets, so a single undifferentiated `EPERM` is needed or the gate becomes
its own oracle — or, more cheaply, refuse unless both targets resolve to the
caller's own process, which closes both leaks, keeps the only form real programs
use (`kcmp(getpid(), getpid(), …)`) working, and can be widened later. Either
way the existing `kernel_ctx` escape must be preserved or the boot self-test
goes red.

**If never fixed:** a latent cross-process fd-enumeration oracle sits behind a
syscall number we have not assigned, guarded by nothing but the fact that we
forgot to assign it — and the comment above it tells every future reader that it
is gated when it is not.
