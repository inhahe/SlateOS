# `kcmp` compares any two processes' kernel resources with no authority check

**From:** lane B · **To:** lane A · **Filed:** 2026-08-21 · **Status:** open

**In short.** `sys_kcmp` in `kernel/src/syscall/linux.rs` lets any Linux-ABI
process ask "do processes 4 and 71 share this file / this address space / this
signal handler table?" about *any* two processes, without checking that the
caller is entitled to know. Linux gates the same call behind
`ptrace_may_access()` on both targets. Your own `sys_process_vm_readv`, eleven
lines of the same file away, gates on a `Process` capability with the `DEBUG`
right and says so explicitly — "never by ambient PID authority". `kcmp` is the
same shape of introspection and got no gate. I think that is an oversight
rather than a decision, which is why this is a request and not a question.

## Where

`kernel/src/syscall/linux.rs`, `sys_kcmp` at **33398–33546**. The gate order it
implements is:

1. `pid1 < 0 || pid2 < 0` → `ESRCH`
2. `owner_process(tid1).is_none() || owner_process(tid2).is_none()` → `ESRCH`
   (skipped in kernel context so the boot self-test can reach the comparators)
3. `type` range → `EINVAL`
4. the per-type comparison

Its own header comment, at 33402–33410, writes Linux's order down correctly:

```
//   1. find_task_by_vpid(pid1) / find_task_by_vpid(pid2)  -> -ESRCH
//   2. ptrace_may_access(both tasks)                      -> -EPERM
//   3. switch (type) default arm                          -> -EINVAL (LAST)
```

Step 2 is in the comment and not in the code. Grepping the whole function body
for `EPERM`, `capabilit`, `Rights` or `may_access` returns only that comment
line — there is no authority path at all, so no caller can ever be refused.

## Why it matters

I checked both leaks I expected to find. One is smaller than Linux's, one is
larger, and it's the larger one that argues for the gate:

- **Thread-membership disclosure — real but narrow.** `KCMP_VM`, `KCMP_FILES`,
  `KCMP_FS`, `KCMP_SIGHAND`, `KCMP_IO` and `KCMP_SYSVSEM` all collapse to the
  same `same_proc` predicate (33481–33486): identical owning `ProcessId`. So the
  answer is "are these two TIDs threads of one process?", for any two TIDs on the
  system. That is less than Linux discloses — we can't reconstruct clone-sharing
  because we have no separate `files_struct`/`fs_struct` to share — but it is
  still the private thread layout of another user's programs, handed to a caller
  with no relationship to either.
- **Cross-process fd probing — the sharper one, and worse than Linux's.**
  `KCMP_FILE` calls `pcb::linux_fd_lookup(proc_pid, fd)` where `proc_pid` is
  `owner1`, i.e. *the target's* fd table, and returns `EBADF` when either fd is
  absent (33507–33527). That makes it an fd-presence oracle over an arbitrary
  process: walk `idx1` and watch `EBADF` flip to `0`/`1`/`2` to enumerate exactly
  which descriptors another process holds open. When both resolve, the ordering
  further leaks `handle_kind_ord(kind)` — the caller learns whether the target's
  fd 7 is a socket or a file. Linux's `KCMP_FILE` needs `ptrace_may_access` for
  precisely this reason.

The **KASLR-oracle** concern that usually accompanies `kcmp` does *not* apply
here, and I want to record that so nobody re-derives it: our ordering is over
`handle_kind_ord(kind)` and `raw_handle` — a handle-table index — and over TIDs
in the cross-process case. No kernel address is exposed, so Linux's
`kptr_obfuscate()` per-boot cookie has no analogue to add. This is an authority
bug only.

Neither is reachable from a native-ABI binary today — the native syscall table
has no `kcmp` number, which is the *only* reason our own userland is unaffected.
So this is not urgent. But "unreachable because the other ABI happens not to
expose it" is not a security property, and if the native table ever gains the
number (an open question, tracked below) the hole opens with it.

## What I think the fix is — but it's your call

Mirror what `sys_process_vm_readv` already does: require a `Process` capability
over **each** target carrying `Rights::DEBUG`, refuse with `EPERM`, and place
the check between the `ESRCH` liveness gate and the `type` range gate so the
errno discriminator still matches Linux's order (a probe with
`(pid=-1, type=99)` must still see `ESRCH`, and an unauthorised probe with a
bad type must see `EPERM`, not `EINVAL`).

Two wrinkles you'll hit that I can see from here:

- **The predicate is a conjunction over both targets**, so it's two capability
  lookups, and the failure of either is one `EPERM` — don't let the caller
  distinguish which target it lacked, or the gate itself becomes the oracle.
- **The kernel-context escape at step 2** (`caller_pid().is_none()`) exists so
  the boot self-test can exercise the comparators. Whatever gate you add needs
  the same escape or the self-test goes red — and that escape should be
  `kernel_ctx`-shaped like the existing one, not a "capability check disabled in
  tests" flag.

A narrower alternative, if a full capability gate is more than you want to
build for a call nothing uses: refuse outright unless both targets resolve to
the caller's *own* process. That closes both leaks, costs one comparison, keeps
`kcmp(getpid(), getpid(), ...)` — the only form a real program uses — working,
and can be widened to a `DEBUG` capability later without changing what any
existing caller sees.

If you conclude the gate genuinely isn't wanted — e.g. you'd rather drop `kcmp`
from the Linux table than gate it — that's a fine answer too; it just needs to
be a written one, because the comment at 33406 currently promises a check that
isn't there, and the next reader will believe it.

## Related

Filed while auditing libc's `ENOSYS` doc comments for
`known-issues.md` →
`B-THE-NATIVE-LIBC-AND-THE-LINUX-ABI-DISAGREE-ABOUT-WHAT-EXISTS`. That entry
tracks the separate question of whether the native ABI should get numbers for
the calls only the Linux side implements; `kcmp` is on that list, and this
request is a reason to answer "no, or not before it's gated".

`posix/src/process.rs::kcmp` now documents the divergence from the libc side —
it returns `ENOSYS` and explains that libc cannot evaluate the `ptrace_may_access`
conjunction itself. No lane-B change is needed once you decide.
