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


def wired_gates() -> tuple[list[Path], str | None]:
    """Scripts a caller runs through `run_checker`, whatever they are named.

    Returns `(paths, why_not)`; `why_not` is a sentence if the set could not be
    determined, and the caller must decline rather than fall back to the glob.

    THE GLOB IS NOT THE CORPUS.  `scripts/check-*.py` finds a gate by how it is
    spelled, and three of this tree's gates are not spelled that way:
    `boot-test.sh` runs `scan-unwrap.py`, `scan-orphan-modules.py` and
    `rustscan.py` through `run_checker`, each of which can refuse the build.
    Judged by the glob alone they were exempt from the one question this file
    asks -- *can you refuse at all?* -- and the exemption was invisible, since
    a corpus that omits a file reports the same "ok" as one that includes it
    and finds nothing wrong.  That is `check-doc-links.py`'s original defect,
    which is why this file exists, reappearing in the file itself.

    THE PARSER IS IMPORTED, NOT COPIED.  Deciding what a shell script runs is
    genuinely hard -- `\\` continuations, `var=path.py` bindings two hundred
    lines from the call, names spliced together at run time -- and
    `check-gates-are-wired.py` has that parser, with four documented wrong
    answers behind its current shape.  A second copy here would start correct
    and drift, and the drift would show up as this file quietly grading a
    corpus that no longer matches the build's.  One parser, one home.
    """
    import importlib.util

    # Located next to THIS FILE, not under `SCRIPTS`. The two are the same
    # directory in normal use and deliberately are not under `--selftest`,
    # which repoints `SCRIPTS` at a temp dir holding one known-bad fixture in
    # order to grade this file's own exit-code contract. The parser's location
    # is a fact about the installation; `SCRIPTS` is the corpus root being
    # varied. Reading the first from the second made a bare run decline with
    # exit 2 under the selftest -- which the selftest caught, and which is the
    # reason it asserts the contract by running `main()` rather than by reading
    # it.
    src = Path(__file__).resolve().parent / "check-gates-are-wired.py"
    spec = importlib.util.spec_from_file_location("_gates_are_wired", src)
    if spec is None or spec.loader is None:
        return [], f"cannot determine the gate corpus: {src.name} is not importable"
    mod = importlib.util.module_from_spec(spec)
    try:
        spec.loader.exec_module(mod)
    except Exception as exc:  # noqa: BLE001 -- any failure here is a decline
        return [], (f"cannot determine the gate corpus: {src.name} would not "
                    f"load ({exc})")

    names: set[str] = set()
    for rel in mod.CALLERS:
        caller = ROOT / rel
        if not caller.is_file():
            # A missing caller means the corpus is unknown, not empty.  Falling
            # back to the glob here would silently restore the exemption this
            # function exists to remove.
            return [], (f"cannot determine the gate corpus: {rel.as_posix()} "
                        f"is missing, so what it runs cannot be read")
        runs, tested, _ = mod.analyse(caller)
        names |= runs | tested

    return [SCRIPTS / n for n in sorted(names) if (SCRIPTS / n).is_file()], None


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


def _could_be_nonzero(node: ast.AST,
                      funcs: dict[str, ast.FunctionDef] | None = None,
                      seen: frozenset[str] = frozenset()) -> bool:
    """Does this `return`/`sys.exit` possibly hand back a non-zero status?"""
    if isinstance(node, ast.Return):
        value = node.value
    else:  # a Call to sys.exit
        value = node.args[0] if node.args else None
    return _value_could_be_nonzero(value, funcs or {}, seen)


