#!/usr/bin/env python3
"""Refuse a truncating write, under `scripts/`, aimed at a file in the tree.

`io.open(path, "w")` and `Path.write_text(...)` truncate the target and *then*
validate their arguments. A typo in `newline=` therefore destroys the file
before raising, and the traceback makes it look as though nothing happened. On
2026-09-17 that emptied `scripts/hooks/pre-push` -- 5,591 lines, the hook every
lane pushes through, and an empty one passes `sh -n`, runs, exits 0, and
silently skips every gate.

`safewrite.write_text` is the fix: build the text, write it beside the target,
rename over it. A rename within a directory is atomic, so a failure anywhere
before it leaves the original exactly as it was.

## What is refused, and what is not

Only writes whose target is a **module-level constant derived from
`__file__`** -- `BASELINE = Path(__file__).resolve().parent / "x-baseline.txt"`
and the like. Those are the paths that name a file the repository already has:
the baselines four gates use to remember what they have seen, and the
documents the `--apply` tools rewrite in place.

A write into a fresh temporary directory is *not* refused, and deliberately.
It cannot destroy anything that existed a moment ago, so refusing it would be
noise -- and a gate whose output is mostly non-defects is one people learn to
skim. `check-text-mode-writes.py` covers every write in the tree for a
different property (that `newline=` is passed at all, because the Windows
default rewrites `\\n` to `\\r\\n`); this one is narrower on purpose.

The two rules compose, and the composition is the point: the older gate is what
put a hand-typed escape at every write site, and a mistyped escape is what
emptied the hook. Passing the newline once, inside `safewrite`, is what makes a
bad one cost a temporary file instead of a tracked one.

Usage:
    python scripts/check-destructive-writes.py [--list]
    python scripts/check-destructive-writes.py --self-test

Exit codes:
    0  no truncating write aims at a tree file
    1  at least one does (each is named, with the constant it targets)
"""

import ast
import pathlib
import sys

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))
import selftestflag  # noqa: E402

HERE = pathlib.Path(__file__).resolve().parent

#: `safewrite` is the one file allowed to call the truncating spelling: it is
#: what turns it into the safe one, and it does so against a temporary name.
EXEMPT = {"safewrite.py"}

#: The truncating spellings. `write_text` is `pathlib`'s, which takes no mode
#: and always truncates.
TRUNCATING_METHOD = "write_text"


def file_derived_constants(tree):
    """Module-level names whose value is built from `__file__`.

    These are the paths that name a file the tree already has. A local
    variable holding a temp path is not one, which is what keeps this quiet.
    """
    out = set()
    for node in tree.body:
        if not isinstance(node, ast.Assign):
            continue
        if "__file__" not in ast.dump(node.value):
            continue
        for target in node.targets:
            if isinstance(target, ast.Name):
                out.add(target.id)
    return out


def names_in(node):
    """Every bare name appearing in an expression."""
    return {n.id for n in ast.walk(node) if isinstance(n, ast.Name)}


def is_open_call(call):
    """`open(...)` or `io.open(...)`, however the module was imported."""
    func = call.func
    if isinstance(func, ast.Name):
        return func.id == "open"
    return isinstance(func, ast.Attribute) and func.attr == "open"


def writes_in_text_mode(call):
    """Whether an `open` call's mode truncates."""
    mode = None
    if len(call.args) > 1 and isinstance(call.args[1], ast.Constant):
        mode = call.args[1].value
    for kw in call.keywords:
        if kw.arg == "mode" and isinstance(kw.value, ast.Constant):
            mode = kw.value.value
    return isinstance(mode, str) and ("w" in mode or "a" in mode)


