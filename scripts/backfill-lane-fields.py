#!/usr/bin/env python3
"""Add the missing ``**Lane:**`` field to old ``design-decisions.md`` entries.

The field was introduced with the band gate on 2026-08-29
(`scripts/check-design-decisions-bands.py`), which requires it on every *new*
heading. Entries written before that date are grandfathered by the baseline and
so pass without it -- but the field is not only there to satisfy the gate. It is
what makes a band collision visible in the diff instead of discoverable only by
grep, and an entry without it is one whose lane can only be recovered by
consulting the band table in the header. Lane C backfilled its own band on
2026-09-02 (169 insertions across sections 400-573); this is lane A's
equivalent.

**This is a one-shot migration, not a gate.** It is checked in because it
records exactly what a 200-line no-prose diff did -- which is the only way to
review such a diff -- and because if a fourth band ever needs the same
treatment, re-deriving the placement rules is the expensive part.

The placement rule, and why it is not simply "the line after the heading":

  The field must land inside `find_lane_field`'s window (12 lines from the
  heading, or from the end of a `> **SUPERSEDED**` banner), and it must not be
  separated from the other metadata, because a reader scanning for "who owns
  this" looks where `**Date:**` and `**Decided by:**` are. So it goes at the end
  of the `**Decided by:**` block -- *after* that field's continuation lines,
  which are common and unmarked ("**Decided by:** Claude (autonomous) -- these
  are the two sub-decisions section 201 / explicitly left to Lane A ...").
  Inserting before a continuation would split a sentence across a field.

Sections 217-220 get ``**Lane:** C``, not A. They are lane C entries numbered
into lane A's band by mistake in August and deliberately never renumbered,
because eight things cite them (see the header's "One exception, settled
2026-08-17"). Writing A on them would put a falsehood in the file in order to
make it look tidier, and it is exactly the confusion the field exists to
prevent. They are baselined, so the gate does not object either way -- which is
precisely why it has to be got right by hand.

Usage:  python scripts/backfill-lane-fields.py [--apply] [--file PATH]
Without --apply it reports what it would do and writes nothing.
"""

import argparse
import importlib.util
import os
import re
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
DEFAULT_DOC = os.path.join(os.path.dirname(HERE), "design-decisions.md")
GATE = os.path.join(HERE, "check-design-decisions-bands.py")

# The bands themselves are NOT hardcoded -- they are read from the document's
# own table, by the gate's own parser, so this script cannot disagree with the
# gate about who owns what. That is the same reason the gate reads the table
# rather than carrying a copy.
#
# The exceptions must be hardcoded, because they are exceptions *to* the table:
# sections 217-220 sit in lane A's band and belong to lane C, deliberately and
# permanently (see the header, "One exception, settled 2026-08-17"). Four lane-C
# entries about the AMD display engine were numbered into lane A's band by
# mistake; renumbering was rejected because eight things cite them, three of
# which are under `kernel/src/drm/ati/` -- lane A's tree -- so lane C could not
# have completed the renumber from its own worktree without a lane violation.
#
# Writing `**Lane:** A` on them would put a falsehood in the file in order to
# make it look tidier, and it is precisely the confusion the field exists to
# prevent. They are baselined, so the gate objects either way -- which is
# exactly why this has to be got right by hand rather than left to the table.
LANE_EXCEPTIONS = {217: "C", 218: "C", 219: "C", 220: "C"}

# Both punctuations are live in the file: `**Decided by:**` is the norm, and
# section 294 writes `**Decided by**:` with the colon outside the bold. Matching
# only the first made 294 look like an entry with no attribution at all, which
# is a much more alarming thing than a stray colon and would have been reported
# as one. The inserted field is always written in the canonical form, because
# that is the one `LANE_FIELD_RE` in the gate can see.
DECIDED_BY_RE = re.compile(r"\*\*Decided by:?\*\*:?")


def load_gate():
    """Reuse the gate's own parser, so placement is judged by the real rule."""
    spec = importlib.util.spec_from_file_location("gate", GATE)
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    return mod


