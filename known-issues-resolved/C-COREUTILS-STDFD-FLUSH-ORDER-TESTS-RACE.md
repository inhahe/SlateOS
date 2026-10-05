### C-COREUTILS-STDFD-FLUSH-ORDER-TESTS-RACE — 2026-08-30 — LANE C reporting, lane B's tree — **FIXED 2026-08-30 (lane B)**

**Fixed as filed, and the diagnosis was right in every particular.**
`a_lost_diagnostic_is_remembered_and_a_delivered_one_is_not` now takes
`shared()` like the other five. The `shared()` doc comment carries the widened
rule — *hold this if you touch descriptor 1 **or** descriptor 2*, because a
write to 2 reaches 1 through `before_diagnostic` — and the test's own comment
records why serialising it costs nothing. `stderr_is_unbuffered` is left
unguarded, also as filed: `Stream::new` does not write, so it cannot flush.
The original report follows.

`cargo test --workspace` came back red on an otherwise-green tree with one
failure in a crate `lane-c` has never touched:

```
---- stdfd::tests::a_stream_on_standard_error_flushes_standard_output_too stdout ----
thread '…' panicked at userspace\coreutils\src\stdfd.rs:1270:9:
the write should still be buffered
```

Line 1270 is the test's *first* assertion, one line after its own seven-byte
write into descriptor 1's process-global buffer. Descriptor 1 is never
`Buffering::None`, so an empty buffer there means another thread flushed it.

Green in isolation (15 passed), green on three consecutive full `-p coreutils
--lib` runs (299 each), red only under the parallelism of a workspace run.
`git diff --stat origin/main -- userspace/coreutils` is empty, so it is
pre-existing rather than merge-induced, and `src/bin/*.rs` are separate
processes, so the racer is inside the lib test binary.

**The racer is `stdfd::tests::a_lost_diagnostic_is_remembered_and_a_delivered_one_is_not`**,
which is the one test in the module that touches descriptor 2 without taking
the `shared()` mutex. `Inner::put`/`Inner::drain` open with
`before_diagnostic(fd)`, which flushes stdout whenever `fd == 2` — so its two
`diag_to(2, b"")` calls empty the buffer that the two flush-ordering tests have
just written and are about to assert on.

Its own doc comment explains why it is unguarded, and the reasoning is sound as
far as it goes: it is the only reader of the sticky `DIAGNOSTIC_LOST` flag. What
it misses is that `diag_to(2, …)` also writes a *different* global. The guard's
rule was stated as "hold this if you assert on descriptor 1"; stated correctly —
"hold this if you touch descriptor 1 **or** descriptor 2" — this test is
obviously inside it.

Fix is one line: `let _shared = shared();` at the top of that test. Filed for
lane B as `requests/c-b-the-lost-diagnostic-test-flushes-the-two-flush-order-tests-buffer.md`.

**Where else to look:** any test-module mutex whose contract is written in terms
of the state a test *reads*. A side effect on a second global — here, "writing a
diagnostic flushes the output it comes after", which is the feature under test —
puts a test inside the guard's scope without it ever mentioning the guarded
state.
