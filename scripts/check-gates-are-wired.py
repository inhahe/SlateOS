#!/usr/bin/env python3
r"""Find `scripts/check-*.py` gates that nothing actually runs.

WHY THIS EXISTS
---------------
A checker under `scripts/` looks like an enforced rule.  It is only enforced if
something *calls* it.  `scripts/boot-test.sh` -- the gate that blocks a merge --
does not glob the directory; it names each checker in an explicit `run_checker`
call.  A gate that is not named there runs only in `scripts/pre-boot.py`, a
local pre-flight nobody is obliged to run and which takes about forty minutes.

Measured 2026-09-02: nine of thirty-one gates were not named by the boot test,
and eight of those nine were absent from the push hook too.  The rules they
enforce could be broken, merged and pushed with nothing objecting.  See
known-issues.md -> TD-B-TEN-GATES-ARE-NEVER-ASKED.

This is the sibling of `check-gates-can-refuse.py`.  That one asks "can this
gate return non-zero?"; this one asks "does anything ask it?".  A gate fails to
enforce anything if *either* answer is no, and the two failures are
indistinguishable from a green log, because in both cases the evidence is an
absence.

WHY IT IS A RATCHET AND NOT A GATE
----------------------------------
Eight gates are unwired as this is written and six of them belong to lane C.
Failing outright would block all three lanes on work none of them scheduled.
So the known-unwired set is *pinned* in `PINNED` below with a reason each, and
this script fails only when the set changes.  It fails in both directions:

  - a gate that is unwired and not pinned  -- the ratchet slipping;
  - a pinned gate that is now wired        -- a stale exemption;
  - a pinned name with no file             -- an exemption for nothing.

The last two matter as much as the first.  An exemption list nobody prunes
stops describing the tree it exempts, and then it is just a list of gates
nobody has to think about.

WHY THE PARSING IS FUSSY
------------------------
Three obvious ways to measure this are wrong, and each produced a confidently
wrong number before this file existed:

1. **Grep the basename.**  Over-counts.  `boot-test.sh` discusses gates in
   prose -- including a worked example named `check-thing.py` that has never
   existed, and post-mortems naming gates it does not run.

2. **Grep non-comment text.**  Still over-counts, and the counter-example is
   the commit that motivated this file.  `b5246478b` added the refusal line

       echo "as scripts/check-doc-links.py now does." >&2

   which is executable shell naming a gate it does not run.  A wiring check
   written the obvious way would have been broken by the commit that
   motivated it.

3. **Match a literal `scripts/X.py` on the `run_checker` line.**  Under-counts,
   twice.  `boot-test.sh` wraps long calls with `\` continuations, so a
   line-at-a-time scan misses them -- it missed a gate wired minutes earlier.
   And `scripts/hooks/pre-push` never writes the path on the call: it binds

       doclinks="${repo_root:-.}/scripts/check-doc-links.py"

   two hundred lines before calling `run_checker … "$doclinks"`.  Judged
   literally, the hook runs zero checkers.

So: join `\` continuations, drop comment lines, resolve simple
`var=<path>.py` assignments, and read the script argument of each
`run_checker` call.

THE CONSERVATIVE DIRECTION
--------------------------
A `run_checker` call whose script argument cannot be resolved is **reported**,
never skipped.  Skipping it would make a wired gate look unwired -- which is
merely a false alarm -- but the same weakness pointed the other way is what
produced every wrong number above.  Erring toward noise here is the cheap
mistake; erring toward silence is the one that reintroduces the bug.

This is not a shell parser and does not try to be.  It understands the two
call shapes this repo actually uses and complains about anything else, which
is the right behaviour for a file whose whole purpose is to not quietly miss
things.
"""

from __future__ import annotations

import argparse
import contextlib
import io
import re
import sys
from pathlib import Path

SCRIPTS = Path(__file__).resolve().parent
ROOT = SCRIPTS.parent

# The files that are allowed to count as "wiring". A gate named by any of them
# is asked by something other than a local pre-flight.
CALLERS = (
    Path("scripts/boot-test.sh"),
    Path("scripts/hooks/pre-push"),
)

