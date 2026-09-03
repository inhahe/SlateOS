#!/usr/bin/env python3
"""Refuse a `check-*.py` gate that cannot fail when run the way it is run.

WHY THIS EXISTS
---------------
On 2026-09-02 `scripts/check-doc-links.py` was found to be structurally
incapable of failing. Run bare -- which is exactly how `pre-boot.py`'s
`check-*.py` glob runs every gate -- it scanned the whole tree, printed every
dead intra-doc link it found, and returned 0, because its only `return 1` sat
under `if args.check:` and nothing passed `--check`. It had done that for 412
seconds of every pre-boot run.

Three things kept it hidden, and the third is the one this file answers:

1. The findings *were* printed, but `pre-boot.py`'s `_report` discarded the
   output of a gate that exited 0.
2. The push hook passes `--check` explicitly, so the gate genuinely worked at
   the place people trust most.
3. **Its self-test was thorough about the wrong layer.** All 52 cases asked
   "does it *find* the dead link?"; none asked "does finding one make the
   process exit non-zero?" Detection was perfect and enforcement was absent,
   and a suite aimed at the finder cannot tell those two apart.

A gate has two halves -- notice the problem, and *refuse* because of it. Every
existing test in this directory tests the first half. This tests the second,
and it tests it for all of them at once rather than one self-test at a time,
because the bug is not in any gate's subject matter: it is in the seam between
a gate's argument parsing and its exit status, and that seam has the same shape
in every one of them.

WHAT IT ACTUALLY CHECKS
-----------------------
For each `scripts/check-*.py`: assuming a **bare** invocation (no arguments,
so every `store_true` flag is False and every optional defaults to None), is
any non-zero exit reachable? If no, the gate cannot refuse anything and is
reported.

This is a static, conservative approximation and deliberately so. It does not
know what any gate checks and does not need to; it needs only to know that
*some* path to a non-zero status survives when no flags are passed.

WHY STATIC AND NOT "RUN THEM ALL"
---------------------------------
Running them proves more, and costs 38 minutes (measured 2026-09-02: the
`check-*.py` phase is 2261s across 27 gates). It also cannot prove the
interesting half: a gate that passes on a clean tree is indistinguishable from
a gate that passes on everything, which is the whole bug. Proving the latter by
execution means planting a defect each gate would notice -- 30 bespoke fixtures
against 30 unrelated subjects. This reads the code instead, in under a second,
and asks the one question that generalises.

CONSERVATIVE BY CONSTRUCTION
----------------------------
A finding requires that *every* route to a non-zero status be guarded by a flag
that a bare run leaves false. Anything unanalysable -- a computed status, a
delegated `return other()`, a raise -- counts as "could refuse" and clears the
gate. So this under-reports rather than over-reports: a clean run does not
prove every gate is sound, it proves none has the specific defect doc-links
had. False alarms would be worse than silence here, because the reflex they
train is to add an exemption.
"""

from __future__ import annotations

import argparse
import ast
import contextlib
import io
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
SCRIPTS = ROOT / "scripts"


def _bare_values(tree: ast.AST) -> dict[str, object]:
    """Each flag's value in a bare run: `False` for store_true, `None` else.

    The distinction matters and cost a false negative when it was missing.
    `--paths-from` is None bare, so `if args.paths_from is not None:` is a
    branch a bare run never enters -- but `--check` is *False* bare, and
    `args.check is not None` would be perfectly true. Conflating the two makes
    the analysis wrong in both directions, so the bare value is carried rather
    than just the fact of falsiness.

    `add_argument("--check", action="store_true")` -> {"check": False}. An
    optional with no default parses to None; a positional with `nargs="?"`
    lands here too, and correctly -- omitted it is None, so a branch under
    `if args.archive:` is unreachable bare.
    """
    dests: dict[str, object] = {}
    for node in ast.walk(tree):
        if not (isinstance(node, ast.Call)
                and isinstance(node.func, ast.Attribute)
                and node.func.attr == "add_argument"):
            continue
        names = [a.value for a in node.args
                 if isinstance(a, ast.Constant) and isinstance(a.value, str)]
        kw = {k.arg: k.value for k in node.keywords}

        # An explicit truthy default means a bare run does NOT leave it false.
        default = kw.get("default")
        if default is not None and isinstance(default, ast.Constant):
            if default.value not in (None, False, 0, "", ()):
                continue
        action = kw.get("action")
        is_store_true = (isinstance(action, ast.Constant)
                         and action.value == "store_true")
        has_default_none = (default is None
                            or (isinstance(default, ast.Constant)
                                and default.value is None))
        if not (is_store_true or has_default_none):
            continue

        bare: object = False if is_store_true else None
        dest = kw.get("dest")
        if isinstance(dest, ast.Constant) and isinstance(dest.value, str):
            dests[dest.value] = bare
            continue
        for n in names:
            if n.startswith("--"):
                dests[n[2:].replace("-", "_")] = bare
            elif not n.startswith("-"):
                dests[n.replace("-", "_")] = bare
    return dests


