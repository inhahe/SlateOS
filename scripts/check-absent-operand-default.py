#!/usr/bin/env python3
"""Refuse a missing shell operand that silently becomes a number.

Why this exists
===============

`kernel/src/kshell.rs` contains this shape in many places:

    let id: u32 = match parts.get(1).unwrap_or(&"0").parse() {
        Ok(v) => v,
        Err(_) => { shell_println!("Invalid vault ID"); set_exit(1); return; }
    };

The `match` refuses an **unreadable** operand correctly, which is why
`check-option-refusal.py` does not count this: that checker looks for a guessed
value surviving a failed parse, and here the parse refuses. What neither it nor a
reader notices is the other half — the default `"0"` is supplied for an **absent**
operand and then parses perfectly, so a *missing* id becomes a real one.

`filevault unlock`, with nothing after it, therefore attempts to unlock vault 0
with the empty password that `parts.get(2)` also defaults to. `screensaver
preview`, bare, previews saver 1. The command does not fail and does not ask; it
acts on an object the operator never named.

This is a cousin of `design-decisions.md` §600's second prohibited shape, not an
instance of it, and the distinction matters: §600 is about a word that could not
be *read*, and this is about a word that was never *said*.

Why it needed a checker before it needed a fix
==============================================

It was measured three times by hand, with three different patterns, and gave
three answers — 14, then 31, then 37 — two of which were written into
`known-issues.md` as though they answered the same question. A population that
cannot be counted the same way twice cannot be burned down: a partial pass leaves
no marker saying how far it got, and the number in the prose drifts from the tree
the moment anyone edits either.

So the definition lives here, in code, and the count lives in
`scripts/absent-operand-ledger.txt`. That is the arrangement that makes §600's
"78 of 800 remain" trustworthy, and the reason asking *its* checker which entries
dropped — rather than grepping — is what kept batch 46's arithmetic right.

What counts
===========

A call of the form

    parts.get(<n>).unwrap_or(&"<literal>")

where `<literal>` parses as a base-10 integer. That is narrower than the harm and
deliberately so:

  * whether the number names a **live object** is judgment (share 0 is real; a
    count of 10 is harmless), and judgment does not belong in a counter;
  * the pattern is exactly decidable, so two runs agree, which is the only
    property that makes the ledger worth having.

A non-numeric default (`&""`, `&"all"`) is not counted: it cannot silently become
an identifier, and `""` is the established spelling for "the operator said
nothing" that the code already tests for.

The ledger, and what fails
==========================

One line per enclosing function, `<fn> <count>`. The check fails when:

  * a function holds more matching sites than its entry claims, or holds them with
    no entry at all — the ratchet slipping;
  * an entry claims MORE than exist — the site was fixed and the count was not
    lowered, or the function was renamed and the entry now exempts something it
    was never written for.

Both directions, for the reason `check-option-refusal.py` gives: an entry that
over-claims is how a fixed site stays counted, and a count nobody can trust is
worse than no count because it is quoted.

Usage
=====

    python scripts/check-absent-operand-default.py            # the gate
    python scripts/check-absent-operand-default.py --list      # every site
    python scripts/check-absent-operand-default.py --pin       # write the ledger
    python scripts/check-absent-operand-default.py --self-test

Exit codes: 0 clean, 1 a finding, 2 the tree could not be read.
"""

from __future__ import annotations

import argparse
import pathlib
import re
import sys
from collections import defaultdict

REPO = pathlib.Path(__file__).resolve().parent.parent
KSHELL = REPO / "kernel" / "src" / "kshell.rs"
LEDGER = REPO / "scripts" / "absent-operand-ledger.txt"

LEDGER_HEADER = (
    "# The counted `absent-operand-becomes-a-number` backlog for\n"
    "# scripts/check-absent-operand-default.py.\n"
    "#\n"
    "# Each line is `<enclosing function> <count>`: that many places inside that\n"
    "# function supply a numeric default for an operand the user did not give, so a\n"
    "# missing id becomes a real one. See that script's header for why the\n"
    "# definition lives in code and the count lives here.\n"
    "#\n"
    "# The number may only go DOWN. An entry claiming more sites than exist fails\n"
    "# too, because that is how a fixed site stays counted.\n"
    "#\n"
)

# `parts.get(1).unwrap_or(&"0")` -- the index and the default literal.
SITE = re.compile(r"""\bparts\s*\.\s*get\(\s*(\d+)\s*\)\s*\.\s*unwrap_or\(\s*&"([^"]*)"\s*\)""")

# `fn cmd_foo(` at any indentation -- kshell's command handlers.
FN = re.compile(r"^\s*(?:pub\s+)?fn\s+([a-z_][a-z0-9_]*)\s*[(<]", re.M)


def is_numeric(literal: str) -> bool:
    """Whether a default literal would parse as a base-10 integer.

    `"0"` does and is the whole problem. `""` does not, and is the established
    spelling for "the operator said nothing" -- code that tests `is_empty()`
    afterwards is asking the right question and must not be flagged for it.
    """
    if not literal:
        return False
    body = literal[1:] if literal[0] in "+-" else literal
    return body.isdigit()


def enclosing_functions(text: str) -> list[tuple[int, str]]:
    """(offset, name) for every function definition, in file order."""
    return [(m.start(), m.group(1)) for m in FN.finditer(text)]


def sites(text: str) -> list[tuple[str, int, str]]:
    """(function, 1-based line, default literal) for every counted site."""
    fns = enclosing_functions(text)
    found: list[tuple[str, int, str]] = []
    for m in SITE.finditer(text):
        if not is_numeric(m.group(2)):
            continue
        name = "<top level>"
        for off, fname in fns:
            if off > m.start():
                break
            name = fname
        found.append((name, text.count("\n", 0, m.start()) + 1, m.group(2)))
    return found


