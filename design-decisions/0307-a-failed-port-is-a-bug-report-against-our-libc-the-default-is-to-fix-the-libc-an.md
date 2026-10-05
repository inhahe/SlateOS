## §307 — A failed port is a bug report against our libc; the default is to fix the libc and re-try, not to reimplement

**Date:** 2026-08-14
**Decided by:** Operator (Claude proposed the framing, the four-way triage and
the two refinements; the operator raised the question — "if upstream pkgconf
doesn't build against our libc, could that be taken as a suggestion to improve
our libc for it and for future apps instead?" — and made the call)

**The question.** `roadmap-detailed.md` now tells you to try cross-compiling an
existing C/C++/Rust program before writing a replacement for it. That rule only
covered the *success* branch. What is the correct response when the port does
**not** build against `toolchain/sysroot/lib/libc.a`?

**The decision.** A failed link is treated as a **defect report against the
libc**, and the default action is to implement the missing surface in `posix/`
and re-try the port. "Fall back to reimplementing the application" is not the
default and now requires a reason from the triage below.

**Why the operator's framing is right, stated as reasons rather than assertion:**

- **A real port is the only honest coverage test a libc has.** You cannot guess
  which of several thousand symbols matter; real software tells you, weighted by
  actual usage rather than by what looked important when the header was written.
- **The fix compounds; a rewrite does not.** A libc function added for one port
  is inherited free by every later port. A reimplementation helps exactly one
  program, and then has to be maintained forever against upstream's bug fixes.
- **The cost asymmetry points the same way.** Implementing a missing libc
  function is minutes to an hour. Reimplementing the application is hours to
  days — and §305 is the measured proof of how far that can run (~25 days on a
  bash reimplementation, against ~a day for cross-compiling GNU bash as-is).
- **Precedent, not theory.** In `scripts/bash-spike/`, `libc.a` defined 2,900
  symbols, bash referenced 2,030, and the first SlateOS link resolved all but
  **three** — `killpg`, `eaccess`/`euidaccess`, `__fpurge`. All three were
  implemented for real in `posix/src` rather than shimmed. One port attempt
  converted into permanent coverage for everything that follows.

**The nuance that makes it a decision rather than a slogan — triage.** Only the
first two categories mean "improve the libc":

1. **Missing standard POSIX/C function** → implement it in `posix/`. Highest
   leverage, and the common case.
2. **Missing non-standard extension** (glibc/BSD-isms) → usually implement, but
   check first whether upstream's `configure` already has a fallback path, in
   which case the gap is imaginary and the fix is to let autoconf find it.
3. **Architectural mismatch → do *not* grow the libc into it.** If the program
   needs something SlateOS deliberately rejects — Unix signals as native process
   control, 4 KiB page assumptions, ambient-authority fds, `/proc` special nodes
   — the Linux Compatibility Boundary governs: `ENOSYS`, or emulation *inside*
   the compat layer, never a hack into native code to satisfy a Linux quirk.
   This is the one case where a failed port genuinely says stop.
4. **Not a libc gap at all** → build-system friction (cross-compile detection,
   sysroot plumbing). Fix the build. The bash spike's own example: `$CC` cannot
   contain spaces because autotools word-splits it, and this repo lives under
   `D:\visual studio projects\`, so the first attempt died with a thoroughly
   misleading "C compiler cannot create executables".

**Two refinements, both learned from that spike:**

- **A kernel gap can masquerade as a libc gap.** `killpg` exists now and still
  returns `ENOSYS`, because process groups do not exist in the kernel — yet the
  symbol had to exist, since bash references it from job-control code and the
  *link* needs it even where job control cannot work. Implement the symbol
  honestly; file the underlying gap against the kernel (cross-lane: a
  `requests/` entry, not an edit outside your lane).
- **Whatever you add must actually work.** "Never accept-without-honoring"
  applies at full force: a stub returning success is *worse* than `ENOSYS`,
  because the port links, appears to work, and fails subtly later. `killpg`
  reporting `ENOSYS` truthfully is the correct shape.

**Where it flips.** The argument is leverage, so it evaporates when there is
none. If one port needs a large, exotic subsystem nothing else will ever use,
that is cost without reuse — weigh it like any other feature rather than
treating "it improves the libc" as automatically decisive.

**The alternative that was rejected**, and why it is tempting: treat a failed
build as evidence that the program is "not portable to SlateOS" and write our
own. It is tempting because it is *unblocking* — reimplementation never fails to
link, so it always feels like progress, and the failure it replaces is concrete
and immediate while the compounding benefit is diffuse and deferred. That is
precisely the bias this entry exists to counter: the cheap-feeling path is the
one that costs 25 days.

**Where it bites:** `roadmap-detailed.md` §"Porting vs. Reimplementing" (the
sub-block "When the port *fails*, that is usually a bug report against our libc
— not a verdict on the port"), the whole of `posix/`, and — immediately —
`userspace/pkgconf/`, whose in-progress Rust rewrite prompted the question and
whose fate now depends on whether upstream pkgconf cross-compiles.
