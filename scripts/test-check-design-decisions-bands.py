#!/usr/bin/env python3
"""Regression tests for `scripts/check-design-decisions-bands.py`.

Run: `python scripts/test-check-design-decisions-bands.py` (0 = pass, 1 = fail).
No pytest dependency, matching the other suites in this directory, so it runs
from a bare checkout and from `scripts/boot-test.sh`.

Why a gate needs its own tests, and what shape they take
--------------------------------------------------------

The failure this gate exists to prevent -- two lanes writing the same section
number -- is one that git reports as an ordinary text conflict and never names.
The failure a *broken* gate produces is identical, except that a green check
now stands where the missing warning used to be. So the tests here are written
against the specific ways a checker goes quietly wrong:

* **Every rule is tested in both directions.** A rule tested only on a
  violating document passes just as well when the checker rejects everything;
  a rule tested only on a clean document passes when the checker accepts
  everything. Each rule below gets a clean case and a dirty case.
* **The heading regex is tested against both live heading styles**, because
  the specific defect this gate was written to close is a regex that saw only
  one of them, and 326 of 527 headings were invisible to every hand-check ever
  run against the file.
* **One test runs against the real `design-decisions.md`** rather than a
  fixture. Two of the properties here -- that both heading styles are still in
  use, and that the band table still parses -- are facts about the *document*,
  and a fixture cannot notice the document changing underneath the gate.
* **A test asserts that the baseline actually grandfathers something.** If the
  nine duplicates were ever quietly renumbered away, the duplicate rule would
  still pass, but its most important case would no longer be exercised by
  anything.
"""

from __future__ import annotations

import inspect
import json
import os
import sys
import tempfile

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import srcload  # noqa: E402

REPO_ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
SCRIPT = os.path.join(REPO_ROOT, "scripts", "check-design-decisions-bands.py")
REAL_DOC = os.path.join(REPO_ROOT, "design-decisions.md")
REAL_BASELINE = os.path.join(REPO_ROOT, "scripts",
                             "design-decisions-baseline.json")

SECT = "\u00a7"
EMDASH = "\u2014"
ENDASH = "\u2013"

_FAILURES: list[str] = []


def load_module():
    """Import check-design-decisions-bands.py by path (name is not an ident).

    Loaded through `srcload` rather than `importlib`: a `SourceFileLoader`
    consults `__pycache__`, whose staleness check is `(mtime, size)` at
    one-second resolution, so two same-size writes inside one second leave the
    second one invisible and the suite validates bytecode that is not on disk.
    That has actually happened here. See `scripts/srcload.py`.
    """
    return srcload.load(SCRIPT, "ddbands")


def check(label, got, want):
    if got == want:
        print(f"PASS  {label}")
        return True
    print(f"FAIL  {label}")
    print(f"        got : {got!r}")
    print(f"        want: {want!r}")
    _FAILURES.append(label)
    return False


def check_true(label, got):
    return check(label, bool(got), True)


# --------------------------------------------------------------------------
# Fixture construction
#
# A miniature design-decisions.md: the band table the gate parses, then some
# sections. Kept small on purpose -- the real file is 50 000 lines and a
# fixture that imitated its bulk would hide which line each test turns on.
# --------------------------------------------------------------------------

BAND_TABLE = "\n".join([
    "## Numbering and file order",
    "",
    "| Band | Owner | Status | Region |",
    "|---|---|---|---|",
    f"| {SECT}1{ENDASH}{SECT}127 | single-agent history | closed | the head |",
    f"| {SECT}200{ENDASH}{SECT}299 | **lane A** | closed {EMDASH} full | mid |",
    f"| {SECT}500{ENDASH}{SECT}599 | **lane C** | **open** | interleaved |",
    f"| {SECT}600{ENDASH}{SECT}699 | **lane A** | **open** | interleaved |",
    f"| {SECT}700{ENDASH}{SECT}799 | **lane B** | **open** | the tail |",
    "",
])


