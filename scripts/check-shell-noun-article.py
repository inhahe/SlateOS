#!/usr/bin/env python3
"""Check that an operand noun whose article English picks by *sound* states it.

The defect this exists for
--------------------------

`kshell.rs`'s operand helpers build their refusal as ``is not {article}{noun}``,
and `article_for` chooses the article by **spelling**: a leading vowel *letter*
gets "an", anything else gets "a".  That is right for the large majority of the
284 nouns in the file, and it is the reason the helper exists at all -- the
shell used to say "is not a element id", "is not a inode ratio".

But English picks the article by **sound**, not spelling, and the two disagree.
`autostart add ... 1OOO` printed::

    autostart: add: `1OOO' is not an user id

"user" is spelled with a vowel and pronounced with a consonant (*yoo*-zer), so
the spelling rule produced "an user".  `article_for` already documents the
escape hatch for exactly this -- a noun that begins with "a " or "an " is
printed verbatim -- and four call sites were already using it (`"a UID"`).
Seven others were not, and said "an user id" or "an uid".

Why this needs a gate and not just a fix
----------------------------------------

The seven wrong sites had been in the tree for some time and nothing noticed,
because the only thing that reads a refusal's wording is a self-test rung that
asserts it, and six of the seven had no rung.  The seventh was caught only when
a new rung asserted the *correct* English -- and it was caught in QEMU, at the
end of an eleven-minute boot, because that is the only place `kshell::self_test`
runs.  A rule about the text of a string literal should not cost a boot cycle.

Note that `check-selftest-wording.py` cannot catch this on its own: it resolves
the fragments a rung asserts against text the command can print, and the article
is not in the format string -- it is the return value of a function call.

What is checked, and why only this class
----------------------------------------

For every call to an operand helper, the **noun** is the last string literal in
the argument list.  If that noun begins with a sound whose article spelling
cannot predict, it must supply its own article ("a " / "an ").

The enforced class is `u`, `eu` and `one` -- the "yoo"/"wun" onset.  This is
chosen from measurement, not intuition.  Surveying all 284 distinct nouns in
`kshell.rs` at the time this gate was written:

============  =====  ===================================================
Class         Sites  Finding
============  =====  ===================================================
`u`               7  **all seven wrong** ("an user id", "an uid") --
                     and three *other* `u` nouns ("an uncompressed
                     size", "an upload limit", "an utterance id") were
                     right, so the letter genuinely splits both ways
`eu`, `one`       0  none yet, but the same open, productive class
`h`              11  **all eleven right** -- handler, hard, height,
                     high, horizontal, hotspot, hue.  Every one hard-h.
============  =====  ===================================================

`h` is deliberately **not** enforced.  The `u` class is *productive*: English
keeps making new "yoo" words (user, unit, uid, URL, unicast, usable) and they
are indistinguishable by spelling from the "uh" ones (unreadable, update,
upper), so a caller writing a new `u` noun genuinely has to decide.  The silent
`h` is a *closed* set inherited from French -- hour, honest, honour, heir and
their derivatives -- which is four words, none of which appears here.  Demanding
an explicit article on eleven correct call sites to guard four words nobody has
written would be noise, and noise in a gate is how a gate stops being read.

If an `h` noun from that closed set ever does appear, the right fix is a
four-entry table in `article_for`, not an extension of this gate: a closed set
is precisely the thing a table handles without "the next noun falling off the
end of it", which is the objection `article_for`'s own doc raises against
tabulating the open case.

Exit status
-----------
0 -- every ambiguous noun states its article.
1 -- at least one does not (the offending call sites are listed).

``--self-test`` runs the fixture below instead, so that a gate which has quietly
stopped analysing anything cannot report a clean tree in the same words as a
gate that is working.
"""

from __future__ import annotations

import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
TARGET = ROOT / "kernel" / "src" / "kshell.rs"

# The operand helpers that take a noun as their last string literal.
HELPERS = (
    "required_num",
    "optional_num",
    "readable_num",
    "readable_hex",
    "required_hex",
    "optional_hex",
    "required_key",
)
HELPER_RE = re.compile(r"\b(?:%s)\b" % "|".join(HELPERS))

# String literals inside an argument list.  `kshell.rs` has no escaped quotes in
# any operand noun; a literal containing one would simply not match and be
# skipped, which fails toward silence rather than toward a false finding.
LITERAL_RE = re.compile(r'"([^"\\]*)"')

# The onsets whose article spelling cannot predict.  See the module docstring
# for why `h` is not among them.
AMBIGUOUS_PREFIXES = ("u", "eu", "one")

# The escape hatch `article_for` documents: a noun that already begins with an
# article is printed verbatim.
ARTICLE_PREFIXES = ("a ", "an ")


def call_argument_lists(text: str) -> list[tuple[int, str]]:
    """Return [(offset, argument_text)] for every operand-helper call.

    The argument text is taken by matching parentheses rather than by regex, so
    a call that `cargo fmt` has split across five lines -- which most of them
    are -- is read as one unit.
    """
    out: list[tuple[int, str]] = []
    for m in HELPER_RE.finditer(text):
        open_paren = text.find("(", m.end())
        if open_paren < 0:
            continue
        # Only a turbofish or whitespace may sit between the name and the `(`.
        between = text[m.end() : open_paren]
        if between.strip() and not between.strip().startswith("::<"):
            continue
        depth = 0
        i = open_paren
        while i < len(text):
            if text[i] == "(":
                depth += 1
            elif text[i] == ")":
                depth -= 1
                if depth == 0:
                    break
            i += 1
        else:
            continue
        out.append((m.start(), text[open_paren + 1 : i]))
    return out


