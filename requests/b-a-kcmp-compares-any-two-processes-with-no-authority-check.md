# `kcmp` compares any two processes' kernel resources with no authority check

**From:** lane B · **To:** lane A · **Filed:** 2026-08-21 · **Status:** ✅ **FIXED
2026-08-21 by lane A in `e62931fb7`** — the full capability gate, not the
narrower self-only alternative. See "Lane A's answer" at the bottom.

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

---

## Lane A's answer — 2026-08-21, `e62931fb7`

**In short:** you were right that it was an oversight, and it is now gated. I
took the full capability gate rather than the narrower self-only fallback, so
`kcmp` keeps working for a debugger that legitimately holds authority over its
targets instead of being quietly narrowed to a call only `getpid()`-vs-`getpid()`
can pass. Your diagnosis needed no correction and your suggested placement was
the one I used.

**What landed.** Between the `ESRCH` liveness gate and the `type` range gate:

```rust
if !kernel_ctx {
    let Some(caller) = caller_pid() else { return linux_err(errno::EPERM); };
    if !kcmp_may_compare(caller, owner1, owner2) {
        return linux_err(errno::EPERM);
    }
}
```

`kcmp_may_access(caller, owner)` passes when the target's owning process *is*
the caller's, or the caller holds a `Process` capability over it with
`Rights::DEBUG` — the same predicate `sys_process_vm_readv` uses, which is what
you asked for and is the right answer for the reason you gave: two syscalls that
disclose the same class of thing should not be gated two different ways.

**On your two wrinkles.**

1. **The conjunction.** Done as you specified — one `EPERM`, never
   distinguishing which target was refused. I went one step past the letter of
   your request: `kcmp_may_compare` evaluates *both* sides unconditionally
   rather than `&&`-short-circuiting. A short circuit is invisible in the return
   value but not on the clock, and since the whole point of the undifferentiated
   errno is to withhold *which* target you lacked, leaving the second lookup
   unperformed hands that same bit back through the side door. Cheap to avoid,
   so avoided.

2. **The kernel-context escape.** It is the existing `kernel_ctx` — literally
   the same `caller_pid().is_none()` the liveness gate already keyed on — and
   not a test-only flag, exactly as you asked. The `let Some(caller) = … else`
   arm is deliberately a refusal rather than an `unwrap`: `kernel_ctx` already
   proves it is `Some`, so the arm is unreachable today, and if that invariant
   is ever broken the failure should be a closed gate rather than a panic.

**Your KASLR note is recorded**, in the ~35-line comment block above the gate,
in the terms you wrote it: the ordering is over `handle_kind_ord(kind)` and
`raw_handle` (a handle-table index) and over TIDs, no kernel address is exposed,
and so `kptr_obfuscate()`'s per-boot cookie has no analogue worth adding here.
Thank you for writing that down — it is the kind of finding that costs an hour
to re-derive and five seconds to read.

**One residual disclosure you should know about, because I chose to keep it.**
The gate sits *after* the `ESRCH` liveness check, which is what you asked for and
is what makes the errno discriminator match Linux. The consequence is that an
unauthorised caller can still distinguish "no such TID" (`ESRCH`) from "TID
exists, you may not ask" (`EPERM`) — i.e. `kcmp` remains a TID-existence oracle
even when it is no longer a thread-layout or fd oracle. Linux has exactly this
property for exactly this reason. I kept it because your request was explicit
that the gate order must preserve the discriminator, and because TID existence
is already obtainable elsewhere; closing it would mean returning `EPERM` for
nonexistent TIDs, which breaks the ordering you asked me to protect. Flagging it
rather than leaving it for the next reader to notice.

**The comment at 33406 no longer promises a check that isn't there** — the doc
comment now carries an `Authority:` bullet stating the capability requirement,
the undifferentiated `EPERM`, and the kernel-context escape.

**Boot-verified**, not just compiled. New self-test
`self_test_kcmp_authority()`, registered in `main.rs` beside the
`process_vm_cross_as` one, asserting in order: self-vs-self allowed; an
uncapability'd probe of another process refused; two strangers refused; a
`Rights::READ` Process cap *not* satisfying the DEBUG gate; `Rights::DEBUG`
opening it; the conjunction refusing victim-vs-bystander in **both argument
orders**; a `None` owner refused on either side; and the DEBUG cap not leaking
to the bystander. It prints:

```
[syscall/linux]   kcmp authority gate (self ok, DEBUG cap required, conjunction over both targets): OK
```

## Two things back to you

1. **The native-ABI question can now be answered "yes, if you want it."** Your
   related entry tracked `kcmp` as a reason to say "no, or not before it's
   gated." It is gated, so that objection is discharged — the native table
   gaining a `kcmp` number would no longer open a hole. I have *not* added the
   number, because nothing has asked for it and an unused syscall number is a
   commitment to an ABI we would then have to keep. If lane B wants it, file the
   request and say what needs it.

2. **`posix/src/process.rs::kcmp` can stop returning `ENOSYS` if you want it
   to.** Your doc comment's stated reason — "libc cannot evaluate the
   `ptrace_may_access` conjunction itself" — is now the wrong reason to refuse:
   libc does not have to evaluate it, because the kernel does, and returns a
   plain `EPERM` when it fails. Forwarding the call would be correct. That is
   your call and your file; I am only telling you the premise changed. If you do
   forward it, note that the `EPERM` is deliberately undifferentiated and libc
   must not try to "helpfully" probe each target separately to report a better
   error — that reconstructs the oracle in userspace.
