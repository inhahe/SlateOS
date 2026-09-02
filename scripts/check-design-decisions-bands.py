#!/usr/bin/env python3
"""Gate: enforce ``design-decisions.md``'s per-lane numbering bands.

Why this exists
---------------

``design-decisions.md`` is written by three lanes at once, and *position*, not
number, is what makes a merge conflict. The file's own "Numbering and file
order" header explains the mechanism: if all three lanes append at end of file
they all edit the same offset and git has to compare three lanes' prose; if
each lane inserts inside its own numeric band, each lane's insertion point is a
different line offset and git never compares them at all.

That convention was enforced by nothing but attention, and attention lost. On
2026-08-27 lanes A and B both wrote a section 626, from the same base commit,
hours apart. Git reported a 350-line ``CONFLICT (content)`` and never said
"these two have the same number" -- it was caught only because lane A happened
to grep afterwards. Writing this gate found **nine more** live duplicates that
nobody had caught at all (268 through 276), for the reason given under
`HEADING_RE` below.

What it checks
--------------

1. **Every ``##`` heading that looks numbered really is numbered.** See
   `HEADING_RE`. A heading whose number the parser cannot see is invisible to
   every other check here, which is precisely how nine duplicates survived.
2. **No new duplicate numbers.** Existing ones are grandfathered by the
   baseline, because the settled convention for this file (sections 217-220,
   and 626) is *record it, never renumber, never reissue*: the numbers are
   cited from source comments and from other entries, so renumbering trades a
   cosmetic inconsistency for dangling citations.
3. **Every new heading sits in an open band**, and its number is above every
   other number in that band. New entries extend their band; they never
   backfill a gap, because a gap below the high-water mark may be a number that
   was spent and withdrawn, and reissuing it makes an existing citation resolve
   to the wrong entry.
4. **Within each open band, headings ascend in file order.** This is the check
   that actually preserves the merge property, and it is a whole-file invariant
   rather than a new-heading one: it holds today for all three open bands with
   nothing grandfathered.
5. **Every new heading carries a ``**Lane:**`` field** naming the lane that
   owns its band. This is what makes a *future* band collision self-evident in
   the diff rather than discoverable only by grep.

Warnings (never fatal): a band that is over `OCCUPANCY_WARN` per cent spent.
The last three band exhaustions were each discovered by running out, and each
one cost a round of cross-lane requests to allot a new band. Lane C is at 74%
as this lands.

The band table is parsed out of ``design-decisions.md`` itself
-------------------------------------------------------------

Deliberately not hardcoded here. The bands have already moved three times
(400-499 to 500-599 to 600-699 to 700-799), and a gate whose idea of the bands
is a constant in a script is a gate that goes quietly wrong the fourth time.
The document a human reads is the source of truth; this reads the same table.

All output is ASCII
-------------------

The document numbers its sections with a section sign, but this runs from
``scripts/boot-test.sh`` into a console whose code page is not UTF-8, where
that character prints as a replacement box. So every message here says
"section 626" and "500-599" in plain ASCII.

Usage
-----

    python scripts/check-design-decisions-bands.py
    python scripts/check-design-decisions-bands.py --update-baseline

Exit codes: 0 clean (warnings still print), 1 violations found, 2 the file or
the baseline could not be read/parsed.
"""

from __future__ import annotations

import argparse
import collections
import json
import os
import re
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
PROJECT_ROOT = os.path.dirname(HERE)
DEFAULT_DOC = os.path.join(PROJECT_ROOT, "design-decisions.md")
DEFAULT_BASELINE = os.path.join(HERE, "design-decisions-baseline.json")

# A numbered heading, in either of the two styles the file actually uses: the
# section-sign form ("##", section sign, number, em dash, title) and the plain
# form ("## 268. title").
#
# Both styles are live and neither is going away -- the file's own header calls
# the drop of the section sign at 499 "drift, not meaning". Matching only the
# plain form is not a cosmetic miss: lane A's original plan for this gate
# specified ``^## (\d+)\.``, which sees 201 of the 527 headings. Every
# duplicate check, every order check and every band check run through such a
# regex would silently skip the other 326 -- which is the most plausible
# mechanism by which 268-276 came to be duplicated nine times over without
# anyone noticing.
HEADING_RE = re.compile(r"^##\s+(\u00a7)?(\d+)\s*[.\u2014\u2013-]")