def is_ambiguous(noun: str) -> bool:
    """Whether `article_for`'s spelling rule can be trusted for this noun."""
    low = noun.lower()
    return low.startswith(AMBIGUOUS_PREFIXES)


def states_its_article(noun: str) -> bool:
    return noun.lower().startswith(ARTICLE_PREFIXES)


# (source fragment, expected offending nouns) -- the fixture for --self-test.
#
# Each case is a claim about the analysis, not about `kshell.rs`: the file is
# expected to be clean, so a gate whose parser had collapsed would agree with it
# perfectly while checking nothing.
SELF_TEST_CASES: list[tuple[str, str, list[str]]] = [
    (
        "the bug this gate was built for",
        'let Some(u) = optional_num::<u64>(&parts, 4, "autostart", sub, "user id", 0)',
        ["user id"],
    ),
    (
        "the escape hatch satisfies it",
        'let Some(u) = optional_num::<u64>(&parts, 4, "autostart", sub, "a user id", 0)',
        [],
    ),
    (
        "an `an` article is accepted too",
        'readable_num::<u32>(w, "speech", sub, "an utterance id")',
        [],
    ),
    (
        "a `u` noun that really does take `an` must still say so",
        'readable_num::<u32>(w, "zram", sub, "uncompressed size")',
        ["uncompressed size"],
    ),
    (
        "the bare `uid` spelling is caught",
        'optional_num::<u32>(&parts, 2, "defaultapps", sub, "uid", 0)',
        ["uid"],
    ),
    (
        "a consonant noun is not the gate's business",
        'required_num::<u32>(&parts, 1, "epollstat", sub, "pid")',
        [],
    ),
    (
        "nor is an unambiguous vowel noun",
        'required_num::<u32>(&parts, 1, "winmgr", sub, "element id")',
        [],
    ),
    (
        "`h` is deliberately out of scope -- all 11 in-tree sites are hard-h",
        'required_num::<u32>(&parts, 1, "winmgr", sub, "height in pixels")',
        [],
    ),
    (
        "the noun is the LAST literal, not the first -- cmd/sub must not be read",
        'required_num::<u32>(&parts, 1, "usbpolicy", "unpin", "a device id")',
        [],
    ),
    (
        "a command named `u...` is not mistaken for a noun when a noun follows",
        'required_num::<u32>(&parts, 1, "useracct", sub, "a UID")',
        [],
    ),
    (
        "a call split across lines by rustfmt is read as one unit",
        'let Some(uid) = optional_num::<u64>(\n'
        '    &parts,\n'
        '    4,\n'
        '    "autostart",\n'
        '    sub,\n'
        '    "user id",\n'
        '    0,\n'
        ')',
        ["user id"],
    ),
    (
        "a nested call in an argument does not truncate the argument list",
        'required_num::<u32>(&parts, idx(1, 2), "winmgr", sub, "uid")',
        ["uid"],
    ),
    (
        "`one` is enforced as well as `u`",
        'required_num::<u32>(&parts, 1, "winmgr", sub, "one-shot timer id")',
        ["one-shot timer id"],
    ),
    (
        "a helper with no string literals at all is skipped, not crashed on",
        "required_num::<u32>(&parts, 1, cmd, sub, noun)",
        [],
    ),
]


def run_self_test() -> int:
    failures = 0
    for label, fragment, expected in SELF_TEST_CASES:
        got = []
        for _offset, args in call_argument_lists(fragment):
            literals = LITERAL_RE.findall(args)
            if not literals:
                continue
            noun = literals[-1]
            if is_ambiguous(noun) and not states_its_article(noun):
                got.append(noun)
        if got == expected:
            print("ok   %s" % label)
        else:
            print("FAIL %s: reported %r, expected %r" % (label, got, expected))
            failures += 1
    print()
    print("%d self-test case(s), %d failed" % (len(SELF_TEST_CASES), failures))
    return 1 if failures else 0


def main() -> int:
    if "--self-test" in sys.argv[1:]:
        return run_self_test()

    if not TARGET.exists():
        print("check-shell-noun-article: %s not found" % TARGET, file=sys.stderr)
        return 1

    text = TARGET.read_text(encoding="utf-8", errors="replace")
    line_of = [0]
    for ch in text:
        line_of.append(line_of[-1] + (1 if ch == "\n" else 0))

    findings: list[tuple[int, str]] = []
    checked = 0
    for offset, args in call_argument_lists(text):
        literals = LITERAL_RE.findall(args)
        if not literals:
            continue
        noun = literals[-1]
        checked += 1
        if is_ambiguous(noun) and not states_its_article(noun):
            findings.append((line_of[offset] + 1, noun))

    if findings:
        print("check-shell-noun-article: %d noun(s) need an explicit article" % len(findings))
        print()
        for lineno, noun in findings:
            article = "an " if noun[:1].lower() in "aeiou" else "a "
            print(
                "  kshell.rs:%d  %r would print \"is not %s%s\"" % (lineno, noun, article, noun)
            )
        print()
        print(
            "English picks the article by sound, not spelling, and `article_for`\n"
            "only knows the spelling. Write the article into the noun itself --\n"
            '`"a user id"`, `"an unreadable frame"` -- which `article_for`\n'
            "prints verbatim. See scripts/check-shell-noun-article.py for why\n"
            "this class is enforced and the silent-`h` class is not."
        )
        return 1

    print(
        "check-shell-noun-article: %d operand noun(s) checked, "
        "every ambiguous one states its article" % checked
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