# Gates known to be unwired, with the reason. Pruning this list is part of
# using it: see the module docstring on stale exemptions.
PINNED: dict[str, str] = {
    # The other five lane-C gates filed here on 2026-09-02 were wired into
    # boot-test.sh on 2026-09-03 (check_lane_c_gui_gates) and their entries
    # deleted with the same commit, which is what this dict is for.
    "check-evdev-elf-asm.py":
        "lane C, and DELIBERATELY unwired: it imports `capstone`, a "
        "third-party disassembler that nothing in this repository declares as "
        "a dependency and no build step installs. Wiring it would make a pip "
        "package a hard requirement of every lane's boot test, to guard a "
        "hand-assembled byte payload in kernel/src/proc/elf.rs that changes "
        "very rarely -- and the gate would then be the reason all three lanes "
        "could not build on a fresh checkout. Its own docstring says it is a "
        "developer check, not part of the build. It exits 2 when capstone is "
        "absent (as of 2026-09-03; it used to exit 1, claiming the payload was "
        "wrong when it had not looked at it), so it is honest under pre-boot "
        "and could not be wired anyway while run_checker aborts on 2. Run it "
        "by hand after touching those byte literals. Decided by lane C "
        "2026-09-03, answering "
        "requests/b-c-six-gui-gates-are-never-run-by-anything.md",
    "check-selftest-reinit.py":
        "lane A (kernel/src); filed to lane A 2026-09-02",
    "check-libc-shape.py":
        "MUST NOT be wired as things stand: it grades a build artifact and "
        "returns 2 when libc.a is stale, and run_checker aborts the build on "
        "any exit but 0/1 -- so wiring it would stop the boot test on every "
        "worktree without a fresh sysroot. Needs an opt-in skip channel in "
        "run-checker.sh first. See known-issues.md -> "
        "TD-B-TEN-GATES-ARE-NEVER-ASKED",
}

_GATE = re.compile(r"(check-[A-Za-z0-9_.-]+\.py)")
_ANY_SCRIPT = re.compile(r"[A-Za-z0-9_.-]+\.py")
_ASSIGN = re.compile(r"^\s*([A-Za-z_][A-Za-z0-9_]*)=(.+)$")
# `run_checker` in command position: line start, after a separator, or after
# `if`/`elif`/`then`/`!`. Not inside an echo, and not as a bare substring.
_CALL = re.compile(r"(?:^|[;&|(]|\b(?:if|elif|then|else|do)\s+)\s*!?\s*run_checker\b")
_VARREF = re.compile(r"\$\{?([A-Za-z_][A-Za-z0-9_]*)\}?")
# Both spellings are live in this repo. A call carrying either runs the gate's
# own cases, not the gate, and must not count as wiring.
_SELFTEST_FLAG = re.compile(r"--self-?test\b")


def _executable_lines(text: str) -> list[str]:
    """Comment-free lines with `\\`-continuations joined onto one line."""
    text = re.sub(r"\\\r?\n", " ", text)
    return [ln for ln in text.splitlines() if not ln.lstrip().startswith("#")]