def _is_bare_false(test: ast.expr, dests: dict[str, object]) -> bool:
    """True when `test` is false in a bare run, so its body is unreachable.

    Only the shapes that actually occur are recognised. Anything else is
    treated as possibly-true, which keeps the analysis conservative -- an
    unrecognised guard clears the gate rather than condemning it.
    """
    # `if args.check:` -- false whenever the bare value is falsy.
    if isinstance(test, ast.Attribute) and test.attr in dests:
        return not dests[test.attr]
    # `if args.paths_from is not None:` -- false when the bare value IS None.
    # Missing this shape was a false negative: it is the guard that stood in
    # front of the real check-doc-links defect's only other non-zero exit, so
    # the tool reported that file clean on its first run. Caught by testing the
    # tool against the historical bug rather than against today's tree.
    if (isinstance(test, ast.Compare) and len(test.ops) == 1
            and isinstance(test.left, ast.Attribute)
            and test.left.attr in dests
            and len(test.comparators) == 1
            and isinstance(test.comparators[0], ast.Constant)
            and test.comparators[0].value is None):
        bare = dests[test.left.attr]
        if isinstance(test.ops[0], ast.IsNot):
            return bare is None
        if isinstance(test.ops[0], ast.Is):
            return bare is not None
    if isinstance(test, ast.BoolOp) and isinstance(test.op, ast.And):
        # An `and` is false if any operand is false.
        return any(_is_bare_false(v, dests) for v in test.values)
    if isinstance(test, ast.BoolOp) and isinstance(test.op, ast.Or):
        return all(_is_bare_false(v, dests) for v in test.values)
    return False


def _could_be_nonzero(node: ast.AST) -> bool:
    """Does this `return`/`sys.exit` possibly hand back a non-zero status?"""
    if isinstance(node, ast.Return):
        value = node.value
    else:  # a Call to sys.exit
        value = node.args[0] if node.args else None
    if value is None:
        return False  # `return` / `sys.exit()` are both a zero status
    if isinstance(value, ast.Constant):
        return value.value not in (0, None)
    return True  # computed or delegated: assume it can refuse


def _refusal_reachable_bare(fn: ast.FunctionDef, dests: dict[str, object]) -> bool:
    """Is any non-zero exit in `fn` reachable with no arguments passed?"""
    found = False

    def walk(body: list[ast.stmt], blocked: bool) -> None:
        nonlocal found
        for stmt in body:
            if found:
                return
            if isinstance(stmt, ast.If):
                # The body needs the test to hold; the else-branch does not.
                walk(stmt.body, blocked or _is_bare_false(stmt.test, dests))
                walk(stmt.orelse, blocked)
                continue
            if isinstance(stmt, (ast.Return,)) and not blocked:
                if _could_be_nonzero(stmt):
                    found = True
                continue
            for sub in ast.walk(stmt):
                if found:
                    return
                if (isinstance(sub, ast.Call)
                        and isinstance(sub.func, ast.Attribute)
                        and sub.func.attr == "exit"
                        and not blocked
                        and _could_be_nonzero(sub)):
                    found = True
                elif isinstance(sub, ast.Return) and not blocked:
                    if _could_be_nonzero(sub):
                        found = True
            # Recurse into compound statements that are not `if`.
            for field in ("body", "orelse", "finalbody"):
                inner = getattr(stmt, field, None)
                if isinstance(inner, list) and not isinstance(stmt, ast.If):
                    walk(inner, blocked)

    walk(fn.body, False)
    return found


def audit(paths: list[Path]) -> list[tuple[str, str]]:
    findings: list[tuple[str, str]] = []
    for path in paths:
        try:
            tree = ast.parse(path.read_text(encoding="utf-8"), str(path))
        except (OSError, SyntaxError) as exc:
            findings.append((path.name, f"could not be parsed: {exc}"))
            continue
        main = next((n for n in ast.walk(tree)
                     if isinstance(n, ast.FunctionDef) and n.name == "main"), None)
        if main is None:
            # No main() is not a defect: several gates do their work at module
            # level and end in sys.exit(...). Analyse the module body instead.
            main = ast.FunctionDef(
                name="<module>", args=ast.arguments(
                    posonlyargs=[], args=[], kwonlyargs=[],
                    kw_defaults=[], defaults=[]),
                body=tree.body, decorator_list=[])
        if not _refusal_reachable_bare(main, _bare_values(tree)):
            findings.append((
                path.name,
                "no non-zero exit is reachable from a bare run -- "
                "this gate cannot refuse anything"))
    return findings


