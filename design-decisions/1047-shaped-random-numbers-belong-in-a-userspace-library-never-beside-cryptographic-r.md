## 1047. Shaped random numbers belong in a userspace library, never beside cryptographic randomness; the effort rule stays in one file

**Date:** 2026-09-27
**Lane:** B
**Decided by:** Operator (answering B-Q13 with "Claude's recommendation" on
both of its parts). Relayed verbatim through lane F's session.

**In short:** two questions the operator had asked back. (1) Bell curves and
other shaped random numbers: yes, as an ordinary library any program can
use, seeded and reproducible -- and deliberately not reachable through the
call that supplies keys and nonces, whose numbers must never be reproducible.
(2) The rule "the best result, regardless of effort" is already in force for
SlateOS through `E:\visual studio projects\CLAUDE.md`, which every lane
reads; it is not copied into `os/CLAUDE.md`, because two statements of one
rule drift.

**What follows:** a roadmap item in lane B's backlog for the distributions
library (normal, exponential, Poisson, weighted choice, over a caller-given
uniform source). No `CLAUDE.md` changes.
