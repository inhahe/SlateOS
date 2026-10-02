## 1009. `SA_ONSTACK` is honoured in libc, and storing the stack without using it would have been worse than the stub

**Date:** 2026-09-09
**Lane:** B
**Decided by:** Claude (autonomous)

**In short:** a program can ask that its crash handler run on a separate,
private stack, so it still works when the crash *is* the main stack running
out. We used to accept that request, report it disabled, and run every handler
on the ordinary stack. Now the stack is stored, reported honestly, and actually
used -- the handler really does run on it. One case is still not covered, and
it is the headline one: a handler recovering from a stack overflow, because the
kernel writes its own bookkeeping onto the exhausted stack before any of our
code runs. That last piece is asked of the kernel lane.

### Why the two halves had to land together

`known-issues.md` said the old stub's virtue plainly, and it was right:
"`sigaltstack` reporting `SS_DISABLE` is *honest* -- it is not the `setgroups`
shape, because it declines rather than pretending. A caller that checks gets a
true answer."

That is the argument against the obvious increment. The entry named the libc
half as "actually storing the alternate stack rather than discarding it", and
doing *only* that would have made `sigaltstack` report a registered stack that
no handler would ever run on -- converting an honest decline into exactly the
`setgroups` shape the same document had just praised it for avoiding. A stub
that says no is better than an implementation that says yes and means no.

So the storage and the use are one change. The switch is what makes the report
true.

### Where the switch is, and where it deliberately is not

`__call_on_alt_stack` is an assembly thunk that moves `RSP` around the call to
the **user handler** and nothing else:

```text
push rbp          ; the interrupted stack, in a register the handler must give back
mov  rbp, rsp
mov  rax, rdi     ; handler
mov  rsp, rdx     ; switch
and  rsp, -16     ; align, whatever ss_sp+ss_size was
mov  edi, esi     ; signum becomes arg0
call rax
mov  rsp, rbp     ; back
pop  rbp
ret
```

Everything `dispatch_self_signal` does either side of the call -- the
disposition lookup, `SA_RESETHAND`, the mask bookkeeping -- stays on the
interrupted stack. The point of an alternate stack is to give the *caller's*
code room, not to relocate ours, and a narrower switch is a smaller thing to
get wrong. `RBP` is callee-saved, which is what makes it a safe place to keep
the interrupted stack pointer across a call into code we did not write.

The entry point is `ss_sp + ss_size`, because x86 stacks grow down and the
caller allocated the region upwards. Getting that backwards hands the handler a
stack pointer at the bottom of its own stack, which writes below the allocation
on its first push -- the precise corruption an alternate stack exists to
prevent. It has its own test for that reason.

### Four conditions, and the third is the one that is easy to miss

A handler runs on the alternate stack only when it asked (`SA_ONSTACK`), a
stack is registered, it is big enough (`sigaltstack` already refused otherwise),
and **we are not already on it**. Without the third, a signal delivered during
a handler already running there restarts at the top and overwrites the frames of
the handler it interrupted -- silently, and only under nesting.

### Ten behaviours, measured on Linux 6.6 rather than recalled

Written as a C program and run under WSL before the Rust was written:

| | Linux says |
|---|---|
| fresh | `ss_flags = SS_DISABLE`, `ss_sp = NULL`, `ss_size = 0` |
| registered, not in use | `ss_flags = 0` -- **not** `SS_DISABLE` |
| stack below `MINSIGSTKSZ` | `ENOMEM` |
| ...and after that refusal | the previous registration is intact |
| `SS_AUTODISARM` | reported back as `0x80000000` |
| inside a handler on it | `ss_flags = SS_ONSTACK` |
| changing it from there | `EPERM` |
| after the handler returns | `ss_flags = 0` again |
| `SS_DISABLE` with zero `ss_sp`/`ss_size` | accepted; neither field is read |
| after disabling | `SS_DISABLE`, null, 0 |

Ours now matches all ten. The second row is the one the old code got wrong in a
way nobody would notice: it reported `SS_DISABLE` unconditionally, so a caller
that registered a stack and asked was told it had none.

### A test that asserted a false thing about Linux, and the reason it was believable

`test_phase75_sigaltstack_invalid_new_does_not_corrupt_old` failed when `oss`
stopped being written on error. Its comment gave the ground: "oss is *still*
populated first (Linux behaviour)".

That is true of `do_sigaltstack`, which fills a kernel-local `old` before it
validates anything. It is false of the function a program calls, because
`SYSCALL_DEFINE2(sigaltstack, ...)` copies that local out under
`if (!err && uoss && copy_to_user(...))`. **The internal ordering of a kernel
helper was mistaken for the ABI** -- a reading of the source that was accurate
about the line it read and wrong about the question being asked.

Measured, with `oss` pre-filled and an `ss` carrying garbage flags:

```text
ret=-1 errno=22 (Invalid argument)
oss.ss_sp=0xdeadbeef oss.ss_flags=0xcafe oss.ss_size=2989
```

Untouched, all three fields. The test's own *name* -- "does not corrupt old" --
described the measured behaviour better than its assertions did.

Worth keeping as a shape: reading upstream source is better than remembering
it, and running upstream is better than reading it. This lane has now made the
same class of error twice in one session in the opposite direction, believing a
README over the program it documented; the correction is the same either way.

### What is left, and it is one write

The kernel builds the `SignalContext` on the interrupted thread's stack and
points `RSP` at it before jumping to `__signal_trampoline`. When the interrupted
stack is the one that just overflowed, that store faults inside the kernel --
before libc exists to switch away. Asked of lane A in
`requests/b-a-honour-sa-onstack-when-building-the-signal-frame.md`, along with
the open question that request cannot answer for itself: the kernel has no way
to *see* the registered stack, so this needs either a new syscall or an
extension to `SYS_SIGNAL_REGISTER`, and which of those is right depends on
whether reading user memory during frame construction is safe -- which is lane
A's to judge.

**Against this change, honestly:** it moves `sigaltstack` from "declines
clearly" to "works in most cases and not the famous one", and someone reading
only the function signature will now assume the famous one works too. That is a
real cost and the reason the doc comment leads with the exception rather than
burying it. The alternative -- waiting for the kernel piece and shipping
nothing -- leaves `SA_ONSTACK` as a constant nothing reads for however long
that takes, and leaves the kernel request without a working half to attach to.
