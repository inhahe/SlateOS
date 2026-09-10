#!/usr/bin/env python3
"""Refuse a shell block that announces a refusal and then returns success.

Why this exists
===============

`scripts/hooks/pre-push` printed this, three times on 2026-09-10, and then carried
on:

    pre-push: REFUSING to push -- a tooling script's own test suite fails.
    ...
    EOF
        fail=1
    fi

Nothing in that file reads a variable called `fail`. Every one of the six older
refusals in the same file ends `exit 1` on the line after its heredoc. So each of the
three new gates printed a refusal in the imperative and allowed the push -- including
the gate built that morning to catch three red builds, which could not itself fail.

This is the narrowest possible instance of the defect this tree keeps paying for: a
check whose verdict is computed, announced, and discarded. It is also the cheapest to
detect, which is the whole argument for gating it. The text says REFUSING; the block
must therefore reach a non-zero exit before it ends. One side is a string, the other
is control flow, and both are in the same twenty lines.

Why shellcheck is not enough, having caught two of the three
===========================================================

SC2034 ("appears unused") found `fail`, and that is how the third instance surfaced.
But it is the wrong instrument for two reasons.

It reports the VARIABLE, not the site: `fail` was assigned at two different
refusals and shellcheck named it once, so a single finding concealed two inert gates
and I nearly fixed one and pushed.

And it only fires when the variable is unused *everywhere*. A file that reads `fail`
for some other purpose -- a summary line, a different gate -- would silence SC2034
while the refusal remained inert, because the assignment is then "used". That is the
version of this defect that survives indefinitely, and it is the version this checker
catches and shellcheck cannot.

What counts as refusing
=======================

`exit` with anything but 0, or `return` with anything but 0. `return 1` is correct in
`boot-test.sh`, whose gates are functions whose caller aborts on a non-zero return;
`exit 1` is correct in the hook, whose blocks are top-level. Both are accepted
because both are right in their own file, and requiring one spelling would mean
rewriting the other file to satisfy a checker.

Scope: shell scripts under `scripts/`. A refusal in Python raises or returns a code
through argparse and is covered by `run-checker.sh`'s exit-code contract instead.

Exit codes: 0 clean, 1 a finding, 2 nothing could be read (no verdict).
"""

from __future__ import annotations

import argparse
import pathlib
import re
import sys

REPO = pathlib.Path(__file__).resolve().parent.parent
SCRIPTS = REPO / "scripts"

# The two spellings this tree uses to announce that it will not proceed. Matched
# case-insensitively on the word, because `pre-push` shouts it and `boot-test.sh`
# does not.
REFUSAL = re.compile(r"REFUSING to (?:push|build)|refusing to (?:push|build)", re.I)

# Openers and closers, for finding where the refusal's own block ends. `case`/`esac`
# and `do`/`done` are included because a refusal inside a loop or a case arm is
# still inside something that can end without refusing.
OPEN = re.compile(r"^\s*(if|for|while|until|case)\b|\bthen\s*$|\{\s*$")
CLOSE = re.compile(r"^\s*(fi|done|esac|\})\s*$")

# Anything that leaves with a non-zero status.
REFUSES = re.compile(r"^\s*(?:exit|return)\s+(?!0\s*$)\S+|^\s*(?:exit|return)\s+\$")


def scripts() -> list[pathlib.Path]:
    out = [p for p in SCRIPTS.rglob("*.sh") if p.is_file()]
    hook = SCRIPTS / "hooks" / "pre-push"
    if hook.is_file():
        out.append(hook)
    return sorted(out)


def findings(text: str) -> list[tuple[int, str]]:
    """(line, the refusal text) for every announced refusal that can return success.

    Walks forward from the announcement, tracking block depth, and stops when the
    block the announcement sits in closes. If no `exit`/`return` with a non-zero
    status appeared by then, the announcement is a claim the code does not keep.
    """
    lines = text.splitlines()
    out: list[tuple[int, str]] = []
    for i, line in enumerate(lines):
        if not REFUSAL.search(line):
            continue
        # One report per block: a refusal printed over several lines (a heredoc
        # paragraph) would otherwise be counted once per line mentioning it.
        if out and out[-1][0] > i - 40 and _same_block(lines, out[-1][0] - 1, i):
            continue
        depth = 0
        refused = False
        for j in range(i + 1, len(lines)):
            nxt = lines[j]
            if REFUSES.match(nxt):
                refused = True
                break
            if CLOSE.match(nxt):
                if depth == 0:
                    break          # the enclosing block ended, still no refusal
                depth -= 1
                continue
            if OPEN.search(nxt):
                depth += 1
        if not refused:
            out.append((i + 1, line.strip()[:88]))
    return out


def _same_block(lines: list[str], a: int, b: int) -> bool:
    """Whether no block boundary separates two lines, so both are one refusal."""
    return not any(CLOSE.match(lines[k]) for k in range(a + 1, b + 1))