# Anything that *starts* like a numbered heading. A heading matching this but
# not HEADING_RE (e.g. "## 631 title", or the section-sign form with a colon
# instead of a dash) is an error rather than something to skip, so a
# punctuation slip cannot make an entry invisible to the checks above.
HEADING_NUMBERISH_RE = re.compile(r"^##\s+\u00a7?\d")

# A row of the band table: "| <sect>500-<sect>599 | **lane C** | open | ... |"
BAND_ROW_RE = re.compile(
    r"^\|\s*\u00a7(\d+)\s*[\u2013\u2014-]\s*\u00a7(\d+)\s*\|(.*)$"
)
BAND_LANE_RE = re.compile(r"lane\s+([ABC])\b", re.IGNORECASE)

# Deliberately NOT anchored to the start of a line. The header of an entry is
# written both ways in practice --
#
#     **Date:** 2026-08-30
#     **Lane:** B
#
# and the one-line form
#
#     **Date:** 2026-08-30 · **Decided by:** ... · **Lane:** B
#
# -- and this used to accept only the first. That cost more than it bought:
# four entries (725, 731, 732, 733) reached for the inline form unprompted and
# three of them *did* declare their lane, correctly, and were failed anyway.
# The rule this gate enforces is that the field is **visible in the diff next
# to the heading**, and the inline form satisfies that exactly as well; the
# line it sits on is house style, not the invariant. What still carries the
# rationale is the *window* below, and that is unchanged.
LANE_FIELD_RE = re.compile(r"\*\*Lane:\*\*\s*([ABC])\b")

# How far below a heading the **Lane:** field may sit. The established shape is
# Date / Decided by / Lane in the first four lines; 12 leaves room for a
# wrapped title without letting the field drift into the body, where a reader
# resolving a band question would not look for it.
LANE_FIELD_WINDOW = 12

OCCUPANCY_WARN = 80


class Heading:
    """One numbered ``##`` heading."""

    __slots__ = ("lineno", "number", "style", "text")

    def __init__(self, lineno, number, style, text):
        self.lineno = lineno
        self.number = number
        self.style = style  # "S" for the section-sign form, "P" for plain
        self.text = text

    def __repr__(self):
        return f"<{self.number} @{self.lineno}>"


class Band:
    __slots__ = ("lo", "hi", "lane", "is_open")

    def __init__(self, lo, hi, lane, is_open):
        self.lo = lo
        self.hi = hi
        self.lane = lane
        self.is_open = is_open

    @property
    def label(self):
        return f"{self.lo}-{self.hi}"

    def contains(self, number):
        return self.lo <= number <= self.hi


def parse_bands(lines):
    """Read the band table out of the document's own header.

    Returns ``(bands, errors)``. A row whose owner is not a lane (the
    single-agent history) is kept with ``lane=None`` so that a number inside it
    is still recognised as *allotted*, just not to anybody who may still write.
    """
    bands = []
    errors = []
    for lineno, line in enumerate(lines, 1):
        m = BAND_ROW_RE.match(line)
        if not m:
            continue
        lo, hi = int(m.group(1)), int(m.group(2))
        rest = m.group(3)
        lane_m = BAND_LANE_RE.search(rest)
        lane = lane_m.group(1).upper() if lane_m else None
        # "open" and "closed" are words in the status column; require exactly
        # one of them, so a row that says neither is a loud failure rather than
        # a band that silently defaults to closed -- and therefore silently
        # rejects every new entry anyone tries to put in it.
        has_open = re.search(r"\bopen\b", rest, re.IGNORECASE) is not None
        has_closed = re.search(r"\bclosed\b", rest, re.IGNORECASE) is not None
        if has_open == has_closed:
            errors.append(
                f"design-decisions.md:{lineno}: band row {lo}-{hi} must say "
                f"exactly one of 'open' or 'closed' in its status column"
            )
            continue
        if has_open and lane is None:
            errors.append(
                f"design-decisions.md:{lineno}: band row {lo}-{hi} is open but "
                f"names no owning lane"
            )
            continue
        bands.append(Band(lo, hi, lane, has_open))

    if not bands:
        errors.append(
            "design-decisions.md: found no band table rows. This gate reads "
            "the bands out of the document rather than hardcoding them; if the "
            "table moved or changed shape, update BAND_ROW_RE."
        )
        return bands, errors

    ordered = sorted(bands, key=lambda b: b.lo)
    for a, b in zip(ordered, ordered[1:]):
        if b.lo <= a.hi:
            errors.append(
                f"design-decisions.md: bands {a.label} and {b.label} overlap; "
                f"the whole point of the bands is that they are disjoint"
            )
    open_lanes = collections.Counter(b.lane for b in bands if b.is_open)
    for lane, count in sorted(open_lanes.items()):
        if count > 1:
            errors.append(
                f"design-decisions.md: lane {lane} has {count} open bands; a "
                f"lane with two open bands has two insertion points and no "
                f"rule for which to use"
            )
    return bands, errors