def section(number, lane=None, style="P", title="a decision", punct=None):
    """One rendered section, in either heading style."""
    if style == "S":
        head = f"## {SECT}{number} {punct or EMDASH} {title}"
    else:
        head = f"## {number}{punct or '.'} {title}"
    body = ["", "**Date:** 2026-08-29", "**Decided by:** Claude (autonomous)"]
    if lane is not None:
        body.append(f"**Lane:** {lane}")
    body += ["", "**In short:** something was decided.", ""]
    return "\n".join([head] + body)


def doc(*sections, table=BAND_TABLE):
    return (table + "\n" + "\n".join(sections) + "\n").split("\n")


def run(mod, lines, baseline_numbers):
    """Run every rule; return (errors, warnings, info)."""
    import collections
    baseline = collections.Counter(baseline_numbers)
    return mod.check(lines, baseline)


def errs(mod, lines, baseline_numbers):
    return run(mod, lines, baseline_numbers)[0]


# --------------------------------------------------------------------------
# The heading parser
# --------------------------------------------------------------------------

def test_both_live_heading_styles_are_parsed(mod):
    """The defect this whole gate was written around.

    `## 268. title` and `## <sect>268 <emdash> title` are both live in the real
    document. A parser that sees only the first sees 201 of 527 headings, and
    every check downstream of it silently skips the rest -- which is how nine
    duplicate numbers survived for weeks.
    """
    lines = doc(section(600, "A", style="P"),
                section(601, "A", style="S"))
    headings, errors = mod.parse_headings(lines)
    check("both heading styles parse", [h.number for h in headings],
          [600, 601])
    check("both styles are recorded as such", [h.style for h in headings],
          ["P", "S"])
    check("neither style is an error", errors, [])


def test_an_endash_heading_is_parsed_too(mod):
    """The document contains both dash characters; neither may be a blind spot."""
    lines = doc(section(600, "A", style="S", punct=ENDASH))
    headings, errors = mod.parse_headings(lines)
    check("en-dash heading parses", [h.number for h in headings], [600])
    check("en-dash heading is not an error", errors, [])


def test_a_heading_the_parser_cannot_number_is_an_error_not_a_skip(mod):
    """The single most important negative case.

    A heading that starts with a number but uses unsupported punctuation is
    invisible to every other rule here. Skipping it is the exact failure the
    gate exists to prevent, one level up: a checker that quietly sees less than
    it is asked to see. So it must be loud.
    """
    for bad in ["## 631 no punctuation", f"## {SECT}631: colon", "## 631) paren"]:
        lines = doc(section(600, "A")) + [bad, ""]
        headings, errors = mod.parse_headings(lines)
        check(f"unparseable heading is not silently numbered: {bad!r}",
              [h.number for h in headings], [600])
        check_true(f"unparseable heading is reported: {bad!r}", len(errors) == 1)
        check_true(f"the report names the line: {bad!r}",
                   "design-decisions.md:" in errors[0])


def test_prose_subheadings_are_not_mistaken_for_sections(mod):
    """The real file has seven unnumbered `##` headings, all legitimate."""
    lines = doc(section(600, "A")) + ["## The decision", "", "## Why B anyway",
                                      ""]
    headings, errors = mod.parse_headings(lines)
    check("prose subheadings are ignored", [h.number for h in headings], [600])
    check("prose subheadings are not errors", errors, [])


# --------------------------------------------------------------------------
# The band table
# --------------------------------------------------------------------------

def test_the_band_table_is_read_from_the_document(mod):
    """Not hardcoded: the bands have moved three times already."""
    bands, errors = mod.parse_bands(BAND_TABLE.split("\n"))
    check("band table parses without error", errors, [])
    check("every row is found",
          [(b.lo, b.hi, b.lane, b.is_open) for b in bands],
          [(1, 127, None, False), (200, 299, "A", False),
           (500, 599, "C", True), (600, 699, "A", True),
           (700, 799, "B", True)])


