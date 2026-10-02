## 1043. Genuine Oils becomes the default shell; our Rust OSH stays as a fallback

**Date:** 2026-09-27
**Lane:** B
**Decided by:** Operator (answering B-Q9; Claude set out (a) keep ours, (b)
replace ours with genuine Oils, (c) not yet, without recommending one. The
operator chose (b) for the default and rejected (b)'s deletion: the Rust
shell stays). Relayed verbatim through lane F's session.

**In short:** the shell a user gets will be the real Oils -- both of its
languages, OSH (bash-compatible) and YSH (its newer one) -- built from
upstream's C++ for SlateOS, instead of our Rust re-creation of OSH. Ours is
kept as a discoverable alternative, because a small shell with no C++ runtime
under it is what still works when little else does.

**The operator's answer, verbatim:**

> The OS has to eventually be able to run Python and C++ correctly anyway,
> and Claude never did finish fully debugging the Rust implementation of
> Oils after 27 days. And as for "our Rust shell is small, boots early, and
> has no C++ runtime under it — which matters for a shell that has to work
> when little else does," the Rust implementation can always be kept as a
> discoverable option that can also be run when little else is working. And
> as for "our copy will always chase upstream, and any behaviour we have not
> re-created is a difference someone eventually trips over," I don't see how
> simply not worrying about chasing upstream changes in Oils could possibly
> be any worse than staying with our own Rust implementation which would
> effectively be a stale snapshot of the Oils version it was reimplemented
> aganist, only buggier. So, make the real Oils the default.

**What follows:** a roadmap item in lane B's backlog -- cross-compile genuine
Oils (its generated C++, `oils-for-unix`) with `zig c++` against SlateOS's C
library, run its spec tests on SlateOS, then make it the default `sh` and
login shell (which touches `init/` and lane D's rootfs recipe). This closes
the fork §73 left open. It is also the first C++ program SlateOS will run,
so it proves the C++ runtime that Mesa, Chromium and WINE need anyway.
