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
        # A test that fails prints the text the program drew, and a program may
        # legitimately draw a character the console's code page cannot spell:
        # gomoku's score panel reads "* Black" with U+25CF, and decoding cargo's
        # UTF-8 output as Windows cp1252 raised UnicodeDecodeError *inside*
        # `subprocess.run`, so `out.stdout` came back None and the sweep died on
        # the next line with a TypeError rather than reporting the mutant.  The
        # harness must be able to read output it cannot render.
        encoding="utf-8",
        errors="replace",
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


def check_the_table(original, mutations):
    """Report every unusable row in the table at once, before any build time.

    Four ways a row says nothing, all of them silent at run time:

    * **The anchor does not appear exactly once.**  Zero and the mutation is
      never applied; more than one and it is applied in places the row does not
      claim to be about.
    * **`old == new`.**  The "mutant" is the program.  Nothing fails, and the
      row is scored `SURVIVED` -- a coverage hole reported where there is none.
    * **`expect` names a test that does not exist.**  The expectation can never
      be met, so the row reports `WRONG TESTS` for as long as it survives.

    An **empty** `expect` is not a problem: it is the table's way of saying "no
    named test can report this, because the program dies first" -- maze's
    `the search renumbers cells it has already reached` re-queues cells until the
    binary is killed on a two-gigabyte allocation.  `sweep` gives that spelling
    teeth (see the `not expect` arm); it used to be the most dangerous row a
    table could hold, because `set() <= failed` holds for *every* `failed`, the
    empty set included, so such a row was scored `[ok]` whatever happened.

    Doing this up front rather than per-mutation is the whole point.  Each row
    costs a build and a suite run -- around fifteen seconds here -- so a table of
    128 spends half an hour to report what one pass over a string already knows.
    That is not hypothetical: snippets' first sweep spent 296 seconds to
    discover that 3 of 19 anchors were mis-indented, and the full table had 8.

    Returns the number of problems; prints each one.
    """
    # Rust test functions take no arguments, so this is exact enough to be worth
    # trusting: it will not match a helper that takes one, and every `#[test]`
    # in the file is of this shape.
    defined = set(re.findall(r"fn\s+([a-z0-9_]+)\s*\(\s*\)", original))
    problems = 0
    for name, old, new, expect in mutations:
        n = original.count(old)
        if n != 1:
            print(f"[table] anchor appears {n}x: {name}")
            print(f"        {old!r}")
            problems += 1
        if old == new:
            print(f"[table] mutation changes nothing: {name}")
            problems += 1
        for t in expect:
            if t not in defined:
                print(f"[table] no such test `{t}`: {name}")
                problems += 1
    return problems


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
    selected = [
        (name, to_source_eol(old), to_source_eol(new), expect)
        for name, old, new, expect in mutations
        if not only or any(o in name for o in only)
    ]

    # Cheapest check first: one pass over a string, before a compiler is started.
    problems = check_the_table(original, selected)
    if problems:
        bak.unlink(missing_ok=True)
        print(f"\n{problems} unusable row(s) in the table.  Fix them first: a row")
        print("that cannot be applied is coverage that is not being verified.")
        return 2

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
        # There used to be a `original.count(old) != 1` skip here.  It could not
        # fire: `original` is read once and every mutant is built from it, so the
        # count this loop would compute is the count `check_the_table` already
        # computed over the same string a moment ago -- a guard in front of a
        # rule that already holds (`known-issues.md` lesson 51), and one no test
        # could reach to own.  The check moved up rather than being duplicated.
        for name, old, new, expect in selected:
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
                # A timeout is scored `[ok]`, so it is the one verdict that can
                # award coverage to a test that did nothing -- and the clock is
                # not evidence on its own.  Three lanes share this machine: on
                # 2026-08-30 a lane-C asteroids build sat at 300s with its own
                # rustc using 26 MB while lane A's kernel rustc held 2.4 GB and
                # the CPU.  Builds survive that, being outside the timed window
                # (`build_tests`), but the test run is not, and asteroids' suite
                # finishes in 0.7s -- so the 240s budget is a ~340x margin that
                # a kernel build can plausibly eat.  A starved run scored "caught
                # by a hang" retires the row's coverage in silence, which is the
                # exact failure `build_tests`' own comment calls the worst kind.
                #
                # So confirm it.  A genuine unbounded loop hangs every time it
                # is asked; contention almost never repeats on demand.  This
                # costs one extra run per timeout, and timeouts are rare -- the
                # price is paid only where the verdict was going to be doubtful.
                print(f"[....] {name}: timed out; re-running to tell a hang from a slow machine")
                compiled, ran, failed, timed_out, crashed, out = run_tests(crate, timeout)
            if timed_out:
                verdicts.append((name, "caught by a hang"))
                print(f"[ok]   {name}: caught \u2014 the suite hung twice")
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
            elif not expect:
                # An empty expectation means "no named test can report this --
                # the program dies before one can".  The `timed_out` and
                # `crashed` arms above are the only ways such a row may pass, so
                # reaching here means it stopped dying, and the row must say so.
                #
                # This arm is the whole reason the spelling is safe.  Without it
                # the `set(expect) <= failed` below caught the empty case first,
                # and `set() <= failed` is true for *every* `failed` including
                # the empty one -- so a row that had quietly stopped killing the
                # program was scored `[ok] caught ()` with nothing having failed
                # at all.  maze has carried such a row since it was written.
                verdicts.append((name, f"NO LONGER DIES: {sorted(failed)}"))
                print(f"[BAD]  {name}: expected the program to die; it survived")
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
    # silent -- the mutation was never applied, so the coverage it stood for
    # quietly stopped being verified, inside a run that still ends "0 survived".
    # The commonest cause of that, an anchor the production code moved out from
    # under, is now a hard stop before the run rather than a skip inside it; what
    # remains here is the mutant that would not compile, which is a real finding
    # about the code (see snake's `let mut moves = 0;`) and not a table typo.
    bad = [(n, v) for n, v in verdicts if not v.startswith("caught")]
    if bad:
        print(f"\nFAIL: {len(bad)} of {len(verdicts)} mutation(s) not caught as named:")
        for name, v in bad:
            print(f"  {v:<32} {name}")
        return 1
    print(f"\nOK: all {len(verdicts)} mutation(s) caught by the tests named for them.")
    return 0