def main() -> int:
    ap = argparse.ArgumentParser(description="an announced refusal must refuse")
    ap.add_argument("--self-test", "--selftest", action="store_true", dest="selftest")
    ap.add_argument("--list", action="store_true")
    args = ap.parse_args()

    if args.selftest:
        return self_test()

    files = scripts()
    if not files:
        print(
            "check-refusals-refuse: no shell scripts found under scripts/. That is "
            "not a clean verdict, it is no verdict.",
            file=sys.stderr,
        )
        return 2

    total = 0
    bad: list[tuple[str, int, str]] = []
    for f in files:
        text = f.read_text(encoding="utf-8", errors="replace")
        total += len(REFUSAL.findall(text))
        for line, msg in findings(text):
            bad.append((f.relative_to(REPO).as_posix(), line, msg))

    if args.list:
        for path, line, msg in bad:
            print(f"{path}:{line}: {msg}")
        print(f"\n{len(bad)} announced refusal(s) that can return success")
        return 0

    if not bad:
        print(
            f"check-refusals-refuse: OK ({total} refusal announcement(s) in "
            f"{len(files)} shell script(s); each reaches a non-zero exit)"
        )
        return 0

    print("A block announces a refusal and can still return success:")
    for path, line, msg in bad:
        print(f"  {path}:{line}: {msg}")
    print()
    print("The text says it is refusing. The block must reach `exit 1` (top level)")
    print("or `return 1` (a boot-test gate, whose caller aborts) before it ends.")
    print("Setting a variable is not refusing unless something reads it -- three")
    print("gates in scripts/hooks/pre-push set `fail=1` on 2026-09-10, nothing read")
    print("it, and each printed REFUSING and allowed the push. One of them was the")
    print("gate written that morning to catch three red builds.")
    return 1


def self_test() -> int:
    failures = 0
    ran = 0

    def check(label: str, got: object, want: object) -> None:
        nonlocal failures, ran
        ran += 1
        if got == want:
            print(f"  ok   {label}")
        else:
            print(f"  FAIL {label} -- got {got!r}, want {want!r}")
            failures += 1

    NL = chr(10)

    def block(tail: list[str]) -> str:
        return NL.join([
            'if [ "$x" = "1" ]; then',
            "    cat >&2 <<EOF",
            "",
            "pre-push: REFUSING to push -- something is wrong.",
            "EOF",
        ] + tail + ["fi", ""])

    # The real defect, three times over in one file on one day.
    check("a refusal that only sets a variable is reported",
          [l for l, _ in findings(block(["    fail=1"]))], [4])
    # The correct form, six times in the same file.
    check("a refusal that exits is clean", findings(block(["    exit 1"])), [])
    # boot-test.sh's gates return to a caller that aborts.
    check("return 1 counts as refusing", findings(block(["    return 1"])), [])
    # `exit 0` is not refusing, however it reads.
    check("exit 0 does not count", [l for l, _ in findings(block(["    exit 0"]))], [4])
    # A refusal nested one deeper still has to refuse before the OUTER block ends.
    nested = NL.join([
        'if [ "$x" = "1" ]; then',
        "    echo 'pre-push: REFUSING to push -- nested'",
        "    if [ -n \"$y\" ]; then",
        "        echo extra",
        "    fi",
        "    exit 1",
        "fi",
        "",
    ])
    check("an inner fi does not end the search", findings(nested), [])
    # ...and the same shape without the exit is still caught.
    check("a nested refusal with no exit is reported",
          [l for l, _ in findings(nested.replace("    exit 1" + NL, ""))], [2])
    # An exit AFTER the enclosing block is unconditional, not this block refusing.
    after = NL.join([
        'if [ "$x" = "1" ]; then',
        "    echo 'pre-push: REFUSING to push -- late'",
        "fi",
        "exit 1",
        "",
    ])
    check("an exit after the block does not count",
          [l for l, _ in findings(after)], [2])
    # A heredoc paragraph mentioning the word twice is one refusal, not two.
    twice = NL.join([
        'if [ "$x" = "1" ]; then',
        "    cat >&2 <<EOF",
        "pre-push: REFUSING to push -- first line",
        "and this line also says REFUSING to push, being prose",
        "EOF",
        "    fail=1",
        "fi",
        "",
    ])
    check("one block reports once", len(findings(twice)), 1)
    # Prose that does not announce a refusal is not this checker's business.
    check("an ordinary echo is ignored",
          findings('if true; then' + NL + '    echo hello' + NL + "fi" + NL), [])

    if failures:
        print(f"{NL}selftest: {failures} case(s) FAILED", file=sys.stderr)
        return 1
    print(f"{NL}check-refusals-refuse --self-test: OK ({ran} cases)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
