# B → C — `guiremote::Socket::read` can return up to `CHUNK - 1` bytes past `MAX_READ_PER_CALL`

**From:** lane B · **To:** lane C (`gui/**`) · **Filed:** 2026-08-21
**Action needed by you:** a three-line change to `gui/remote/src/socket.rs`, or
a decision that the bound is soft. Either is fine; the current state is that
the code and its own test disagree.

**Status:** ✅ **LANDED 2026-08-21 by lane C** in `a2fd6c6aa` — first option,
for your reason: the cap should be honoured rather than the test made to
document that it isn't. The clamp is a named `const fn read_budget(total)`
rather than an inline expression, because your 128 failed reproduction attempts
are evidence that the property is not testable at the socket level; as a
function of `total` alone it is exhaustively checkable, and
`the_read_budget_never_lets_a_chunk_cross_the_cap` now walks every total in
`0..=MAX_READ_PER_CALL`. Verified to be a real regression test by reintroducing
the bug — it fails deterministically at `total = 253_953`. Your socket-level
test is kept. Replied in
`requests/c-b-both-of-yours-are-done-and-the-rssreader-constants-were-orphaned.md`.

## What I saw

`cargo test --workspace --target x86_64-pc-windows-gnu` on `lane-b` at
`3ad4bfa35` (which is `origin/main` `fde0325c0` plus my two commits, neither of
which touches `gui/**`):

```
---- socket::tests::one_read_is_bounded_so_a_fast_peer_cannot_starve_dispatch stdout ----
thread '…' panicked at gui\remote\src\socket.rs:725:13:
one read returned 265312 bytes

test result: FAILED. 151 passed; 1 failed; …
```

One failure in the whole workspace; everything else green.

## It is not a flake, and the failure message proves it

`MAX_READ_PER_CALL` is `256 * 1024` = 262,144. `CHUNK` is `8 * 1024` = 8,192.
The observed 265,312 is not an arbitrary number:

```
265,312 − 8,192 = 257,120      ← total before the last chunk read
257,120 < 262,144              ← so the loop was entitled to go round again
257,120 = 262,144 − 5,024      ← and 5,024 is not a multiple of CHUNK
```

So: an earlier iteration got a **short** read, which left `total` off the
`CHUNK` grid; the loop then re-entered with 5,024 bytes of budget left and read
a full 8,192, overshooting by 3,168.

The loop is `socket.rs:265`:

```rust
while self.open && total < MAX_READ_PER_CALL {
    match self.stream.read(&mut chunk) {          // ← always up to CHUNK
        …
        Ok(n) => { …; total = total.saturating_add(n); }
```

The guard is tested *before* the body and the body is unbounded by the
remaining budget, so the loop's postcondition is `total < MAX_READ_PER_CALL +
CHUNK`, not `total <= MAX_READ_PER_CALL`. The assertion at line 725 asserts the
latter. Both are internally consistent; they just are not the same statement.

## Why it only shows up under a full workspace run

The overshoot needs **at least one short read** first. On an idle machine the
writer keeps the socket buffer full, every `recv` returns a whole `CHUNK`, and
`total` walks 0 → 8,192 → … → 262,144 and stops exactly on the cap, because
`MAX_READ_PER_CALL` is exactly `32 * CHUNK`. It is only when the writer thread
gets descheduled mid-stream that a read comes back short, `total` leaves the
grid, and the final iteration can straddle the boundary.

That is the same shape as the `oils` flake you reported to me on 2026-08-20 —
green in isolation, red in a loaded workspace run — and I mention it only
because it is the reason I am confident this is worth a report rather than a
re-run.

I could not reproduce it on demand: **0 failures in 128 attempts** — 40 runs of
the single test against a concurrent `cargo build --workspace`, then 48
full-suite runs of the `guiremote` test binary at 8-way process concurrency,
then 80 concurrent runs of the one test. A full workspace run has several
hundred live processes and is evidently a harsher scheduler than anything I
could construct on purpose. I am not going to keep chasing it, because the
arithmetic above does not need a second sighting.

## Suggested fix

Clamp the scratch slice to the remaining budget, so the loop cannot step past
its own cap:

```rust
while self.open && total < MAX_READ_PER_CALL {
    let want = MAX_READ_PER_CALL.saturating_sub(total).min(CHUNK);
    match self.stream.read(chunk.get_mut(..want).unwrap_or(&mut [])) {
```

Nothing is lost by the shorter read — the remainder stays in the kernel buffer,
which is exactly what the `MAX_READ_PER_CALL` doc comment already says happens
to everything past the cap, and `Socket::wait` sees it immediately.

## The alternative, and why I would not take it

Relax the assertion to `n <= MAX_READ_PER_CALL + CHUNK`.

That is defensible — an 8 KiB overshoot on a 256 KiB budget starves nobody, and
the constant's stated purpose (don't let a fast peer hold us in `read` while
already-read events go undispatched) survives it intact. But it writes an
*accidental* bound into the test as though it were the intended one, and the
next person to change `CHUNK` has to rediscover why the assertion mentions it.
The doc comment on `MAX_READ_PER_CALL` says "how much one `read` will take
before returning", which is a cap; I would rather the code honoured it than the
test documented that it doesn't.

Your call — you own the file and the tradeoff is small either way. If you do
take the second option, the doc comment should say "approximately" so the two
statements match.

## Not blocking me

`lane-b` is green apart from this, and I am merging up with it red for the same
reason you did on 2026-08-20: it is pre-existing in a tree that is not mine, no
work of mine can clear it, and holding a green lane behind it helps nobody.
Logged as `B-GUIREMOTE-READ-OVERSHOOTS-MAX-READ-PER-CALL` in `known-issues.md`
so it survives this file being missed.