# Synthetic gates, each one line of `main()` away from its neighbour. The
# first is check-doc-links.py's defect reduced to its bones; the rest are the
# shapes that must NOT be reported, because a tool like this earns its keep by
# being quiet on correct code.
_FIXTURES: tuple[tuple[str, bool, str], ...] = (
    ("""
import argparse, sys
def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--check", action="store_true")
    ap.add_argument("--paths-from")
    args = ap.parse_args()
    if args.paths_from is not None:
        return 2
    findings = scan()
    if args.check:
        if findings:
            return 1
        return 0
    ap.print_help()
    return 0
""", True, "the historical defect: every refusal behind a flag"),
    ("""
import argparse, sys
def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--check", action="store_true")
    args = ap.parse_args()
    findings = scan()
    if findings:
        return 1
    return 0
""", False, "the fix: refusal is the default path"),
    ("""
import argparse, sys
def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--list", action="store_true")
    args = ap.parse_args()
    if args.list:
        return 0
    return run_everything()
""", False, "a delegated status is unanalysable, so it clears"),
    ("""
import argparse, sys
def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--verbose", action="store_true")
    args = ap.parse_args()
    if not ok():
        sys.exit(1)
    sys.exit(0)
""", False, "sys.exit(1) under an unrecognised guard clears"),
    ("""
import argparse
def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("archive", nargs="?", default=None)
    args = ap.parse_args()
    if args.archive:
        return 1
    return 0
""", True, "an omitted positional is None, so its branch is unreachable"),
)


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
        for i, (src, want, why) in enumerate(_FIXTURES):
            p = Path(tmp) / f"check-fixture{i}.py"
            p.write_text(src, encoding="utf-8")
            got = bool(audit([p]))
            check(got == want,
                  f"{why}: reported={got}, expected {want}")

        # THE EXIT-CODE CONTRACT -- the very defect this file exists to catch,
        # asserted against this file. A gate that reports a finding and then
        # exits 0 is the bug; it would be absurd to ship it here.
        real_glob_dir = globals()["SCRIPTS"]
        real_argv = sys.argv
        try:
            bad_dir = Path(tmp) / "bad"
            bad_dir.mkdir()
            (bad_dir / "check-cannot-refuse.py").write_text(
                _FIXTURES[0][0], encoding="utf-8")
            for argv, want, why in (
                (["x"], 1, "a finding must make a bare run FAIL"),
                (["x", "--list"], 0, "--list reports without failing"),
            ):
                globals()["SCRIPTS"] = bad_dir
                sys.argv = argv
                buf = io.StringIO()
                with contextlib.redirect_stdout(buf), \
                        contextlib.redirect_stderr(buf):
                    got_rc = main()
                check(got_rc == want, f"{why}: exit {got_rc}, want {want}")

            # An empty corpus is exit 2, not a pass: "nothing to judge" must
            # never be able to look like "judged, and fine".
            empty = Path(tmp) / "empty"
            empty.mkdir()
            globals()["SCRIPTS"] = empty
            sys.argv = ["x"]
            buf = io.StringIO()
            with contextlib.redirect_stdout(buf), \
                    contextlib.redirect_stderr(buf):
                got_rc = main()
            check(got_rc == 2, f"an empty corpus must exit 2, got {got_rc}")
        finally:
            globals()["SCRIPTS"] = real_glob_dir
            sys.argv = real_argv

    if bad:
        print(f"selftest: {bad} of {checks} cases FAILED", file=sys.stderr)
        return 1
    print(f"selftest: {checks}/{checks} cases pass")
    return 0


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.split("\n", 1)[0])
    ap.add_argument("--list", action="store_true",
                    help="print findings and exit 0")
    ap.add_argument("--selftest", action="store_true",
                    help="verify the checker itself")
    ap.add_argument("paths", nargs="*", type=Path,
                    help="grade these files instead of scripts/check-*.py; "
                         "for testing this tool against a known-bad file")
    args = ap.parse_args()

    if args.selftest:
        return selftest()

    # Named paths exist so this tool can be aimed at a file that is NOT in
    # scripts/ -- above all a historical copy pulled out of git. That is how its
    # one real false negative was found: pointed at
    #     git show 165766dbf~1:scripts/check-doc-links.py
    # it reported clean, because `if args.paths_from is not None:` is an
    # ast.Compare it did not then model. Grading today's tree could never have
    # shown that, because today's tree is the fixed one. A checker you cannot
    # aim at a known-bad input is a checker you can only ever confirm, and
    # confirmation is not evidence.
    #
    # Deliberately NOT excluding this file from the default glob. It is a
    # `check-*.py`, `pre-boot.py` runs it bare like the rest, and it must
    # satisfy its own rule -- an analyser that exempts itself is the first
    # exemption, and this file's header argues that exemptions are the failure
    # mode to avoid.
    paths = args.paths or sorted(SCRIPTS.glob("check-*.py"))
    if not paths:
        # No corpus is not a clean result. Same reasoning as the other gates:
        # "nothing to judge" must not be able to look like "judged, and fine".
        print("check-gates-can-refuse: no check-*.py found -- nothing to "
              "judge, which is not the same as a clean tree.", file=sys.stderr)
        return 2

    findings = audit(paths)
    for name, why in findings:
        print(f"{name}: {why}")

    if args.list:
        print(f"\n{len(findings)} gate(s) that cannot refuse.")
        return 0
    if findings:
        print(f"\n{len(findings)} of {len(paths)} gate(s) cannot refuse "
              f"anything.", file=sys.stderr)
        return 1
    print(f"ok -- all {len(paths)} gates can reach a non-zero exit bare.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
