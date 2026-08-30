"""The mutation-sweep harness shared by every app's `mutate.py`.

A sweep breaks one piece of production code at a time and checks that the test
which claims to cover it is the one that fails.  A test that passes against a
broken program is not testing the program.

## Why this file exists

It used to not.  Each `apps/<app>/mutate.py` carried its own copy of the whole
harness -- the runner, the verdict classification, the backup, the restore and
the summary -- and only the `MUTATIONS` table at the top was genuinely per-app.
Thirteen copies had accumulated when two separate correctness bugs were found
in one of them on the same day, and both fixes landed in that one copy while
the other twelve kept the faults:

* **A run killed by something that never asks for cleanup** -- a SIGKILL, or the
  machine going down -- leaves the source mutated and the truth in the `.bak`.
  The `finally` below cannot run in that case.  A mutant still *compiles*, so
  the next run reads it as `original` and mutates on top of it; every verdict
  then describes a program already broken somewhere else, and a sweep reporting
  `[ok]` throughout proves nothing.  See `refuse_a_dirty_start`.
* **The exit code was always 0.**  A sweep returning 182 `[ok]`, one `[skip]`
  and two wrong-test verdicts looked from the outside exactly like a clean one.
  The three were found by reading 185 lines of log, which is a habit, not a
  check.  See the end of `sweep`.

That is `known-issues.md` lesson 63 -- a rule kept only by copying is a rule
that will be dropped -- landing in the tooling that exists to enforce it.  The
tables stay per-app: they are the part that is genuinely different, and the part
worth reading.

## Using it

    import sys
    from pathlib import Path

    sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "scripts"))
    from mutation_harness import sweep

    MUTATIONS = [ (name, old, new, [tests that must fail]), ... ]

    if __name__ == "__main__":
        sys.exit(sweep(Path(__file__).parent / "src" / "main.rs", MUTATIONS, "myapp"))

It lives in `scripts/` and not in `apps/` for a reason recorded at `REPO` below:
`apps/*` is a workspace member glob, and a Python module imported from there
leaves a `__pycache__` directory that cargo reads as a crate.
"""

import difflib
import re
import subprocess
import sys
from pathlib import Path

# The repository root, from `scripts/mutation_harness.py`.
#
# This module must NOT live in `apps/`.  The workspace lists `apps/*` among its
# members, so importing it from there created `apps/__pycache__/`, cargo tried to
# read `apps/__pycache__/Cargo.toml`, and every `cargo test` in the tree failed
# before running a single test.  `__pycache__/` is in `.gitignore`, so the
# directory that broke the build never appeared in `git status`.
REPO = Path(__file__).resolve().parent.parent


def build_tests(crate, timeout=1800):
    """Compile the crate's test binaries without running them.

    The per-mutation timeout exists to catch a mutant that loops forever, so it
    is sized for how long the *tests* take -- sliding allows 120s.  But
    `cargo test` builds before it runs, and a cold workspace build is far slower
    than any suite: sliding's baseline was killed at 120s having compiled
    dependencies the whole time, and the first mutation after a cold build would
    have been scored "caught by a hang" for the same reason -- a false `[ok]`,
    which is the worst kind.

    So the expensive build is paid once, here, outside the timed window.  Every
    run after this one recompiles a single crate, which is what the timeout was
    calibrated against.

    Its output is deliberately NOT captured.  A cold build is ten minutes long,
    and a silent ten minutes is indistinguishable from a hang to whoever is
    watching -- which is the failure this whole file exists to stop making.
    Letting cargo and the runner's heartbeat through means the slowest step of a
    sweep reports that it is alive.
    """
    return subprocess.run(
        [
            "python",
            "scripts/run-timeout.py",
            "--poll",
            "60",
            str(timeout),
            "cargo",
            "test",
            "-p",
            crate,
            "--target",
            "x86_64-pc-windows-gnu",
            "--no-run",
        ],
        cwd=REPO,
    )


def run_tests(crate, timeout):
    """Run one crate's suite and classify what happened to it.

    Returns `(compiled, ran, failed, timed_out, crashed, out)`, where `failed` is
    the set of test names that reported a failure and `ran` says whether a test
    binary actually started.
    """
    out = subprocess.run(
        [
            "python",
            "scripts/run-timeout.py",
            str(timeout),
            "cargo",
            "test",
            "-p",
            crate,
            "--target",
            "x86_64-pc-windows-gnu",
        ],
        capture_output=True,
        text=True,
        cwd=REPO,
    )
    failed = set(re.findall(r"^    tests::(\S+)$", out.stdout, re.M))
    compiled = "could not compile" not in out.stdout + out.stderr
    timed_out = out.returncode == 124
    # Did a test binary actually start?  Without this the harness cannot tell a
    # mutant that killed the tests from a cargo that never reached them, and it
    # scored the second as the first: a stray `apps/__pycache__` made every
    # invocation fail to load the workspace manifest, and a 20-mutation sweep
    # reported all 20 "caught by a crash" and exited 0 having run no test at all.
    ran = re.search(r"^running \d+ tests?$", out.stdout, re.M) is not None
    # An unbounded loop does not fail a test; it runs until the runner kills it.
    # A harness that only counted named failures would score that as a mutant
    # nobody noticed, which is the opposite of the truth: the hang IS the
    # symptom.  Same for a mutant that aborts before any test can report -- but
    # only if the tests got far enough to be aborted.
    crashed = compiled and ran and not timed_out and not failed and out.returncode != 0
    return compiled, ran, failed, timed_out, crashed, out


