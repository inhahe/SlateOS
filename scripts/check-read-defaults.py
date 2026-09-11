#!/usr/bin/env python3
"""Find `.unwrap_or_default()` on a call that returns Option or Result to say
it could not find out.

## Three families, one defect

    fs::read_to_string(path).unwrap_or_default()   a file read
    env::var("USER").unwrap_or_default()           an environment lookup
    read_cpu_stats().unwrap_or_default()           a local fallible function

The third was added on 2026-09-10 after `userspace/iostat` was found printing

    avg-cpu:  %user   %nice %system %iowait  %steal   %idle
              0.00    0.00    0.00    0.00    0.00    0.00

on a machine whose /proc/stat could not be read at all. `read_cpu_stats`
returns Option precisely to say so, and both callers discarded it. THE DEFECT
WAS IDENTICAL TO THE ONE THIS GATE EXISTS FOR AND ONE CALL DEEPER THAN ITS
PATTERN COULD SEE -- a checker that recognises only the spelling of the first
instance it was written for passes, and the passing means nothing.

Only functions DEFINED IN THE FILE BEING SCANNED are considered, and only those
whose signature says they can fail: the return type is read from the definition
rather than guessed from the name. Method calls (`x.get(..)`) are excluded, and
that exclusion is the difference between 39 findings and 171 -- one local
`fn get(..) -> Option<T>` otherwise implicates every slice in the file.

## What it looks for, and why only this shape

    let text = fs::read_to_string(path).unwrap_or_default();

collapses three situations into one empty string:

    the file is there        its contents
    the file is absent       nothing configured yet
    the file cannot be read  WE DO NOT KNOW

The third is the defect. It is lane A's rule from `mkfs`/`fsck`'s
`is_mounted` -- for a value that guards a decision, "I do not know" and "there
is nothing there" must not be the same -- and on 2026-09-10 it was found four
times in this lane, written independently each time:

  * `userspace/sudo`'s visudo opened an EMPTY EDITOR over `/etc/sudoers`. The
    user adds a rule to what looks like a blank file, saves, and every existing
    rule is replaced by the one line they typed.
  * `userspace/xdg` rewrote `~/.config/mimeapps.list` holding only the
    association just set.
  * `userspace/hostnamectl` rewrote `/etc/machine-info` holding only the field
    just set.
  * `userspace/ntpd` fell back to `pool.ntp.org`, `time.google.com` and
    `time.cloudflare.com`, so an administrator who had restricted time sync to
    internal servers silently took the clock from outside their network.

`optionalfile::read_or_empty` is the answer and says which failure means what.

**`unwrap_or_else(|_| something)` is NOT flagged.** A caller writing an
explicit default has thought about the failure; `nologin`'s built-in message
and `perf`'s `[pid:N]` placeholder are both deliberate and correct. It is
`unwrap_or_default()` specifically that reads as "I did not consider this".

**A `fs::read` (bytes) is not flagged either.** The whole-file UTF-8 failure --
where ONE byte anywhere in the file empties all of it -- is what makes the text
version reachable without unusual permissions, and `userspace/pwdb` reads bytes
deliberately for exactly that reason.

## Usage

    python scripts/check-read-defaults.py            # report
    python scripts/check-read-defaults.py --check    # 1 if a new one appeared
    python scripts/check-read-defaults.py --update-baseline
"""

from __future__ import annotations

import argparse
import os
import re
import sys
from pathlib import Path
from typing import NamedTuple

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import gittree  # noqa: E402
import rustlex  # noqa: E402
from rustlex import live_code, strip_noise  # noqa: E402

ROOT = Path(__file__).resolve().parent.parent
# Repo-relative and `/`-separated: the only spelling `gittree.Tree` accepts.
BASELINE_REL = "scripts/read-defaults-baseline.txt"
BASELINE = Path(__file__).resolve().parent / "read-defaults-baseline.txt"

