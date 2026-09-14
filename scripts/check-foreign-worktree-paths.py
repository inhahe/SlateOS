"""Refuse a hardcoded absolute path to a lane worktree in executable code.

## Why this exists

On 2026-09-14 `scripts/check-collapsed-messages.py` carried

    ROOT = pathlib.Path(r"E:/visual studio projects/os-lane-c")

so every lane's boot ran that gate against **lane C's** working tree. Lane A's
boot failed at gate 60 naming a message in a file whose line 122, in lane A's
tree, is `.collect()`.

Two properties, and the second is worse than the failure:

* Another lane's **uncommitted** edits could refuse your build. Nothing
  committed, pushed or merged -- read straight off their disk.
* Every **pass** it ever gave was about lane C's tree. However many times it
  went green for lane A, it never once described what lane A was building.

Our isolation is built out of branches and worktrees. An absolute path defeats
all of it in one line, and no rule we had described that coupling, because every
rule was about branches.

## Why `ast` and not a grep

The first attempt at measuring the blast radius was
`grep -l 'os-lane-[abc]' scripts/*.py`, which returns five files and reads as a
four-gate cross-lane defect. Four of the five are **prose**: a comment in
`check-eol.py` recommending the correct derivation, a paragraph in
`ctest-fixtures.py` about a past bug, a usage example in
`prune-build-cache.py`'s docstring. Only one had it in executable code.

A grep that counts documentation as defect would have filed a systemic bug that
does not exist -- so this parses instead. Comments never reach the AST at all,
and docstrings are identified and skipped explicitly.

## Scope, and what it does not cover

`scripts/**/*.py` only. Shell scripts are **not** scanned, because the
technique here is an AST walk and there is no equally reliable way to tell
code from a comment in shell without writing a parser for it.

Checked once, on 2026-09-14: `scripts/*.sh` and `scripts/hooks/*` contain
**zero** lane-worktree paths, so the gap is empty today rather than merely
unexamined. `build/*.json` run records do contain them, correctly -- they are
records of runs that happened in a particular tree, not code that addresses
one.

That measurement expires. A `.sh` that hardcodes a lane path would pass this
gate silently, and `boot-test.sh` -- the file that runs every other gate -- is
shell. If that becomes a live risk the honest fix is a second, cruder check for
shell rather than pretending this one covers it.

## The rule

A string constant in code, naming `os-lane-*` under the projects directory, is
refused -- including this lane's own. Hardcoding your own worktree is the same
defect wearing a friendlier face, and it is what broke when the tree moved from
`D:` to `E:`. Derive the root from `__file__`, as `check-eol.py` documents:

    ROOT = pathlib.Path(__file__).resolve().parent.parent
"""

from __future__ import annotations

import ast
import io
import os
import pathlib
import sys

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))
import selftestflag  # noqa: E402

# Practises what it preaches: derived, never hardcoded.
ROOT = pathlib.Path(__file__).resolve().parent.parent

MARKER = "os-lane-"
CONTEXT = "visual studio projects"


def docstring_nodes(tree: ast.AST) -> set[int]:
    """`id()` of every string constant that is a docstring, to be skipped."""
    out: set[int] = set()
    for node in ast.walk(tree):
        if not isinstance(node, (ast.Module, ast.FunctionDef, ast.AsyncFunctionDef, ast.ClassDef)):
            continue
        body = getattr(node, "body", None)
        if not body:
            continue
        first = body[0]
        if isinstance(first, ast.Expr) and isinstance(first.value, ast.Constant):
            if isinstance(first.value.value, str):
                out.add(id(first.value))
    return out


