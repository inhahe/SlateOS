#!/usr/bin/env python3
"""Tests for `scripts/backfill-lane-fields.py`.

The script is a one-shot migration, which is exactly why it is worth testing:
its output was a 136-line diff with no prose in it, and a diff that size is
reviewed by spot-check, not by reading. The rules that a spot-check will not
catch are the placement ones -- the field has to land after the whole
`**Decided by:**` block including its unmarked continuation lines, inside the
gate's 12-line window, and before any other bolded field -- plus the four-number
exception where the value written is *not* the band's owner.

Every test asserts against `check-design-decisions-bands.py`'s own
`find_lane_field`, not against a line offset, because the only property that
actually matters is "the gate can see it".
"""

import importlib.util
import os
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
FAILURES = []
PASSES = []


def load(name, filename):
    spec = importlib.util.spec_from_file_location(
        name, os.path.join(HERE, filename))
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    return mod


def check(label, ok):
    (PASSES if ok else FAILURES).append(label)
    print(f"{'PASS' if ok else 'FAIL'}  {label}")


TABLE = "\n".join([
    "## Numbering and file order",
    "",
    "| Band | Owner | Status | Region |",
    "|---|---|---|---|",
    "| \u00a71\u2013\u00a7127 | single-agent history | closed | head |",
    "| \u00a7200\u2013\u00a7299 | **lane A** | closed \u2014 full | mid |",
    "| \u00a7400\u2013\u00a7499 | **lane C** | closed \u2014 full | mid |",
    "| \u00a7600\u2013\u00a7699 | **lane A** | **open** | interleaved |",
    "",
])


def build(*entries):
    return (TABLE + "\n" + "\n\n".join(entries) + "\n").split("\n")


def run_backfill(bf, lines, lane="A"):
    """Drive the script over ``lines`` in memory, returning the new lines.

    The script's `main` is file-oriented, so this reimplements its loop over
    the same helpers. That is deliberate: it keeps the tests on the placement
    logic, which is where the risk is, rather than on argparse and file I/O.
    """
    gate = bf.load_gate()
    bands, errors = gate.parse_bands(lines)
    assert not errors, errors
    out = list(lines)
    edits = []
    for idx, line in enumerate(lines):
        m = gate.HEADING_RE.match(line)
        if not m:
            continue
        number = int(m.group(2))
        if bf.band_lane(number, bands) != lane:
            continue
        h = type("H", (), {"lineno": idx + 1, "number": number})()
        if gate.find_lane_field(lines, h) is not None:
            continue
        at = bf.insertion_index(lines, idx + 1)
        if at is None:
            continue
        edits.append((at, number, bf.true_lane(number, bands)))
    for at, _n, lane_letter in sorted(edits, reverse=True):
        out.insert(at, f"**Lane:** {lane_letter}")
    return out, edits


def lane_of(gate, lines, number):
    for idx, line in enumerate(lines):
        m = gate.HEADING_RE.match(line)
        if m and int(m.group(2)) == number:
            h = type("H", (), {"lineno": idx + 1, "number": number})()
            return gate.find_lane_field(lines, h)
    return "<no such heading>"


# --------------------------------------------------------------------------

def test_a_plain_entry_gets_a_field_the_gate_can_see(bf, gate):
    lines = build("## \u00a7250 \u2014 a decision\n\n"
                  "**Date:** 2026-08-17\n"
                  "**Decided by:** Claude (autonomous)\n\n"
                  "**In short:** something.")
    out, _ = run_backfill(bf, lines)
    check("a plain entry gains a lane the gate can read",
          lane_of(gate, out, 250) == "A")