def parse_headings(lines):
    """Return ``(headings, errors)`` for every ``##`` line in the document."""
    headings = []
    errors = []
    for lineno, line in enumerate(lines, 1):
        if not line.startswith("## "):
            continue
        m = HEADING_RE.match(line)
        if m:
            headings.append(
                Heading(lineno, int(m.group(2)), "S" if m.group(1) else "P",
                        line.rstrip())
            )
        elif HEADING_NUMBERISH_RE.match(line):
            errors.append(
                f"design-decisions.md:{lineno}: heading starts with a number "
                f"but is in neither supported form -- expected '## N. title', "
                f"or the same with a section sign before the number and an em "
                f"dash in place of the period. A heading this gate cannot "
                f"number is invisible to every check in it."
            )
    return headings, errors


def find_lane_field(lines, heading):
    """The lane letter declared under ``heading``, or None."""
    start = heading.lineno  # lines[] is 0-based, so this is the line *after*

    # A superseded entry carries a `> **SUPERSEDED ... by section N.**` banner
    # between its heading and its fields -- deliberately first, because it is
    # the thing a reader must see before believing anything below it. Those
    # lines are not body text and must not eat the window: section 741 declared
    # its lane correctly and was failed anyway, purely because an 11-line
    # banner had pushed the field to heading+14. The invariant is "next to the
    # heading, visible in the diff", and a banner does not break it -- the
    # banner is itself part of that same diff. Section 30 has the identical
    # shape and predates this gate, so it was never caught.
    # Only a banner is skipped -- never plain blank lines, and never body text.
    # An earlier attempt skipped any leading run of blanks-or-quotes, which also
    # let a field buried below prose count, and the suite caught it.
    probe = start
    while probe < len(lines) and not lines[probe].strip():
        probe += 1
    if probe < len(lines) and lines[probe].lstrip().startswith(">"):
        while probe < len(lines) and (
            lines[probe].lstrip().startswith(">") or not lines[probe].strip()
        ):
            probe += 1
        start = probe

    for line in lines[start:start + LANE_FIELD_WINDOW]:
        if line.startswith("## "):
            break  # ran into the next section
        # `search`, not `match`: `match` anchors at position 0 whatever the
        # pattern says, which is what made the inline `... · **Lane:** B` form
        # invisible even after the `^` came off the regex.
        m = LANE_FIELD_RE.search(line)
        if m:
            return m.group(1).upper()
    return None


def load_baseline(path):
    with open(path, encoding="utf-8") as fh:
        data = json.load(fh)
    counts = data.get("counts")
    if not isinstance(counts, dict):
        raise ValueError("baseline has no 'counts' object")
    return collections.Counter({int(k): int(v) for k, v in counts.items()})


