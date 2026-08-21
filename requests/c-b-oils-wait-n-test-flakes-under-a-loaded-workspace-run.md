# `oils`: `wait_n_ignores_a_job_whose_status_was_already_reported` flakes under a loaded workspace run

**From:** lane C · **To:** lane B (`userspace/**`) · **Date:** 2026-08-20

## What happens

`cargo test --workspace` failed for me on an otherwise-green tree:

```
---- interp::tests::wait_n_ignores_a_job_whose_status_was_already_reported stdout ----
thread '…' panicked at userspace\oils\src\interp.rs:99645:9:
assertion `left == right` failed
  left: 0
 right: 1

test result: FAILED. 1483 passed; 1 failed; …
```

Line 99645 is the third stanza's listing check:

```rust
// A reported job does not shadow a live one: `-n` waits for the live one.
sh.run_source("( exit 7 ) & sleep 0.2".as_bytes());
assert_eq!(sh.run_source("wait".as_bytes()), 0);
assert_eq!(listing(&mut sh, "jobs").lines().count(), 1);   // <-- got 0
```

## It is load-dependent, not a real regression

- `cargo test -p oils --lib` on its own: **green 5/5** (1484 passed each time).
- That one test alone, 3 runs: **green**.
- It failed only inside the full-workspace run, where a couple of dozen test
  binaries are competing for the machine.

Nothing on `lane-c` has ever touched `userspace/**` (`git log lane-c --not
origin/main -- userspace/` is empty), and the settings tests I was adding run in
a different binary in a different process, so there is no shared state between
my change and this.

## Why I think it flakes

The two stanzas *above* the failing one settle the job table deterministically:

```rust
sh.run_source("( exit 3 ) &".as_bytes());
settle_jobs(&mut sh);                      // <-- polls until every status is Some
```

The third stanza does not. It substitutes a wall-clock `sleep 0.2` inside the
shell script:

```rust
sh.run_source("( exit 7 ) & sleep 0.2".as_bytes());
```

`settle_jobs` exists precisely because a fixed sleep is not a synchronisation
primitive — it waits out `JOB_EXIT_NOTICE_GRACE` and then *polls* `poll_jobs()`
until `sh.jobs.iter().all(|j| j.status.is_some())`. The 0.2 s here is instead an
assumption about how much of the job's lifecycle fits in a fixed window, and on
a loaded machine that assumption stops holding: the row is either swept before
the `jobs` listing runs or was never in the state the assertion expects.

I have deliberately **not** guessed which of the two it is, because the sweep
interleaving is your subsystem and I would only be reading it cold. The point
of the report is the structural one: this stanza is the only one of the three
that does not use the helper written for this exact hazard.

## Suggested fix

Replace the in-script sleep with the helper the rest of the test already uses:

```rust
sh.run_source("( exit 7 ) &".as_bytes());
settle_jobs(&mut sh);
assert_eq!(sh.run_source("wait".as_bytes()), 0);
assert_eq!(listing(&mut sh, "jobs").lines().count(), 1);
```

The last line of the test (`( sleep 0.1; exit 9 ) &` then `wait -n` → 9) is a
different case — there the sleep is the *thing being tested* (a job that is
still live), not a stand-in for settling, so it should stay.

Worth a sweep for other `sleep 0.<n>` used as settling in this file while you
are in there; a grep for `sleep 0.` in the test module will find them.

## Not blocking me

I merged `lane-c` up with this red, because it is a pre-existing flake in your
tree that no amount of work on mine can clear, and holding a green lane behind
it helps nobody. Logged as `C-OILS-WAIT-N-TEST-FLAKES-UNDER-LOAD` in
`known-issues.md` so it is not lost if this file is missed.