def test_a_band_row_that_says_neither_open_nor_closed_is_an_error(mod):
    """Not "default to closed".

    A band silently defaulting to closed rejects every new entry in it with a
    message about the wrong thing, and the lane that owns it has no way to tell
    the table is at fault.
    """
    table = BAND_TABLE.replace("| **lane C** | **open** |", "| **lane C** | |")
    bands, errors = mod.parse_bands(table.split("\n"))
    check_true("ambiguous status is reported", len(errors) == 1)
    check_true("the ambiguous row is dropped, not guessed at",
               all(b.lo != 500 for b in bands))


def test_overlapping_bands_are_an_error(mod):
    table = BAND_TABLE.replace(f"{SECT}700{ENDASH}{SECT}799",
                               f"{SECT}690{ENDASH}{SECT}799")
    _, errors = mod.parse_bands(table.split("\n"))
    check_true("overlapping bands are reported",
               any("overlap" in e for e in errors))


def test_a_lane_with_two_open_bands_is_an_error(mod):
    """Two open bands means two insertion points and no rule for which to use."""
    table = BAND_TABLE.replace("| **lane B** | **open** |",
                               "| **lane A** | **open** |")
    _, errors = mod.parse_bands(table.split("\n"))
    check_true("a lane with two open bands is reported",
               any("two open bands" in e or "2 open bands" in e
                   for e in errors))


def test_a_missing_band_table_is_an_error_not_an_empty_ruleset(mod):
    """With no bands, every band rule vacuously passes. That must be loud."""
    _, errors = mod.parse_bands(["# design-decisions.md", "", "no table here"])
    check_true("a missing band table is reported",
               any("no band table rows" in e for e in errors))


# --------------------------------------------------------------------------
# Duplicates
# --------------------------------------------------------------------------

def test_a_new_duplicate_number_is_rejected(mod):
    """The 626 collision, reproduced."""
    lines = doc(section(626, "A", title="diskquota"),
                section(626, "B", title="dd"))
    found = errs(mod, lines, {626: 1})
    check_true("a new duplicate is reported",
               any("is used by 2 headings" in e for e in found))
    # Both line numbers, not just the second: the point of the message is that
    # the reader can open the two entries and decide which keeps the number.
    linenos = [i for i, l in enumerate(lines, 1) if l.startswith("## 626")]
    check("the fixture really has two 626 headings", len(linenos), 2)
    check_true("the report names both lines",
               any(all(f"line {n}" in e for n in linenos) for e in found))


def test_a_baselined_duplicate_is_grandfathered(mod):
    """The nine live duplicates (268-276) must not fail the build forever.

    The convention for this file, settled at 217-220 and again at 626, is
    record-and-never-reissue rather than renumber: the numbers are cited from
    source comments, so renumbering trades a cosmetic inconsistency for
    dangling citations.
    """
    lines = doc(section(268, style="S", title="one"),
                section(268, style="P", title="the other"))
    check("a baselined duplicate passes", errs(mod, lines, {268: 2}), [])
    found = errs(mod, lines, {268: 1})
    check_true("but one more than the baseline allows does not",
               any("is used by 2 headings" in e for e in found))


def test_a_third_copy_of_a_grandfathered_duplicate_is_still_rejected(mod):
    """Grandfathering two must not grandfather any number of them."""
    lines = doc(section(268, style="S"), section(268, style="P"),
                section(268, style="P", title="a third"))
    found = errs(mod, lines, {268: 2})
    check_true("a third copy is reported",
               any("is used by 3 headings" in e for e in found))


def test_a_vanished_section_is_reported(mod):
    """A number that stops existing is a deletion or a renumber.

    Both break citations from the other lanes' source comments, which is the
    harm the never-reissue convention exists to prevent. The message has to
    point at the escape hatch, because there is a legitimate case for it.
    """
    lines = doc(section(600, "A"))
    found = errs(mod, lines, {600: 1, 601: 1})
    check_true("the vanished number is named",
               any(e.startswith("section 601:") for e in found))
    check_true("the message points at --update-baseline",
               any("--update-baseline" in e for e in found))


