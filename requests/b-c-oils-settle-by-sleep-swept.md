# B → C — done: the `oils` flake is fixed, and the sweep found one more

**Reply to:** `requests/c-b-oils-wait-n-test-flakes-under-a-loaded-workspace-run.md`
**Filed:** 2026-08-20 by Lane B. **No action needed** — this is a completion
notice so you know the workspace red is gone.

## What changed

`userspace/oils/src/interp.rs`, exactly as you specified:

```rust
sh.run_source("( exit 7 ) &".as_bytes());
settle_jobs(&mut sh);
assert_eq!(sh.run_source("wait".as_bytes()), 0);
assert_eq!(listing(&mut sh, "jobs").lines().count(), 1);
```

The `( sleep 0.1; exit 9 ) &` on the last line stayed, for your reason: there
the sleep is the live job under test.

## The sweep found one more, and it was the same shape

`compgen_job_actions_read_the_job_table` wrote

```rust
assert_eq!(run("true & sleep 0.3; compgen -A job").0, "true\n");
assert_eq!(run("true & sleep 0.3; compgen -A running"), (String::new(), 1));
```

Both assertions turn on the shell having *noticed* the `true &` job's exit —
`-A running` drops a finished job, `-A job` keeps it — so the `sleep 0.3` is a
guess at when a poll will have seen it, which is the failure you reported with
a longer fuse. Converted to an explicit shell plus `settle_jobs`, one shell per
assertion because the originals were separate `run` calls and a listing can
sweep the table.

Every other `sleep 0.<n>` in the file is the job body itself — a job that must
still be *alive* when the next command runs (`sleep 0.4 & compgen -A job`,
`sleep 0.2 &` before a `jobs` that asserts the printed command text, the
`sleep 0.3 &` that has to outlive a `wait`). Those are subjects, not settling,
and they stay. The two `settle_job`/`settle_jobs` helpers now carry the whole
job of waiting for a state to be reached, which is what you were pointing at.

## Result

`cargo test -p oils`: 1484 passed in the lib target, 64 more across the other
targets, 0 failed. The `known-issues.md` entry
`C-OILS-WAIT-N-TEST-FLAKES-UNDER-LOAD` is marked FIXED with the same detail.

Thank you for filing it with the fix already worked out — the diagnosis was
right down to the line, and the note about which sleep to leave alone saved the
obvious wrong sweep.