def _value_could_be_nonzero(value: ast.expr | None,
                            funcs: dict[str, ast.FunctionDef],
                            seen: frozenset[str]) -> bool:
    """Can this expression evaluate to a non-zero exit status?

    `funcs` is the module's own top-level functions, and the reason it exists
    is that "delegated, so assume it can refuse" was letting every gate in this
    tree through on a single line.

    The idiom that exposed it is `return _decline(reason, detail)` -- a helper
    that prints a no-verdict message and returns 2, defined thirty lines up in
    the same file.  Treating that as unanalysable is not conservatism, it is
    declining to read a function that is right there; and once one such call
    appears anywhere in `main`, the whole file clears regardless of what every
    other path does.  Measured before this change: a mutant of
    `scan-unwrap.py` with *every* non-zero return rewritten to 0 -- a gate that
    provably cannot refuse anything -- was reported as able to refuse, because
    of its two `return _decline(...)` lines alone.

    So a call to a function defined in this module is resolved by analysing
    that function.  A call to anything else -- imported, a method, a value
    built at run time -- keeps the old benefit of the doubt, which is where
    that rule was always right.

    `seen` breaks recursion by treating a re-entrant call as contributing
    nothing.  That errs toward reporting a finding rather than toward silence,
    which is this file's stated direction: a function whose only non-zero
    return is reached by recursing into itself still has a base case, and the
    base case is analysed on its own terms.
    """
    if value is None:
        return False  # `return` / `sys.exit()` are both a zero status
    if isinstance(value, ast.Constant):
        return value.value not in (0, None)
    # `return 1 if findings else 0` -- both arms are right here, and reading
    # them is strictly better than assuming. This is the single most common
    # refusal shape in the tree, and it used to clear on the `IfExp` node type
    # alone, which means `return 0 if findings else 0` would have cleared too.
    if isinstance(value, ast.IfExp):
        return (_value_could_be_nonzero(value.body, funcs, seen)
                or _value_could_be_nonzero(value.orelse, funcs, seen))
    if isinstance(value, ast.Call) and isinstance(value.func, ast.Name):
        name = value.func.id
        if name in seen:
            return False
        callee = funcs.get(name)
        if callee is not None:
            # No `dests`: inside a helper, nothing is known about the caller's
            # parsed arguments, so no branch is treated as unreachable. The
            # question here is "can this function ever hand back non-zero",
            # not "does it on a bare run" -- the bare-run part was already
            # decided at the call site.
            return _refusal_reachable_bare(callee, {}, funcs, seen | {name})
    return True  # genuinely computed or external: assume it can refuse