# --------------------------------------------------------------------------
# Band membership, for new headings only
# --------------------------------------------------------------------------

def test_a_new_heading_in_its_own_open_band_passes(mod):
    lines = doc(section(600, "A"), section(631, "A"))
    check("a well-formed new entry passes", errs(mod, lines, {600: 1}), [])


def test_a_new_heading_in_a_closed_band_is_rejected(mod):
    """200-499 are full, and their numbers are spent, not free."""
    lines = doc(section(250, "A"))
    found = errs(mod, lines, {})
    check_true("a new entry in a closed band is reported",
               any("which is closed" in e for e in found))
    check_true("the message names the former owner",
               any("was lane A's" in e for e in found))


def test_a_new_heading_outside_every_band_is_rejected(mod):
    lines = doc(section(900, "A"))
    found = errs(mod, lines, {})
    check_true("a number in no band is reported",
               any("falls in no band at all" in e for e in found))


def test_a_new_heading_below_its_bands_high_water_mark_is_rejected(mod):
    """New entries extend a band; they never backfill a gap in it.

    A gap below the high-water mark may be a number that was spent and then
    withdrawn. Reissuing it makes a citation written months ago resolve to an
    entry about something else -- silently, since nothing checks that a
    citation still means what it meant.
    """
    lines = doc(section(600, "A"), section(630, "A"), section(610, "A"))
    found = errs(mod, lines, {600: 1, 630: 1})
    check_true("backfilling below the high-water mark is reported",
               any("but below 630" in e for e in found))


def test_two_new_sections_in_one_change_are_fine_if_they_ascend(mod):
    """One batch of work can settle two questions, so one diff can add two
    sections. The high-water mark is therefore taken over what was *already
    established* -- the baseline, plus any new heading standing above this one
    in the file -- and not over new headings further down.

    Without that carve-out the earlier and lower of the pair is reported as
    backfilling below the later and higher one, and the change can never be
    made to pass by writing it correctly. That is not hypothetical: it fired on
    631 and 632 the first time two lane-A sections landed together.
    """
    lines = doc(section(600, "A"), section(631, "A"), section(632, "A"))
    check("a new pair written in ascending order passes",
          errs(mod, lines, {600: 1}), [])


def test_but_a_new_pair_written_out_of_order_is_still_rejected(mod):
    """The carve-out above is scoped to *file order*, so it takes nothing away.

    A run of new sections still has to be written lowest-first, which is what
    keeps a band's single insertion point at its tail. Here 632 stands above
    631, so 631 is backfilling below something already written.
    """
    lines = doc(section(600, "A"), section(632, "A"), section(631, "A"))
    found = errs(mod, lines, {600: 1})
    check_true("the lower of a descending new pair is reported",
               any("section 631 is new but below 632" in e for e in found))


def test_a_new_section_below_a_baselined_one_further_down_is_rejected(mod):
    """And the carve-out is scoped to *new* headings, so history still binds.

    File order is what excuses a new heading above; a baselined heading counts
    wherever it sits, because it is a number that has genuinely been spent.
    Here 610 is new and 630 is old, and 610 backfills below it even though 630
    comes later in the file. (The out-of-order 630 is a second, separate
    complaint -- this asserts only that the backfill is caught.)
    """
    lines = doc(section(600, "A"), section(610, "A"), section(630, "A"))
    found = errs(mod, lines, {600: 1, 630: 1})
    check_true("a new number below a spent one is reported wherever it sits",
               any("section 610 is new but below 630" in e for e in found))