def test_the_field_goes_after_the_decided_by_continuation(bf, gate):
    """The shape that a naive "insert after the heading" gets wrong.

    `**Decided by:**` wraps onto unmarked continuation lines all over the real
    file. Inserting before one splits a sentence across a field, and the result
    reads as though the attribution were the lane.
    """
    lines = build("## \u00a7205 \u2014 a decision\n\n"
                  "**Date:** 2026-08-16\n"
                  "**Decided by:** Claude (autonomous) \u2014 these are the two\n"
                  "sub-decisions section 201 explicitly left to Lane A.\n\n"
                  "**In short:** something.")
    out, _ = run_backfill(bf, lines)
    check("the continuation case still yields a readable lane",
          lane_of(gate, out, 205) == "A")
    i = out.index("**Lane:** A")
    check("and the field lands after the continuation, not inside it",
          out[i - 1].startswith("sub-decisions"))
    check("the continuation sentence is left intact",
          "sub-decisions section 201 explicitly left to Lane A." in out[i - 1])


def test_the_field_precedes_a_following_bolded_field(bf, gate):
    """Several entries put `**Where:**` straight after `**Decided by:**`."""
    lines = build("## \u00a7209 \u2014 a decision\n\n"
                  "**Date:** 2026-08-16\n"
                  "**Decided by:** Claude (autonomous)\n"
                  "**Where:** `scripts/boot-history.py`\n\n"
                  "**In short:** something.")
    out, _ = run_backfill(bf, lines)
    i = out.index("**Lane:** A")
    check("the lane sits between Decided by and Where",
          out[i - 1].startswith("**Decided by:**")
          and out[i + 1].startswith("**Where:**"))
    check("and Where is not displaced out of the window",
          lane_of(gate, out, 209) == "A")


def test_the_inline_date_and_decided_by_form_is_handled(bf, gate):
    lines = build("## \u00a7208 \u2014 a decision\n\n"
                  "**Date:** 2026-08-16. **Decided by:** Claude (autonomous).\n\n"
                  "**In short:** something.")
    out, _ = run_backfill(bf, lines)
    check("the one-line metadata form gains a lane",
          lane_of(gate, out, 208) == "A")


def test_the_colon_outside_the_bold_is_still_an_attribution(bf, gate):
    """Section 294 writes `**Decided by**:`, and only section 294.

    Matching only `**Decided by:**` made it look like an entry with no
    attribution at all -- a far more alarming finding than a stray colon, and
    one that would have been reported as such.
    """
    lines = build("## \u00a7294 \u2014 a decision\n\n"
                  "**Date**: 2026-08-25\n"
                  "**Decided by**: Claude (autonomous)\n\n"
                  "**In short:** something.")
    out, _ = run_backfill(bf, lines)
    check("the variant punctuation is recognised as the attribution",
          lane_of(gate, out, 294) == "A")


def test_an_entry_that_already_declares_its_lane_is_left_alone(bf):
    lines = build("## \u00a7679 \u2014 a decision\n\n"
                  "**Date:** 2026-09-02. **Decided by:** Claude. **Lane:** A.\n\n"
                  "**In short:** something.")
    out, edits = run_backfill(bf, lines)
    check("no edit is proposed for an entry that has the field", edits == [])
    check("and the file is byte-identical", out == lines)


def test_the_four_exceptions_are_written_as_lane_c(bf, gate):
    """217-220 sit in lane A's band and are lane C's, permanently.

    Writing A on them would put a falsehood in the file in order to tidy it,
    which is the exact confusion the field exists to prevent.
    """
    lines = build("## \u00a7217 \u2014 an AMD display decision\n\n"
                  "**Date:** 2026-08-17\n"
                  "**Decided by:** Claude (autonomous)\n\n"
                  "**In short:** something.",
                  "## \u00a7221 \u2014 a lane A decision\n\n"
                  "**Date:** 2026-08-17\n"
                  "**Decided by:** Claude (autonomous)\n\n"
                  "**In short:** something.")
    out, edits = run_backfill(bf, lines)
    check("217 is annotated as lane C", lane_of(gate, out, 217) == "C")
    check("its neighbour 221 is still lane A", lane_of(gate, out, 221) == "A")
    check("both were selected, because both are in lane A's band",
          sorted(n for _, n, _l in edits) == [217, 221])


