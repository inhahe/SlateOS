#!/usr/bin/env python3
r"""The note a report-only scanner keeps of findings already read -- and the check that it still covers what the scan reports.

`find-claimed-acts.py`, `find-stale-admissions.py` and `find-swallowed-ticks.py`
each keep a note in their own docstring, "THE <COUNT> IT STILL REPORTS",
listing findings read against the source and found correct, so nobody reads a
known-good finding twice. A note like that is only worth anything while it
covers everything the scan reports. Twice on 2026-09-17 one sat beside a larger
scan and nobody noticed -- `find-claimed-acts` listed four while reporting six,
`find-stale-admissions` three while reporting eight, and three of that eight
were real -- so each scanner compares the two and says when they have drifted.

That was three copies of one function, until the copy in
`find-swallowed-ticks.py` failed in the way copies fail: its note said FOURTEEN,
its table of words stopped at TWELVE, an unknown word read as "no note", and
the drift warning never fired while the scan reported twenty. Six findings went
unread for eleven days. One copy now, and a word it cannot read is said.

Usage:  from stillreports import report_drift
        report_drift(__doc__, len(findings))

        python scripts/stillreports.py --self-test
"""

from __future__ import annotations

import io
import re
import sys

_UNITS = "ONE TWO THREE FOUR FIVE SIX SEVEN EIGHT NINE".split()
_TEENS = "TEN ELEVEN TWELVE THIRTEEN FOURTEEN FIFTEEN SIXTEEN SEVENTEEN EIGHTEEN NINETEEN".split()
_TENS = {"TWENTY": 20, "THIRTY": 30, "FORTY": 40, "FIFTY": 50}

#: The words a note may count in, ZERO to FIFTY-NINE.
COUNT_WORDS: dict[str, int] = {"ZERO": 0}
COUNT_WORDS.update({w: n for n, w in enumerate(_UNITS, 1)})
COUNT_WORDS.update({w: n for n, w in enumerate(_TEENS, 10)})
for _word, _base in _TENS.items():
    COUNT_WORDS[_word] = _base
    COUNT_WORDS.update({f"{_word}-{u}": _base + n for n, u in enumerate(_UNITS, 1)})

_NOTE = re.compile(r"THE ([A-Z]+(?:-[A-Z]+)?) IT STILL REPORTS")


class UnreadableCount(ValueError):
    """A note whose count word is not in `COUNT_WORDS`."""


def documented_count(doc: str | None) -> int | None:
    """How many findings the note in `doc` says it covers; None with no note.

    A note whose word cannot be read raises `UnreadableCount` rather than
    answering None, because None means "no note" -- and a note read as no note
    is exactly how FOURTEEN went unread.
    """
    match = _NOTE.search(doc or "")
    if match is None:
        return None
    word = match.group(1)
    if word not in COUNT_WORDS:
        raise UnreadableCount(f"the note says {word!r}, which is not a count this reads")
    return COUNT_WORDS[word]


def report_drift(doc: str | None, found: int, out=sys.stdout) -> bool:
    """Say so when the scan reports more than the note in `doc` covers.

    One direction only. Finding *fewer* than the note lists is usually not
    staleness: a documented entry can sit outside the roots this run scanned,
    and warning on that would cry wolf on every default run. A checker nobody
    believes is worse than no checker. The dangerous direction is the other
    one, where something is reported that nobody has ever read.

    Returns whether it said anything.
    """
    try:
        documented = documented_count(doc)
    except UnreadableCount as why:
        print("", file=out)
        print(f"  NOTE UNREADABLE: {why}; the scan reports {found}.", file=out)
        return True
    if documented is None or found <= documented:
        return False
    print("", file=out)
    print(
        f"  NOTE OUT OF DATE: this file documents {documented} known-good finding(s)"
        f" and the scan reports {found}.",
        file=out,
    )
    print(
        f"  The {found - documented} not covered have never been read. Read them,"
        " and either fix what they",
        file=out,
    )
    print("  found or add them to the note with the reason.", file=out)
    return True


def _self_test() -> int:
    failures = 0

    def expect(label, got, want):
        nonlocal failures
        if got != want:
            failures += 1
            print(f"FAIL  {label}\n  got  {got!r}\n  want {want!r}")
        else:
            print(f"  ok    {label}")

    expect("no note is no count", documented_count("A scanner.\n"), None)
    for word, n in (("ZERO", 0), ("TWELVE", 12), ("FOURTEEN", 14), ("TWENTY", 20),
                    ("TWENTY-THREE", 23), ("FIFTY-NINE", 59)):
        expect(f"{word} reads as {n}", documented_count(f"THE {word} IT STILL REPORTS"), n)
    try:
        documented_count("THE ELEVENTY IT STILL REPORTS")
        expect("an unknown word raises", "no exception", "UnreadableCount")
    except UnreadableCount:
        expect("an unknown word raises", "UnreadableCount", "UnreadableCount")

    def said(doc, found):
        out = io.StringIO()
        return report_drift(doc, found, out), out.getvalue()

    spoke, text = said("THE FOURTEEN IT STILL REPORTS", 20)
    expect("more reported than noted is said", spoke and "NOTE OUT OF DATE" in text, True)
    expect("...with how many were never read", "The 6 not covered" in text, True)
    expect("as many as noted is quiet", said("THE TWENTY IT STILL REPORTS", 20), (False, ""))
    expect("fewer than noted is quiet", said("THE TWENTY IT STILL REPORTS", 3), (False, ""))
    spoke, text = said("THE ELEVENTY IT STILL REPORTS", 20)
    expect("an unreadable note is said, not silent", spoke and "NOTE UNREADABLE" in text, True)
    print(f"\n{failures} failure(s)")
    return 1 if failures else 0


if __name__ == "__main__":
    if any(a in ("--self-test", "--selftest", "--self_test") for a in sys.argv[1:]):
        raise SystemExit(_self_test())
    print(__doc__)