def refuse_a_dirty_start(src, bak):
    """Stop if a previous run died without restoring the source.

    The `finally` in `sweep` covers a Ctrl-C, an exception and a full disk, but
    it cannot cover the two ways a process is *not* asked to clean up: a SIGKILL
    and the machine going down.  This actually happened -- a restart during a
    sweep left `apps/simon/src/main.rs` holding a live mutation (`Layout`'s
    `avail_h` missing its `pad * 2.0`) and `main.rs.bak` holding the truth.

    Restoring the backup automatically is no safer than proceeding: if the
    source was edited since the crash -- the usual reason to rerun -- the
    restore silently throws those edits away.  Neither choice is safe to make
    silently, so this makes neither.
    """
    if not bak.exists():
        return
    src_text = src.read_text(encoding="utf-8", newline="")
    bak_text = bak.read_text(encoding="utf-8", newline="")
    print(f"{bak.name} exists: a previous run did not restore the source.")
    if src_text == bak_text:
        print("The source matches it, so nothing was lost -- removing the backup.")
        bak.unlink()
        return
    print("\nThe source differs from the backup.  Either the run died holding a")
    print("mutation, or the source was edited after it died.  Diff (backup -> source):\n")
    for line in difflib.unified_diff(
        bak_text.splitlines(),
        src_text.splitlines(),
        bak.name,
        src.name,
        lineterm="",
        n=2,
    ):
        print("  " + line)
    print("\nDecide which one is the real program, then rerun:")
    print(f"  the backup is  ->  cp {bak} {src} && rm {bak}")
    print(f"  the source is  ->  rm {bak}")
    sys.exit(2)