def test_another_lanes_entries_are_not_touched(bf):
    """One shared file: a lane that rewrites another's lines invites the
    merge conflict the bands exist to prevent."""
    lines = build("## \u00a7450 \u2014 a lane C decision\n\n"
                  "**Date:** 2026-08-17\n"
                  "**Decided by:** Claude (autonomous)\n\n"
                  "**In short:** something.")
    out, edits = run_backfill(bf, lines, lane="A")
    check("lane A proposes no edit inside lane C's band", edits == [])
    check("and leaves the file untouched", out == lines)


def test_the_single_agent_history_is_not_claimed_by_anyone(bf):
    lines = build("## 42. an old decision\n\n"
                  "**Date:** 2026-08-01\n"
                  "**Decided by:** Claude (autonomous)\n\n"
                  "**In short:** something.")
    for lane in ("A", "B", "C"):
        _out, edits = run_backfill(bf, lines, lane=lane)
        check(f"lane {lane} does not annotate the pre-lane history",
              edits == [])


def test_the_bands_come_from_the_document_not_from_the_script(bf):
    """If the table moves a band, the script must move with it.

    A hardcoded band would let this script and the gate disagree about who owns
    a number, and the disagreement would be invisible: the script would write a
    field the gate then rejects, or skip one it requires.
    """
    table = TABLE.replace("| \u00a7600\u2013\u00a7699 | **lane A** | **open** | interleaved |",
                          "| \u00a7600\u2013\u00a7699 | **lane B** | **open** | interleaved |")
    lines = (table + "\n" + "## \u00a7650 \u2014 a decision\n\n"
             "**Date:** 2026-08-17\n**Decided by:** Claude\n").split("\n")
    _out, a_edits = run_backfill(bf, lines, lane="A")
    _out, b_edits = run_backfill(bf, lines, lane="B")
    check("re-owning the band in the table moves it away from A",
          a_edits == [])
    check("and hands it to whoever the table now names",
          [n for _, n, _l in b_edits] == [650])


def test_the_real_document_is_fully_backfilled(bf, gate):
    """The migration's own success condition, asserted against the real file.

    This is what stops the script being re-run to no effect, and what would
    catch a future lane-A entry written without the field -- which the gate
    already rejects, but only for entries not yet baselined.
    """
    path = os.path.join(os.path.dirname(HERE), "design-decisions.md")
    with open(path, encoding="utf-8") as fh:
        lines = fh.read().split("\n")
    _out, edits = run_backfill(bf, lines, lane="A")
    check("no lane-A entry in the real document still lacks its lane field",
          edits == [])


def main():
    bf = load("bf", "backfill-lane-fields.py")
    gate = load("gate", "check-design-decisions-bands.py")
    test_a_plain_entry_gets_a_field_the_gate_can_see(bf, gate)
    test_the_field_goes_after_the_decided_by_continuation(bf, gate)
    test_the_field_precedes_a_following_bolded_field(bf, gate)
    test_the_inline_date_and_decided_by_form_is_handled(bf, gate)
    test_the_colon_outside_the_bold_is_still_an_attribution(bf, gate)
    test_an_entry_that_already_declares_its_lane_is_left_alone(bf)
    test_the_four_exceptions_are_written_as_lane_c(bf, gate)
    test_another_lanes_entries_are_not_touched(bf)
    test_the_single_agent_history_is_not_claimed_by_anyone(bf)
    test_the_bands_come_from_the_document_not_from_the_script(bf)
    test_the_real_document_is_fully_backfilled(bf, gate)

    print()
    if FAILURES:
        print(f"{len(FAILURES)} backfill-lane-fields test(s) FAILED:")
        for f in FAILURES:
            print(f"  - {f}")
        return 1
    print(f"all {len(PASSES)} backfill-lane-fields tests passed")
    return 0


if __name__ == "__main__":
    sys.exit(main())
