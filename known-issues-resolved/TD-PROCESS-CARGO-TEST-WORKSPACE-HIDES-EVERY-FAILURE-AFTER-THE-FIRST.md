### TD-PROCESS-CARGO-TEST-WORKSPACE-HIDES-EVERY-FAILURE-AFTER-THE-FIRST. A polkit test broke in June and stayed green-looking for two months — 2026-08-12 — ✅ FIXED 2026-08-12 (`scripts/workspace-test.py`)

**What.** `cargo test --workspace` stops at the first failing *target*. With
~3000 test targets in this workspace, one failure early in the alphabet means
every crate after it is never even built, let alone run — and the summary line
you skim looks like an ordinary single-test failure rather than "and N hundred
targets did not run."

**How it bit.** `userspace/polkit`'s `test_pattern_trailing_star` was
invalidated by the June `OuRoS → Slate OS` rename (commit `6a7212573`, fixed in
`b7618cc38`). It went unnoticed until 2026-08-12 because `apps/diffcore` — which
sorts earlier — was itself failing intermittently
(`TD-DIFFCORE-AN-MTIME-RECORDED-IN-THE-SAME-TICK-AS-ITS-OWN-WRITE-WAS-TRUSTED`,
fixed in `306f55e14`). Fixing diffcore is what finally exposed polkit.

**Two more traps in the same family**, both also hit on 2026-08-12 and both
worse than the first because they make a *failing* run report success:

- **Piping to `tail`.** A shell pipeline's exit status is the last command's, so
  `cargo test … | tail -60` exits 0 for a failing run *and* truncates away every
  failure not in the last 60 lines. This is how the polkit failure was nearly
  missed a second time — the runner printed `FAIL (exit 101)` inside the
  captured text while the tool reported exit code 0.
- **Trusting the exit code alone.** Under `--no-fail-fast` cargo's status says
  only that *something* failed, never what, and a build/launch error is
  indistinguishable from a red test.

**Fix.** `scripts/workspace-test.py` — the gate with all three mistakes made
impossible. It hard-codes `--no-fail-fast`, runs through `run-timeout.py` (so a
deadlocked test cannot hang the tree), redirects to `build/workspace-test.log`
instead of piping, and then *parses the log*: it prints the passing-target count
and, on failure, every individual failing test by name. It distinguishes "a test
failed" (exit 1) from "timed out" (124) from "nothing failed but the runner
still errored, i.e. a build error" — that last case used to be silently reported
as a green suite.

```bash
python scripts/workspace-test.py                    # the whole workspace
python scripts/workspace-test.py -p diffcore        # narrowed re-run
```

Unrecognised arguments pass through to cargo verbatim, and naming packages
replaces `--workspace` rather than adding to it (cargo rejects the combination).

**Verified both ways:** the pass path on a two-crate scope, and the
failure-detection regexes replayed against the archived log of the run that
caught polkit — they recover `tests::test_pattern_trailing_star` by name.

**Remaining.** CLAUDE.md's "Running Tests" section still documents the bare
`cargo test --workspace`. That file is operator-owned and may only be edited on
an explicit instruction, so the operator should be asked to point it at the
script. Until then this entry is the pointer.