def test_an_existing_heading_is_not_judged_by_rules_invented_today(mod):
    """Grandfathering is per *number*, not per line.

    Otherwise a merge that moved an old section, or an edit to its title, would
    suddenly subject a two-month-old entry to today's rules -- and the file's
    own convention explicitly permits editing entries in place. 610 below
    carries no `**Lane:**` field and sits below its band's high-water mark;
    both would be violations if it were new, and neither is.
    """
    lines = doc(section(600, "A"), section(610, lane=None), section(630, "A"))
    check("a baselined heading is exempt from the new-heading rules",
          errs(mod, lines, {600: 1, 630: 1, 610: 1}), [])


def test_but_a_baselined_heading_is_not_exempt_from_the_order_rule(mod):
    """The one rule that is a whole-file invariant rather than a new-heading one.

    Grandfathering is what keeps the gate from re-litigating four months of
    history -- but the per-band ascending order is not history, it is the
    property that gives each band a single insertion point. If a merge ever
    shuffled two old entries out of order, that band would acquire a second
    insertion point and the merge-safety the bands exist for would be gone,
    silently. So this fires on baselined headings too, deliberately.
    """
    lines = doc(section(600, "A"), section(630, "A"), section(610, "A"))
    found = errs(mod, lines, {600: 1, 630: 1, 610: 1})
    check_true("an old section moved out of band order is still reported",
               any("section 610 appears after section 630" in e
                   for e in found))
    check("and that is the only complaint about it", len(found), 1)


# --------------------------------------------------------------------------
# The Lane field
# --------------------------------------------------------------------------

def test_a_new_heading_without_a_lane_field_is_rejected(mod):
    lines = doc(section(631, lane=None))
    found = errs(mod, lines, {})
    check_true("a missing Lane field is reported",
               any("has no '**Lane:** A' field" in e for e in found))


def test_a_lane_field_that_contradicts_the_band_is_rejected(mod):
    """The collision-detector.

    Lane B writing into lane A's band is exactly what happened with the
    ziparchive/cal/renice/free/dd entries, and what produced 626. With the
    field required, that shows up as a one-line contradiction in the diff
    instead of as a 350-line merge conflict that never mentions the number.
    """
    lines = doc(section(631, "B"))
    found = errs(mod, lines, {})
    check_true("a contradictory Lane field is reported",
               any("declares '**Lane:** B'" in e for e in found))
    check_true("the message says which band it is in",
               any("lane A's band" in e for e in found))


def test_a_lane_field_beyond_the_window_does_not_count(mod):
    """The field has to be where a reader resolving a band question looks."""
    head = f"## 631. a decision\n\n" + "\n".join([""] * 20) + "\n**Lane:** A\n"
    lines = doc(head)
    found = errs(mod, lines, {})
    check_true("a Lane field buried in the body is not found",
               any("has no '**Lane:** A' field" in e for e in found))


def test_a_superseded_banner_does_not_eat_the_window(mod):
    """A superseded entry puts its banner above its fields, and must still pass.

    Regression cover for a real false positive. Section 741 was superseded the
    same day it landed, which put an 11-line `> **SUPERSEDED ... by 745.**`
    blockquote between its heading and its `**Lane:** B` -- deliberately first,
    because it is what a reader must see before believing anything below it.
    The field was then at heading+14 and the gate reported it missing. Section
    30 has the identical shape and escaped only by predating the gate.

    The invariant is "the field is visible in the diff next to the heading",
    and a banner does not break it: the banner is part of that same diff. What
    must *not* be skipped is prose, which the sibling test above pins down --
    the first attempt at this fix skipped any leading run of blank-or-quote
    lines and thereby let a buried field count, and that test caught it.
    """
    banner = "\n".join(f"> line {i} of the supersession notice" for i in range(11))
    head = f"## 631. a decision\n\n{banner}\n\n**Date:** 2026-09-01\n**Lane:** A\n"
    found = errs(mod, doc(head), {})
    check_true("a Lane field below a banner is found",
               not any("has no '**Lane:** A' field" in e for e in found))


