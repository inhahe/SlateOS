## TD-B-CARGO-TEST-WORKSPACE-NO-LONGER-FINISHES (lane B, 2026-08-20)

**What.** `cargo test --workspace` can no longer reach the *first test* inside a
bounded timeout on a cold cache. Measured 2026-08-20 from a clean-ish target
dir: 2,428 crates compiled in 50 minutes, **zero `test result` lines produced**,
and `scripts/run-timeout.py`'s 3000 s limit killed the run mid-compile. The
suite did not fail — it never started. `userspace/` alone now holds 2,751 crate
directories, nearly all of them near-identical single-binary `*-cli` crates.

**Why it matters more than it looks.** The project's testing rule is that the
full workspace suite gates a merge to `main`. A suite that cannot finish is a
gate that is quietly always skipped, and the failure mode is indistinguishable
from success at a glance: the log ends in `Compiling …` with no failures in it.
This session hit exactly that — a 50-minute run was left in flight for most of
an hour under the impression it was validating a change, when it could never
have reached the tests, and the change actually being validated (`posix`) took
**3 seconds** once run on its own (20,398 tests, 0 failed).

**Where.** Root `Cargo.toml` `members` (`userspace/*`); the timeout lives in
whatever `scripts/run-timeout.py` invocation the agent chooses, so there is no
single place that is "wrong" — which is part of the problem.

**Proper fix — options, none yet chosen.**
1. **Split the workspace.** Move the `*-cli` crates into their own workspace so
   the core (`posix`, `kernel`, `fs`, `net`, `init`, `services`, coreutils) can
   be tested in minutes, and run the CLI workspace on its own cadence. Biggest
   win, biggest churn, and it cuts across all three lanes' trees, so it needs
   coordination rather than a unilateral edit.
2. **`cargo test --workspace --exclude`-list the `*-cli` crates** for the
   routine gate and run the full thing only before a `main` merge. Cheap, but
   the exclusion list has to be maintained and will silently rot.
3. **Test only the crates a change touches** (`-p <crate>`), with the full run
   reserved for release points. This is what actually happened here and it
   worked well, but it is currently an ad-hoc judgement call rather than a
   documented rule, so it is applied inconsistently.

**Interim rule being followed.** Use `-p <crate> --target x86_64-pc-windows-gnu`
for the change under test, and do not treat a timed-out `--workspace` run as
evidence of anything. If a `--workspace` run is started, give it a timeout that
accounts for a >50-minute compile, and check that the log actually contains
`test result` lines before believing it.

**Not filed as a request to another lane** because it is not yet clear which of
the three options is right, and picking one is a workspace-layout decision with
consequences for every lane. Promote to `open-questions.md` if the routine
per-crate workaround starts letting real regressions through.
