## TD-B-DIG-TRACE-EXITS-ZERO-WHEN-THE-TRACE-GOT-NOWHERE (lane B, 2026-08-21)

**In short:** `dig +trace` walks down from the DNS root servers, printing each
step. If every server along the way refuses or times out, it prints the failures
and then exits with status 0 — "success". A script that runs `dig +trace` and
checks the exit code is told the lookup worked when nothing was resolved at all.

**Where.** `userspace/dig/src/main.rs`, `trace_query`. Every failure inside the
walk is handled in place: the error is printed as a line of trace output and the
loop either moves to the next server or `break`s. The function then falls off
the end. `run` calls it and returns `Ok(())`.

**How it was found.** The crate was put under the workspace lints
(`TD-B-USERSPACE-CRATES-DO-NOT-INHERIT-THE-WORKSPACE-LINTS`, stage 2) and
clippy's `unnecessary_wraps` pointed out that `trace_query` returned
`Result<(), DigError>` without a single `Err` path — the signature was
advertising an error channel nothing ever used. Removing the phantom `Result`
is what made the exit-code behaviour visible; it did not cause it.

**The proper fix.** Have `trace_query` report whether it reached an answer —
the natural shape is to return whether the final iteration produced an answer
record — and have `run` map "got nowhere" to a non-zero exit. Real `dig` exits 9
("no reply from server") for this case, which is the value to match.

**Why it wasn't done in the same change.** The change that found it was a lint
sweep, and an exit code is a user-visible contract: a script that currently
treats `dig +trace; echo $?` as always-0 would start failing. That belongs in
its own commit with its own reasoning, not folded into a batch of lint fixes
where nobody would look for it.

**If never fixed:** unchanged from today — no regression, but `+trace` stays
unusable in any script that checks status rather than parsing stdout. Note this
affects only `+trace`; ordinary lookups already propagate their errors through
`run` and exit non-zero.