def test_a_banner_does_not_let_prose_be_skipped(mod):
    """The banner exemption ends at the first line that is not part of it."""
    head = (
        "## 631. a decision\n\n"
        "> a one-line supersession notice\n\n"
        + "\n".join([""] * 20)
        + "\nbody prose that is not a banner\n"
        + "\n".join([""] * 20)
        + "\n**Lane:** A\n"
    )
    found = errs(mod, doc(head), {})
    check_true("a field buried below prose is still not found",
               any("has no '**Lane:** A' field" in e for e in found))


def test_an_inline_lane_field_counts(mod):
    """The one-line header form declares a lane just as well as its own line.

    This is regression cover for a real false positive: the check used
    `LANE_FIELD_RE.match`, which anchors at column 0 whatever the pattern
    says, so an entry whose header read

        **Date:** ... - **Decided by:** ... - **Lane:** B

    was reported as having no lane field at all. Three of lane B's entries
    (731, 732, 733) had declared their lane correctly and were failed on the
    line it sat on, which put the whole gate -- and therefore boot-test.sh,
    which refuses to build while it is red -- into a state no lane could
    clear by fixing its own work. The rule is that the field is visible in
    the diff beside the heading; both forms are.
    """
    head = ("## 631. a decision\n\n"
            "**Date:** 2026-08-29 \u00b7 **Decided by:** Claude \u00b7 **Lane:** A\n\n"
            "**In short:** something was decided.\n")
    lines = doc(head)
    found = errs(mod, lines, {})
    check_true("an inline **Lane:** field is found",
               not any("has no '**Lane:**" in e for e in found))

    # And it is still the *declared* lane, not merely any lane -- an inline
    # field that contradicts its band has to be caught like any other.
    head_b = ("## 631. a decision\n\n"
              "**Date:** 2026-08-29 \u00b7 **Lane:** B\n\n"
              "**In short:** something was decided.\n")
    found_b = errs(mod, doc(head_b), {})
    check_true("an inline field in the wrong band is still reported",
               any("declares '**Lane:** B'" in e for e in found_b))


def test_the_lane_field_search_stops_at_the_next_heading(mod):
    """Otherwise a section with no field borrows its neighbour's."""
    lines = doc(section(631, lane=None), section(632, "A"))
    found = errs(mod, lines, {})
    check_true("a fieldless section does not borrow the next one's field",
               any("section 631 is new" in e and "has no '**Lane:**" in e
                   for e in found))
    check_true("and the next section, which has one, is not reported",
               not any("section 632" in e for e in found))


# --------------------------------------------------------------------------
# Per-band file order -- the rule that actually preserves the merge property
# --------------------------------------------------------------------------

def test_a_band_out_of_order_in_file_order_is_rejected(mod):
    lines = doc(section(601, "A"), section(600, "A"))
    found = errs(mod, lines, {600: 1, 601: 1})
    check_true("a descending pair inside a band is reported",
               any("appears after section 601" in e for e in found))


def test_interleaved_bands_are_fine_as_long_as_each_ascends(mod):
    """The property the real file actually has, and the reason for the rule.

    The 500s and the 600s are thoroughly interleaved by four months of merges
    -- the first 600 heading precedes twenty 500s. That is harmless. What
    matters is that each band's *own* run ascends, because then each band has
    exactly one insertion point, and three bands give three distinct line
    offsets that git never has to compare.
    """
    lines = doc(section(600, "A"), section(554, "C"), section(601, "A"),
                section(555, "C"), section(700, "B"))
    base = {600: 1, 601: 1, 554: 1, 555: 1, 700: 1}
    check("interleaved but individually ascending bands pass",
          errs(mod, lines, base), [])


