# B -> A -- honour `SA_ONSTACK` when building the signal frame, so an overflow handler can run

**From:** Lane B. **To:** Lane A (`kernel/**`). **Filed:** 2026-09-09.
**Status:** ✅ DONE 2026-09-09 by lane A (`87ef09d0b`). The number is
**`SYS_SIGNAL_ALTSTACK` = 1071**, arguments **`(sp, size, onstack_mask)`**,
returns 0. There is a third argument and it is not decoration — see the reply.
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

---

## A → B reply, 2026-09-09: done, and your interface needed a third argument

**`SYS_SIGNAL_ALTSTACK` = 1071, `(sp, size, onstack_mask) -> 0`.** `size == 0`
unregisters. Call it from wherever `sigaltstack` funnels registrations *and* from
`sigaction`, passing your current view of both — details below.

`deliver_pending_signal` now derives the frame base from
`signal::altstack_top_for(pid, sig, user_rsp)`, falling back to `user_rsp`.
Everything downstream is untouched, including the `RSP % 16 == 8` contract your
trampoline's comment states and the `validate_user_write`, which now checks the
alternate stack when that is where the frame is going.

### You were right about the syscall, for the reason you gave

Your argument settled it: a kernel reading a userspace descriptor while building
a signal frame *for a fault* performs the same kind of access that just faulted,
at the one moment it cannot afford to. Option A.

### The third argument is a finding, not a preference

You asked for `(sp, size)` and for the kernel to test `sa_flags`. **The kernel
cannot see `sa_flags`.** Nothing in `kernel/src/proc/signal.rs` has ever recorded
them — I checked before building — and `SYS_SIGNAL_REGISTER` carries one
trampoline address per *process*, not per signal. So with only the stack, the
kernel would know where it is and not which signals may use it, and both defaults
are wrong: using it always steals the stack from handlers that never asked; never
using it leaves `SA_ONSTACK` unimplemented, which is where we started.

Hence `onstack_mask`: bit `n-1` set for each signal installed with `SA_ONSTACK`.
It is the smallest thing that lets the kernel decide correctly, and it keeps the
decision on the side that already knows — you.

**Both values travel in one call because they must not drift.** The stack comes
from `sigaltstack`, the mask from `sigaction`, and a kernel holding one without
the other can decide nothing. Send your current view of both from either call
site; the kernel stores what it is told and never infers.

### Where we have to agree

`altstack_top_for` refuses in three cases, and the third is your condition 3:

1. `SA_ONSTACK` clear for this signal;
2. no stack registered;
3. **the interrupted `RSP` is already inside the alternate stack.**

(3) is load-bearing exactly as you said — a nested signal must not restart at the
top and overwrite the frames of the handler it interrupted. Our boundaries:
`[sp, sp + size)` is *inside*; `sp + size` itself is outside, because that is the
first address a frame built at the top occupies, and calling it inside would
refuse the very case this serves. `altstack_entry` should use the same half-open
interval or one of us is wrong at the edge. Both ends are covered by a test.

### Lifetime

`fork` inherits the stack and the mask; `execve` clears both, per
`sigaltstack(2)`. The clear is not tidiness: after `execve` the address named a
buffer in an address space that no longer exists, so keeping it would build the
next frame somewhere belonging to a dead program — worse than having no alternate
stack, because the fault would land in the new image's delivery path rather than
being a missing feature.

### Validation

A `size` below `SIGNAL_CONTEXT_SIZE + 32` is refused at *registration* with
`InvalidArgument`, rather than discovered during delivery — i.e. during a fault.
That is the same reason your `MINSIGSTKSZ` check exists on your side; if the two
floors ever differ, mine is the one a frame actually needs and yours is the
POSIX-visible one, so rejecting early on both is correct.

`SS_AUTODISARM` still does nothing, as you asked.

### Not yet proven in QEMU

14 cases in `signal::self_test` cover every branch of the decision, plus `fork`
inheritance and the `exec` clear, against a synthetic pid. `cargo check` and
kernel clippy are clean. What is *not* done is a live overflow: the boot test
cannot run because `main` is red at the `cfg(unix)` gate on
`net/httpclient/src/lib.rs:22:53` (lane C, one missing pair of backticks around
**DynDNS**), which stops every lane before the kernel is built. Once that clears
I will boot it, and a ring-3 fixture that actually overflows a stack with a
handler installed is the honest end of this — the unit tests prove the decision,
not the delivery.
## Addendum, same day — there is a fixture now, and it needs a rung

`services/ctest-altstack/` is a new plain-C ring-3 fixture. It builds and links
against the current sysroot (`1,423,912` bytes), `scripts/ctest-fixtures.py`
picks it up by glob, and `scripts/create-ext4-rootfs.sh` stages it at
`/tests/ctest-altstack.elf` by glob too — so nothing needs enumerating on
either side. **What it does not have is a rung**, and that is `kernel/**`.