def write_baseline(path, headings, doc_rel):
    counts = collections.Counter(h.number for h in headings)
    dups = sorted(n for n, k in counts.items() if k > 1)
    payload = {
        "_comment": [
            "Grandfathered heading numbers for "
            "scripts/check-design-decisions-bands.py.",
            "A number's count is how many headings legitimately bear it. The "
            "duplicates recorded here are real and are deliberately NOT being "
            "renumbered: the settled convention for this file (see its header "
            "on sections 217-220 and 626) is that a spent number is never "
            "reissued, because the numbers are cited from source comments and "
            "from other entries, so renumbering would trade a cosmetic "
            "inconsistency for dangling citations.",
            "Regenerate with: python scripts/check-design-decisions-bands.py "
            "--update-baseline -- and only when the change was deliberate. A "
            "count that DROPS means a section was deleted or renumbered in a "
            "file that three lanes cite.",
        ],
        "file": doc_rel,
        "total_headings": len(headings),
        "grandfathered_duplicates": dups,
        "counts": {str(n): c for n, c in sorted(counts.items())},
    }
    with open(path, "w", encoding="utf-8", newline="\n") as fh:
        json.dump(payload, fh, indent=2)
        fh.write("\n")
    return counts, dups


def check(lines, baseline):
    """Run every rule. Returns ``(errors, warnings, info)``."""
    errors = []
    warnings = []
    info = []

    bands, band_errors = parse_bands(lines)
    errors.extend(band_errors)
    headings, heading_errors = parse_headings(lines)
    errors.extend(heading_errors)
    if not headings or not bands:
        return errors, warnings, info

    def band_for(number):
        for b in bands:
            if b.contains(number):
                return b
        return None

    counts = collections.Counter(h.number for h in headings)

    # --- Rule: nothing in the baseline may vanish -------------------------
    #
    # This file is lane-partitioned rather than append-only, so entries may be
    # edited in place -- but a number that stops existing is a different thing
    # from an edit: it is either a deletion or a renumber, and both break
    # citations from other lanes' source comments, which is exactly the harm
    # the never-reissue convention exists to prevent.
    for number, want in sorted(baseline.items()):
        have = counts.get(number, 0)
        if have < want:
            errors.append(
                f"section {number}: the baseline records {want} heading(s) "
                f"with this number, the file now has {have}. A section was "
                f"deleted or renumbered. If that was deliberate, re-run with "
                f"--update-baseline and say so in the commit message."
            )

    # --- Rule: no new duplicates ------------------------------------------
    for number, have in sorted(counts.items()):
        allowed = max(baseline.get(number, 0), 1)
        if have > allowed:
            where = ", ".join(
                f"line {h.lineno}" for h in headings if h.number == number
            )
            errors.append(
                f"section {number} is used by {have} headings ({where}); the "
                f"baseline allows {allowed}. Two lanes picked the same number "
                f"-- git reports that as an ordinary text conflict and never "
                f"mentions the number, which is why it needs a gate."
            )

    # --- Per-heading rules, for headings that are new ---------------------
    #
    # "New" is decided per *number*, not per line, so that editing an existing
    # entry's title, or a merge moving it, does not suddenly subject a
    # two-month-old section to rules invented today.
    new_headings = [h for h in headings if h.number not in baseline]
    for h in sorted(new_headings, key=lambda x: (x.number, x.lineno)):
        band = band_for(h.number)
        if band is None:
            errors.append(
                f"design-decisions.md:{h.lineno}: section {h.number} is new "
                f"and falls in no band at all. Add a band row to the table in "
                f"the 'Numbering and file order' header before using the "
                f"number."
            )
            continue
        if not band.is_open:
            owner = f" (was lane {band.lane}'s)" if band.lane else ""
            errors.append(
                f"design-decisions.md:{h.lineno}: section {h.number} is new "
                f"and falls in {band.label}, which is closed{owner}. Take the "
                f"next number in your own open band."
            )
            continue

        # Number must extend the band, never backfill it.
        #
        # The high-water mark is taken over the sections that were *already
        # established* when this one was written -- everything in the baseline,
        # plus any other new heading that stands above it in the file. A new
        # heading further *down* the file is deliberately not counted, and that
        # is not a loosening: without the carve-out a single change that adds
        # two sections to one band could never pass, because the earlier and
        # lower of the two would be reported as backfilling below the later and
        # higher one. That is a real shape -- one batch of work can settle two
        # questions -- and it was hit the first time it arose (631 and 632,
        # 2026-08-29). The rule that matters is preserved in full: numbers still
        # only ever go up, both against history and down the page, so a run of
        # new sections must be written in ascending order and none of them may
        # reissue a number below anything that already existed.
        established = [x.number for x in headings
                       if band.contains(x.number) and x is not h
                       and (x.number in baseline or x.lineno < h.lineno)]
        if established and h.number < max(established):
            errors.append(
                f"design-decisions.md:{h.lineno}: section {h.number} is new "
                f"but below {max(established)}, the highest number already in "
                f"{band.label}. New entries extend their band; a gap below the "
                f"high-water mark may be a number that was spent and "
                f"withdrawn, and reissuing it makes an existing citation "
                f"resolve to the wrong entry."
            )

        declared = find_lane_field(lines, h)
        if declared is None:
            errors.append(
                f"design-decisions.md:{h.lineno}: section {h.number} is new "
                f"and has no '**Lane:** {band.lane}' field within "
                f"{LANE_FIELD_WINDOW} lines of its heading. That field is what "
                f"makes a band collision visible in the diff instead of "
                f"discoverable only by grep."
            )
        elif declared != band.lane:
            errors.append(
                f"design-decisions.md:{h.lineno}: section {h.number} declares "
                f"'**Lane:** {declared}' but sits in {band.label}, which is "
                f"lane {band.lane}'s band. One of the two is wrong -- and if "
                f"it is the number, fix it now, before anything cites it."
            )

    # --- Rule: each open band ascends in file order -----------------------
    #
    # This is the check that preserves the merge property, and it is the reason
    # the numbers matter at all: a band whose entries ascend in file order has
    # exactly one insertion point -- immediately after its last entry -- and
    # three such bands give three distinct line offsets that git never has to
    # compare. Note this holds even though the bands themselves are thoroughly
    # interleaved with each other: the invariant is per-band, so "insert after
    # my band's last entry" stays correct under any amount of interleaving,
    # whereas "insert before the next band's first entry" does not, and is
    # already false for lane C.
    for band in bands:
        if not band.is_open:
            continue
        seq = [h for h in headings if band.contains(h.number)]
        for a, b in zip(seq, seq[1:]):
            if a.number > b.number:
                errors.append(
                    f"design-decisions.md:{b.lineno}: section {b.number} "
                    f"appears after section {a.number} (line {a.lineno}) "
                    f"inside {band.label}. Entries within a band must ascend "
                    f"in file order, so that the band has a single insertion "
                    f"point."
                )

    # --- Insertion points, occupancy, and the shape of the file -----------
    for band in sorted(bands, key=lambda b: b.lo):
        if not band.is_open:
            continue
        seq = [h for h in headings if band.contains(h.number)]
        owner = f"lane {band.lane}"
        if not seq:
            # An empty band is the one band with no last entry to anchor on --
            # and so, until now, the one band that got a number but no line,
            # which is precisely backwards: a fresh band is where the writer is
            # least likely to already know where their region of the file is.
            # Lane C hit this on 2026-09-02 with the empty 800-899 and had to
            # fall back on the prose in the header (requests/
            # c-ab-lane-c-closed-500-599-at-579-and-opened-800-899.md).
            #
            # The anchor is the last entry of the same lane's *previous* band,
            # because that is what the new band was opened to continue from:
            # it keeps the lane's region of the file contiguous, which is the
            # whole reason the bands are interleaved rather than appended. If
            # the lane has no earlier entry at all there is nothing to be
            # contiguous with, and the position is genuinely the writer's to
            # choose -- so say that, rather than invent an anchor.
            prior = [h for h in headings
                     if h.number < band.lo
                     and any(b.lane == band.lane and b.contains(h.number)
                             for b in bands)]
            if prior:
                anchor = max(prior, key=lambda h: h.lineno)
                info.append(
                    f"{band.label:<10} {owner}  empty; first entry is "
                    f"{band.lo}, insert after line {anchor.lineno} "
                    f"(section {anchor.number}, the last of this lane's "
                    f"previous band)"
                )
            else:
                info.append(
                    f"{band.label:<10} {owner}  empty; first entry is "
                    f"{band.lo}. This lane has no earlier entry to sit after, "
                    f"so the position is yours -- record it in the header "
                    f"when you take it."
                )
            continue
        last = seq[-1]
        spent = last.number - band.lo + 1
        size = band.hi - band.lo + 1
        pct = 100 * spent // size
        info.append(
            f"{band.label:<10} {owner}  {len(seq):>3} entries, next is "
            f"{last.number + 1}, insert after line {last.lineno} "
            f"({pct}% spent)"
        )
        if pct >= OCCUPANCY_WARN:
            warnings.append(
                f"band {band.label} ({owner}) is {pct}% spent -- "
                f"{band.hi - last.number} numbers left. Each of the three "
                f"previous band exhaustions was discovered by running out, and "
                f"cost a round of cross-lane requests to allot a new one. "
                f"Allot the next band now, while it is cheap."
            )

    return errors, warnings, info