def test_the_insertion_point_reported_is_the_bands_own_tail(mod):
    """Not "before the next band's first heading", which is already false.

    The document originally told lane C to insert before the first 600 heading.
    In the fixture below -- and in the real file -- that would put a new 500
    section *above* the 500s that already exist, out of order with its own
    neighbours, and at the same offset lane A is editing.
    """
    lines = doc(section(600, "A"), section(554, "C"), section(601, "A"))
    _, _, info = run(mod, lines, {600: 1, 601: 1, 554: 1})
    c_row = [row for row in info if row.startswith("500-599")]
    check_true("lane C gets an insertion point", len(c_row) == 1)
    check_true("and it is after C's own last entry, not before A's first",
               "next is 555" in c_row[0])
    first_600 = next(i for i, l in enumerate(lines, 1)
                     if l.startswith("## 600"))
    last_500 = next(i for i, l in enumerate(lines, 1)
                    if l.startswith("## 554"))
    check_true("the fixture really does put a 600 before the last 500",
               first_600 < last_500)
    check_true("the reported line is the 500's, not the 600's",
               f"insert after line {last_500}" in c_row[0])


# --------------------------------------------------------------------------
# Occupancy
# --------------------------------------------------------------------------

def test_a_nearly_full_band_warns_but_does_not_fail(mod):
    """Each of the three band exhaustions so far was discovered by running out."""
    lines = doc(section(585, "C"))
    found, warnings, _ = run(mod, lines, {585: 1})
    check("a full band is not an error", found, [])
    check_true("a nearly-full band warns",
               any("86% spent" in w for w in warnings))
    check_true("the warning says how many numbers are left",
               any("14 numbers left" in w for w in warnings))


def test_a_band_below_the_threshold_does_not_warn(mod):
    lines = doc(section(630, "A"))
    _, warnings, _ = run(mod, lines, {630: 1})
    check("a 31%-spent band does not warn", warnings, [])


# --------------------------------------------------------------------------
# Against the real document
#
# These cannot be fixtures. Each is a fact about design-decisions.md that the
# gate depends on, and that a fixture would keep asserting long after the
# document stopped having it.
# --------------------------------------------------------------------------

def test_the_real_document_passes_its_own_gate(mod):
    with open(REAL_DOC, encoding="utf-8") as fh:
        lines = fh.read().split("\n")
    baseline = mod.load_baseline(REAL_BASELINE)
    found, _, info = mod.check(lines, baseline)
    check("the real design-decisions.md passes", found, [])
    check_true("all three open bands report an insertion point", len(info) == 3)


def test_the_real_document_still_uses_both_heading_styles(mod):
    """If it ever stopped, the two-style regex would look like dead generality.

    It is not: 326 of the 527 headings use the section-sign form, and a future
    reader simplifying the regex to the plain form would silently reintroduce
    exactly the blindness that let 268-276 be duplicated nine times over.
    """
    with open(REAL_DOC, encoding="utf-8") as fh:
        lines = fh.read().split("\n")
    headings, errors = mod.parse_headings(lines)
    check("the real document has no unparseable headings", errors, [])
    styles = {h.style for h in headings}
    check("both heading styles are still live in the real document",
          styles, {"S", "P"})
    check_true("and neither is a rounding error",
               min(sum(1 for h in headings if h.style == s)
                   for s in styles) > 50)


def test_the_real_baseline_grandfathers_the_nine_known_duplicates(mod):
    """If these were ever renumbered, the duplicate rule's main case would be
    exercised by nothing, and nobody would notice."""
    with open(REAL_BASELINE, encoding="utf-8") as fh:
        data = json.load(fh)
    check("the recorded duplicates are 268-276",
          data.get("grandfathered_duplicates"), list(range(268, 277)))
    counts = data["counts"]
    check_true("each is recorded as borne by exactly two headings",
               all(counts[str(n)] == 2 for n in range(268, 277)))


def test_the_gates_own_output_is_ascii(mod, tmpdir):
    """It runs into a console whose code page is not UTF-8.

    A section sign there prints as a replacement box, which is how a message
    that is supposed to tell a lane exactly which line to edit turns into
    something the reader skips.
    """
    lines = doc(section(626, "A"), section(626, "B"), section(250, "A"),
                section(631, lane=None))
    found, warnings, info = run(mod, lines, {626: 1})
    check_true("the fixture really does produce output to check",
               len(found) >= 3)
    for text in found + warnings + info:
        try:
            text.encode("ascii")
        except UnicodeEncodeError:
            check(f"output is ASCII: {text[:60]!r}", False, True)
            return
    check_true("every error, warning and info line is ASCII", True)


