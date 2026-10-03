## 1007. The test machine produces real randomness, and the tests that asserted otherwise are rewritten

**Date:** 2026-09-07
**Lane:** B
**Decided by:** Operator (answering `open-questions.md` "The test machine
cannot produce random numbers, on purpose, and about eighteen tests in the
apps now depend on that")

The test platform's randomness source was deliberately inert, so every part of
the OS that needs an unpredictable number got a predictable one. Four tests in
`userspace/ssh` were permanently red because of it, and the "what happens when
the kernel says no" branch was only ever exercised on the platform we do not
ship. Eighteen tests in `apps/` and `gui/` had come to depend on the inertness
-- they assert that two draws are *equal*, which is true only of a machine
that cannot produce randomness.

**Option A: land it.** The test machine produces real randomness; lane C
rewrites its eighteen tests into the `assert_ne!` form, in its own tree. Lane
B files the request and the list. `main` is red in between unless lane C moves
first -- which was the cost the operator accepted rather than have lane B
write eighteen tests inside lane C's globs (option C), because that is exactly
what the lane split exists to prevent.

**What changes:** four red `userspace/ssh` tests go green, the
client-against-server test becomes possible, and eighteen tests in `apps/` and
`gui/` go red until lane C converts them.

**Independent of the answer** (and already done): `SshSession` and
`ConnectionState` take their randomness as an injected byte source, so the
handshake can be made deterministic for a test without weakening it in
production. That is a better design regardless and it is what un-redded the
four SSH tests; this decision is about whether the rest of the tree gets the
same honest platform.