def analyse(path: Path) -> tuple[set[str], set[str], list[str]]:
    """Return (gates run, gates self-tested, calls we could not resolve).

    The first two are deliberately separate. Running a gate's own cases is not
    running the gate, and conflating them certified a deleted check as present
    -- see the comment at the `_SELFTEST_FLAG` test below.

    Unresolved calls are returned rather than dropped -- see the module
    docstring, "THE CONSERVATIVE DIRECTION".
    """
    try:
        text = path.read_text(encoding="utf-8", errors="replace")
    except OSError as exc:
        return set(), set(), [f"{path}: could not be read: {exc}"]

    bound: dict[str, str] = {}
    runs: set[str] = set()
    selftested: set[str] = set()
    unresolved: list[str] = []

    for line in _executable_lines(text):
        # Track `var=<...>.py` bindings for any script, not just check-*.py:
        # a call binding `gopt=.../getopt-ambiguity-check.py` is perfectly
        # resolvable and simply out of scope, and must not be reported as a
        # call we failed to understand.
        m = _ASSIGN.match(line)
        if m:
            hit = _ANY_SCRIPT.search(m.group(2))
            if hit:
                bound[m.group(1)] = hit.group(0)

        if not _CALL.search(line):
            continue

        # A self-test invocation does not count as wiring, and missing this
        # left a mutant alive: deleting the real
        #     run_checker check-tick-wiring "$py" ".../check-tick-wiring.py"
        # changed nothing, because the line above it runs the same script with
        # `--self-test`. So a gate whose own cases still run but whose actual
        # check has been deleted read as fully wired -- which is exactly the
        # "appears enforced, is not" failure this file exists to catch, in the
        # checker meant to catch it.
        #
        # Both spellings are in use here (`--selftest` and `--self-test`), so
        # match both rather than picking one and quietly missing the other.
        literal = _ANY_SCRIPT.findall(line)
        named = ([] if literal
                 else [bound[v] for v in _VARREF.findall(line) if v in bound])
        scripts = [n for n in (literal or named) if _GATE.fullmatch(n)]

        if _SELFTEST_FLAG.search(line):
            # Recorded separately, never as wiring. A gate's own cases are not
            # the gate -- see above. But whether they run at all is its own
            # question, and an unrun self-test rots in exactly the way a self-
            # test exists to prevent.
            selftested.update(scripts)
            continue

        if literal or named:
            runs.update(scripts)
            continue

        unresolved.append(f"{path.name}: cannot tell what this runs: "
                          f"{line.strip()[:110]}")

    return runs, selftested, unresolved


def audit(root: Path,
          pinned: dict[str, str] | None = None) -> tuple[list[str], list[str]]:
    """Return (findings, notes). Findings are failures; notes are informational.

    `pinned` is a parameter rather than a direct read of the module constant so
    the self-test can grade a synthetic tree against a synthetic exemption list.
    Grading a fixture against the *real* PINNED reports every real exemption as
    "pinned, but no such file" -- eight findings of pure noise that drown the
    one assertion under test, and make "wiring it clears the finding"
    impossible to state.
    """
    pinned = PINNED if pinned is None else pinned
    findings: list[str] = []
    notes: list[str] = []

    wired: set[str] = set()
    selftested: set[str] = set()
    for rel in CALLERS:
        caller = root / rel
        if not caller.is_file():
            # A missing caller is not "no gates are wired" -- it is a question
            # this script cannot answer, and saying "everything is unwired"
            # would be a wrong answer rather than an absent one.
            findings.append(f"{rel.as_posix()}: missing -- cannot tell what "
                            f"it runs")
            continue
        runs, tested, unresolved = analyse(caller)
        wired |= runs
        selftested |= tested
        findings.extend(unresolved)
        notes.append(f"{rel.as_posix()}: runs {len(runs)} gate(s), "
                     f"self-tests {len(tested)}")

    gates = sorted(p.name for p in (root / "scripts").glob("check-*.py"))
    if not gates:
        findings.append("scripts/: no check-*.py found -- nothing to judge, "
                        "which is not the same as a clean tree")
        return findings, notes

    unwired = [g for g in gates if g not in wired]

    for g in unwired:
        if g not in pinned:
            findings.append(
                f"{g}: nothing runs it. Wire it into scripts/boot-test.sh, or "
                f"add it to PINNED in {Path(__file__).name} with the reason.")

    for name, why in sorted(pinned.items()):
        if not (root / "scripts" / name).is_file():
            findings.append(
                f"{name}: pinned as unwired, but no such file. Remove the "
                f"PINNED entry (reason on record: {why}).")
        elif name not in unwired:
            findings.append(
                f"{name}: pinned as unwired, but something runs it now. "
                f"Remove the PINNED entry -- a stale exemption stops "
                f"describing the tree.")

    # Third arm: a gate that ships its own cases, is wired, and whose cases
    # nothing runs. That is not a wiring gap, it is a rotting one -- a
    # self-test only stays honest while something executes it. The concrete
    # case: check-gates-can-refuse.py's first version was green and wrong, and
    # only its own cases (run every time) would have caught the regression.
    #
    # Reported only for gates that ARE wired. For an unwired gate the unrun
    # self-test is a consequence of the wiring finding above, and saying it
    # twice trains the reader to skim.
    for g in gates:
        if g in unwired or g in selftested:
            continue
        try:
            text = (root / "scripts" / g).read_text(encoding="utf-8",
                                                    errors="replace")
        except OSError as exc:
            findings.append(f"{g}: could not be read: {exc}")
            continue
        if _SELFTEST_FLAG.search(text):
            findings.append(
                f"{g}: ships a self-test that nothing runs. Add a "
                f"`run_checker {g[:-3]}-selftest ... --self-test` call beside "
                f"the gate's own call, so a scanner that has stopped scanning "
                f"is reported as one.")

    notes.append(f"{len(gates)} gate(s); {len(unwired)} unwired, "
                 f"{len(pinned)} pinned; {len(selftested)} self-tested")
    return findings, notes


