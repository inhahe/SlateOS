## §317 — A libc function with no backend is *composed over the missing primitive*, not stubbed

**Date:** 2026-08-16
**Decided by:** Claude (autonomous)

**In short:** CPython needs three functions — `openpty`, `forkpty`, `login_tty`
— that create a *pseudo-terminal* (a fake keyboard-and-screen pair that lets
one program drive another as if a human were typing at it; this is how `ssh`,
`script`, `expect` and every terminal window work). SlateOS does not have
pseudo-terminals yet. The choice was whether to write these three as
placeholders that pretend to succeed, or to write them properly *now* — as real
code on top of the primitive that is missing, so they fail honestly today and
start working by themselves the day the primitive arrives. We wrote them
properly.

**The two options.**

| | Stub | Compose over the missing primitive |
|---|---|---|
| *What changes:* | `os.openpty()` returns a plausible-looking pair of numbers that are not a terminal | `os.openpty()` fails with `ENOSYS` ("this system call is not implemented"), which is true |
| When the pty layer lands | someone must remember to come back and rewrite all three | they work, with no edit |
| Error a caller sees | invented by the stub | the real one, from `posix_openpt` |
| Cost today | ~10 lines | ~490 lines, five tests |

**Why compose.** The whole point of the CPython spike was to replace a claim
with a measurement. A stub would have restored the claim: the symbol table
would say `openpty` exists, the linker would be satisfied, and the first
program to actually open a terminal would get silent nonsense instead of a
diagnosable failure. `ENOSYS` from `posix_openpt`, propagated verbatim, is a
*better* outcome than a fake success — it names the missing piece and points at
the file that will supply it.

The composition is not speculative. `openpty` is the standard
`posix_openpt` → `grantpt` → `unlockpt` → `ptsname_r` → `open(slave)` dance;
all five already exist as entry points, four of them functional, and only
`posix_openpt` returns `ENOSYS`. `login_tty` is fully functional *today* —
`setsid` + `TIOCSCTTY` + three `dup2`s — and `forkpty` carries musl's
`O_CLOEXEC` synchronisation pipe so a `login_tty` failure inside the child is
reported to the parent rather than yielding a child whose stdio silently is not
the pty. A test pins the property that matters: `openpty` reports
`posix_openpt`'s errno rather than one of its own, so the day `posix_openpt`
stops returning `ENOSYS` the test still passes and the function starts working.

**Against.** Composing costs ~50× the lines of a stub for zero present-day
capability, and it front-loads work whose requirements could still change — if
the eventual pty layer does not expose a `/dev/ptmx`-shaped interface, some of
this glue is rewritten anyway. That risk is small and bounded (the glue is thin
and the interface is 40 years old and specified), and it is the *right* kind of
risk to take: the failure mode is wasted effort, whereas the stub's failure
mode is a program that misbehaves for reasons nothing in the system explains.

**Generalisation.** This is now the rule for the `posix` crate: when a symbol
is required but its backend is not built, implement it against the backend's
real entry point and let the honest error propagate. Do not stub, and do not
`unimplemented!()`. Where the honest behaviour is a *degradation* rather than a
failure — as with `pthread_kill` on a peer thread, which currently delivers
process-directed because the kernel has no per-task pending set — document the
degradation at the call site *and* in `todo.txt` with the kernel change that
would remove it.

**Where it lives.** `posix/src/pty.rs` (new in `5531f816c`).
