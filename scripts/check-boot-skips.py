#!/usr/bin/env python3
"""Fail when a self-test skip has fired on *every* recorded boot.

Why this exists
---------------

A self-test that announces `SKIP: <section> (<why>)` is being honest, and
honesty is not the property that was missing. The failure this gate exists to
catch is a skip whose precondition has never once been true: the section has
never run, the suite above it still prints `PASSED`, and the whole thing reads
as coverage. Six of those were found by eye in `kernel/src/syscall/` on
2026-08-31 -- three suites gated on `is_mounted_rw("/")` that ran at boot steps
11 and 19 while the root mounts at step 20f, so the predicate was a *constant*
and had been false on every boot ever recorded (`design-decisions.md` sec 650).

Eye-finding does not scale to the seventh. The gate that does is dynamic
rather than static, and deliberately so: deciding "is this function reachable
only from a pre-mount call site?" needs cross-module call-graph resolution over
a 100k-line file, which is the kind of check that becomes unreliable and then
gets ignored. Instead `bench/boot-history.jsonl` -- which already records a row
per boot -- now carries the *names* of the skips that fired, and this script
asks the one question the static version was trying to approximate:

    has this skip fired on 100% of the last N boots, N >= 10?

A "yes" does not prove the predicate is a constant. It proves nobody has ever
observed it being anything else, which for a test is the same news: the section
is not covering what its name says it covers, and either the call site is wrong
(sec 650's six) or the precondition belongs in an allowlist entry that says so
out loud.

A skip is not the same thing as a section that did not run
-----------------------------------------------------------

The `skips` field this reads is **not** every SKIP line in the log; it is the
ones no other line in that boot reports as having run. That distinction is the
whole reason the first version of this gate was wrong on three of its four
findings. `ipc::io_ring` calls its two file-handle cases once before `/tmp` is
mounted -- where they skip -- and again after, where they pass. The pre-mount
call is a **deliberate tripwire**, and its source comment says why: if the
later call ever stopped happening, "the only evidence would be these two lines
never being followed by an OK". Flagging it would have been a permanent false
positive whose suggested fix -- delete the pre-mount call -- destroys the
tripwire. `boot-history.py`'s `partition_skips` computes the tripwire's own
stated condition, so a covered skip is silent here *until the day its coverage
disappears*, at which point it moves into this gate's evidence by itself.

The allowlist, and why it is not a dumping ground
-------------------------------------------------

Some skips genuinely fire on every boot *on this host* and are not defects:
`[pcid] live alloc_pcid tests` needs a CPU feature QEMU does not expose here.
The gate cannot tell those from sec 650's six, because from the log they look
identical -- which is the point. `ALLOWED` below converts that invisible
sameness into a reviewed, dated line of text, and every entry must state the
*observable condition* under which the skip would stop firing. If you cannot
write that sentence, the entry does not belong here and the skip is a defect.

Two properties keep it from rotting:

  * An allowlisted skip that is still firing is *printed* on every run, so it
    stays in front of a reader rather than becoming a file nobody opens.
  * An allowlisted skip that has stopped firing is a **failure**, not a
    shrug. Either its precondition now holds -- in which case the entry is a
    lie about the current tree -- or the section was renamed or deleted and the
    entry names nothing. Both are fixed by deleting a line.

Usage
-----

    python scripts/check-boot-skips.py [--history PATH] [--window N] [--min N]
    python scripts/check-boot-skips.py --list     # per-skip standing, never fails

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
#: Bounded rather than "all of history" because the interesting question is
#: about the tree as it is now. A skip fixed six months ago would otherwise
#: keep its 100% rate for as many boots as it had accumulated before the fix,
#: and the gate would go on failing long after the defect was gone -- which is
#: how a gate teaches its readers to pass `--no-verify`.
DEFAULT_WINDOW = 25

#: Below this many qualifying boots the gate declines to answer.
#:
#: Named in sec 650 as `N >= 10`. The number is a floor on *confidence*, not a
#: tuning knob: a skip that fired on both of the last two boots is not evidence
#: of anything, and a gate that failed on it would be wrong most of the time
#: and disabled within a week.
DEFAULT_MIN = 10

#: Skips that are expected to fire on every boot of this tree on this host,
#: with the observable condition that would stop them.
#:
#: Read the module docstring before adding to this. Each value must name what
#: would have to change in the *world* for the skip to stop firing -- a CPU
#: feature, a mounted filesystem, a second core. "It is fine" is not an entry.
ALLOWED: dict[str, str] = {
    "[selftest] suffix rendering": (
        "Deliberate and permanent: `fs::selftest::self_test` records one skip "
        "against itself so that `SkipSuffix`'s singular rendering (`1 "
        "section(s) SKIPPED`) is exercised. It stops firing only if that "
        "self-test stops testing its own reporting path, which would be a "
        "regression rather than a fix."
    ),
    "[pcid] live alloc_pcid tests": (
        "Needs PCID (CPUID.01H:ECX bit 17) on the running CPU. The harness "
        "boots `-cpu qemu64,+smep,+smap,+umip`, which does not add PCID; stops "
        "firing on bare metal, under WHPX with a host CPU model, or if `-cpu` "
        "gains `+pcid`. Verified 2026-08-31 that this is a real CPU-model fact "
        "and not the register-handling bug that made SMEP and SMAP look absent "
        "on the same boots -- pcid.rs reads ECX and EBX through named "
        "registers, so it was never exposed to it (design-decisions.md "
        "sec 652)."
    ),
    "[hotplug] offline/online cycle": (
        "Needs a second CPU to take offline. The boot test runs single-CPU; "
        "stops firing the moment the harness boots with `-smp 2` or more."
    ),
    "[iso9660] integration test": (
        "Needs an ISO 9660 filesystem mounted. The boot test attaches no "
        "optical image; stops firing when one is attached."
    ),
}


def load(path: str) -> list[dict]:
    """Every well-formed JSON object in the history, oldest first.

    A malformed line is skipped with a warning rather than raising: this file
    is committed and merged across three lanes, and one bad line arriving
    through a merge must not blind the gate to the other several hundred good
    ones.
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
                print(f"check-boot-skips: skipping malformed record at "
                      f"{path}:{lineno}: {exc}", file=sys.stderr)
                continue
            if isinstance(rec, dict):
                out.append(rec)
    return out


