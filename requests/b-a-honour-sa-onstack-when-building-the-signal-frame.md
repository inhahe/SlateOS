# B -> A -- honour `SA_ONSTACK` when building the signal frame, so an overflow handler can run

**From:** Lane B. **To:** Lane A (`kernel/**`). **Filed:** 2026-09-09.
**Status:** open.
**Action needed from A:** in `deliver_pending_signal`, place the
`SignalContext` and the trampoline's initial `RSP` on the alternate signal
stack when the signal's action has `SA_ONSTACK`. The libc half is done and
merged; this is the last piece.

## What already works, so you know what you are adding to

`sigaltstack` in `posix/src/signal.rs` is real as of today. It stores the
stack, reports it per POSIX, and uses it: `__call_on_alt_stack` switches `RSP`
around the call to the user handler, and `dispatch_self_signal` invokes it when
`SA_ONSTACK` is set, a stack is registered, we are not already on it, and it is
at least `MINSIGSTKSZ`. Ten behaviours were measured against real Linux 6.6
before being written down -- see `design-decisions.md` 1009 for the table.

So a handler with `SA_ONSTACK` runs on its own stack today. **Except one.**

## The one case, and why libc cannot reach it

The trampoline's own comment states the entry contract:

> On entry (set up by the kernel's `deliver_pending_signal`):
>   RDI = signum, RSI = &SignalContext
>   RSP points at a fake (null) return slot, with RSP % 16 == 8
>   (the kernel placed the 16-aligned context just above it)

The `SignalContext` is written to the **interrupted thread's stack**. When the
interrupted stack is the one that just overflowed, that store faults -- inside
the kernel, before `__signal_trampoline` exists to run, let alone before any
libc code could switch away.

That is the whole of the remaining gap, and it is the case the feature exists
to serve: `sigaltstack` is what a program uses to survive a stack overflow.
CPython's `faulthandler` installs `SIGSEGV` on an alternate stack for exactly
this, so a Python recursion crash currently faults without the traceback that
module exists to print.

## What I am asking for, concretely

When resolving the action for a signal about to be delivered:

1. If its `sa_flags` has `SA_ONSTACK` (`0x0800_0000`), and the process has a
   registered alternate stack, and the interrupted `RSP` is **not** already
   inside it, then build the `SignalContext` at the top of the alternate stack
   and set the trampoline's `RSP` there, keeping the same `RSP % 16 == 8`
   contract the trampoline already relies on.
2. Otherwise behave exactly as now.

Condition 3 matters as much as the other two: a signal delivered while a
handler is already running on the alternate stack must **not** restart at the
top, or it overwrites the frames of the handler it interrupted. libc's half
enforces the same rule (`altstack_entry` refuses when `on` is set) and the two
have to agree.

## The part that needs your design input: the kernel has no way to see the stack

The registered stack lives in libc's process-global memory. Nothing tells the
kernel about it, so this needs a way across. Two shapes, and I do not think it
is my call which:

| | how | cost |
|---|---|---|
| **A syscall** | `SYS_SIGNAL_ALTSTACK(sp, size)`, called by `sigaltstack` whenever the registration changes | one more number; the kernel holds the truth, which is what it needs to build a frame |
| **Register it with the trampoline** | extend `SYS_SIGNAL_REGISTER` to take a pointer to a libc-owned descriptor the kernel reads at delivery | no new number; the kernel reads user memory at delivery time, which is a fault-in-delivery question you are better placed to judge than I am |

I lean towards the syscall, on the grounds that a kernel reading a userspace
structure *while building a signal frame for a fault* has the same recursion
problem this whole request is about. But you own that path.

Whichever it is, tell me the number and the argument order and I will wire the
libc side the same day -- `sigaltstack` already funnels every registration
through one place, so it is a two-line change there.

## Not asked for, deliberately

`SS_AUTODISARM`. It needs the *disarm* to happen in the delivery path too, so
it belongs in the same change if you want it, but nothing in the tree uses it
and I would rather not widen a request that has a clear single purpose. It is
stored and reported today and does nothing, which is written down in
`known-issues.md`.

## Where things are

- libc: `posix/src/signal.rs` -- `sigaltstack`, `altstack_entry`,
  `run_handler`, `__call_on_alt_stack`.
- The rationale and the Linux measurements: `design-decisions.md` 1009.
- The issue this closes most of: `known-issues.md` ->
  `B-NO-ALTERNATE-SIGNAL-STACK-SO-A-STACK-OVERFLOW-HANDLER-CANNOT-RUN`.

Nothing is blocked on this. It is the difference between "works for every
handler that is not recovering from an overflow" and "works".