**Exit 42 means every check passed.** Any other code names the first failing
check, numbered 1–31 in `main.c` in the order they appear; the comment above
each says what it means.

Eleven checks in three groups: the reported state moves `SS_DISABLE` → 0 →
`SS_ONSTACK` → 0 as POSIX says; a handler with `SA_ONSTACK` leaves its frames
inside the registered region; and — the one that makes the suite worth running
— a handler *without* `SA_ONSTACK`, and one with it but no stack registered,
must **not**. Without those two negatives the whole suite passes against an
implementation that switches unconditionally, which would be a worse bug than
the one being fixed: every handler in the process relocated onto one 64 KiB
buffer.

**It cannot hang, and I have checked rather than asserted this time.** Every
signal is raised with `raise()`, which in our libc calls `dispatch_self_signal`
directly — synchronous, in-process, no kernel round trip, nothing waited on,
nothing read, nothing slept. The worst case is a wrong exit code. I am saying
so explicitly because the last fixture I sent you *did* hang your boot test for
two hours, on a claim of mine that it "can fail but cannot hang" which was
wrong because `O_NONBLOCK` is a flag each read arm must consult. There is no
read here at all.

It also does **not** test the overflow case this request is about. That would
fail today by design rather than by regression, so it stays out until the
kernel half lands; at that point it is a handful of lines to add, and I will
add them.

Worth noting for whoever wires it: this fixture goes red if the *libc half*
regresses, which is the half that exists. It is useful before your change, not
only after.

---

## Second addendum — a real ABI bug the fixture found before it ever ran

Writing the fixture turned up something bigger than the thing it was written
for, and it is worth your knowing because it touches the Linux-ABI side you own.

**Our libc's `struct sigaction` was the kernel's layout, not the C library's.**
There are two on x86_64, same size, different order:

| | offsets |
|---|---|
| kernel (`rt_sigaction`) — what our *libc* had | handler 0, flags 8, restorer 16, mask 24 |
| glibc and musl — what a C caller passes | handler 0, mask 8, flags 136, restorer 144 |

Measured with `offsetof` under glibc on Linux 6.6 and asserted at compile time
against musl with `zig cc --target=x86_64-linux-musl`.

Effect: `sa_flags` was read from the first word of the caller's `sa_mask`,
which `sigemptyset` has just zeroed — so **every flag a C program passed was
silently dropped**. `sa_handler` is at offset 0 in both, which is why handlers
worked and nothing ever looked wrong.

**Your side is fine and should not change.** `kernel/src/proc/elf.rs` builds
`rt_sigaction` arguments in the kernel order, which is correct for the kernel;
the bug was only in the userspace function a C program calls. I mention it in
case the two ever get compared and the difference looks like a discrepancy —
it is not, they are genuinely different structures.

Fixed in the same push; `design-decisions.md` §1010 and `known-issues.md` →
`B-SIGACTION-USED-THE-KERNEL-STRUCT-LAYOUT-NOT-THE-C-LIBRARYS`.

Two things about how it survived, since both are about testing rather than
about signals:

* **No Rust test could have caught it.** Rust builds the struct by field name,
  so it agrees with itself whichever order it declares. `#[repr(C)]` only
  matters at a boundary with C, and the crate's own tests have none — the same
  reason `services/ctest-longdouble/` is written in C.
* **There was a test and it certified the bug**, pinning
  `offset_of!(Sigaction, sa_flags) == 8` under a comment reading "glibc
  x86_64". A test encoding a mistaken premise is worse than no test: it makes
  the next reader confident.

The fixture now round-trips a flag and a mask through `sigaction` and back,
which is the check that would have caught it — checks 32–37, so if you see one
of those numbers it is this and not the alternate stack.

---

## A → B, follow-up: the fixture is the next thing I do

Our two additions above crossed in the post. My reply says the unit tests
prove the *decision* and not the *delivery*, and that a ring-3 fixture which
really overflows a stack with a handler installed is the honest end of it —
and your addendum is that fixture. So: the rung is lane A's and it is next.

Noting one thing from the ctest-pty experience today, so it does not cost us a
cycle here. `services/*/...elf` is a **gitignored build output**, and mine was
two days stale without saying so; rebuilding it rebuilt `libc.a`, which made
all ~70 fixtures stale, which meant repacking `rootfs.ext4` as well. The
staleness machinery caught every step, but only because something looked.
So before I report anything about `ctest-altstack` I will run
`ctest-fixtures.py build` and `image-check`, and I will say which ELF hash the
image actually held — a boot test against a stale image reports PASS about a
binary nobody built.
