"""Keep `known-issues.md` countable: unique entry slugs, uppercase markers.

Nobody reads 224 tech-debt entries; everybody greps them. Every triage number
in this project -- "34 open", "11 name an operator question" -- comes out of a
`grep "^## TD-"` and a filter on the heading. That makes the heading line a
machine interface, and it has silently broken twice:

  2026-09-13, morning: four entries were closed with a lowercase `**fixed**`
  marker. The triage grep was case-sensitive, so it counted them open and
  reported 39 where the truth was 34.

  2026-09-13, afternoon: two closed entries quote their own original text
  under "### The original entry, for the record", and the quoted copy kept
  its `## TD-C-...` heading. Every count saw those entries twice -- once
  closed, once open, because the quoted copy carries no marker. Two items on
  the "still open" list had been fixed hours earlier.

Both are the same defect, and it is the one this project keeps meeting: a
count taken over a population that is not the one the reader believes it is.
Neither was visible by reading, because both files read perfectly.

TWO RULES, each derived from a firing rather than from taste:

  1. No two `## TD-` headings share a slug. A slug is the heading up to the
     first ` -- ` or ` (`, which is what a triage grep keys on.
  2. A status marker on a `## TD-` heading is uppercase. `-- fixed` and
     `-- FIXED` read identically and count differently.

WHAT IS DELIBERATELY NOT CHECKED: that an entry carries `**Date:**`. 200 of
the 224 headings put the date inline as `(lane B, 2026-09-11)` instead, so a
Date rule would report the file's own majority convention as an error. That
was measured before it was dropped, which is the only reason it is known.
"""

import pathlib
import re
import sys

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))
import selftestflag  # noqa: E402

ROOT = pathlib.Path(__file__).resolve().parent.parent
NL = chr(10)
EMDASH = chr(0x2014)

HEADING = re.compile(r"^## (TD-[A-Z]-.*)$", re.MULTILINE)

# The marker follows a " -- " separator. Anything before that separator is the
# slug and is free-form, so `TD-B-FIXED-POINT-MATH-IS-WRONG` is not a marker.
# `re.MULTILINE` is load-bearing and was missing for about four minutes. The
# per-line `findings()` scan did not care -- it matches one line at a time --
# but the "did we find any headings at all" guard uses `findall` over the whole
# file, and without MULTILINE that anchored pattern can only match at the start
# of the string. It returned zero for a file with 224 of them. The guard
# reported "no TD- headings found; refusing to call that a pass" and exited 2,
# which is the entire reason the bug was visible: had it been written to exit 0
# on an empty scan, this checker would have passed every file forever.

# The status vocabulary, in one place ON PURPOSE.
#
# It is four words and not one, and that is the file's convention rather than
# a choice available here: entries in it are marked FIXED, RESOLVED, CLOSED and
# WITHDRAWN, and all four are in use. Which makes a hand-written triage grep a
# trap -- on 2026-09-13 one written as `grep -viE "FIXED|RESOLVED|WITHDRAWN"`
# reported four closed entries as open, because it did not know CLOSED. That is
# the second miscount from this line in one day; the first was case.
#
# Hence `--triage`, below. The point is not that counting is hard, it is that
# the vocabulary has exactly one home and a reader who wants a number should
# not have to reconstruct it.
MARKERS = ("fixed", "resolved", "withdrawn", "closed", "done")
MARKER = re.compile(r" -- [*]{0,2}(" + "|".join(MARKERS) + r")", re.IGNORECASE)
# Whether a heading is marked closed, decided from the part AFTER the slug.
#
# Not by matching a separator, because the file uses three: " -- ", an em dash,
# and a bare bold run, sometimes with a tick emoji in front. Two entries were
# reported open by a regex that anchored on " -- " and had no idea what
# `## TD-C-DISKCLEANUP-DELETES-NOTHING — [tick] **FIXED** 2026-08-26` was.
# That was the third miscount from this one line in a day -- case, then a
# missing word, then decoration -- and each time the fix was a better list.
#
# So this stops keying on decoration entirely. `slug_of` already knows where a
# heading stops being a name, and anything after that point which *is* one of
# the status words is a status. `TD-B-FIXED-POINT-MATH-IS-WRONG` is unaffected
# because that FIXED is inside the slug, which is exactly the distinction the
# separator was a bad proxy for.
def slug_of(heading: str) -> str:
    """The part a triage grep keys on: the heading up to its first separator."""
    for sep in (" -- ", " (", " " + EMDASH + " "):
        if sep in heading:
            heading = heading.split(sep, 1)[0]
    return heading.strip()

def is_closed(heading: str) -> bool:
    """Whether this heading carries a status marker of any spelling."""
    tail = heading[len(slug_of(heading)) :]
    return any(re.search(r"[^A-Za-z]" + m + r"([^A-Za-z]|$)", tail, re.IGNORECASE)
               for m in MARKERS)


def findings(text: str):
    """Yield (line_number, message) for every heading that breaks a rule."""
    seen = {}
    for i, line in enumerate(text.split(NL), start=1):
        m = HEADING.match(line.rstrip(chr(13)))
        if not m:
            continue
        heading = m.group(1)
        slug = slug_of(heading)
        if slug in seen:
            yield (
                i,
                f"{slug} is already a heading at line {seen[slug]}, so every "
                "triage count sees this entry twice. If this is an entry "
                "quoting its own earlier text, make the quoted copy bold "
                "rather than a heading.",
            )
        else:
            seen[slug] = i
        mk = MARKER.search(heading)
        if mk and mk.group(1) != mk.group(1).upper():
            yield (
                i,
                f"status marker {mk.group(1)!r} is not uppercase, so a "
                "case-sensitive triage grep counts this entry as open.",
            )


