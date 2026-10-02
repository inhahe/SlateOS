## B-THE-PINNED-FAST-PATH-DOWNGRADED-ITS-FIRST-REAL-REFUSAL-TO-THE-RACY-ROUTE — FIXED 2026-08-31

**Status:** FIXED. Recorded because the hole existed on `main` from 2026-08-24
(the landing of syscall 662) until 2026-08-31, and because the *shape* of the
mistake — inferring a fact the other side could have simply told us — is worth
having written down somewhere other than a commit message.

**In short:** the POSIX layer has a fast path that asks the kernel to do a
`unlinkat`/`fstatat`/`chmod`/… against a directory *handle* rather than a
remembered path, so that swapping the directory mid-operation cannot redirect
it. When such a call came back refused, this side could not tell "your kernel is
too old to have this call" apart from "the handler ran and this filesystem
cannot do it" — both were error code -2 — so it guessed. On the guess's wrong
side, a filesystem's honest refusal was quietly retried by path name, which is
exactly the race the fast path exists to close.

**Where it lived.** `posix/src/file.rs`, `fn pinned_answer`, plus seven
`PINNED_*_ANSWERED` statics beside it.

**The guess.** Each syscall carried its own latch, `false` until that syscall
returned anything other than -2, `true` forever after:

| latch | -2 arrives | meaning taken | what happened |
|---|---|---|---|
| never set | first call | "kernel too old" | fall back to the path-based route |
| set | any later call | "filesystem refused" | returned to the caller as the answer |

The reasoning was sound as far as it went — a kernel that *has* the call answers
the first invocation with a success or a real error, which flips the latch, so
every subsequent -2 is honoured. The defect is the window before that first
answer. On a kernel that has the call but a filesystem that cannot perform it,
*every* invocation is -2, the latch never flips, and every invocation falls back
by path. Lane A's `dispatch.rs` comment states the cost exactly: the call "gets
silently downgraded to the racy path-based route, on the failure path, where
nobody is looking."

**Two further costs, both smaller but real.** The behaviour differed between the
first invocation and every later one, so a test could pin only one of the two at
a time and the suite's result depended on call ordering. And each of the seven
syscalls needed its own latch — 662's first success must not vouch for 663 —
which is seven statics to keep in step by hand as the family grew.

**What fixed it.** Lane A split the two facts onto two numbers on 2026-08-31:
an *empty dispatch slot* now answers `NoSuchSyscall` (-10), a *registered
handler that refused* still answers `NotSupported` (-2). `pinned_answer` became
a comparison against -10 (plus `HOST_ENOSYS`, which is the host build's
compiled-out `SYSCALL` reporting itself), the seven statics and the `answered`
parameter are gone, and `only_an_empty_dispatch_slot_falls_back` pins the rule
in both directions with no ordering to arrange.

**The general lesson.** The old code inferred, by observation over time, a fact
the kernel already knew at the moment it answered. Any such inference has a
window in which it is wrong, and the window is invisible because it closes
itself. When two conditions need different responses, ask for two answers rather
than building a heuristic to separate one — and if the other side is another
lane, file the request. That is what happened here, and it took a day.
