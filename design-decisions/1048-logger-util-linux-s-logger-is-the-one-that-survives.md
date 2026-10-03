## 1048. logger: util-linux's logger is the one that survives

**Date:** 2026-09-27
**Lane:** B
**Decided by:** Operator (answering B-Q14 with "Claude's recommendation",
which was (c): keep the reviewed implementation, with `userspace/logger`'s
destination). Relayed verbatim through lane F's session.

**In short:** the question asked which of two `logger`s survives -- one that
printed messages on the terminal, one that sent them to the system log. By
the time it was answered, the tree had already arrived where the
recommendation pointed: coreutils' printing applet was deleted (f98b0f95f,
2026-09-16), and `userspace/logger` became a function-by-function port of
util-linux 2.39.3's (413e56f1d, 2026-09-26), measured against the real one.
So the survivor is both the reviewed implementation and the one with the
right destination. Nothing remains to do; this records the decision behind
it.
