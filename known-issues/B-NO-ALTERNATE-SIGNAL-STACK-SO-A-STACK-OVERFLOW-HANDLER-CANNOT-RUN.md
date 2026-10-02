## B-NO-ALTERNATE-SIGNAL-STACK-SO-A-STACK-OVERFLOW-HANDLER-CANNOT-RUN (lane B, 2026-09-07) -- MOSTLY FIXED 2026-09-09

**In short:** a program can ask that its crash handler run on a separate, small,
private stack, so that it still works when the crash *is* the main stack running
out. We accept the request, report it disabled, and run every handler on the
ordinary stack. For most handlers this is invisible. For the one case the
feature exists to serve -- a stack overflow -- the handler cannot run at all.

**Where.** `posix/src/signal.rs`, `sigaltstack` (reports `SS_DISABLE`) and
`__signal_trampoline`, which pushes the saved-context pointer and calls
`__signal_dispatch` with no stack switch anywhere in the path. `SA_ONSTACK` is
a defined constant that nothing reads.

**Severity is genuinely low, and the reason is worth stating** so nobody
promotes this on the strength of the title. `sigaltstack` reporting `SS_DISABLE`
is *honest* -- it is not the `setgroups` shape, because it declines rather than
pretending. A caller that checks gets a true answer. What is lost is only the
overflow case: an ordinary `SIGSEGV`, `SIGFPE` or `SIGINT` handler runs
perfectly well on the interrupted stack.

**Who trips it.** CPython's `faulthandler` installs `SIGSEGV` on an alternate
stack precisely to survive recursion-depth crashes; it will get the disposition
recorded and no alternate stack behind it, so a genuine Python stack overflow
faults without the traceback that module exists to print. Relevant to the
interactive-CPython work, which is why it was found.

**The proper fix** is for the kernel's `deliver_pending_signal` to honour
`SA_ONSTACK` by switching to the registered stack before entering the
trampoline, which makes it partly lane A's. ~~Not filed as a request yet: the
libc half (actually storing the alternate stack rather than discarding it) has
to exist first, and that is this lane's and not written.~~

### The libc half is written -- 2026-09-09, and it does more than store

`sigaltstack` now stores the stack, reports it per POSIX, and **uses** it: a
handler registered with `SA_ONSTACK` runs on it. `posix/src/signal.rs` gained
`__call_on_alt_stack`, an assembly thunk that switches `RSP` around the call to
the user handler and nothing else, and `dispatch_self_signal` calls it when all
four conditions hold (asked for, registered, not already in use, big enough).
See `design-decisions.md` 1009.

**Storing it without using it would have been worse than the stub**, which is
why the two landed together. This entry says the old behaviour was *honest* --
"it declines rather than pretending" -- and that is exactly right. A version
that recorded the stack and reported it back while still running every handler
on the interrupted stack would have converted an honest decline into the
`setgroups` shape.

~~**What is left is one kernel write.**~~ The kernel builds the `SignalContext`
on the interrupted thread's stack and points `RSP` at it before jumping to
`__signal_trampoline`, so when the interrupted stack is the one that just ran
out, the fault happens in the kernel's own store -- before any libc code exists
to switch away from it. Filed as
`requests/b-a-honour-sa-onstack-when-building-the-signal-frame.md`.

### Both halves are connected -- 2026-09-10

Lane A landed `SYS_SIGNAL_ALTSTACK` (1071), and `posix` now calls it. The
kernel could not do this alone and the reason is worth keeping: **it builds
signal frames and has never recorded `sa_flags`** -- those live in libc. So it
cannot tell whether the signal it is about to deliver asked for the alternate
stack, and either default is wrong. Always using it steals the stack from
handlers that never asked; never using it leaves `SA_ONSTACK` unimplemented for
every frame the kernel builds, which is exactly the stack-overflow case the
feature exists for.

So the syscall reports **both** halves together -- `(sp, size, onstack_mask)`,
the stack from `sigaltstack` and the mask from `sigaction` -- because a kernel
holding one without the other can decide nothing. `posix::publish_altstack`
calls it after either half changes.

**The result is deliberately ignored**, which is the one judgement call here. A
kernel without the syscall answers `ENOSYS`, and that must not make
`sigaltstack` fail: the userspace half still works, since `run_handler`
switches stacks for every signal libc delivers itself. Failing the call would
take away a working feature to punish a kernel for not having a newer one. The
kernel's own error cases -- a size too small for a frame, an `sp + size` that
overflows -- are both rejected by `sigaltstack` before the syscall is reached,
so a rejection would mean the two validations disagree, which is a bug in one
of them rather than something to report to the application.

So the title is now too broad. There *is* an alternate signal stack; what a
stack-overflow handler still cannot do is reach it.

**Also not done: `SS_AUTODISARM` is stored and reported, not acted on.** It
exists so that `siglongjmp` out of a handler does not leave the stack
permanently marked in use, and without it we have the footgun Linux has: a
handler that jumps out never reaches the line clearing the flag, so
`sigaltstack` reports `SS_ONSTACK` for ever after and further changes are
`EPERM`. Honouring it means disarming in the delivery path, which is where the
kernel work above lands, so it waits for the same request rather than being
half done.