# ---------------------------------------------------------------------------
# Floors on what was INSPECTED, not on what was found.
#
# The healthy state of this gate is "no new sites", and every number in that
# sentence counts something that is WRONG. So a scan that reads no files at all
# reports zero found, which passes -- and worse, reports all 17 pinned entries
# as `fixed:`, which is a congratulation. A total failure to look was spelled
# as the best possible news.
#
# That is lane A's §3 finding about `check-selftest-reinit.py`, arriving here
# by two routes on the same day: their request said the detect/refuse pair can
# pass while DISCOVER has silently failed, and lane A independently sent word
# that over-masking took their own absent-operand count from 37 to 0 with the
# gate still green. Both are this shape.
#
# Two floors, because there are two independent ways to stop seeing:
#
#   MIN_FILES  the walk stopped finding files -- `files_under` changed, the
#              prefix was renamed, `.rs` stopped matching.
#   MIN_READS  the walk still finds files but `strip_noise` blanked their
#              contents, so there is nothing left to match. This is the one
#              lane A hit; a file count alone cannot see it.
#
# Measured on main at a1228a0fa: 422 files, 398 live `read_to_string`.
# The floors sit near a third of that, well under ordinary attrition -- 225
# crates were deleted from this lane in the past week and the count must
# survive more of that -- but far above the zero-or-near-zero that every
# failure above produces.
MIN_FILES = 150
MIN_READS = 120



# The standard-library calls whose failure means the outside world could not be
# consulted. A CLOSED LIST, not a heuristic: every name here is one whose Err or
# None says "I could not find out", and whose `unwrap_or_default()` therefore
# turns that into a value indistinguishable from a real reading.
#
# `env::var` earns its place twice over -- an absent variable becoming an empty
# string is how six programs came to take the caller's identity from the
# environment (TD-B-FIVE-PROGRAMS-STILL-TAKE-THE-CALLERS-IDENTITY...).
#
# `fs::read` (bytes) is DELIBERATELY ABSENT and stays absent. The docstring
# above gives the reason and names the crate that relies on it: the text
# version's failure is reachable without unusual permissions because one bad
# byte anywhere empties the whole file, and `userspace/pwdb` reads bytes for
# precisely that reason. I added `read` to this tuple while widening the gate,
# did not notice I was reversing a documented decision, and took it back out
# when the docstring said so. A closed list is only safe if the reasons for
# each absence are written next to it.
STD_READERS = (
    "read_to_string",
    "read_link",
    "read_dir",
    "metadata",
    "symlink_metadata",
    "canonicalize",
)

_ARGS = r"(?:[^()]|\([^()]*\))*"
_TAIL = r"\s*(?:\n\s*)?\.unwrap_or_default\s*\(\s*\)"

PATTERN = re.compile(
    r"(?:std::)?(?:fs::)?(?:" + "|".join(STD_READERS) + r")\s*\(" + _ARGS + r"\)" + _TAIL,
    re.S,
)
ENV_PATTERN = re.compile(
    r"(?:std::)?env::var(?:_os)?\s*\(" + _ARGS + r"\)" + _TAIL,
    re.S,
)
# `fs::read` -- ONLY with its module qualifier.
#
# It is missing from STD_READERS above, and the reason is that the names there
# go in unqualified: a bare `read` alternative would match `buf.read(..)` and
# `sock.read(..)`, every one of which is a method call and none of which is
# this defect. Requiring the `fs::` makes it unambiguous, and `read_to_string`
# is not swept up because the `(` has to follow `read` directly.
#
# It reached this list the long way. `oils/src/interp.rs` has
# `std::fs::read(&path).unwrap_or_default()` in `fc -e`, which reads back the
# file the user just edited and RUNS it; the gate only saw it because oils also
# defines a local `fn read`, and the local half's lookbehind did not exclude a
# `::` prefix. The site was real, the reason it was reported was not.
QUALIFIED_PATTERN = re.compile(
    r"(?:std::)?fs::read\s*\(" + _ARGS + r"\)" + _TAIL,
    re.S,
)

# A function DEFINED IN THIS FILE returning Option<..> or Result<..>.
#
# WHY THIS HALF EXISTS. `userspace/iostat` read /proc/stat through
# `read_cpu_stats() -> Option<CpuStats>` and both callers wrote
# `.unwrap_or_default()`, so an unreadable /proc/stat printed as a CPU that was
# 0.00% busy in every column -- a perfectly idle machine. The defect was
# identical to the one this gate was written for and one call deeper than its
# pattern could see, so the gate passed the file for a year.
# `[ \t]*`, NOT `\s*`. Indentation is spaces and tabs; `\s` also matches the
# newline, so `^\s*` at a line start consumes every following blank line as
# well -- and then backtracks over the whole run one character at a time
# looking for `fn`. `rustlex.live_code` BLANKS test code to spaces rather than
# deleting it, so a crate with a big test module now carries a whitespace run
# the size of that module: `oils/src/interp.rs` has one of 1.5 MB. Scanning it
# is quadratic in the run length, and this pattern alone did not finish in 290
# seconds on that file. It was fast before only because `live_code` used to cut
# the run off instead of blanking it.
LOCAL_FALLIBLE = re.compile(
    r"^[ \t]*(?:pub\s+)?fn\s+(\w+)\s*\([^)]*\)\s*->\s*(?:Option|Result)\s*<", re.M
)


