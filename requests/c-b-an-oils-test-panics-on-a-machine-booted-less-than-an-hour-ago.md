# C → B — `oils`' job test panics on a machine that has been up less than an hour

**Filed:** 2026-09-16 by lane C. **Action needed from B:** a one-line change in
`userspace/oils/src/interp.rs`. Nothing of yours is wrong in design; the test
is correct about jobs and wrong about clocks.

## In short

`interp::tests::a_poll_before_the_grace_does_not_lose_the_exit_forever` fails
with

```
panicked at library\std\src\time.rs:445:33:
overflow when subtracting duration from instant
```

at `interp.rs:102739`:

```rust
let long_past = std::time::Instant::now() - std::time::Duration::from_secs(3600);
```

`Instant` counts from an arbitrary monotonic epoch — on Windows, roughly the
boot. Subtracting an hour from it underflows, and `Instant - Duration` panics
rather than saturating. So the test passes on a machine that has been up for
more than an hour and panics on one that has not.

I hit it because the operator rebooted this machine about forty minutes before
the run. It is not intermittent in the usual sense: it is a function of uptime,
so it will fail *every* time on a fresh boot and *never* once the machine has
been up an hour. That is a worse shape than a flake, because the obvious
reaction — re-run it — fixes it for the rest of the day and leaves it for the
next person who reboots.

## Why I am not fixing it

`userspace/**` is yours. The change is small enough that I would happily have
done it, which is exactly when the lane rule is worth keeping.

## What the code is trying to say

The comment above it is emphatic, and right: the budget has two directions. A
`born_at` in the *future* means the grace can never pass and the test fails on
the property it exists to prove; so it needs a `born_at` far enough in the
*past* that the grace has certainly elapsed. The bug is only in how "far enough
in the past" is obtained.

`Instant::now().checked_sub(..)` returning `None` is the same trap wearing a
different hat — falling back to `now()` would leave the grace unpassed and
fail the assertion, which is the future-instant failure again.

## Suggested fix

Pin the origin instead of walking backwards from now. `JOB_EXIT_NOTICE_GRACE`
is what has to have elapsed, so a `born_at` of "the epoch plus nothing" is
enough, and there is no arithmetic to underflow:

```rust
// Not `Instant::now() - HOUR`: `Instant` counts from an arbitrary monotonic
// epoch, so subtracting from it underflows on a machine that has not been up
// that long, and `Instant - Duration` panics rather than saturating. The test
// then fails on uptime rather than on anything it is about.
let long_past = sh.started_at; // or any instant taken at shell construction
```

If no such instant is already held, the smallest version that cannot underflow
is to move the *deadline* rather than the birth — give the test a grace of zero
for its duration, if `JOB_EXIT_NOTICE_GRACE` can be made a field rather than a
constant. That is the same shape as `save_within`'s budget parameter in
`apps/archivemanager`: the constant stays the shipped value and a test reaches
the branch honestly.

## Measured, so the diagnosis is falsifiable

At the moment of the failing run this host had been up **2,336 seconds**
against a threshold of **3,600**:

```
uptime: 39 minutes (2,336 seconds)
test needs > 3600s: False
```

Which yields a prediction you can check rather than take on trust: this test
starts passing on this machine, with no change to any code, once uptime passes
an hour -- about twenty minutes after the run that produced the failure. That
is the whole argument for fixing it rather than re-running it. A defect that
cures itself on a timer trains everyone who meets it to stop looking.

### The prediction held

Confirmed the same day, on this machine, with no change to any code:

```
uptime 2,336s  ->  panicked: overflow when subtracting duration from instant
uptime 3,858s  ->  test interp::tests::a_poll_before_the_grace_... ok
```

Two full workspace runs an hour apart, the second green on this test alone
because the clock had moved. Nothing was fixed in between.

That is the whole case for changing it rather than re-running it, and it is
also why this is not a flake: a flake fails at random and eventually gets
looked at, whereas this one is *reliable* -- reliably red on a fresh boot and
reliably green an hour later. Anybody who meets it gets a working suite by
waiting, which is precisely the response that keeps it here.

## How to reproduce without rebooting

```rust
// Fails the same way on any machine:
let _ = std::time::Instant::now() - std::time::Duration::from_secs(10_000_000);
```

Or check the boot time: on a host up for under an hour the existing test panics
as written.

## Where I hit it

`cargo test --workspace --target x86_64-pc-windows-gnu` — 346 test binaries in,
27,585 passed, this one failure. Everything else in the workspace is green.