_FIXTURE_CALLER = """\
#!/usr/bin/env bash
# A comment naming scripts/check-commented.py must not count as wiring.
doclinks="$root/scripts/check-viavar.py"
echo "see scripts/check-echoed.py for details" >&2
if run_checker literal "$py" "$root/scripts/check-literal.py"; then :; fi
if ! run_checker viavar "$py" "$doclinks"; then :; fi
run_checker wrapped "$py" \\
    "$root/scripts/check-wrapped.py"
gopt="$root/getopt-ambiguity-check.py"
run_checker outofscope "$py" "$gopt"
run_checker mystery "$py" "$undefined_var"
if ! run_checker sto-selftest "$py" "$root/scripts/check-selftest-only.py" --selftest; then :; fi
if ! run_checker sto2-selftest "$py" "$root/scripts/check-hyphen-selftest-only.py" --self-test; then :; fi
"""


def selftest() -> int:
    import tempfile

    bad = 0
    checks = 0

    def check(ok: bool, msg: str) -> None:
        nonlocal bad, checks
        checks += 1
        if not ok:
            print(f"selftest FAIL: {msg}", file=sys.stderr)
            bad += 1

    with tempfile.TemporaryDirectory() as tmp:
        caller = Path(tmp) / "fixture.sh"
        caller.write_text(_FIXTURE_CALLER, encoding="utf-8")
        runs, tested, unresolved = analyse(caller)

        # Each of these was a real wrong answer before this file existed.
        check("check-literal.py" in runs, "a literal path must count as wiring")
        check("check-viavar.py" in runs,
              "a path bound to a variable must count (the pre-push shape)")
        check("check-wrapped.py" in runs,
              "a call wrapped with a backslash continuation must count")
        check("check-commented.py" not in runs,
              "a gate named only in a comment must NOT count")
        check("check-echoed.py" not in runs,
              "a gate named only in an echo must NOT count -- this is the "
              "shape b5246478b added")
        check(len(unresolved) == 1 and "mystery" in unresolved[0],
              f"an unresolvable call must be reported, got {unresolved!r}")
        check(not any("outofscope" in u for u in unresolved),
              "a resolvable non-gate script is out of scope, not unresolved")

        # This pair is a regression test for a mutant that survived. Deleting
        # the real run_checker call for check-tick-wiring.py changed nothing,
        # because its --self-test call kept naming the same script. Running a
        # gate's own cases is not running the gate.
        check("check-selftest-only.py" not in runs,
              "a --selftest-only invocation must NOT count as wiring")
        check("check-hyphen-selftest-only.py" not in runs,
              "the --self-test spelling must not count either")
        check(not any("selftest" in u for u in unresolved),
              "a self-test call is skipped deliberately, not unresolved")
        check(tested == {"check-selftest-only.py",
                         "check-hyphen-selftest-only.py"},
              f"self-test calls must be recorded separately, got {tested!r}")

        # The exit-code contract, asserted rather than assumed: this file's
        # own bug would be reporting a finding and exiting 0.
        #
        # Graded against an EMPTY pinned list, not the real one -- see audit()'s
        # docstring. And with both streams captured, because a self-test that
        # prints a full report drowns its own verdict.
        fake = Path(tmp) / "tree"
        (fake / "scripts" / "hooks").mkdir(parents=True)
        (fake / "scripts" / "check-orphan.py").write_text("", encoding="utf-8")
        (fake / "scripts" / "boot-test.sh").write_text("", encoding="utf-8")
        (fake / "scripts" / "hooks" / "pre-push").write_text("", encoding="utf-8")

        findings, _ = audit(fake, {})
        check(any("check-orphan.py" in f for f in findings),
              "an unwired, unpinned gate must be reported")

        real_argv = sys.argv
        try:
            def run(argv: list[str]) -> int:
                sys.argv = argv
                out, err = io.StringIO(), io.StringIO()
                with contextlib.redirect_stdout(out), \
                        contextlib.redirect_stderr(err):
                    return main(_root=fake, _pinned={})

            check(run(["x"]) == 1, "a bare run with a finding must exit 1")
            check(run(["x", "--list"]) == 0, "--list reports without failing")

            # A pinned entry naming a file that is not there is a finding in
            # its own right: an exemption list nobody prunes stops describing
            # the tree it exempts.
            check(run(["x"]) == 1 and
                  any("no such file" in f
                      for f in audit(fake, {"check-ghost.py": "gone"})[0]),
                  "a pinned entry with no file must be reported")

            # And a pinned entry that is now wired.
            (fake / "scripts" / "boot-test.sh").write_text(
                'run_checker orphan "$py" "$r/scripts/check-orphan.py"\n',
                encoding="utf-8")
            check(any("something runs it now" in f
                      for f in audit(fake, {"check-orphan.py": "stale"})[0]),
                  "a stale exemption must be reported")

            # The complement, and the one that matters most: wiring the gate
            # clears the finding. Without it this could report everything and
            # still pass every case above.
            check(run(["x"]) == 0,
                  "wiring the gate must clear it -- otherwise this reports "
                  "everything and discriminates nothing")

            # Third arm: the gate is wired, ships a self-test, and nothing
            # runs it. Live case when this was written: check-option-refusal.py.
            (fake / "scripts" / "check-orphan.py").write_text(
                'if "--self-test" in sys.argv:\n    pass\n', encoding="utf-8")
            check(any("nothing runs" in f
                      for f in audit(fake, {})[0]),
                  "a wired gate whose self-test nothing runs must be reported")

            (fake / "scripts" / "boot-test.sh").write_text(
                'run_checker orphan "$py" "$r/scripts/check-orphan.py"\n'
                'run_checker orphan-selftest "$py" '
                '"$r/scripts/check-orphan.py" --self-test\n',
                encoding="utf-8")
            check(run(["x"]) == 0,
                  "adding the self-test call must clear it")

            # And the arm must not fire for an unwired gate, where it would
            # merely restate the wiring finding.
            (fake / "scripts" / "boot-test.sh").write_text("", encoding="utf-8")
            found = audit(fake, {})[0]
            check(not any("nothing runs it. Add" in f or
                          "ships a self-test" in f for f in found),
                  "an unwired gate's unrun self-test must not be said twice")
        finally:
            sys.argv = real_argv

    if bad:
        print(f"selftest: {bad} of {checks} cases FAILED", file=sys.stderr)
        return 1
    print(f"selftest: {checks}/{checks} cases pass")
    return 0


def main(_root: Path | None = None,
         _pinned: dict[str, str] | None = None) -> int:
    ap = argparse.ArgumentParser(description=__doc__.split("\n", 1)[0])
    ap.add_argument("--list", action="store_true",
                    help="print findings and exit 0")
    ap.add_argument("--selftest", action="store_true",
                    help="verify the checker itself")
    args = ap.parse_args()

    if args.selftest:
        return selftest()

    root = _root if _root is not None else ROOT
    findings, notes = audit(root, _pinned)

    for n in notes:
        print(n)
    for f in findings:
        print(f)
    # Flush before stderr: run_checker merges both streams into one log, and
    # Python block-buffers stdout to a file while stderr is unbuffered, so the
    # summary would otherwise overtake the findings it refers to.
    sys.stdout.flush()

    if args.list:
        print(f"\n{len(findings)} finding(s).")
        return 0
    if findings:
        print(f"\n{len(findings)} gate-wiring finding(s).", file=sys.stderr)
        return 1
    print("ok -- every gate is either run by something or pinned with a reason.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
