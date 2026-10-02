### TD-OILS-TEST-SCRATCH-NAME-COLLISION. Two oils tests could pick the same scratch directory, so one deleted the other's cwd mid-run — 2026-08-03 — ✅ **RESOLVED 2026-08-03**

**Symptom:** roughly **one lib-test run in three** failed, on a *different*
arbitrary test each time — `a_backtick_body_in_double_quotes_loses_its_quote_escapes`,
`exec_replaces_shell_and_stops`, `env_prefix_subscript_is_refused_as_a_name` were
three of them — and every one passed on its own. The failures were always a
spawn going wrong: an external's output arriving empty, or the giveaway
`osh: sh: Not a directory`, which is Windows `ERROR_DIRECTORY` from a
`Command::current_dir` that no longer exists.

**Cause:** `tests::uniq_name` built a scratch basename from the pid and a
`SystemTime::now()` nanosecond stamp, with a doc comment asserting that "no two
of them can pick the same name however the harness happens to schedule them."
That is false, because the clock advances in *steps* — measured on this host at
~150 ns (336 413 samples in 50 ms yielded only 311 198 distinct values), and far
coarser elsewhere. Two threads calling inside one step got the same name, and
`ScratchDir::at` used `create_dir_all`, which accepts an existing directory
silently. The two tests then shared a cwd and ran fine until the *first* one
finished — at which point its `ScratchDir::drop` did `remove_dir_all` on the tree
the second was still standing in. Everything the second did afterwards failed for
a reason with no connection to what it was testing.

**Resolution.** `uniq_name` now appends a process-wide
`AtomicU64::fetch_add` sequence number, so the name is unique by construction
rather than by clock luck; the stamp stays only because it makes a leftover
directory's age readable. And the two constructors that consume a `uniq_name`
(`ScratchDir::new`, `ScratchDir::relative`) now go through `ScratchDir::fresh`,
which uses `create_dir` — a collision is a panic naming the path, at the point of
the collision, instead of a silent share that bites some other test later.
`ScratchDir::at` keeps `create_dir_all` for the handful of tests that name their
own directory. Verified by running the lib suite **20 times: 0 failures**,
against ~1-in-3 before.

**Note for future test infrastructure:** a timestamp is not a unique identifier.
Anywhere two threads may name a shared resource, the name needs a counter.
