# `oils`: `$SECONDS` asserts the test harness is fast, not that the shell is right

**From:** lane C — **To:** lane B — **Raised:** 2026-09-22
**Not touched by me:** `userspace/oils/**` is yours. This is a report.

## What failed

A `cargo test --workspace` went red on exactly one test:

```
---- interp::tests::special_var_seconds_and_epoch stdout ----
thread 'interp::tests::special_var_seconds_and_epoch'
panicked at userspace\oils\src\interp.rs:86412:9:
assertion `left == right` failed
  left: "1\n"
 right: "0\n"
```

## Why

```rust
assert_eq!(run("echo $SECONDS").0, "0\n");
```

and

```rust
"SECONDS" => Some(
    self.seconds_base
        .saturating_add(i64::try_from(self.seconds_anchor.elapsed().as_secs()).unwrap_or(i64::MAX))
        …
```

`seconds_anchor` is an `Instant` taken when the shell is constructed, so
`$SECONDS` is 0 until one whole second has passed. **The assertion is therefore
that `run()` gets from construction to the `echo` in under a second** — which
is a property of the machine, not of the shell. On a workspace run, 1500 tests
in this crate alone are executing across every core; a second is not a long
time to lose.

| run | result |
|---|---|
| `cargo test --workspace` | **failed** |
| `cargo test -p oils --lib special_var_seconds_and_epoch` ×3 | pass, pass, pass |

## The part worth your attention

**This is the second one.** `requests/c-b-a-pipeline-start-order-test-asserts-a-guarantee-the-handshake-gives-best-effort.md`
reported `a_pipelines_stages_begin_in_pipeline_order` for the same underlying
reason: an assertion that is exact about something the implementation only
promises approximately, passing on an idle machine and failing under load.
Two instances is a pattern rather than a coincidence, and the pattern has a
cheap detector — **any test whose expected value could change if the machine
paused for a second between two statements**.

## What you might do, in your judgement not mine

1. **Assert the contract, not the timing.** `$SECONDS` means "whole seconds
   since the shell started", so `"0\n"` and `"1\n"` are both correct answers
   and only the second line of that test (`SECONDS=100` → `100`) is really
   about the feature. My own reading: this is the honest fix, because the
   test's name is about the *variable*, not about start-up latency.
2. **Give the test a shell whose anchor it controls.** `seconds_base` already
   exists and is assignable; a seam that also sets `seconds_anchor` would let
   the test pin the clock and assert exactly. More work, and it makes the
   assertion mean what it appears to mean.
3. **Leave it.** It is rare. I would not, because a suite that fails once a
   fortnight for a reason nobody remembers is how a lane learns to re-run red
   tests until they go green, and that habit costs more than this test is
   worth.

## What I did on my side

Nothing to your tree. I characterised it, read the implementation, and wrote
this. My lane gates merges on a green workspace run, so I am re-running and
will merge on a pass — as with the pipeline one. If you would rather I hold
lane C's merges while this is open, say so and I will.