def findings_in(text: str, label: str) -> list[str]:
    """Every code-level string constant naming a lane worktree."""
    try:
        tree = ast.parse(text)
    except SyntaxError as exc:  # a file that does not parse is not ours to judge
        return ["%s: does not parse (%s)" % (label, exc.msg)]
    skip = docstring_nodes(tree)
    out: list[str] = []
    for node in ast.walk(tree):
        if not isinstance(node, ast.Constant) or not isinstance(node.value, str):
            continue
        if id(node) in skip:
            continue
        v = node.value
        if MARKER in v and CONTEXT in v.lower():
            out.append("%s:%d: hardcoded worktree path %r" % (label, node.lineno, v))
    return out


def _self_test() -> int:
    # The fixtures are BUILT, not written literally, so this file contains no
    # hardcoded worktree path of its own. The first version spelled them out
    # and the gate flagged its own test data, four findings, all fixtures.
    #
    # The alternative -- skipping this file -- was rejected: an exemption is a
    # blind spot exactly where the rule is defined, and a checker that cannot
    # be run against itself is one nobody can confirm is honest.
    proj = "E:/" + CONTEXT + "/"
    lane_c = proj + MARKER + "c"
    lane_b = proj + MARKER + "b"
    lane_a = proj + MARKER + "a"

    good_root = "ROOT = pathlib.Path(__file__).resolve().parent.parent" + chr(10)
    bad = 'ROOT = pathlib.Path(r"' + lane_c + '")' + chr(10)
    doc = '"""Usage: --target-dir ' + lane_b + '/target."""' + chr(10)
    comment = "# ROOT from __file__, so a lane checks " + lane_a + chr(10)
    prose_like = 'X = "see ' + lane_b + ' for the fixture"' + chr(10)

    cases = [
        (good_root, 0, "a derived root is fine"),
        (bad, 1, "a hardcoded worktree path in code is refused"),
        (doc, 0, "the same path in a module docstring is prose, not defect"),
        (comment, 0, "the same path in a comment never reaches the AST"),
        (doc + bad, 1, "a docstring does not excuse the code below it"),
        # Order matters and the first version of this case had it wrong: a
        # triple-quoted string is a docstring only as the FIRST statement.
        # Written the other way round it is a bare expression in code, and
        # flagging it is correct -- the case was wrong, not the checker.
        (doc + good_root + comment, 0, "a real module docstring, then code, then a comment"),
        (good_root + doc, 1, "a string after code is an expression, not a docstring"),
        ('X = "' + MARKER + 'b"' + chr(10), 0, "a bare lane name is not a path"),
        (prose_like, 1, "a path in a code string counts even when the sentence is prose-like"),
    ]
    failed = 0
    for src, want, why in cases:
        got = len(findings_in(src, "t.py"))
        ok = got == want
        failed += 0 if ok else 1
        print("%s   %s (want %d, got %d)" % ("ok  " if ok else "FAIL", why, want, got))
    print()
    print("%d self-test case(s), %d failed" % (len(cases), failed))
    return 1 if failed else 0

def main(argv: list[str]) -> int:
    if selftestflag.wants_selftest(argv):
        return _self_test()
    unknown = selftestflag.unknown_options(argv, known=())
    if unknown:
        print("check-foreign-worktree-paths: unknown option(s): %s" % ", ".join(unknown))
        return 2

    findings: list[str] = []
    scanned = 0
    scripts = ROOT / "scripts"
    if not scripts.is_dir():
        print("check-foreign-worktree-paths: no scripts/ under %s" % ROOT)
        return 2
    for path in sorted(scripts.rglob("*.py")):
        if "__pycache__" in path.parts:
            continue
        scanned += 1
        rel = os.path.relpath(path, ROOT).replace(os.sep, "/")
        findings.extend(findings_in(io.open(path, encoding="utf-8", errors="replace").read(), rel))

    if findings:
        for f in findings:
            print(f)
        print()
        print("%d hardcoded lane-worktree path(s) in code. Derive the root instead:" % len(findings))
        print("    ROOT = pathlib.Path(__file__).resolve().parent.parent")
        return 1
    print("ok -- no hardcoded lane-worktree paths in code (%d script(s))." % scanned)
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