def local_pattern(names: set[str]) -> re.Pattern | None:
    """`name(..).unwrap_or_default()` for ANY of `names`, and NOT `x.name(..)`.

    The negative lookbehind is the whole difference between 39 findings and
    171. A file that happens to define any local `fn get(..) -> Option<T>`
    otherwise makes every `slice.get(a..b).unwrap_or_default()` in it look like
    a discarded read -- and slicing with a default IS legitimate, which is how
    a gate becomes noise and then becomes bypassed. Caught by sampling twelve
    of the 171 before writing any of this down.

    # One pattern for all the names, not one pattern each

    This took a `str` and the caller ran it once per name, which is a full scan
    of the file per locally-defined fallible function. That was affordable only
    because the scanner could not see most of the code: when `rustlex.live_code`
    stopped truncating at the first `#[cfg(test)] mod`, `oils/src/interp.rs`
    went from 3,181 visible lines to 67,968 and from a handful of local fallible
    functions to 262 -- and `--check` went from 19.6 seconds to over eight
    minutes. Every lane pays that on every push.

    An alternation scans once for all of them. The results are identical: a span
    begins with exactly one of the names, and `record` reads only the match
    offsets. Longest-first ordering keeps a name that is a prefix of another
    (`read` before `read_all`) from matching first and forcing a backtrack
    through the rest of the pattern.
    """
    if not names:
        return None
    alts = "|".join(re.escape(n) for n in sorted(names, key=len, reverse=True))
    # THE LOOKBEHIND DOES NOT EXCLUDE `:`, and that was tried. The thought was
    # that a qualified path is not the local function -- but `Self::f(..)` is
    # exactly the local function, written the way an associated function has to
    # be written. Adding `:` dropped `pkg: Self::extract_string(..)`, a real
    # pinned finding, silently and in the shrinking direction. A qualified
    # std call reaching this pattern is instead de-duplicated by span in
    # `record`, which fixes the attribution without losing anything.
    return re.compile(
        r"(?<![.\w])(?:" + alts + r")\s*\(" + _ARGS + r"\)" + _TAIL, re.S
    )


class Scan(NamedTuple):
    """What was found, and -- separately -- how much was looked at.

    The two must be reported apart. `found` shrinking is the goal; `files` or
    `reads` shrinking is a broken scan wearing the goal's clothes.
    """

    found: list[str]
    files: int
    reads: int


def scan_is_too_thin(scan: Scan) -> bool:
    """Did we look at enough to be entitled to an opinion?

    Split out of `main` so the self-test can drive both directions. A floor
    that has never been observed to refuse is the same kind of claim as a gate
    nothing runs: it looks exactly like a floor that has nothing to say.
    """
    return scan.files < MIN_FILES or scan.reads < MIN_READS


