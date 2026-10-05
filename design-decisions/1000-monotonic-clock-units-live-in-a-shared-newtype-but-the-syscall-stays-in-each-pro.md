## 1000. Monotonic-clock units live in a shared newtype, but the syscall stays in each program

**Date:** 2026-09-05
**Lane:** B
**Decided by:** Claude (autonomous)

**In short:** two programs broke on the same day because a number of nanoseconds
was stored in a variable called "milliseconds" and then treated as milliseconds.
`sshd` locked every user out; `inetd`'s flood limiter stopped limiting. The
obvious repair — rename the variables — fixes today's two and leaves the next one
available, because a name is not checked by anything. So the unit was moved into
the type system: a new crate, `monoclock`, where a clock reading (`Instant`) and
a length of time (`Elapsed`) are different types and mixing them is a compile
error. The decision recorded here is the *second* half, which had a genuine
tradeoff: the new crate deliberately does **not** read the clock. Each program
still makes its own syscall and hands the raw number over in one place. See
`known-issues.md`
`B-EVERY-PROGRAM-HAD-ITS-OWN-MONOTONIC-CLOCK-AND-HALF-GOT-THE-UNIT-WRONG`.

**Jargon, once:** a *newtype* is a wrapper around a plain number that the
compiler treats as a distinct type, so the number cannot be used where a
different kind of number is expected. `no_std` means a crate that does not use
the standard library, which is what lets bare-metal code — code that runs with no
operating system underneath it — use it. An *rlib* is a compiled Rust library
linked into a program.

| Option | *What changes:* | Why not |
|---|---|---|
| **Types only; each program keeps its own syscall** (chosen) | Every program keeps its existing four-line clock reader, and adds one line naming the unit: `Instant::from_nanos_since_boot(raw)`. | The syscall shim itself is still written out per program — five copies of four lines. |
| One `monoclock::now()` that makes the syscall | Programs delete their clock shim entirely and call one function. | Cannot be linked off-target, so the host tests — the only tests that exist for the daemons — could not use it; and it would have to pick one failure behaviour for callers that correctly disagree. |
| Put the types in `posix` | No new workspace member. | Depending on the `posix` rlib links a second libc whose syscalls all answer `-ENOSYS`; that trap is already documented as `TD-B-THE-POSIX-RLIB-IS-A-SECOND-LIBC-WITH-EVERY-SYSCALL-STUBBED-OUT`. |

**Why not `now()`, in more detail.** The callers are not alike and their
disagreement is not sloppiness:

- `sshd` and `inetd` are ordinary `std` programs whose test suites run on the
  development machine, where the SlateOS syscall does not exist. `init` and
  `ticker` are bare-metal binaries with no standard library and hand-written
  assembly shims for their entire syscall surface. A crate containing that
  assembly is unusable by the first pair; a crate without it is usable by all
  four.
- They want opposite things when the clock cannot be read. `sshd` refuses the
  connection (§775 above): a login timeout it cannot measure must not be treated
  as satisfied. `inetd` substitutes boot, because its consumer is a sliding
  window, and a window containing everything over-counts — which errs towards
  rejecting a flood rather than admitting one. A shared `now()` must choose one
  of those and would be wrong for the other caller.

**Against it, honestly:** this is not full deduplication, and someone reading
five near-identical `clock_monotonic` functions will reasonably ask why. The
answer is that the duplication that was *hurting* is not the syscall — five
copies of `unsafe { syscall0(SYS_CLOCK_MONOTONIC) }` have never produced a bug —
it is the arithmetic downstream of it, which produced two severe ones in a day.
The boundary is drawn at what the number *means* rather than where it came from,
because that is where the defects were.

**Revisit if** the bare-metal services grow a shared syscall crate for their own
reasons. At that point the shim has a natural home that the `std` daemons do not
need to link, and `monoclock` can stay exactly as it is while the four-line
functions collapse into it.