def _refusal_reachable_bare(fn: ast.FunctionDef, dests: dict[str, object],
                            funcs: dict[str, ast.FunctionDef] | None = None,
                            seen: frozenset[str] = frozenset()) -> bool:
    """Is any non-zero exit in `fn` reachable with no arguments passed?

    `funcs`/`seen` are only used to resolve a delegated return -- see
    `_value_could_be_nonzero`. They default to empty so this stays callable as
    a two-argument function, which is how the fixtures and the callers below
    that do not care about delegation still read.
    """
    funcs = funcs or {}
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
                if _could_be_nonzero(stmt, funcs, seen):
                    found = True
                continue
            for sub in ast.walk(stmt):
                if found:
                    return
                if (isinstance(sub, ast.Call)
                        and isinstance(sub.func, ast.Attribute)
                        and sub.func.attr == "exit"
                        and not blocked
                        and _could_be_nonzero(sub, funcs, seen)):
                    found = True
                elif isinstance(sub, ast.Return) and not blocked:
                    if _could_be_nonzero(sub, funcs, seen):
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
        # This module's own top-level functions, so a `return _decline(...)`
        # can be followed instead of waved through. Nested defs are left out
        # deliberately: a closure's status depends on where it was built, and
        # guessing is what this change is removing.
        funcs = {n.name: n for n in tree.body
                 if isinstance(n, ast.FunctionDef)}
        if not _refusal_reachable_bare(main, _bare_values(tree), funcs):
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

    # -- delegation to a function defined right here is followed, not assumed.
    # The pair below is the whole point: identical at the call site, opposite
    # in fact, and before this was resolved BOTH cleared. `_decline` is the
    # real idiom -- a helper that prints a no-verdict message and returns 2 --
    # and it appears in most gates in this tree, which meant most gates were
    # clearing this check on one line regardless of everything else they did.
    ("""
import argparse
def _decline(reason):
    print(reason)
    return 2
def main():
    ap = argparse.ArgumentParser()
    args = ap.parse_args()
    if broken():
        return _decline("cannot look")
    return 0
""", False, "a helper that returns 2 is followed, and clears"),
    ("""
import argparse
def _decline(reason):
    print(reason)
    return 0
def main():
    ap = argparse.ArgumentParser()
    args = ap.parse_args()
    if broken():
        return _decline("cannot look")
    return 0
""", True, "a helper that returns 0 is followed, and is reported"),

    # Both arms of a conditional return are read. `return 1 if x else 0` is the
    # commonest refusal shape in the tree and used to clear on being an IfExp,
    # which means the second of these cleared as well -- a gate that computes
    # findings and returns 0 either way.
    ("""
import argparse
def main():
    ap = argparse.ArgumentParser()
    args = ap.parse_args()
    findings = scan()
    return 1 if findings else 0
""", False, "a conditional return with a non-zero arm clears"),
    ("""
import argparse
def main():
    ap = argparse.ArgumentParser()
    args = ap.parse_args()
    findings = scan()
    return 0 if findings else 0
""", True, "a conditional return with two zero arms is reported"),

    # The old benefit of the doubt survives where it was always right: a
    # callee this file cannot see is not a callee it may assume things about.
    ("""
import argparse
from helpers import run_everything
def main():
    ap = argparse.ArgumentParser()
    args = ap.parse_args()
    return run_everything()
""", False, "an imported callee is still unanalysable, so it clears"),
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
            p.write_text(src, encoding="utf-8", newline="")
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
                _FIXTURES[0][0], encoding="utf-8", newline="")
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
    # The corpus is the union of two questions, because either alone omits
    # gates. The glob finds a gate written but never called -- the only way to
    # find one, since nothing names it. `wired_gates()` finds a gate a caller
    # actually runs whatever it is called, which the glob cannot see; see its
    # docstring for the three this tree has and why omitting them was the same
    # defect as the one this file was written to catch.
    paths = list(args.paths)
    if not paths:
        wired, why_not = wired_gates()
        if why_not is not None:
            # Declining, not falling back. A fallback to the glob would be a
            # smaller corpus reported in the same words as the full one, which
            # is the failure this whole file is about.
            print(why_not, file=sys.stderr)
            print("", file=sys.stderr)
            print("The set of gates comes from what scripts/boot-test.sh and "
                  "the hooks actually run, parsed by check-gates-are-wired.py. "
                  "Without it this could still grade scripts/check-*.py, but "
                  "that is a strictly smaller corpus reported in identical "
                  "words -- so it stops instead.", file=sys.stderr)
            return 2
        seen = {p.name for p in wired}
        paths = sorted(wired + [p for p in SCRIPTS.glob("check-*.py")
                                if p.name not in seen],
                       key=lambda p: p.name)
    if not paths:
        # No corpus is not a clean result. Same reasoning as the other gates:
        # "nothing to judge" must not be able to look like "judged, and fine".
        print("check-gates-can-refuse: no gates found -- nothing to "
              "judge, which is not the same as a clean tree.", file=sys.stderr)
        return 2

    findings = audit(paths)
    for name, why in findings:
        print(f"{name}: {why}")
    # Flush before anything goes to stderr.  boot-test.sh's run_checker merges
    # both streams into one log (`>"$log" 2>&1`), and Python block-buffers
    # stdout to a file while stderr stays unbuffered -- so without this the
    # summary overtakes the findings and the refusal text says "named above"
    # about names printed below it.  Observed exactly that while testing the
    # boot-test wiring, on a report of one line; a longer one would interleave.
    sys.stdout.flush()

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
