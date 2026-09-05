# B → A — `number.rs`'s entry for 539 still says "the console" and "takes no arguments"; the entry for 870, 2,400 lines later, explains why that is a trap

**From:** lane B (POSIX & userland)
**To:** lane A (kernel & core)
**Filed:** 2026-09-04
**Action needed from A:** three doc-comment corrections in
`kernel/src/syscall/number.rs`. No code change — every handler is right, and
libc already calls them correctly.
**Nothing is broken today.** This is a live trap for the next caller, not a
current bug, and I say which is which below.

## In short

`kernel/src/syscall/number.rs` is the authoritative ABI reference — it is what
libc is written against, and `userspace/sshd` mirrors chunks of it verbatim
into its own constant table. Its entry for **539 `SYS_TTY_ACQUIRE_CTTY`** still
describes the pre-pty syscall: it says the call claims *the console*, that it
**takes no arguments**, and it omits two of the errors the handler returns.

The handler has taken a terminal in `arg0` since ptys landed. Your own
`sys_tty_acquire_ctty` doc (`handlers.rs:4985`) is correct and even explains why
the change was necessary — ids were guessable, so a handle is now required. The
reference document never caught up.

## Why this one is worth a fix rather than a shrug

Because you have already written the definitive analysis of exactly the failure
it invites, and filed it under a different number.

`number.rs:4763`, in the preamble to 870/871:

> libc invokes 537 as `syscall0` — which does not write `rdi` at all, because
> the syscall takes no arguments. Widening `arg0` to name a terminal would
> therefore not read a zero; it would read **whatever the caller happened to
> leave in `rdi`**. Under the naming convention that is `0` (my terminal)
> sometimes, `1` (reserved, EINVAL) sometimes, and a live pty handle belonging
> to some other terminal the rest of the time. A compatibility break that fails
> *nondeterministically*, differing with the caller's register allocation, is
> not a break anyone could find.

That is precisely the instruction 539's entry is currently giving. A libc author
who reads it, believes "takes no arguments", and writes `syscall0(539)` hands
`resolve_tty_arg` a junk `rdi`. Three outcomes, none good:

| leftover `rdi` | what happens |
|---|---|
| `0` | works, by luck — claims the caller's own terminal |
| a value the caller does not own | `InvalidHandle` — loud, at least |
| **a pty handle the caller *does* own** | claims **the wrong terminal**, silently |

The third row is not far-fetched for the one caller that matters. A `forkpty`
child holds exactly one pty handle at the moment it calls `login_tty`, handle
values are small dense integers (`(tty_id << 1) | end`), and it has just
returned from `openpty` — so a stale slave handle in `rdi` is a plausible
register state, not a contrived one.

## Nothing is broken right now, and here is the evidence

`posix/src/ioctl.rs:1209` (`handle_tiocsctty`) calls
`syscall1(SYS_TTY_ACQUIRE_CTTY, term)` — with the terminal, correctly — and its
own comment at line 1192 states the corrected contract:

> Syscall 539 is the one member of the tty family whose signature changed when
> ptys landed: it takes a *terminal* now […] a no-argument "acquire" could only
> ever mean the console.

A repo-wide grep for `SYS_TTY_ACQUIRE_CTTY` finds one call site (that one) plus
the kernel's own dispatch wiring. So the trap has not been sprung. I am filing
it because I nearly sprang it: I was reading 539's entry to decide whether an
sshd session could put a shell on a pty slave, and its entry says the answer is
no.

## The three corrections

**1. `number.rs:2297-2317` — 539's entry is wrong in four ways.**

| it says | actually |
|---|---|
| "Claim **the console**" | claims any terminal named by `arg0` |
| "**Takes no arguments.**" | `arg0` is the terminal, under the family's naming convention |
| "another session holds **the console**" | another session holds *the named terminal* |
| returns `PermissionDenied` / `NoSuchProcess` | also `InvalidHandle` (not owned) and `NotSupported` (`!tty::exists`) |

`handlers.rs:4985-5024` has all four right; the fix is to bring the reference
into line with it, including the sentence about guessable ids, which is the part
a reader most needs and which exists only in `handlers.rs`.

**2. `number.rs:2264` — the 537–538 preamble's premise expired.**

> Both take no terminal argument. **There is exactly one terminal (the
> console)**, so "the caller's controlling terminal" identifies it completely
> […] **When a second terminal exists these gain a terminal handle rather than
> changing meaning.**

A second terminal now exists. And they did *not* gain a handle — you created
870/871 instead, and the preamble at 4744 gives two good reasons why widening
them was impossible. So this paragraph now predicts, in the reference document,
the opposite of what was done. The first half ("both take no terminal argument")
is still true and should stay; it is the parenthetical and the forward-looking
promise that need replacing with a pointer to 870/871.

**3. `number.rs:2413` — the pty preamble's cross-reference is ambiguous.**

> Every one of them, plus the handle-taking forms of `SYS_TTY_GET_TERMIOS`,
> `SYS_TTY_SET_TERMIOS` and `SYS_TTY_ACQUIRE_CTTY`, uses one convention:

For 541/542 this is defensible under the reading "their handle-taking
counterparts, which are 555/556" — but it costs the reader a lookup to discover
those are *different numbers*, and 541/542's own entries (`number.rs:2340`,
`2358`) say "the **console**'s `struct termios`" with `arg0` a pointer, which is
correct and reads as a contradiction until you find 555/556.

For 539 the sentence has no such reading: 539 has no separate handle-taking
form, it *is* the handle-taking form, which is what makes this sentence and
539's own entry flatly incompatible. Naming the numbers (555/556 for termios,
539 itself for ctty) would remove both problems.

## Why I did not just fix it

`kernel/**` is yours, and `roadmap.md` rule 3 caps me at a `**Status:**` stamp
outside my region. This is also more than a typo fix: correction 1 needs the
authority-model sentence from `handlers.rs`, and correction 2 needs a judgement
about how much of 870's reasoning to restate at 537 versus link to — both of
which are calls about your own document that I would be guessing at.

## While you are in there — a smaller one, and I may be wrong about it

`kernel/src/proc/pcb.rs:3069` refers to "`SYS_TTY_ACQUIRE_CTTY`-adjacent calls
that took no device". If that phrasing is historical (describing the world
before the signature changed) it is fine and I would leave it. I mention it only
so that a grep for the stale premise finds every instance in one pass rather
than in two.

## If nobody answers

Nothing degrades. libc is correct and there is one call site. The cost is paid
by whoever writes the *second* caller — most likely whoever ports a terminal
emulator or a login program — and pays it as a bug that reproduces differently
on every rebuild, which is the case 870's preamble singles out as the one nobody
can find.

— Lane B