def survey(tree: gittree.Tree) -> Scan:
    """`<crate>: <call>` for every live occurrence.

    Keyed on the call text rather than a line number, because a line number
    churns on every edit above it and would make the baseline unreadable.

    # Two things this gets right that the first draft did not

    **The snippet comes from the ORIGINAL source, not the stripped copy.**
    `strip_noise` blanks string literals, so matching text is not the text to
    show: `read_to_string(format!("{sys_path}/removable"))` came out as
    `read_to_string(format!( ))`, and the three `eject` calls -- for
    `removable`, `device/model` and `device/vendor` -- became the same
    unreadable line. That is why `strip_noise` preserves length: the match
    offsets index the original just as well.

    **Identical calls are numbered.** Two of `newgrp`'s reads normalise to the
    same text even with their arguments restored, and a `set` comparison
    silently merges them -- so fixing one of two would look like fixing both.
    A `#2` suffix keeps them distinct.
    """
    found: list[str] = []
    files = 0
    reads = 0
    for rel in sorted(tree.files_under("userspace")):
        if not rel.endswith(".rs"):
            continue
        src = tree.read_text(rel)
        if src is None:
            continue
        files += 1
        crate = rel.split("/")[1]
        original, body = live_code(src)
        # Counted AFTER stripping, so a `strip_noise` that blanks too much
        # drives this to zero and trips MIN_READS. Counting before would make
        # the floor blind to precisely the failure it exists to catch.
        reads += body.count("read_to_string")
        seen: dict[str, int] = {}
        # One site is one finding, however many patterns reach it. Every
        # pattern ends at the same `.unwrap_or_default()`, so its end offset
        # identifies the site: `std::fs::read(p).unwrap_or_default()` is found
        # by the qualified reader AND, in a file that defines its own `fn
        # read`, by the local half. The specific patterns run first, so the
        # first to arrive is the one that names the cause correctly.
        ends: set[int] = set()

        def record(m: re.Match) -> None:
            if m.end() in ends:
                return
            ends.add(m.end())
            call = " ".join(original[m.start() : m.end()].split())
            key = f"{crate}: {call}"
            seen[key] = seen.get(key, 0) + 1
            if seen[key] > 1:
                # `[2]`, NOT `#2`. `read_baseline` strips everything after a
                # `#` as a comment, so a `#2` suffix was eaten on the way back
                # in and every second-and-later occurrence of an identical call
                # looked new FOREVER -- unpinnable by construction. Ten entries
                # in this file were in that state the moment the gate widened
                # enough to find duplicates.
                key = f"{key}  [{seen[key]}]"
            found.append(key)

        for m in PATTERN.finditer(body):
            record(m)
        for m in ENV_PATTERN.finditer(body):
            record(m)
        for m in QUALIFIED_PATTERN.finditer(body):
            record(m)
        # Only functions this file defines, and only those whose signature
        # SAYS they can fail. The return type is read from the definition
        # rather than guessed from the name, so `read_config` that returns a
        # plain String is not accused of anything.
        local_names = {m.group(1) for m in LOCAL_FALLIBLE.finditer(body)}
        local_re = local_pattern(local_names)
        if local_re is not None:
            for m in local_re.finditer(body):
                record(m)
    return Scan(sorted(found), files, reads)


def read_notes() -> dict[str, str]:
    """Each pinned entry's trailing ` # note`, keyed by the entry.

    # Why this exists

    A line in the baseline means one of two things -- "a defect nobody has
    fixed" or "read in context and correct as it stands" -- and the file could
    not tell them apart. Every entry therefore had to be re-read by whoever
    next worked the ledger, which is how `acpi`'s seven and `mktemp`'s four
    were each examined twice before anybody wrote down that they were fine.

    The obvious fix -- a comment beside the entry -- is one regeneration from
    being lost, because `--update-baseline` rewrites this file wholesale.
    `multicall-aliases` records that exact loss: "Policy written into a
    generated file is one regeneration from being lost, and nothing reports the
    loss: the gate stays green, the entries stay right, and only the reasoning
    goes." So the notes are read back off the previous file and re-attached to
    the entries that survive. An entry that has gone takes its note with it,
    which is right: the note was about that site.

    Read from the DISK rather than the revision, because the only caller is
    `--update-baseline`, which writes the disk.
    """
    notes: dict[str, str] = {}
    try:
        text = BASELINE.read_text(encoding="utf-8")
    except OSError:
        return notes
    for line in text.splitlines():
        if not line.strip() or line.lstrip().startswith("#"):
            continue
        entry, sep, note = line.partition("#")
        if sep and note.strip():
            notes[entry.strip()] = "  # " + note.strip()
    return notes


def read_baseline(tree: gittree.Tree) -> set[str] | None:
    """The pinned set, out of the REVISION rather than the disk.

    `None` means the revision carries no baseline at all, which is not the
    same as an empty one: absent says this revision has no policy -- it
    predates the ratchet, or it is a fixture -- and judging a commit by a file
    it does not carry is judging it by a rule it never had.

    Both halves of this were wrong in `multicall-aliases.py` earlier today and
    broke every lane's boot test: it read the disk, and it turned absent into
    empty so that every pinned entry looked new.
    """
    text = tree.read_text(BASELINE_REL)
    if text is None:
        return None
    names = set()
    for line in text.splitlines():
        line = line.split("#", 1)[0].strip()
        if line:
            names.add(line)
    return names