def test_update_baseline_refuses_a_file_with_unparseable_headings(mod, tmpdir):
    """Baselining an invisible heading would record it as absent and make it
    permanently invisible -- the gate would then be actively hiding it."""
    path = os.path.join(tmpdir, "dd.md")
    with open(path, "w", encoding="utf-8", newline="\n") as fh:
        fh.write("\n".join(doc(section(600, "A"))) + "\n## 631 no punctuation\n")
    baseline = os.path.join(tmpdir, "baseline.json")
    rc = mod.main(["--file", path, "--baseline", baseline, "--update-baseline"])
    check("--update-baseline refuses an unparseable heading", rc, 2)
    check_true("and writes no baseline", not os.path.exists(baseline))


def test_a_missing_baseline_is_an_error_not_an_empty_one(mod, tmpdir):
    """An absent baseline read as empty would make every one of the 527
    existing headings 'new', i.e. a 527-line failure -- or worse, if the
    default were 'grandfather everything', a gate that never fires."""
    path = os.path.join(tmpdir, "dd.md")
    with open(path, "w", encoding="utf-8", newline="\n") as fh:
        fh.write("\n".join(doc(section(600, "A"))))
    rc = mod.main(["--file", path,
                   "--baseline", os.path.join(tmpdir, "nope.json")])
    check("a missing baseline exits 2, not 0", rc, 2)


def test_the_exit_code_distinguishes_violation_from_breakage(mod, tmpdir):
    """1 means the document is wrong; 2 means the gate could not run. A caller
    that cannot tell them apart reports a broken checker as a clean file or as
    a content failure, and both send the reader to the wrong place."""
    good = os.path.join(tmpdir, "good.md")
    with open(good, "w", encoding="utf-8", newline="\n") as fh:
        fh.write("\n".join(doc(section(600, "A"))))
    baseline = os.path.join(tmpdir, "b.json")
    check("baselining a clean file exits 0",
          mod.main(["--file", good, "--baseline", baseline,
                    "--update-baseline"]), 0)
    check("checking it then exits 0",
          mod.main(["--file", good, "--baseline", baseline, "--quiet"]), 0)

    bad = os.path.join(tmpdir, "bad.md")
    with open(bad, "w", encoding="utf-8", newline="\n") as fh:
        fh.write("\n".join(doc(section(600, "A"), section(250, "A"))))
    check("a violation exits 1",
          mod.main(["--file", bad, "--baseline", baseline, "--quiet"]), 1)
    check("an unreadable document exits 2",
          mod.main(["--file", os.path.join(tmpdir, "gone.md"),
                    "--baseline", baseline]), 2)


def main():
    mod = load_module()
    tests = [(name, fn) for name, fn in list(globals().items())
             if name.startswith("test_") and callable(fn)]
    # A discovery mechanism that discovers nothing looks exactly like a suite
    # that passes. Assert a floor, as the sibling suites do.
    if len(tests) < 25:
        print(f"FATAL: test discovery found only {len(tests)} tests; the suite "
              f"has at least 25. Discovery is broken, not the code.")
        return 1
    for name, fn in tests:
        params = inspect.signature(fn).parameters
        with tempfile.TemporaryDirectory() as tmpdir:
            avail = {"mod": mod, "tmpdir": tmpdir}
            fn(**{p: avail[p] for p in params if p in avail})

    print()
    if _FAILURES:
        print(f"{len(_FAILURES)} FAILED: {', '.join(_FAILURES)}")
        return 1
    print(f"all {len(tests)} design-decisions-bands tests passed")
    return 0


if __name__ == "__main__":
    sys.exit(main())