def _self_test() -> int:
    ok = "## TD-C-A-THING-IS-BROKEN" + NL + "body" + NL
    cases = [
        (ok, 0, "a lone heading is fine"),
        (ok + "## TD-C-ANOTHER-THING" + NL, 0, "two different slugs are fine"),
        (
            ok + "## TD-C-A-THING-IS-BROKEN -- FIXED 2026-09-13" + NL,
            1,
            "the same slug twice is the quoted-original bug",
        ),
        (
            "## TD-C-X -- fixed 2026-09-13" + NL,
            1,
            "a lowercase marker is the triage-miscount bug",
        ),
        ("## TD-C-X -- FIXED 2026-09-13" + NL, 0, "an uppercase marker is the convention"),
        (
            "**TD-C-A-THING-IS-BROKEN** " + EMDASH + " as originally filed:" + NL + ok,
            0,
            "a bolded quote of an entry is not a heading and does not collide",
        ),
        (
            "## TD-C-X (lane C, 2026-08-22)" + NL + "## TD-C-X -- FIXED" + NL,
            1,
            "an inline-dated heading still collides with its own closed copy",
        ),
        (
            "### TD-C-X" + NL + "## TD-C-X" + NL,
            0,
            "a deeper heading level is not a top-level entry",
        ),
        (
            "## TD-B-FIXED-POINT-MATH-IS-WRONG" + NL,
            0,
            "FIXED inside a slug is not a marker; a marker follows the separator",
        ),
    ]
    # `is_closed`, against the heading shapes this file actually contains.
    # Every one of these is transcribed from `known-issues.md` rather than
    # invented, because the three miscounts this checker exists to prevent
    # were all "a shape I did not know was in there".
    EM = chr(0x2014)
    TICK = chr(0x2705)
    closed_cases = [
        ("TD-C-DISKCLEANUP-DELETES-NOTHING " + EM + " " + TICK + " **FIXED** 2026-08-26",
         True, "em dash, tick emoji and bold"),
        ("TD-C-THE-HTTP-CLIENT-CANNOT-MAKE-A-REQUEST " + EM + " **WITHDRAWN, THE CLAIM WAS FALSE**",
         True, "em dash and a marker inside a bold sentence"),
        ("TD-C-A-THING (lane C, 2026-08-22) -- CLOSED 2026-09-07",
         True, "CLOSED, which a three-word grep missed for a day"),
        ("TD-C-A-THING -- FIXED 2026-09-13", True, "the plain form"),
        ("TD-C-A-THING (lane C, 2026-08-22)", False, "an inline date is not a status"),
        ("TD-B-FIXED-POINT-MATH-IS-WRONG", False, "FIXED inside the slug is a name"),
        ("TD-C-RESOLVED-CONFLICTS-ARE-LOST", False, "RESOLVED inside the slug is a name"),
    ]
    bad = 0
    for heading, want, why in closed_cases:
        got = is_closed(heading)
        if got != want:
            print(f"SELF-TEST FAIL: is_closed, {why}: expected {want}, got {got}",
                  file=sys.stderr)
            bad += 1
    for text, want, why in cases:
        got = len(list(findings(text)))
        if got != want:
            print(f"SELF-TEST FAIL: {why}: expected {want}, got {got}", file=sys.stderr)
            bad += 1
    if bad:
        return 1
    print(f"self-test ok -- {len(cases) + len(closed_cases)} case(s)")
    return 0


def _triage(text: str) -> int:
    """Print the open/closed split, using the one vocabulary above."""
    by_lane: dict[str, list[str]] = {}
    closed = 0
    for line in text.split(NL):
        m = HEADING.match(line.rstrip(chr(13)))
        if not m:
            continue
        heading = m.group(1)
        if is_closed(heading):
            closed += 1
            continue
        lane = heading[3:4] if heading[2] == "-" and heading[4] == "-" else "?"
        by_lane.setdefault(lane, []).append(slug_of(heading))
    total = closed + sum(len(v) for v in by_lane.values())
    open_n = total - closed
    print(f"{total} entr(ies): {closed} closed, {open_n} open.")
    for lane in sorted(by_lane):
        slugs = by_lane[lane]
        print(f"{NL}lane {lane} -- {len(slugs)} open:")
        for slug in slugs:
            print("  " + slug.removeprefix("TD-").removeprefix(lane + "-"))
    return 0


def main(argv) -> int:
    if selftestflag.wants_selftest(argv):
        return _self_test()
    unknown = selftestflag.unknown_options(argv, known=("--triage",))
    if unknown:
        print("unrecognised option(s): " + " ".join(unknown), file=sys.stderr)
        return 2

    path = ROOT / "known-issues.md"
    if not path.is_file():
        print(f"{path} does not exist; refusing to call that a pass", file=sys.stderr)
        return 2
    text = path.read_text(encoding="utf-8")
    heads = HEADING.findall(text)
    if not heads:
        print("no TD- headings found; refusing to call that a pass", file=sys.stderr)
        return 2

    if "--triage" in argv:
        return _triage(text)

    bad = list(findings(text))
    for ln, msg in bad:
        print(f"known-issues.md:{ln}  {msg}", file=sys.stderr)
    if bad:
        print(
            NL + f"{len(bad)} heading(s) break the triage-grep contract.",
            file=sys.stderr,
        )
        return 1
    print(f"ok -- {len(heads)} TD- heading(s), all unique, all markers uppercase.")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
