## `B-GUIREMOTE-READ-OVERSHOOTS-MAX-READ-PER-CALL` — `Socket::read` can return up to `CHUNK - 1` bytes past its documented cap — 2026-08-21 (lane B, found; lane C owns the fix)

**Status:** FIXED 2026-08-21 by `a2fd6c6aa` (`read_budget` clamps each read to what is left of the cap; `the_read_budget_never_lets_a_chunk_cross_the_cap` checks it for every `total`), on `main` since. Stamped 2026-09-28 by lane F, whose tree `gui/remote` has been since the six-lane split.

**In short:** the remote-display socket promises that one `read` call takes at
most 256 KiB before returning, and its own test asserts it. The loop that
implements the promise checks the budget *before* each chunk and then reads a
whole chunk regardless, so it can finish up to 8 KiB over. Seen once, in a full
workspace run; the failure message is enough to prove the mechanism.

**Where:** `gui/remote/src/socket.rs:265` (`impl Transport for Socket::read`),
asserted at `:725`
(`socket::tests::one_read_is_bounded_so_a_fast_peer_cannot_starve_dispatch`).
**Lane C's tree** — filed as
`requests/b-c-guiremote-read-can-overshoot-its-own-cap-by-one-chunk.md`.

**Observed:** `cargo test --workspace --target x86_64-pc-windows-gnu` on
`lane-b` `3ad4bfa35`, one failure out of the whole workspace:

```
panicked at gui\remote\src\socket.rs:725:13: one read returned 265312 bytes
```

**Why it is not a flake.** `MAX_READ_PER_CALL = 256 * 1024 = 262_144`;
`CHUNK = 8 * 1024`. `265_312 − 8_192 = 257_120`, which is below the cap (so the
loop was entitled to iterate) and is *not* on the `CHUNK` grid (so an earlier
read had come back short). The guard `while … total < MAX_READ_PER_CALL` is
tested before a body that adds up to `CHUNK`, so the postcondition is
`total < MAX + CHUNK`. The assertion says `total <= MAX`. Both are consistent;
they are not the same statement.

**Why it needs load.** The overshoot requires one short read first.
`MAX_READ_PER_CALL` is exactly `32 * CHUNK`, so when every `recv` returns a
full chunk — an idle machine, writer keeping the buffer full — `total` lands on
the cap exactly and stops. Only a descheduled writer knocks it off the grid.

**Not reproducible on demand: 0 failures in 128 attempts** — 40 runs of the
single test against a concurrent `cargo build --workspace`; 48 full-suite runs
of the `guiremote` test binary at 8-way process concurrency; 80 concurrent runs
of the one test. A full workspace run is a harsher scheduler than anything
constructible on purpose. Not chased further, because the arithmetic does not
need a second sighting.

**Proper fix** (three lines, lane C's to make): clamp the scratch slice to the
remaining budget.

```rust
let want = MAX_READ_PER_CALL.saturating_sub(total).min(CHUNK);
match self.stream.read(chunk.get_mut(..want).unwrap_or(&mut [])) {
```

Nothing is lost — the remainder stays in the kernel buffer, which is what the
`MAX_READ_PER_CALL` doc comment already says happens to everything past the
cap, and `Socket::wait` sees it immediately.

**The alternative, recorded so it is not re-proposed silently:** relax the
assertion to `n <= MAX_READ_PER_CALL + CHUNK`. Defensible — an 8 KiB overshoot
on a 256 KiB budget starves nobody and the constant's purpose survives — but it
writes an accidental bound into the test as if it were intentional, and the doc
comment ("how much one `read` will take before returning") would then need to
say "approximately".

**Fixed 2026-08-21 by lane C**, on the clamp, for the reason lane B gave: the
cap should be honoured rather than the test made to document that it isn't.

The clamp is a named `const fn read_budget(total) -> usize` rather than the
inline expression above, and that is the part worth keeping. **The defect was
unreachable by any test that could be written against the socket**: provoking
it needs a short read to land mid-loop, which depends on when the OS
deschedules the peer's writer thread. Lane B saw it twice in ordinary workspace
runs and could not reproduce it in 128 attempts aimed directly at it. The
existing socket-level assertion is therefore a test that finds this class of
bug only by luck, and it is the only kind of test the shape of the code allowed.

As a function of `total` alone the property is exhaustively checkable, and
`socket::tests::the_read_budget_never_lets_a_chunk_cross_the_cap` now walks
every `total` in `0..=MAX_READ_PER_CALL` — including all the off-grid ones a
short read produces, which is precisely the region the scheduler decides
whether to visit. Verified to be a real regression test by reintroducing the
bug (`read_budget` returning `CHUNK` unconditionally): it fails deterministically
at `total = 253_953`, in a run that takes under a second. The socket-level test
is kept as well; it is the one that found the bug and it covers the loop's use
of the budget rather than the budget itself.

**The reusable point.** The general shape here is the same one lane B drew out
in `design-decisions.md` §343 about `civil_from_days`: an invariant that only a
second opinion can check needs to be *extracted to where a second opinion can
be written*. A property that depends on thread scheduling is not testable; the
same property as a pure function of one integer is exhaustively testable. The
fix was three lines, but the reason it will not come back is the extraction.