HEADER = """\
# `read_to_string(...).unwrap_or_default()` sites pinned by
# `scripts/check-read-defaults.py --check`.
#
# Each line is a read whose failure is indistinguishable from an empty file.
# THIS FILE SHOULD ONLY EVER SHRINK. The fix is `optionalfile::read_or_empty`,
# which separates "absent" from "could not read"; adding a line here to turn a
# red --check green is the defect itself.
#
# TWO KINDS OF ENTRY LIVE HERE AND THEY ARE NOT EQUALLY VOUCHED FOR.
#
# The `read_to_string` entries were inspected one at a time on 2026-09-10: all
# of them report or display, none rewrites the file it read. The four that DID
# rewrite (sudo/visudo, xdg, hostnamectl, ntpd) are fixed and are not here.
#
# The rest arrived the same day when the gate was widened to see env::var and
# local fallible functions, and they were pinned AS A SET, not read one by one.
# Sampling them found real defects -- `ftp` turning a failed password read into
# an empty password it then sends, `stty` reading a 0x0 terminal when the ioctl
# fails, `crontab` taking an empty username. Those are recorded in
# known-issues.md as TD-B-EIGHTY-THREE-DISCARDED-FAILURES-ARE-PINNED-UNREAD.
#
# So: being pinned means "known", and for the first group also "not
# destructive". It has never meant "correct".
#
#     python scripts/check-read-defaults.py --update-baseline
#
#
# A trailing `  # note` on an entry says it has been READ IN CONTEXT and is
# correct as it stands -- the empty string reaches something that handles it,
# or the field is printed only when non-empty. An entry with NO note has not
# been examined, or has been and is a real defect.
#
# That distinction is the whole point: a bare line used to mean either "a
# defect nobody has fixed" or "fine, and somebody already checked", and the
# file could not tell them apart -- so `acpi`'s seven and `mktemp`'s four were
# each read twice before anyone wrote the answer down.
#
# `--update-baseline` carries these forward; see `read_notes`. Do not write
# anything here that is not about one entry: this file is regenerated, and a
# free-standing paragraph would be lost silently.
"""


class _FakeTree:
    """The two methods `survey` uses, backed by a dict.

    A real `gittree` needs a git repository; these fixtures need to run
    anywhere, including in a checkout with no history, which is the point of
    running the self-test before the check.
    """

    def __init__(self, files: dict[str, str]) -> None:
        self._files = files

    def files_under(self, prefix: str):
        return [r for r in self._files if r.startswith(prefix + "/")]

    def read_text(self, rel: str):
        return self._files.get(rel)


