#!/usr/bin/env python3
"""Fail when a conditionally-called self-test has never once announced itself.

Why this exists
---------------

`check-self-tests-wired.py` answers "is this suite wired up". For six call
sites in `kernel/src/main.rs` that is the wrong question, because they sit
inside an `if`: they *are* wired, and whether they ran depends on the boot
path. For a year that gate printed a note asking a human to check each one
against the serial log, and for a year nobody did -- seven suites behind one
false `fat_ok` never executed while the gate said, accurately and uselessly,
"check each against the serial log". A guard whose output is a homework
assignment is a guard that is read once.

The `RAN-IF` marker turned that note into data: each gated site declares the
serial line it prints, `--emit-markers` writes them out, and `boot-history.py`
records per boot which of them appeared. This script is the consumer, and it
asks the one question the note was asking a reader to ask:

    has this marker been absent on 100% of the last N boots that recorded it,
    N >= 10?

A "yes" does not prove the branch is unreachable. It proves nobody has ever
observed it being taken, which for a test is the same news: the suite has never
run, and every summary line that counts it as coverage is counting a suite that
does not execute.

Silence, not a SKIP -- which is why `check-boot-skips.py` cannot see this
--------------------------------------------------------------------------

The sibling gate reads the `skips` field, and a skip is a *statement*: the
suite ran far enough to say `SKIP: <section> (<why>)`. That is the honest case,
and it is legible precisely because something was printed.

A suite behind `if fat_ok` prints nothing at all when the condition is false.
There is no SKIP line, no section name, no evidence of any kind -- the log of a
boot where it never ran is byte-identical, in that region, to the log of a boot
where it was never written. `skips` asks "did a suite say it was not running";
`gated_ran` asks "did a suite that says *nothing* simply not run". The second
question needs the marker because the log alone cannot answer it.

Counting: the denominator is per marker, and it has to be
---------------------------------------------------------

`gated_ran` is a dict of every marker declared *at the time that boot ran*, so
a marker added today is absent from the key set of every older row. Treating
that absence as "did not run" would make every new marker read as a 100%
never-run offender on the day it was introduced -- a gate that fails loudest at
the exact moment someone is doing the right thing. So each marker's rate is
computed only over the rows that carry its key, and the N >= 10 floor is
applied to *that* count, not to the window size. A new marker therefore has no
verdict until it has ten boots of its own, which is correct: it has no evidence
yet.

The live set is the newest row's key set. A marker that has disappeared from it
was deleted or renamed, and its history is evidence about code that no longer
exists; it is reported by `--list` and excluded from the verdict rather than
failing forever on behalf of a deleted line.

The allowlist
-------------

A gated suite may legitimately never run on this host -- the boot test attaches
no optical drive, boots one CPU, exposes no PCID. `ALLOWED` converts that into
a reviewed, dated line of text, and every entry must state the *observable
condition* under which the marker would start appearing. If you cannot write
that sentence, the entry does not belong here and the never-running suite is a
defect. As in `check-boot-skips.py`, an allowlisted marker that *has* been seen
running is a failure, not a shrug: either the entry is a false statement about
the current tree or it names a marker that no longer exists, and both are fixed
by deleting a line.

Usage
-----

    python scripts/check-gated-selftests.py [--history PATH] [--window N]
    python scripts/check-gated-selftests.py --list   # standing, never fails

Exit status: 0 clean (or not enough history yet), 1 findings, 2 could not read
the history at all.
"""

from __future__ import annotations

import argparse
import json
import os
import sys

SCRIPT_DIR = os.path.dirname(os.path.abspath(__file__))
REPO_ROOT = os.path.dirname(SCRIPT_DIR)
DEFAULT_HISTORY = os.path.join(REPO_ROOT, "bench", "boot-history.jsonl")

#: How many recent qualifying boots the verdict is drawn from.
#:
#: Bounded rather than all of history for the same reason as
#: `check-boot-skips.py`: the interesting question is about the tree as it is
#: now, and a branch that started being taken last week should clear its record
#: within a week's worth of boots rather than carrying a 100% never-ran rate for
#: as many boots as it accumulated beforehand.
DEFAULT_WINDOW = 25

#: Below this many boots *carrying a given marker*, that marker gets no verdict.
#:
#: The same floor on confidence as `check-boot-skips.py`, and for the same
#: reason: a suite that did not run on the last two boots is not evidence of
#: anything, and a gate that failed on it would be wrong most of the time and
#: disabled within a week.
DEFAULT_MIN = 10