def band_lane(number, bands):
    """The lane whose *band* ``number`` falls in. None for the history."""
    for band in bands:
        if band.contains(number):
            return band.lane
    return None


def true_lane(number, bands):
    """The lane that actually wrote ``number`` -- exceptions override the band.

    Distinct from `band_lane` on exactly four numbers, and the distinction is
    the point. *Selection* is by band, because the band is what says whose file
    region this is and therefore who may edit these lines; the *value written*
    is the true owner, because a field that repeated the band would carry no
    information the band table does not already have, and on 217-220 it would
    carry a falsehood.
    """
    return LANE_EXCEPTIONS.get(number) or band_lane(number, bands)


def insertion_index(lines, start):
    """Index at which to insert, given ``start`` = index after the heading.

    Returns None when there is no `**Decided by:**` field to attach to -- an
    entry shaped unlike every other, which is worth reporting rather than
    guessing at.
    """
    # Find the `**Decided by:**` line within the metadata block. Stop at the
    # next heading or at a second blank-line-separated paragraph, so that a
    # stray occurrence deep in the prose is never mistaken for the field.
    limit = min(len(lines), start + 12)
    decided = None
    for i in range(start, limit):
        if lines[i].startswith("## "):
            break
        if DECIDED_BY_RE.search(lines[i]):
            decided = i
            break
    if decided is None:
        return None

    # Walk past the field's continuation lines: non-blank lines that do not
    # begin a new bolded field and are not a new heading.
    i = decided + 1
    while (i < len(lines) and lines[i].strip()
           and not lines[i].startswith("**") and not lines[i].startswith("## ")
           and not lines[i].lstrip().startswith(">")):
        i += 1
    return i


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--file", default=DEFAULT_DOC)
    ap.add_argument(
        "--lane", required=True, choices=("A", "B", "C"),
        help="only backfill entries this lane owns -- required, and there is "
             "no 'all' option on purpose: every lane's entries live in one "
             "shared file, and a lane that rewrites another's lines invites "
             "exactly the merge conflict the bands exist to prevent.",
    )
    ap.add_argument("--apply", action="store_true")
    args = ap.parse_args(argv)

    gate = load_gate()
    with open(args.file, encoding="utf-8") as fh:
        lines = fh.read().split("\n")

    bands, band_errors = gate.parse_bands(lines)
    if band_errors:
        for e in band_errors:
            print(e, file=sys.stderr)
        return 2

    # Collect edits first, then apply back-to-front, so earlier insertions do
    # not shift the indices of later ones.
    edits, skipped, already = [], [], 0
    for idx, line in enumerate(lines):
        m = gate.HEADING_RE.match(line)
        if not m:
            continue
        number = int(m.group(2))
        if band_lane(number, bands) != args.lane:
            continue
        lane = true_lane(number, bands)
        heading = type("H", (), {"lineno": idx + 1, "number": number})()
        if gate.find_lane_field(lines, heading) is not None:
            already += 1
            continue
        at = insertion_index(lines, idx + 1)
        if at is None:
            skipped.append(number)
            continue
        edits.append((at, number, lane))

    for at, _number, lane in sorted(edits, reverse=True):
        lines.insert(at, f"**Lane:** {lane}")

    mine = sum(1 for _, _, lane in edits if lane == args.lane)
    other = sorted(n for _, n, la in edits if la != args.lane)
    print(f"{len(edits)} insertion(s) in lane {args.lane}'s band(s): "
          f"{mine} as lane {args.lane}"
          + (f", {len(other)} as another lane by exception {other}"
             if other else "")
          + f"; {already} already had the field")
    if skipped:
        print(f"SKIPPED (no '**Decided by:**' to attach to): {skipped}",
              file=sys.stderr)

    if not args.apply:
        print("dry run; nothing written (pass --apply)")
        return 1 if skipped else 0

    with open(args.file, "w", encoding="utf-8", newline="\n") as fh:
        fh.write("\n".join(lines))
    print(f"wrote {args.file}")
    return 1 if skipped else 0


if __name__ == "__main__":
    sys.exit(main())