def qualifying(records: list[dict]) -> list[dict]:
    """The rows this gate is allowed to reason about.

    Three filters, each closing a way the answer could be wrong in the
    direction that *hides* a never-running section:

      * `"skips" in rec` -- a row written before the field existed says nothing
        about which skips fired, and counting it as "this skip did not fire"
        would break the 100% streak of every genuine offender. (The field holds
        the *uncovered* skips only. Exactly one row was written during the few
        hours the field held every skip; it was recomputed from its own serial
        log when the split landed, so no row in the file means the older thing.)
      * `boot_ok` -- a boot that died early never reached most suites, so their
        skips could not fire. Counting it has the same effect as the above.
      * not an experiment -- a probe runs the kernel under conditions no
        checkout reproduces, so what it did or did not skip is evidence about
        the probe. `boot-history.py` keeps probes out of every other statistic
        for exactly this reason.
    """
    return [
        rec for rec in records
        if isinstance(rec.get("skips"), list)
        and rec.get("boot_ok") is True
        and not rec.get("experiment")
    ]


def tally(window: list[dict]) -> dict[str, int]:
    """How many boots in `window` each skip name fired on."""
    counts: dict[str, int] = {}
    for rec in window:
        for name in set(rec.get("skips") or []):
            counts[name] = counts.get(name, 0) + 1
    return counts


