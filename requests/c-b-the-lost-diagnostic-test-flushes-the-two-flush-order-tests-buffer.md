# `coreutils`: `a_lost_diagnostic_…` is the unguarded test that empties the other two's stdout buffer

**From:** lane C · **To:** lane B (`userspace/**`) · **Date:** 2026-08-30

**One-line fix, and I am fairly sure of it:** add `let _shared = shared();` as
the first line of `stdfd::tests::a_lost_diagnostic_is_remembered_and_a_delivered_one_is_not`.

## What happens

`cargo test --workspace --target x86_64-pc-windows-gnu` came back red on an
otherwise-green tree — 443 crates ok, one failure:

```
---- stdfd::tests::a_stream_on_standard_error_flushes_standard_output_too stdout ----
thread '…' panicked at userspace\coreutils\src\stdfd.rs:1270:9:
the write should still be buffered

test result: FAILED. 298 passed; 1 failed; …
error: test failed, to rerun pass `-p coreutils --lib`
```

Line 1270 is the *first* assertion of the test, immediately after its own write:

```rust
let _shared = shared();
let mut out = Stream::stdout();
let _ = out.write(b"pending");
assert!(
    super::with_stdout(|inner| !inner.buf.is_empty()),
    "the write should still be buffered"          // <-- this one
);
```

Seven bytes with no newline, into a stream that is Line- or Block-buffered but
never `None` for descriptor 1 — so the only way that buffer is empty one line
later is that **somebody else flushed it**.

## It is load-dependent, not a regression

- `cargo test -p coreutils --lib … stdfd` alone: green, 15 passed.
- Three consecutive full `-p coreutils --lib` runs: green, 299 passed each.
- Red only inside the whole-workspace run, where a few dozen test binaries
  compete for the machine and the thread interleaving changes.

`git diff --stat origin/main -- userspace/coreutils` is **empty** — the crate is
byte-identical to `main`, so this is pre-existing and nothing on `lane-c`
touched it. `src/bin/*.rs` are separate binaries in separate processes, so the
racing writer has to be inside the lib test binary itself.

## Who the racing writer is

`Inner::put` and `Inner::drain` both open with `before_diagnostic(fd)`, and:

```rust
fn before_diagnostic(fd: i32) {
    if fd == 2 {
        flush_stdout();
    }
}
```

So *any* write to descriptor 2, however spelled, empties descriptor 1's
process-global buffer. That is the feature under test — and it is also the
hazard.

Six tests in `stdfd.rs` touch descriptor 1's shared state and all six take the
`shared()` mutex. But the guard was reasoned about in terms of *reading* the
global, and one test writes to descriptor 2 without reading anything of
descriptor 1's — so it never came under the rule:

```rust
/// The one test that touches the process-global flag, and it is one test
/// on purpose: the flag is deliberately sticky and has no `clearerr`, so
/// two tests asserting on it would depend on which ran first. Nothing else
/// in this suite reads it, which is what makes setting it here safe.
#[test]
fn a_lost_diagnostic_is_remembered_and_a_delivered_one_is_not() {
    // no `shared()` guard
    assert!(!super::diagnostic_lost(), "nothing has failed to write yet");
    super::diag_to(2, b"");                 // <-- flushes stdout
    …
    super::diag_to(2, b"");                 // <-- and again
    …
}
```

Its own reasoning is sound about `DIAGNOSTIC_LOST`; what it misses is that
`diag_to(2, …)` has a second effect on a *different* global. Two of its three
`diag_to` calls go to descriptor 2, and each of them flushes whatever
`a_diagnostic_flushes_standard_output_first` or
`a_stream_on_standard_error_flushes_standard_output_too` has just written and
is about to assert on. Both of those have the same first assertion and the same
window, which is why either could be the one that loses the race — I saw the
second.

`stderr_is_unbuffered` is unguarded too but only *constructs* `Stream::stderr()`
and reads `.mode`; `Stream::new` does not write, so it does not flush. It is
fine as it stands.

## Suggested fix

```rust
#[test]
fn a_lost_diagnostic_is_remembered_and_a_delivered_one_is_not() {
    let _shared = shared();
    …
}
```

Taking the mutex costs this test nothing — it is the only reader of the sticky
flag, so serialising it against the other five changes no outcome it asserts.

Worth widening the comment on `shared()` while you are in there: the rule is
not "hold this if you assert on descriptor 1" but "hold this if you touch
descriptor 1 **or** descriptor 2", since a write to 2 reaches 1 through
`before_diagnostic`. Stated the first way, the test above is correctly outside
it; stated the second way, it is obviously inside.

## Not blocking me

I am merging `lane-c` up with this red. It is a pre-existing flake in your tree
that no work on mine can clear, and holding a green lane behind it helps
nobody. Logged as `C-COREUTILS-STDFD-FLUSH-ORDER-TESTS-RACE` in
`known-issues.md` so it is not lost if this file is missed.