def sweep(src, mutations, crate, timeout=240, only=None):
    """Apply each mutation in turn and report which tests noticed.

    `src` is the file to mutate, `mutations` the `(name, old, new, expect)`
    table, `crate` the cargo package whose suite to run.  `only` filters the
    table by substring, defaulting to the command line.  Returns a process exit
    code: 0 only if every mutation ran and was caught by the tests named for it.
    """
    bak = src.with_suffix(src.suffix + ".bak")
    refuse_a_dirty_start(src, bak)

    # Written fresh every run and removed at the end.  It exists only so a
    # Ctrl-C mid-mutation leaves the real program on disk.  It must never be
    # *reused* across runs: a stale backup restored over a fixed source silently
    # throws away every fix made since, and then reports the same survivors --
    # output that looks like evidence and is not.
    original = src.read_text(encoding="utf-8", newline="")
    bak.write_text(original, encoding="utf-8", newline="")

    # The source is read and written byte-exact -- `newline=""` -- so that a
    # restore puts back what was there rather than a re-line-ended copy of it.
    # The mutation tables, though, are Python literals written with `\n`, so a
    # working copy that has picked up CRLF matches none of the multi-line
    # anchors and every one of them is reported as `SKIP anchor appears 0x`.
    #
    # That is a failure mode with no symptom worth trusting: a sweep is expected
    # to skip the odd anchor, so 35 skips among 75 reads as a table that has
    # drifted from the code rather than as a table that was never applied at
    # all.  It cost a 20-minute run on wordle, whose `main.rs` had been rewritten
    # by a Python splice that let the platform choose the line ending.  Meeting
    # the file where it is costs one substitution and cannot be got wrong.
    eol = "\r\n" if "\r\n" in original else "\n"

    def to_source_eol(text):
        return text.replace("\n", eol) if eol != "\n" else text

    verdicts = []
    only = sys.argv[1:] if only is None else only

    # Prove the suite passes on the REAL program before believing anything about
    # a broken one.  A sweep that never establishes this cannot tell "the
    # mutation was caught" from "nothing works here": when `apps/__pycache__`
    # broke the workspace manifest, all 20 of sliding's mutations were scored
    # `[ok] caught -- the harness died` and the run exited 0 without compiling a
    # single test.  One run against the unmutated source, up front, is the
    # cheapest possible check and it is the one that catches that whole class.
    print(f"warm-up: building {crate}'s tests, outside the sweep's timeout ...")
    b = build_tests(crate)
    if b.returncode != 0:
        bak.unlink(missing_ok=True)
        # The compiler's own diagnostics are already above this line: the build
        # streams rather than being captured, so there is nothing to reprint.
        print(f"\nThe tests do not build (exit {b.returncode}).  Fix that first.")
        return 2

    print(f"baseline: running {crate}'s suite against the unmutated source ...")
    compiled, ran, failed, timed_out, crashed, out = run_tests(crate, timeout)
    if not (compiled and ran and not failed and out.returncode == 0):
        bak.unlink(missing_ok=True)
        print("\nThe suite does not pass on the unmutated source, so no verdict")
        print("this sweep produced could mean anything.  Fix that first.\n")
        print(f"  compiled={compiled} tests-ran={ran} timed-out={timed_out}")
        print(f"  exit={out.returncode} failed={sorted(failed)}\n")
        print(out.stdout[-2000:] or out.stderr[-2000:])
        return 2
    print("baseline: clean.\n")

    try:
        for name, old, new, expect in mutations:
            if only and not any(o in name for o in only):
                continue
            old, new = to_source_eol(old), to_source_eol(new)
            if original.count(old) != 1:
                verdicts.append((name, f"SKIP anchor appears {original.count(old)}x"))
                print(f"[skip] {name}: anchor appears {original.count(old)} times")
                continue
            src.write_text(original.replace(old, new), encoding="utf-8", newline="")
            # Build this mutant OUTSIDE the timed window, every time -- not just
            # once at startup.  The timeout is there to catch a mutant that loops
            # forever, so anything in that window that is not the tests running
            # is a chance to call slowness a hang.  That is not hypothetical: on
            # sliding, the one genuinely unbounded mutation timed out, the kill
            # left build work to redo, and the redo landed inside the NEXT
            # mutation's 120s -- so the two mutations after it were also scored
            # "caught by a hang", and neither had hung.  A hang verdict never
            # checks the expected tests, so a cascade of them silently retires
            # the coverage the table was written to prove.
            b = build_tests(crate)
            if b.returncode != 0:
                verdicts.append((name, "SKIP did not compile"))
                print(f"[skip] {name}: mutant did not compile")
                src.write_text(original, encoding="utf-8", newline="")
                continue
            compiled, ran, failed, timed_out, crashed, out = run_tests(crate, timeout)
            if timed_out:
                verdicts.append((name, "caught by a hang"))
                print(f"[ok]   {name}: caught \u2014 the suite hung")
            elif not compiled:
                verdicts.append((name, "SKIP did not compile"))
                print(f"[skip] {name}: mutant did not compile")
                print(out.stdout[-1500:])
            elif not ran:
                # The baseline passed, so the tree was fine a moment ago and this
                # is not a verdict about the mutation -- something broke under
                # the sweep.  Scoring it would be inventing evidence, and going
                # on would invent it once per remaining mutation.
                print(f"\n[STOP] {name}: no test binary ran, but the mutant compiled.")
                print("The tree changed under the sweep; this is not a verdict.\n")
                print(out.stdout[-2000:] or out.stderr[-2000:])
                raise SystemExit(2)
            elif crashed:
                verdicts.append((name, "caught by a crash"))
                print(
                    f"[ok]   {name}: caught \u2014 the harness died (exit {out.returncode})"
                )
            elif set(expect) <= failed:
                verdicts.append((name, f"caught by {len(failed)} test(s)"))
                print(f"[ok]   {name}: caught ({', '.join(sorted(failed))})")
            elif failed:
                verdicts.append((name, f"WRONG TESTS: {sorted(failed)}"))
                print(f"[??]   {name}: expected {expect}, got {sorted(failed)}")
            else:
                verdicts.append((name, "SURVIVED"))
                print(f"[BAD]  {name}: SURVIVED \u2014 no test failed")
            src.write_text(original, encoding="utf-8", newline="")
    finally:
        # Whatever happens -- a Ctrl-C, an exception, a full disk -- the tree is
        # left with the real program in it and not a mutant, and with no backup
        # for the next run to mistake for the truth.
        src.write_text(original, encoding="utf-8", newline="")
        bak.unlink(missing_ok=True)

    print("\n=== summary ===")
    for name, v in verdicts:
        print(f"{v:<34} {name}")

    # The run is green only if every mutation was caught by the tests named for
    # it.  A `[skip]` counts as a failure and that is the important half.  A
    # survivor is loud: some test should have failed and none did.  A skip is
    # silent -- the anchor stopped matching because the production code moved
    # under it, so the mutation was never applied and the coverage it stood for
    # quietly stopped being verified, inside a run that still ends "0 survived".
    bad = [(n, v) for n, v in verdicts if not v.startswith("caught")]
    if bad:
        print(f"\nFAIL: {len(bad)} of {len(verdicts)} mutation(s) not caught as named:")
        for name, v in bad:
            print(f"  {v:<32} {name}")
        return 1
    print(f"\nOK: all {len(verdicts)} mutation(s) caught by the tests named for them.")
    return 0