def analyse(records: list[dict], window_size: int, minimum: int) -> dict:
    """The whole verdict as data, so the tests can assert on it directly."""
    rows = qualifying(records)
    window = rows[-window_size:] if window_size > 0 else rows
    n = len(window)
    counts = tally(window)

    result: dict = {
        "n": n,
        "enough": n >= minimum,
        "counts": counts,
        "always": [],       # fired on every boot, not allowlisted -> failure
        "allowed_firing": [],   # fired on every boot, allowlisted  -> printed
        "stale_allowlist": [],  # allowlisted but not always firing -> failure
    }
    if n < minimum:
        return result

    for name, count in sorted(counts.items()):
        if count != n:
            continue
        if name in ALLOWED:
            result["allowed_firing"].append(name)
        else:
            result["always"].append(name)

    always_all = set(result["always"]) | set(result["allowed_firing"])
    result["stale_allowlist"] = sorted(
        name for name in ALLOWED if name not in always_all)
    return result


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--history", default=DEFAULT_HISTORY)
    ap.add_argument("--window", type=int, default=DEFAULT_WINDOW,
                    help=f"boots to consider (default {DEFAULT_WINDOW})")
    ap.add_argument("--min", dest="minimum", type=int, default=DEFAULT_MIN,
                    help=f"decline to answer below this many (default "
                         f"{DEFAULT_MIN})")
    ap.add_argument("--list", action="store_true",
                    help="print per-skip standing and exit 0")
    args = ap.parse_args(argv)

    try:
        records = load(args.history)
    except OSError as exc:
        print(f"check-boot-skips: cannot read {args.history}: {exc}",
              file=sys.stderr)
        return 2

    res = analyse(records, args.window, args.minimum)

    if args.list:
        print(f"[boot-skips] {res['n']} qualifying boot(s) in the window")
        for name, count in sorted(res["counts"].items(),
                                  key=lambda kv: (-kv[1], kv[0])):
            mark = " ALLOWED" if name in ALLOWED else ""
            print(f"[boot-skips]   {count}/{res['n']}  {name}{mark}")
        return 0

    if not res["enough"]:
        # Not a pass and not a failure: the gate has nothing to say yet, and
        # says so with the number, so a reader can tell "no evidence" from
        # "evidence of no problem". Silence here would read as the latter.
        print(f"check-boot-skips: {res['n']} qualifying boot(s) recorded, "
              f"need {args.minimum} before a 100%-of-N verdict means anything "
              f"-- no verdict")
        return 0

    for name in res["allowed_firing"]:
        print(f"check-boot-skips: note: {name} skipped on all {res['n']} "
              f"boot(s) -- allowlisted: {ALLOWED[name]}")

    failed = False
    for name in res["always"]:
        failed = True
        print(f"check-boot-skips: FAIL: {name} has been skipped on all "
              f"{res['n']} of the last {res['n']} recorded boot(s). Its "
              f"precondition has never once been observed to hold, so the "
              f"section has never run and the suite reporting PASSED above it "
              f"is not covering it. Either move the call to where the "
              f"precondition is true (design-decisions.md sec 650), assert the "
              f"precondition instead of skipping on it, or add it to ALLOWED "
              f"in scripts/check-boot-skips.py with the observable condition "
              f"that would stop it firing.")

    for name in res["stale_allowlist"]:
        failed = True
        seen = res["counts"].get(name, 0)
        print(f"check-boot-skips: FAIL: {name} is in ALLOWED but fired on "
              f"only {seen} of the last {res['n']} boot(s). Either its "
              f"precondition now holds sometimes -- in which case the entry is "
              f"a false statement about the current tree -- or the section was "
              f"renamed or removed and the entry names nothing. Delete the "
              f"entry.")

    if not failed:
        print(f"check-boot-skips: OK ({res['n']} boot(s), "
              f"{len(res['counts'])} distinct skip(s), "
              f"{len(res['allowed_firing'])} allowlisted)")
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main())
