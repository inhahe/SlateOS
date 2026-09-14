#!/usr/bin/env python3
"""workspace-test.py — the workspace test gate, with the three mistakes
that hide failures made impossible.

`cargo test --workspace` is easy to run in a way that reports success while
hiding real failures.  Three separate traps, all of which were hit on
2026-08-12 (see `known-issues.md`,
`TD-PROCESS-CARGO-TEST-WORKSPACE-HIDES-EVERY-FAILURE-AFTER-THE-FIRST`):

1. **Fail-fast.** Cargo stops at the first failing *target*.  With ~3000 test
   targets here, one early failure means every crate after it is never built,
   let alone run — and the summary you skim looks like an ordinary one-test
   failure rather than "and several hundred targets did not run."  A polkit
   test stayed broken for two months behind an unrelated `diffcore` failure
   because of exactly this.  So: always `--no-fail-fast`.

2. **Piping to `tail`.** A shell pipeline's exit status is the *last*
   command's, so `cargo test … | tail -60` exits 0 for a failing run and
   truncates away every failure that is not in the final 60 lines.  So: never
   pipe; redirect to a log and summarise the log afterwards.

3. **Trusting the exit code alone.** `--no-fail-fast` means cargo's status is
   the only signal that *something* failed, but it says nothing about what.
   So: scan the log for every `test result: FAILED` and every individual
   `… FAILED` line, and print them.

AND THE SAME TRAP ONE LEVEL UP, which this script cannot defend against and
which caught its own author on 2026-09-14.  Trap 2 is about piping *cargo*;
piping **this script** does it again.  `workspace-test.py | tail -3` exits 0
for a failing run, because a pipeline's status is still the last command's --
and the verdict line is only in the last three when the run is clean.  A red
run prints the failing test names *after* the summary, so `tail` shows a test
name and a zero status, which reads exactly like a pass.

Run it unpiped and look at `$?`:

    python scripts/workspace-test.py > /tmp/wt.txt 2>&1; echo "exit: $?"

That session had been piping it for hours and was never misled, because every
run until then happened to print PASS inside the window.  Not being caught by
a trap you are standing in is not the same as avoiding it.

A deadlocked test never exits on its own, so the run goes through
`run-timeout.py`, which holds the whole process tree in one killable unit.

Usage:
    python scripts/workspace-test.py [--timeout SECS] [--target TRIPLE]
                                     [--log PATH] [-- <extra cargo args>]

Exit codes:
    0    every target passed
    1    at least one test failed (the failures are printed)
    124  timed out — the process tree was killed
    other  passed through from the runner
"""

import argparse
import re
import subprocess
import sys
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent

# The host triple the workspace's unit tests are run under.  Not the OS's own
# `x86_64-slateos` target: these are host-side tests of logic that also
# compiles into the OS, and `slateos` has no test harness to run them with.
DEFAULT_TARGET = "x86_64-pc-windows-gnu"

# Measured, not guessed: a cold run on 2026-09-01 — 1910 crates compiled, ~2240
# test targets, ~20600 tests — took **4468 s (74 min)** wall clock while the
# other two lanes were building concurrently.  The previous value here was 2400,
# derived from a ~13 min run on an otherwise idle machine, i.e. it was set from
# the best case and would have killed that run at 54% complete and reported it
# as a hang.
#
# 9000 is 2x the measured worst case.  A timeout must bound *genuine hangs*, and
# a genuine hang is unbounded — so the only cost of setting this too high is
# waiting longer to learn about a deadlock, whereas setting it too low
# manufactures failures that look exactly like the thing it is watching for.
# Prefer raising it again over trimming it toward the observed time.
DEFAULT_TIMEOUT = 9000

# `test result: FAILED. 82 passed; 1 failed; …` — the per-target summary.
RESULT_FAILED = re.compile(r"^test result: FAILED", re.MULTILINE)
# `test tests::some_name ... FAILED` — the individual test.
TEST_FAILED = re.compile(r"^test (.+) \.\.\. FAILED\s*$", re.MULTILINE)
# `test result: ok. 12 passed; …` — counted only to report scale.
RESULT_OK = re.compile(r"^test result: ok\b", re.MULTILINE)