def offences(source, consts):
    """(line, what) for every truncating write aimed at one of `consts`."""
    found = []
    for node in ast.walk(ast.parse(source)):
        if not isinstance(node, ast.Call):
            continue

        # `SOMETHING.write_text(...)`, where SOMETHING names a tree file.
        func = node.func
        if isinstance(func, ast.Attribute) and func.attr == TRUNCATING_METHOD:
            hit = names_in(func.value) & consts
            if hit:
                found.append((node.lineno, f"{sorted(hit)[0]}.write_text(...)"))
            continue

        # `open(SOMETHING, "w")`, including `io.open`.
        if is_open_call(node) and node.args and writes_in_text_mode(node):
            hit = names_in(node.args[0]) & consts
            if hit:
                found.append((node.lineno, f'open({sorted(hit)[0]}, "w")'))
    return found


def scan(root):
    """Every offence under `root`, as printable lines."""
    problems = []
    for path in sorted(root.glob("*.py")):
        if path.name in EXEMPT:
            continue
        try:
            source = path.read_text(encoding="utf-8")
        except (OSError, UnicodeDecodeError):
            continue
        try:
            tree = ast.parse(source)
        except SyntaxError:
            # Not this gate's business to report; the file will not import
            # either, and something else says so with a better message.
            continue
        consts = file_derived_constants(tree)
        if not consts:
            continue
        for line, what in offences(source, consts):
            problems.append(
                f"{path.name}:{line}: {what} truncates a file the tree already "
                f"has. Use `from safewrite import write_text`."
            )
    return problems


SELF_TESTS = [
    (
        "a baseline written with write_text is refused",
        'from pathlib import Path\n'
        'BASELINE = Path(__file__).resolve().parent / "b.txt"\n'
        'BASELINE.write_text("x", encoding="utf-8", newline="")\n',
        1,
    ),
    (
        "a baseline opened for writing is refused",
        'import os\n'
        'HERE = os.path.dirname(os.path.abspath(__file__))\n'
        'open(os.path.join(HERE, "b.txt"), "w")\n',
        1,
    ),
    (
        "appending to a tree file is refused too",
        'from pathlib import Path\n'
        'BASELINE = Path(__file__).resolve().parent / "b.txt"\n'
        'open(BASELINE, "a")\n',
        1,
    ),
    (
        "a temp file is not refused, which is what keeps this quiet",
        'import tempfile, os\n'
        'BASELINE = os.path.abspath(__file__)\n'
        'with tempfile.TemporaryDirectory() as d:\n'
        '    open(os.path.join(d, "f.txt"), "w")\n',
        0,
    ),
    (
        "reading a tree file is not a write",
        'from pathlib import Path\n'
        'BASELINE = Path(__file__).resolve().parent / "b.txt"\n'
        'BASELINE.read_text(encoding="utf-8")\n'
        'open(BASELINE)\n',
        0,
    ),
    (
        "safewrite's own spelling is not a truncating write",
        'from pathlib import Path\n'
        'from safewrite import write_text\n'
        'BASELINE = Path(__file__).resolve().parent / "b.txt"\n'
        'write_text(BASELINE, "x", newline="")\n',
        0,
    ),
    (
        "a constant not built from __file__ is not a tree file",
        'OUT = "/tmp/elsewhere.txt"\n'
        'open(OUT, "w")\n',
        0,
    ),
]


def self_test():
    bad = 0
    for label, source, want in SELF_TESTS:
        consts = file_derived_constants(ast.parse(source))
        got = len(offences(source, consts))
        ok = got == want
        bad += not ok
        print(f"{'ok  ' if ok else 'FAIL'}  {label}: {got} (want {want})")
    print(f"\n{len(SELF_TESTS)} cases, {bad} failed")
    return 1 if bad else 0


def main(argv):
    if selftestflag.wants_selftest(argv):
        return self_test()
    problems = scan(HERE)
    for p in problems:
        print(p)
    print(
        f"check-destructive-writes: {len(problems)} truncating write(s) aimed "
        f"at a tree file, in {len(list(HERE.glob('*.py')))} script(s)"
    )
    return 1 if problems else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