#: Gated suites expected never to run on this tree on this host, with the
#: observable condition that would make them start.
#:
#: Read the module docstring before adding to this. Each value must name what
#: would have to change in the *world* for the marker to appear -- a device
#: attached, a second core, a CPU feature. "It is fine" is not an entry.
#:
#: Empty today, and that is a fact rather than an oversight: all six gated sites
#: were audited on 2026-08-31 against a full serial log and every one of them
#: was found to run on this host.
ALLOWED: dict[str, str] = {}


def load(path: str) -> list[dict]:
    """Every well-formed JSON object in the history, oldest first.

    A malformed line is skipped with a warning rather than raising: this file is
    committed and merged across three lanes, and one bad line arriving through a
    merge must not blind the gate to the other several hundred good ones.
    """
    out: list[dict] = []
    with open(path, "r", encoding="utf-8") as handle:
        for lineno, line in enumerate(handle, 1):
            line = line.strip()
            if not line:
                continue
            try:
                rec = json.loads(line)
            except json.JSONDecodeError as exc:
                print(f"check-gated-selftests: skipping malformed record at "
                      f"{path}:{lineno}: {exc}", file=sys.stderr)
                continue
            if isinstance(rec, dict):
                out.append(rec)
    return out


def qualifying(records: list[dict]) -> list[dict]:
    """The rows this gate is allowed to reason about.

    Three filters, each closing a way the answer could be wrong in the direction
    that manufactures a false accusation against working code:

      * `gated_ran` present and a dict -- a row written before the field existed,
        or one where `boot-history.py` could not read the markers file, says
        nothing about which suites announced themselves. `boot-history.py`
        deliberately *omits* the field rather than writing `{}` in that case,
        precisely so this filter can tell "no data" from "no markers declared".
      * `boot_ok` -- a boot that died early never reached most suites, so their
        markers could not appear. Counting it would break the record of every
        suite that runs late.
      * not an experiment -- a probe runs the kernel under conditions no
        checkout reproduces, so what it did or did not print is evidence about
        the probe, not about the tree.
    """
    return [
        rec for rec in records
        if isinstance(rec.get("gated_ran"), dict)
        and rec.get("boot_ok") is True
        and not rec.get("experiment")
    ]


def tally(window: list[dict]) -> dict[str, tuple[int, int]]:
    """Per marker: (boots it was seen on, boots that recorded it at all).

    The denominator is per marker rather than `len(window)` because the key set
    of `gated_ran` grows as markers are declared -- see the module docstring.
    """
    counts: dict[str, tuple[int, int]] = {}
    for rec in window:
        for name, ran in (rec.get("gated_ran") or {}).items():
            seen, total = counts.get(name, (0, 0))
            counts[name] = (seen + (1 if ran else 0), total + 1)
    return counts