def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--timeout", type=int, default=DEFAULT_TIMEOUT,
                        help=f"hard timeout in seconds (default {DEFAULT_TIMEOUT})")
    parser.add_argument("--target", default=DEFAULT_TARGET,
                        help=f"rustc target triple (default {DEFAULT_TARGET})")
    parser.add_argument("--log", default=None,
                        help="log path (default build/workspace-test.log)")
    parser.add_argument("--poll", type=int, default=60,
                        help="heartbeat interval in seconds (default 60)")
    # `parse_known_args` rather than a positional `nargs="*"`: pass-through args
    # are typically flags (`-p diffcore`, `--lib`), and argparse refuses to put
    # anything starting with `-` into a positional, so the positional form fails
    # on exactly the arguments people actually pass. Anything this parser does
    # not recognise goes to cargo verbatim, with or without a `--` separator.
    args, cargo_args = parser.parse_known_args(argv[1:])
    if cargo_args and cargo_args[0] == "--":
        cargo_args = cargo_args[1:]

    log_path = Path(args.log) if args.log else REPO_ROOT / "build" / "workspace-test.log"
    log_path.parent.mkdir(parents=True, exist_ok=True)

    # `cargo test --workspace -p foo` is rejected by cargo, so naming
    # packages (which the smoke tests and any narrowed re-run do) replaces
    # the whole-workspace selection rather than adding to it.
    scope = [] if any(a in ("-p", "--package") for a in cargo_args) else ["--workspace"]
    cmd = [
        sys.executable, str(REPO_ROOT / "scripts" / "run-timeout.py"),
        "--poll", str(args.poll), str(args.timeout),
        "cargo", "test", *scope, "--no-fail-fast",
        "--target", args.target,
        *cargo_args,
    ]

    print(f"[workspace-test] {' '.join(cmd[2:])}")
    print(f"[workspace-test] log: {log_path}")
    # No pipeline, no `tail`: the child writes straight into the log so the
    # status we observe is the runner's own and the log is complete.
    with log_path.open("wb") as log:
        runner = subprocess.run(cmd, cwd=REPO_ROOT, stdout=log,
                                stderr=subprocess.STDOUT, check=False)

    # Test binaries can emit stray non-UTF-8 bytes; a decode error here must not
    # be mistaken for a test failure, so replace rather than raise.
    text = log_path.read_text(encoding="utf-8", errors="replace")

    ok_targets = len(RESULT_OK.findall(text))
    failed_targets = len(RESULT_FAILED.findall(text))
    failed_tests = TEST_FAILED.findall(text)

    print(f"[workspace-test] targets passed: {ok_targets}")

    if failed_tests or failed_targets:
        print(f"[workspace-test] targets FAILED: {failed_targets}")
        print(f"[workspace-test] failing tests ({len(failed_tests)}):")
        for name in failed_tests:
            print(f"    {name}")
        print(f"[workspace-test] full output: {log_path}")
        return 1

    if runner.returncode == 124:
        print(f"[workspace-test] TIMED OUT after {args.timeout}s — tree killed."
              f" See {log_path}")
        return 124

    if runner.returncode != 0:
        # No test reported failure, so this is a build/link/launch error rather
        # than a red test — say so instead of claiming the suite is green.
        print(f"[workspace-test] runner exited {runner.returncode} with no failing"
              f" test — build or launch error. See {log_path}")
        return runner.returncode

    # A fourth trap, added 2026-09-13 by lane C, and the same shape as the
    # three above: ZERO PASSING TARGETS IS NOT A PASS.
    #
    # Every check to this point asks whether something went wrong. None asks
    # whether anything happened. A run that produced no `test result` line at
    # all -- a target triple with no std, a `--target` typo, a filter that
    # matched nothing, cargo output going somewhere other than this log --
    # reaches here with no failures and a zero exit, and prints
    # "targets passed: 0" immediately above "PASS". The two lines disagree and
    # the second is the one people act on.
    #
    # This is the defect the whole file is about, one level up, and it is what
    # `TD-C-A-TEST-BINARY-CAN-BE-BROKEN-WITHOUT-ANYONE-NOTICING` describes at
    # the scale of one crate: a population that reports nothing is
    # indistinguishable from a population with nothing wrong, unless somebody
    # counts it. Exit 2 rather than 1, because the tree is not red -- the
    # measurement is missing.
    if ok_targets == 0:
        print("[workspace-test] NO TARGET REPORTED A RESULT - this is not a pass.")
        print("    A run with nothing to report and nothing wrong looks exactly")
        print("    like a clean one. Check the target triple, any package")
        print(f"    filter, and that cargo output reached {log_path}.")
        return 2

    print("[workspace-test] PASS")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