def main(argv=None):
    ap = argparse.ArgumentParser(
        description="Enforce design-decisions.md's per-lane numbering bands."
    )
    ap.add_argument("--file", default=DEFAULT_DOC)
    ap.add_argument("--baseline", default=DEFAULT_BASELINE)
    ap.add_argument(
        "--update-baseline", action="store_true",
        help="rewrite the baseline from the current file, then exit",
    )
    ap.add_argument("--quiet", action="store_true",
                    help="suppress the per-band summary on success")
    args = ap.parse_args(argv)

    try:
        with open(args.file, encoding="utf-8") as fh:
            lines = fh.read().split("\n")
    except OSError as exc:
        print(f"check-design-decisions-bands: cannot read {args.file}: {exc}",
              file=sys.stderr)
        return 2

    if args.update_baseline:
        headings, heading_errors = parse_headings(lines)
        for err in heading_errors:
            print(f"  ERROR {err}", file=sys.stderr)
        if heading_errors:
            print("check-design-decisions-bands: refusing to baseline a file "
                  "with unparseable headings -- they would be recorded as "
                  "absent and stay invisible.", file=sys.stderr)
            return 2
        # relpath() raises ValueError across Windows drive letters, which is
        # not hypothetical: the tests baseline a fixture under the temp
        # directory, which is on C: while the checkout is on D:. The path here
        # is a provenance note in the baseline, so falling back to the absolute
        # path is strictly better than aborting over it.
        try:
            rel = os.path.relpath(args.file, PROJECT_ROOT).replace("\\", "/")
        except ValueError:
            rel = args.file.replace("\\", "/")
        counts, dups = write_baseline(args.baseline, headings, rel)
        print(f"check-design-decisions-bands: baselined {len(headings)} "
              f"headings ({len(counts)} distinct numbers, "
              f"{len(dups)} grandfathered duplicate(s)) to {args.baseline}")
        if dups:
            print("  grandfathered duplicates: "
                  + ", ".join(str(n) for n in dups))
        return 0

    try:
        baseline = load_baseline(args.baseline)
    except (OSError, ValueError, json.JSONDecodeError) as exc:
        print(f"check-design-decisions-bands: cannot read baseline "
              f"{args.baseline}: {exc}\n"
              f"  Create it with --update-baseline.", file=sys.stderr)
        return 2

    errors, warnings, info = check(lines, baseline)

    if info and not args.quiet:
        for line in info:
            print(f"  {line}")
    for warn in warnings:
        print(f"  WARN  {warn}")
    for err in errors:
        print(f"  ERROR {err}", file=sys.stderr)

    if errors:
        print(f"check-design-decisions-bands: FAILED ({len(errors)} violation"
              f"{'s' if len(errors) != 1 else ''})", file=sys.stderr)
        return 1
    print(f"check-design-decisions-bands: OK "
          f"({len(warnings)} warning{'s' if len(warnings) != 1 else ''})")
    return 0


if __name__ == "__main__":
    sys.exit(main())
