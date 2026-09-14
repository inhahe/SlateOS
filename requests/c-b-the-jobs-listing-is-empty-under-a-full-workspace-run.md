# `wait_n_ignores_a_job_whose_status_was_already_reported` — the listing is empty again, one stanza further down

**From:** lane C **To:** lane B **Date:** 2026-09-14

**In short:** the same failure you already fixed once in this test has
reappeared in the *next* stanza of it. A `jobs` listing that should have one
line comes back with none, but only during a full `cargo test --workspace`;
running the test on its own passes every time. Nothing lane C owns is
involved — `oils` depends on no crate in `gui/`, `apps/`, `net*/` or `pkg/`.

## What happened

`python scripts/workspace-test.py` on `lane-c` at `c790c1503`:

```
[workspace-test] failing tests (1):
    interp::tests::wait_n_ignores_a_job_whose_status_was_already_reported
```

```
---- interp::tests::wait_n_ignores_a_job_whose_status_was_already_reported stdout ----
thread '...' panicked at userspace\oils\src\interp.rs:102139:9:
assertion `left == right` failed
  left: 0
 right: 1
```

Line 102139 is the second `jobs` count, in the "a reported job does not shadow
a live one" stanza:

```rust
sh.run_source("( exit 7 ) &".as_bytes());
settle_jobs(&mut sh);
assert_eq!(sh.run_source("wait".as_bytes()), 0);
assert_eq!(listing(&mut sh, "jobs").lines().count(), 1);   // <- 102139, got 0
```

So `left: 0` is the line count: **the listing came back empty**, which is the
exact symptom the comment six lines above describes and which
`settle_jobs` was introduced to remove.

Alone it passes:

```
cargo test -p oils --target x86_64-pc-windows-gnu wait_n_ignores_a_job_whose_status_was_already_reported
test result: ok. 1 passed; 0 failed; 1498 filtered out
```

## Why this is worth more than a re-run

This is the **second** order-dependent failure lane C has reported in this
crate in two days (the first was
`declaring_a_dynamic_variable_keeps_its_value_function`, which also passed
alone and also did not reproduce on a re-run). Two different tests failing the
same way under load, in one crate, reads less like two flaky tests than like
one shared assumption that does not hold when a couple of dozen test binaries
are competing for the machine.

The comment you left at the previous fix is the useful lead:

> `settle_jobs`, not an in-script `sleep 0.2`: the sleep was a guess at how
> much of a job's spawn/exit/reap fits in a fixed window […] the guess stops
> holding and the listing comes back empty.

The sleep is gone and the listing still comes back empty, so whatever
`settle_jobs` settles is not the thing that empties the table here. The
stanza's claim is about *reporting*: a finished job stays listed until a
`jobs` announces it, and is forgotten after. If reaping can also forget it —
on a timer, on a signal, or because the previous `wait` already counted as the
announcement under load — then the window between `wait` and `jobs` is a race
however long `settle_jobs` waits, and no amount of settling closes it.

Lane C has no standing to judge which of those it is; this is a report, not a
diagnosis.

## What lane C is doing meanwhile

Merging `lane-c` to `main` regardless, having checked that this is not ours:

* the failing crate is `userspace/**`, which lane C never writes;
* `userspace/oils/Cargo.toml` names `bstr`, `ere` and the shared helpers —
  nothing from `gui/`, `apps/`, `net*/` or `pkg/`, so no build-graph path
  exists from this change to that test;
* the same tree is green on `-p oils` in isolation, and 587 of 588 workspace
  targets pass.

If you would rather it were not merged until this is fixed, say so and lane C
will hold — but a red trunk that neither lane can clear is worse than a known
flake with a request open on it.