def _self_test() -> int:
    """Fixtures for `strip_noise`, which has been wrong twice.

    Both times it failed in the direction that looks fine: the scan still
    produced a plausible list of sites, still passed `--check`, and still
    refused when a line was unpinned. Nothing in the output said the input had
    been misread. These pin the cases by name so a third rewrite cannot lose
    them silently.
    """
    failures = 0

    def expect(label: str, got: object, want: object) -> None:
        nonlocal failures
        ok = got == want
        failures += not ok
        print(f"  {'ok  ' if ok else 'FAIL'}  {label}")
        if not ok:
            print(f"          got  {got!r}")
            print(f"          want {want!r}")

    # A KEY MUST SURVIVE THE ROUND TRIP INTO THE BASELINE AND BACK.
    #
    # `read_baseline` strips everything after a `#` as a comment, so the
    # duplicate marker `#2` was eaten on the way back in: the second and every
    # later occurrence of an identical call could never be pinned, and was
    # reported NEW on every run forever. Ten entries were in that state the
    # moment the gate widened enough to find duplicates. The marker is `[2]`
    # now, and these are the cases that would have caught it.
    def baseline_roundtrip(key: str) -> str:
        return key.split("#", 1)[0].strip()

    expect(
        "a duplicate marker survives the baseline's comment stripping",
        baseline_roundtrip("crate: f().unwrap_or_default()  [2]"),
        "crate: f().unwrap_or_default()  [2]",
    )
    expect(
        "...where the old `#2` marker was eaten by it",
        baseline_roundtrip("crate: f().unwrap_or_default()  #2"),
        "crate: f().unwrap_or_default()",
    )

    # `strip_noise`'s own fixtures live in `scripts/rustlex.py` beside the
    # function, because a caller's suite proves the caller reads the lexer
    # correctly rather than that the lexer is correct. Run there.
    expect("the shared lexer's fixtures pass", rustlex._self_test(), 0)

    # -----------------------------------------------------------------------
    # `live_code`: a `#[cfg(test)]` on a HELPER must not hide the rest of the
    # file. `survey` used to do `src.split("#[cfg(test)]")[0]`, which is
    # "everything before the tests" only when the first such attribute IS the
    # test module. Measured before the fix: 35,706 lines across userspace/,
    # 7% of the lane, invisible -- fdisk/src/main.rs read as 21 lines of
    # 3,818, tar.rs as 592 of 5,635.
    #
    # The floors did not catch it and could not: they are aggregate, and
    # losing 7% of the corpus leaves 398 live reads against a floor of 120
    # while every file is still opened so the file count never moves. A floor
    # on the total cannot see a hole in the distribution.
    # -----------------------------------------------------------------------
    helper_first = (
        "fn a() {}" + chr(10)
        + "#[cfg(test)]" + chr(10)
        + "fn helper() { let _ = 1; }" + chr(10)
        + "fn live() { let x = fs::read_to_string(p).unwrap_or_default(); }" + chr(10)
        + "#[cfg(test)]" + chr(10)
        + "mod tests { fn t() {} }" + chr(10)
    )
    kept, _ = live_code(helper_first)
    expect("a #[cfg(test)] helper does not truncate the file",
           "fn live()" in kept, True)
    expect("...and the helper itself is still excluded",
           "fn helper()" in kept, False)
    expect("...and the test module is still cut",
           "mod tests" in kept, False)
    # Blanking preserves position; cutting the test module legitimately
    # shortens. What `survey` needs is that offsets into the RETURNED text
    # index the same characters, which holds because it derives both the
    # match offsets and the displayed text from this one string. Tested on a
    # file with no test module, where nothing is cut.
    no_module = (
        "#[cfg(test)]" + chr(10)
        + "fn helper() { let _ = 1; }" + chr(10)
        + "fn live() { let x = fs::read_to_string(p).unwrap_or_default(); }" + chr(10)
    )
    blanked, _ = live_code(no_module)
    expect("blanking an item preserves length", len(blanked), len(no_module))
    expect("...and the surviving text sits at its original offset",
           blanked.index("fn live()"), no_module.index("fn live()"))

    # A brace inside a string must not close the item early -- this file has
    # been wrong about string boundaries twice, so the masking is reused.
    braces = (
        "#[cfg(test)]" + chr(10)
        + 'fn h() { let s = "}"; let _ = s; }' + chr(10)
        + "fn live() { let x = fs::read_to_string(p).unwrap_or_default(); }" + chr(10)
    )
    expect("a brace inside a string does not end the test item early",
           "fn live()" in live_code(braces)[0], True)

    # -----------------------------------------------------------------------
    # `survey` itself. Everything above this line feeds `strip_noise` a string;
    # nothing above it proves the scan can still FIND anything. These pin exact
    # non-zero counts, because "reported nothing" and "found nothing" print the
    # same way and only a fixture that must yield a specific number can tell
    # them apart. Lane A's suggestion, 2026-09-10.
    # -----------------------------------------------------------------------
    site = "fs::read_to_string(p).unwrap_or_default()"

    tree = _FakeTree({
        # two crates, three sites, plus decoys that must NOT count
        "userspace/alpha/src/main.rs":
            "fn a() { let x = " + site + "; }" + chr(10)
            + "fn b() { let y = " + site + "; }" + chr(10),
        "userspace/beta/src/lib.rs":
            "fn c() { let z = " + site + "; }" + chr(10),
        # an epitaph: the pattern quoted in a comment where a site was FIXED.
        # Lane A's checker scored one of these as a live site, so its ledger
        # could never reach zero while the explanation existed and the cheapest
        # way to lower the number was to delete the record.
        "userspace/gamma/src/main.rs":
            "// was " + site + " before optionalfile" + chr(10)
            + "fn d() { optionalfile::read_or_empty(p); }" + chr(10),
        # below #[cfg(test)] is not live code
        "userspace/delta/src/main.rs":
            "fn e() {}" + chr(10) + "#[cfg(test)]" + chr(10)
            + "mod t { fn f() { let q = " + site + "; } }" + chr(10),
        # not Rust, and not under a crate we scan
        "userspace/alpha/README.md": "read_to_string(p).unwrap_or_default()",
    })
    scan = survey(tree)

    expect("survey finds EXACTLY the three live sites", len(scan.found), 3)
    expect("...attributed to the right crates",
           sorted({f.split(":")[0] for f in scan.found}), ["alpha", "beta"])
    expect("...a quoted site in a comment is not one",
           any(f.startswith("gamma") for f in scan.found), False)
    expect("...nor is one below #[cfg(test)]",
           any(f.startswith("delta") for f in scan.found), False)
    expect("...two identical calls in one crate stay distinct",
           sum(1 for f in scan.found if f.startswith("alpha")), 2)
    expect("...and the second is numbered `[2]`, not `#2`",
           any(f.endswith("[2]") for f in scan.found), True)
    expect("only .rs files are read", scan.files, 4)
    # Three, not four: `gamma`'s is inside a comment and `delta`'s is below
    # `#[cfg(test)]`, and BOTH are removed before the count. That is the right
    # population for a floor -- it must measure the live text the pattern
    # actually searches, or it would stay comfortable while the searchable code
    # went to nothing. (Written as 4 on the first attempt; this fixture caught
    # it, which is the argument for the fixture.)
    expect("the read_to_string population counts live code only -- not "
           "comments, not test modules",
           scan.reads, 3)

    # A note beside an entry must survive `--update-baseline`, or the reason an
    # entry is known-good is one regeneration from being lost -- and losing it
    # is silent, because the entries stay right and only the reasoning goes.
    import tempfile

    with tempfile.TemporaryDirectory() as tmp:
        fake = Path(tmp) / "baseline.txt"
        # newline='' so the fixture is LF on every platform. Without it this
        # file is CRLF on Windows and LF on Linux, so the parser under test
        # sees different bytes depending on who ran the suite -- which is the
        # one thing a fixture must not do.
        fake.write_text(
            "# header\n"
            "alpha: keeps(x).unwrap_or_default()  # examined: maps to Unknown\n"
            "beta: gone(y).unwrap_or_default()  # a note on an entry that disappears\n"
            "gamma: bare(z).unwrap_or_default()\n",
            encoding="utf-8",
            newline="",
        )
        saved = globals()["BASELINE"]
        globals()["BASELINE"] = fake
        try:
            notes = read_notes()
        finally:
            globals()["BASELINE"] = saved

    expect("a note is read back off the previous baseline",
           notes.get("alpha: keeps(x).unwrap_or_default()"),
           "  # examined: maps to Unknown")
    expect("an entry with no note has none",
           "gamma: bare(z).unwrap_or_default()" in notes, False)
    expect("the header is not an entry", any(k.startswith("#") for k in notes), False)

    # The writer's half: a surviving entry keeps its note, a new one has none,
    # and the note of an entry that is gone goes with it.
    found_fx = ["alpha: keeps(x).unwrap_or_default()", "delta: new(w).unwrap_or_default()"]
    rendered = "".join(f"{f}{notes.get(f, '')}\n" for f in found_fx)
    expect("the surviving entry keeps its note",
           "alpha: keeps(x).unwrap_or_default()  # examined: maps to Unknown" in rendered,
           True)
    expect("a new entry is written bare",
           "delta: new(w).unwrap_or_default()\n" in rendered, True)
    expect("a vanished entry takes its note with it", "beta" in rendered, False)

    # The floors, both directions. A floor never observed to refuse is a claim
    # with no evidence behind it.
    expect("a full scan is not too thin",
           scan_is_too_thin(Scan([], MIN_FILES, MIN_READS)), False)
    expect("a scan that read almost no files is refused",
           scan_is_too_thin(Scan([], MIN_FILES - 1, MIN_READS)), True)
    expect("a scan whose files survived but whose CONTENT was blanked "
           "is refused too",
           scan_is_too_thin(Scan([], MIN_FILES, MIN_READS - 1)), True)

    print(f"check-read-defaults: self-test "
          f"{'FAILED' if failures else 'passed'} ({failures} failure(s))")
    return 1 if failures else 0


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--check", action="store_true",
                    help="exit 1 if a site appeared that is not pinned")
    ap.add_argument("--update-baseline", action="store_true", dest="update")
    ap.add_argument("--self-test", "--selftest", dest="self_test",
                    action="store_true", help="run this script's own fixtures")
    ap.add_argument("--head", metavar="REV",
                    help="judge this revision rather than the working tree")
    args = ap.parse_args()

    if args.self_test:
        return _self_test()

    try:
        tree = gittree.open_tree(str(ROOT), args.head)
    except gittree.GitTreeError as exc:
        # Exit 2, not 1: `run-checker.sh` reads 1 as "the checker found
        # something" and would print a refusal over this. A revision that
        # cannot be opened is not a finding against anyone's code.
        print(f"check-read-defaults: cannot read {args.head!r}: {exc}",
              file=sys.stderr)
        return 2

    with tree:
        scan = survey(tree)
        pinned = read_baseline(tree)
    found = scan.found
    notes = read_notes()

    # Before any verdict: did we actually look? A floor breach is not a finding
    # against anyone's code, it is "no verdict reached" -- so it exits 2 and
    # `run_checker` aborts the build in the same words as a crashed checker,
    # which is right because it is the same event. That is lane A's §4 in
    # `requests/a-b-wiring-check-selftest-reinit-and-a-correction-it-runs-nowhere.md`,
    # and I agree with it.
    if scan_is_too_thin(scan):
        for line in [
            "check-read-defaults: REFUSING TO ANSWER -- the scan is too thin "
            "to mean anything.",
            f"  .rs files read  {scan.files:>5}  (floor {MIN_FILES})",
            f"  read_to_string  {scan.reads:>5}  (floor {MIN_READS})",
            "",
            "This is not a clean tree. It is a scan that stopped seeing.",
            "A file count that collapsed means the walk broke. A read count "
            "that collapsed",
            "while the file count held means `strip_noise` blanked the code it "
            "was meant to",
            "preserve -- the failure lane A hit the same day, which took their "
            "absent-operand",
            "count from 37 to 0 with the gate still green.",
            "",
            "Either way every pinned site would report as no longer present, "
            "so a total",
            "failure to look would arrive spelled as the best possible news.",
        ]:
            print(line, file=sys.stderr)
        return 2

    if args.update:
        BASELINE.write_text(
            HEADER + "".join(f"{f}{notes.get(f, '')}\n" for f in found),
            encoding="utf-8",
            # newline="" so Python does not translate to CRLF on Windows, which
            # would leave the file dirty against the repo's `eol=lf` attribute.
            newline="",
        )
        print(f"wrote {BASELINE.relative_to(ROOT)} with {len(found)} entries")
        return 0

    if not args.check:
        print(f"inspected {scan.files} .rs file(s) under userspace/, "
              f"{scan.reads} live read_to_string call(s)")
        print(f"{len(found)} read_to_string(..).unwrap_or_default() site(s):\n")
        for f in found:
            print(f"  {f}")
        return 0

    if pinned is None:
        print(f"no baseline at {BASELINE.relative_to(ROOT)}; run --update-baseline",
              file=sys.stderr)
        return 2

    current = set(found)
    new = sorted(current - pinned)
    gone = sorted(pinned - current)

    if new:
        print(
            f"\n{len(new)} NEW discarded failure(s) -- `.unwrap_or_default()` on a\n"
            "call that returns Option or Result to say it could not find out:\n"
            "\n"
            "The default is then indistinguishable from a real answer. An empty\n"
            "string reads as an empty file, a zeroed struct reads as an idle\n"
            "machine, and a missing username reads as nobody. None of those is\n"
            "what happened; what happened is that the question went unanswered.\n",
            file=sys.stderr,
        )
        for f in new:
            print(f"  {f}", file=sys.stderr)
        print(
            "\nFor a FILE read, `optionalfile::read_or_empty` separates a file\n"
            "that is absent from one that could not be read. See its module docs.\n"
            "\n"
            "For anything else -- a local `fn x() -> Option<T>`, an ioctl, an\n"
            "environment lookup -- there is no helper and there should not be:\n"
            "keep the Option and let the caller print `?`, skip the row, or\n"
            "refuse. `userspace/iostat` prints six question marks where it used\n"
            "to print six zeroes, which is the whole of the fix.",
            file=sys.stderr,
        )
        sys.stdout.flush()
        return 1

    # A pin that matches nothing is not harmless bookkeeping. The key is
    # `<crate>: <call text>`, so an exemption left behind after its site was
    # fixed is INHERITED by the next identical call written in that crate: the
    # new defect arrives pre-forgiven and this gate stays green. Lane A named
    # the same hazard in their absent-operand ledger on the same day -- "a
    # blessing matching no site fails too, or a fixed site stays exempt and the
    # next real one inherits the exemption".
    #
    # Refusing here is safe in a way it would not be for a shared checker.
    # `userspace/**` is lane B's tree, so a stale pin can only ever be produced
    # by the lane that owns the baseline; no other lane's push can be reddened
    # by it, and the remedy is one command inside the commit that caused it.
    if gone:
        print("", file=sys.stderr)
        print(f"{len(gone)} pinned site(s) NO LONGER EXIST:", file=sys.stderr)
        print("", file=sys.stderr)
        for f in gone:
            print(f"  {f}", file=sys.stderr)
        for line in [
            "",
            "If you fixed them, run --update-baseline in the SAME commit. An "
            "exemption that",
            "outlives its site forgives the next identical call written in "
            "that crate.",
            "",
            "If you did NOT fix them, the scan stopped finding them and the "
            "floors above were",
            "too generous to notice. That is the more urgent reading of these "
            "lines.",
        ]:
            print(line, file=sys.stderr)
        sys.stdout.flush()
        return 1

    print(f"ok -- {len(current)} pinned site(s), none new; inspected "
          f"{scan.files} file(s), {scan.reads} live read_to_string")
    return 0


if __name__ == "__main__":
    sys.exit(main())
