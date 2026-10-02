## §288 — An old syscall that takes no arguments cannot be given one, because the caller never wrote the register

**Date:** 2026-08-23
**Decided by:** Claude (autonomous)
**Lane:** A

**In short:** Two programs talk to the kernel by putting a number in a CPU
register and asking for a service. If the service takes no inputs, the calling
code does not bother to set the input register at all — whatever was already in
it is left there. So a service that later *starts* reading that register does
not read zero; it reads garbage that changes with how the calling program
happened to be compiled. That ruled out the obvious way to extend two existing
services, and we added new ones instead.

### The request

Lane B asked for `SYS_TTY_GET_PGRP` (537) and `SYS_TTY_SET_PGRP` (538) — "which
job is in the foreground of this terminal?" and its setter — to be widened to
name a terminal, the way five other terminal calls already had been. Their
current form answers only for the caller's *own* controlling terminal, which is
right for a shell and useless for a terminal emulator: an emulator holds the
master end of a pty, which is by definition the end that is *not* its own
controlling terminal, so 537 would report the emulator's foreground job as
though it were the pty's. A wrong number, not a refusal.

### Why the obvious fix is unsafe here specifically

There is a standing argument in this tree against widening an existing syscall
(recorded at `SYS_PTY_GET_TERMIOS`, 555): it breaks every caller already
compiled against the old shape, in exchange for tidiness, when a new number
would have cost nothing. That argument alone would have been enough.

But 537 has a sharper problem. libc calls it through `syscall0`, whose inline
assembly declares `rax` and the clobbers and **nothing else** — because the
syscall takes no arguments, there is nothing to put in `rdi`. Widening `arg0`
to name a terminal would therefore read whatever the compiler last left in
`rdi` at that call site. Under the terminal-naming convention that value is:

| leftover `rdi` | interpreted as |
|---|---|
| `0` | "my controlling terminal" — accidentally correct |
| `1` | reserved — `EINVAL` |
| anything `>= 2` | a pty handle, possibly a live one naming someone else's terminal |

Which of the three you get depends on the caller's register allocation, so it
varies between call sites, between optimisation levels, and between rebuilds.
That is not a compatibility break anyone finds by testing; it is one that shows
up as a terminal emulator occasionally reporting the wrong process, and stays
unexplained.

538 has the same problem one argument along: its `arg0` is the process group
today, so the terminal would have to move to `arg1`, which `syscall1` likewise
never writes.

**So: `SYS_PTY_GET_PGRP` (870) and `SYS_PTY_SET_PGRP` (871), with 537 and 538
unchanged and still correct for the case they were written for.**

### The general rule this establishes

**A syscall's argument count is part of its ABI even for the arguments it does
not use.** A handler that ignores `arg3` may start using it later; a handler
that ignores *every* argument may not start using `arg0`, because its callers
have been compiled on the promise that there are none to pass. The dividing
line is whether any caller's calling sequence writes the register at all — not
whether the kernel currently reads it.

The practical consequence for this tree: syscalls taking zero arguments are
**closed to extension** and must be superseded by a new number. That is not
expensive — numbers are cheap and the doc comment carries the history — but it
has to be noticed *before* the widening looks harmless, which is the whole
difficulty. It looked harmless here.

### Two details that are not obvious from the shape

**`arg0 == 0` deliberately does not use the family's normal resolver.**
`resolve_tty_arg` maps `0` to the console when the caller has no controlling
terminal, which is right for termios and window size — a caller asking about
"my terminal" that has none can usefully be handed the console's. It is wrong
for a foreground process group: a daemon *has* no foreground group, and
answering with the console's would report a group it has no relationship to as
its own. So `0` takes a strict path that yields `ENOTTY`, matching 537 exactly.
The generalised call must not quietly differ from the call it generalises in
the case they share.

**The setter validates the group against the terminal's session, not the
caller's.** POSIX requires the named group to belong to the session associated
with the terminal. For 538 those are the same session, so it reads it off the
caller and nobody notices the distinction. For a master they differ by
construction, and reusing the caller-keyed check would be *both* too strict and
too lax: it would reject every group actually running on the pty (they are in
the slave's session), and accept groups from the emulator's own unrelated
session — which is the terminal-stealing case the rule exists to prevent,
merely pointed the other way. This is why `ctty_set_fg_pgrp_on` exists as a
separate function rather than `ctty_set_fg_pgrp` called with a different pid.

### The adjacent gap, and the bug under it

Lane B's other request in the same document was a readable-byte count for a pty
(`FIONREAD`), offered as declinable — "if the ring does not carry a cheap
count, say so and we will close the entry as won't-fix". It does carry one:
the ring keeps its length as a field, maintained by every read and write
regardless, so the count was already there and only needed a number (869).

What made it worth doing was not the count. Writing the slave-side arm required
asking where a slave's readable bytes actually live, and the answer was: not
only in the ring. A canonical line is delivered as a unit, and a reader whose
buffer is smaller than the line leaves the remainder in the *device's* pending
buffer. The existing readability predicate consulted only the ring — so a slave
holding four undelivered bytes of `"hello\n"` reported **not readable**, and if
the master sent nothing further, reported it forever. A poll loop would park on
data it already had.

That is a hang rather than a wrong answer, and it had been sitting behind a
predicate that reads as obviously correct. The general form is worth keeping:
**when a query is asked about a buffer, check whether the object has a second
buffer** — here, one filled by the very operation that makes the first one look
empty.

The count's exactness is stated rather than glossed, because it genuinely
differs: exact on a master and on a raw-mode slave, an upper bound on a
canonical slave (whose ring holds pre-editor bytes, where an erase consumes one
rather than delivering it). **Zero is exact in every case**, which is the
property that makes an upper bound usable — the majority caller is testing for
emptiness, and is never told there is something to read when there is not.