def read_ledger() -> dict[str, int]:
    if not LEDGER.is_file():
        return {}
    out: dict[str, int] = {}
    for line in LEDGER.read_text(encoding="utf-8", errors="replace").splitlines():
        parts = line.split()
        if len(parts) == 2 and parts[1].isdigit():
            out[parts[0]] = int(parts[1])
    return out


def counted(text: str) -> dict[str, int]:
    tally: dict[str, int] = defaultdict(int)
    for name, _line, _dflt in sites(text):
        tally[name] += 1
    return dict(tally)


def main() -> int:
    ap = argparse.ArgumentParser(description="absent operands that become numbers")
    ap.add_argument("--self-test", "--selftest", action="store_true", dest="selftest")
    ap.add_argument("--list", action="store_true")
    ap.add_argument("--pin", action="store_true", help="write the ledger from the tree")
    args = ap.parse_args()

    if args.selftest:
        return self_test()

    if not KSHELL.is_file():
        print(
            f"check-absent-operand-default: no {KSHELL.name}; nothing to check. "
            "That is not a clean verdict, it is no verdict.",
            file=sys.stderr,
        )
        return 2

    text = KSHELL.read_text(encoding="utf-8", errors="replace")
    found = sites(text)

    if args.list:
        for name, line, dflt in found:
            print(f"kernel/src/kshell.rs:{line}: {name} defaults a missing operand to {dflt}")
        print(f"\n{len(found)} site(s) across {len(counted(text))} function(s)")
        return 0

    if args.pin:
        tally = counted(text)
        LEDGER.write_text(
            LEDGER_HEADER + "".join(f"{k} {tally[k]}\n" for k in sorted(tally)),
            encoding="utf-8",
            newline="",
        )
        print(f"pinned {sum(tally.values())} site(s) across {len(tally)} function(s)")
        return 0

    tally = counted(text)
    ledger = read_ledger()
    over: list[str] = []
    under: list[str] = []
    for name in sorted(set(tally) | set(ledger)):
        have, claim = tally.get(name, 0), ledger.get(name, 0)
        if have > claim:
            over.append(f"  {name}: {have - claim} more than the ledger allows ({claim})")
        elif have < claim:
            under.append(f"  {name}: {claim - have} fewer than it claims (fixed? renamed?)")

    if over:
        print("A missing operand silently becomes a number in a place the ledger does not allow:")
        for line in over:
            print(line)
        print()
        print("An operand the user did not give is not an operand whose value you may")
        print("choose: `parts.get(n)` returning None means \"you did not say\", and the")
        print("usage line is the answer to it. That is a different question from \"you")
        print("said something I could not read\", which the surrounding match already")
        print("answers by name.")
        print()
        print("Do NOT raise a count to make this pass -- the ledger only shrinks.")
    if under:
        print("Ledger entries claiming more sites than exist -- lower or remove them in")
        print(f"{LEDGER.relative_to(REPO).as_posix()}:")
        for line in under:
            print(line)
    if over or under:
        return 1

    print(
        f"check-absent-operand-default: OK ({sum(tally.values())} site(s) across "
        f"{len(tally)} function(s), all ledgered)"
    )
    return 0


def self_test() -> int:
    """Cases that pin the definition, not the count.

    The count is the tree's business and changes with every batch. What must not
    drift is *what counts*, because three hand-measurements of this population
    gave three different answers and that is the whole reason this file exists.
    """
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

    # The literal decides it.
    check("a zero default counts", is_numeric("0"), True)
    check("a one default counts", is_numeric("1"), True)
    check("a multi-digit default counts", is_numeric("10"), True)
    check("a signed default counts", is_numeric("-1"), True)
    # And these must not, or the checker reports code that is asking correctly.
    check("an empty default does not count", is_numeric(""), False)
    check("a word default does not count", is_numeric("all"), False)
    check("a mixed default does not count", is_numeric("1O"), False)

    # Attribution: a site is charged to the function it sits in, not the one
    # before it. Getting this wrong would put every count on one entry and the
    # ledger would be a single number again.
    sample = (
        "fn cmd_alpha(x: u32) {\n"
        '    let a = parts.get(1).unwrap_or(&"0");\n'
        "}\n"
        "fn cmd_beta(y: u32) {\n"
        '    let b = parts.get(2).unwrap_or(&"7");\n'
        '    let c = parts.get(3).unwrap_or(&"");\n'
        "}\n"
    )
    got = counted(sample)
    check("each site is charged to its own function", got, {"cmd_alpha": 1, "cmd_beta": 1})

    found = sites(sample)
    check("the line number is the site's", [ln for _n, ln, _d in found], [2, 5])
    check("the default is reported", [d for _n, _l, d in found], ["0", "7"])

    # Whitespace inside the call must not hide a site: rustfmt can wrap these.
    spaced = 'fn cmd_g() {\n    let a = parts . get( 4 ) . unwrap_or( &"2" );\n}\n'
    check("spacing does not hide a site", counted(spaced), {"cmd_g": 1})

    if failures:
        print(f"\nselftest: {failures} case(s) FAILED", file=sys.stderr)
        return 1
    # Counted, not written down. The first draft of this line said 12 while the
    # suite ran 11, which is the same defect the rest of this file exists to stop:
    # a number in prose that nothing recomputes.
    print(f"\ncheck-absent-operand-default --self-test: OK ({ran} cases)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