def analyse(records: list[dict], window_size: int, minimum: int) -> dict:
    """The whole verdict as data, so the tests can assert on it directly."""
    rows = qualifying(records)
    window = rows[-window_size:] if window_size > 0 else rows
    counts = tally(window)

    # The markers that still exist. Anything else in the window belongs to a
    # call site that has since been deleted or renamed, and failing on its
    # behalf would be an accusation with no address.
    live: set[str] = set(window[-1].get("gated_ran") or {}) if window else set()

    result: dict = {
        "n": len(window),
        "counts": counts,
        "live": sorted(live),
        "retired": sorted(set(counts) - live),
        "never": [],           # never seen, enough evidence, not allowlisted
        "allowed_never": [],   # never seen, allowlisted -> printed, not failed
        "stale_allowlist": [],  # allowlisted but has been seen -> failure
        "undecided": [],       # live, never seen, but too few boots to say
    }

    for name in sorted(live):
        seen, total = counts.get(name, (0, 0))
        if seen:
            continue
        if name in ALLOWED:
            # Tested before the evidence floor, not after: an allowlist entry
            # says "this is expected never to run", and a marker that is indeed
            # never running is confirming it whether or not there are ten boots
            # yet. Routing it through `undecided` instead would leave it out of
            # `never_all` below and fail it as stale -- a new entry rejected for
            # being new.
            result["allowed_never"].append(name)
        elif total < minimum:
            result["undecided"].append((name, total))
        else:
            result["never"].append(name)

    # An entry is stale when the tree contradicts it: either the marker is gone
    # (so the entry names nothing) or it has been seen (so the entry is false).
    # Absent-and-still-declared is the one case it is *not* stale, which is the
    # case it exists to describe.
    result["stale_allowlist"] = sorted(
        name for name in ALLOWED
        if name not in live or counts.get(name, (0, 0))[0] > 0)
    return result


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--history", default=DEFAULT_HISTORY)
    ap.add_argument("--window", type=int, default=DEFAULT_WINDOW,
                    help=f"boots to consider (default {DEFAULT_WINDOW})")
    ap.add_argument("--min", dest="minimum", type=int, default=DEFAULT_MIN,
                    help=f"decline to answer below this many boots carrying "
                         f"the marker (default {DEFAULT_MIN})")
    ap.add_argument("--list", action="store_true",
                    help="print per-marker standing and exit 0")
    args = ap.parse_args(argv)

    try:
        records = load(args.history)
    except OSError as exc:
        print(f"check-gated-selftests: cannot read {args.history}: {exc}",
              file=sys.stderr)
        return 2

    res = analyse(records, args.window, args.minimum)

    if args.list:
        print(f"[gated-selftests] {res['n']} qualifying boot(s) in the window")
        for name in res["live"]:
            seen, total = res["counts"].get(name, (0, 0))
            mark = " ALLOWED" if name in ALLOWED else ""
            print(f"[gated-selftests]   ran on {seen}/{total}  {name}{mark}")
        for name in res["retired"]:
            seen, total = res["counts"][name]
            print(f"[gated-selftests]   ran on {seen}/{total}  {name} "
                  f"(retired -- not declared by the current tree)")
        return 0

    if not res["n"]:
        # Not a pass and not a failure. Said out loud because silence here would
        # read as "checked, nothing wrong" when the truth is "nothing to check
        # against yet" -- and because a permanently-zero count is how this gate
        # would look if the `--gated-markers` wiring in boot-test.sh broke.
        print("check-gated-selftests: no boot has recorded `gated_ran` yet "
              "-- no verdict. It is written by scripts/boot-history.py when "
              "boot-test.sh passes --gated-markers, which happens once "
              "check-self-tests-wired.py has emitted them for that run.")
        return 0

    if not res["live"] and res["counts"]:
        # Every marker the window knows about has been retired at once. That is
        # what a genuine mass deletion looks like, and it is also exactly what a
        # broken `--emit-markers` looks like -- an empty `markers` object writes
        # an empty `gated_ran`, after which this gate is silently unable to
        # accuse anyone. Say it rather than returning a clean OK.
        print(f"check-gated-selftests: note: the most recent boot declared no "
              f"gated markers at all, while earlier boots in the window "
              f"declared {len(res['counts'])}. Either every gated call site was "
              f"removed, or check-self-tests-wired.py --emit-markers is no "
              f"longer finding them -- in the second case this gate can no "
              f"longer see anything.")

    for name, total in res["undecided"]:
        print(f"check-gated-selftests: note: {name} has not been seen on any "
              f"of the {total} boot(s) recording it, which is below the "
              f"{args.minimum} needed for that to mean anything -- no verdict "
              f"on it yet.")

    for name in res["allowed_never"]:
        seen, total = res["counts"][name]
        print(f"check-gated-selftests: note: {name} has never appeared in "
              f"{total} boot(s) -- allowlisted: {ALLOWED[name]}")

    failed = False
    for name in res["never"]:
        seen, total = res["counts"][name]
        failed = True
        print(f"check-gated-selftests: FAIL: the serial line {name!r} has not "
              f"appeared on any of the last {total} recorded boot(s). The "
              f"self-test that prints it is called from inside a conditional "
              f"in kernel/src/main.rs, and that conditional has never once "
              f"been observed to be true, so the suite has never run -- it is "
              f"wired up and silent, which is the one failure mode that looks "
              f"exactly like success. Either fix the precondition, move the "
              f"call to where it holds, or add the marker to ALLOWED in "
              f"scripts/check-gated-selftests.py with the observable condition "
              f"that would make it start running.")

    for name in res["stale_allowlist"]:
        failed = True
        seen, total = res["counts"].get(name, (0, 0))
        if name not in res["live"]:
            why = ("no call site in the current tree declares it, so the entry "
                   "names nothing -- it was renamed or removed")
        else:
            why = (f"it was seen on {seen} of the {total} boot(s) recording it, "
                   f"so the suite does run and the entry is a false statement "
                   f"about the current tree")
        print(f"check-gated-selftests: FAIL: {name!r} is in ALLOWED but "
              f"{why}. Delete the entry.")

    if not failed:
        print(f"check-gated-selftests: OK ({res['n']} boot(s), "
              f"{len(res['live'])} live marker(s), "
              f"{len(res['undecided'])} without a verdict yet, "
              f"{len(res['allowed_never'])} allowlisted)")
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main())
