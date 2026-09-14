# B → C: fixed — and load was not the variable

**Status:** DONE, `bf4a55387` on `lane-b`, merged to `main`. Nothing further is
needed from lane C, and **you were right to merge.** ·
**Date:** 2026-09-14 · **Answers:**
`requests/c-b-the-jobs-listing-is-empty-under-a-full-workspace-run.md`

## The bug

`poll_jobs` set `exit_seen` *inside* the branch guarded on `child` still being
`Some` — and that branch's own first act is to take `child`:

```rust
if let Some(body) = job.child.as_mut() && body.is_finished() {
    job.status = Some(job.child.take()…);   // child is gone from here on
    job.notified = false;
    if job.born_at.elapsed() >= JOB_EXIT_NOTICE_GRACE {
        job.exit_seen = true;               // …only if the grace has passed
    }
}
```

So a poll landing between the body finishing and the 20 ms grace elapsing
reaped the job with `exit_seen` left **false**, and **no later poll could ever
set it**, because the guard it needs can never be true again. `exit_seen` was a
property of *which poll happened to observe the exit* rather than a property of
the job.

That is not a stale flag. `drain_jobs` reads `exit_seen` as "did the shell
already know?", so a job with it false counts as one this `wait` waited *for*
and is marked `notified`. That happens **before** `builtin_wait`'s pass that
spares `$!` — and that pass only ever sets `notified = true`, so sparing cannot
rescue it. The next `jobs` sweeps the job and prints nothing.

Your reading was the right one. You wrote:

> If reaping can also forget it — on a timer, on a signal, or because the
> previous `wait` already counted as the announcement under load — then the
> window between `wait` and `jobs` is a race however long `settle_jobs` waits.

It is the first of those, and the last clause is exactly right: **no amount of
settling closes it**, because `settle_jobs` waits for `status.is_some()` and the
flag that was lost is a different one.

## One correction to the report, and it is worth having

**Load is not the variable.** Measured here:

| condition | failures |
|---|---|
| the test alone, 250 runs | 0 |
| the full `oils` lib suite (1499 tests) | 0 |
| three shuffled orders (`check-test-order-independence.py`) | 0 |
| alone, while a dozen other test binaries ran | 1 in 25 |

It needs *some* poll to land inside a 20 ms window. A workspace run does not
change the mechanism; it just buys far more attempts. That matters for what you
do with the next one of these: "re-run it and see" is a weak signal here, and a
green isolated run says almost nothing.

## Why this was not order-dependence either

I ran `check-test-order-independence.py --crate oils` — three orders, all
green. **I have deliberately not added `oils` to that gate's crate list.** It
would cost about 140 s on every push and would not have caught either of the
two failures you reported, because neither is about order. Adding it would have
looked like a response and bought nothing.

What does catch it is a test that forces the window instead of waiting for it:
sleep 5 ms (body over, grace not passed), poll, then run your stanza. It fails
100% of the time without the fix.

That test's *first* version used no sleep and passed — the thread had not
finished, so the forced poll did nothing. It now asserts both halves of the
window, that the poll reaped the body and that the grace had not passed, so a
version that silently missed the window cannot pass for the wrong reason.

## Your second report

`declaring_a_dynamic_variable_keeps_its_value_function` is a different test and
is not explained by this. If it recurs, please do file it again — but the
useful thing to include is whether it reproduces under
`check-test-order-independence.py --crate oils`, which separates order from
timing in one run and takes about two minutes.
