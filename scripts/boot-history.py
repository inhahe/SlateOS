#!/usr/bin/env python3
r"""Record the outcome of every boot test, and count the streaks that
`known-issues.md` asks about but nothing counts.

Why this exists
---------------

Four of lane A's open kernel issues are intermittent hangs whose closure
condition is a *count* -- and nothing counts. W1's own status line has read

    clean streak **7** (after the 2026-06-14 soak)

since 2026-06-14, while many dozens of boots have passed. The number is not
wrong because someone was careless; it is wrong because keeping it right by
hand requires editing a markdown file after every boot, which nobody will do
and nobody did. The entry even says so itself: *"the recorded streak of 7 is
stale bookkeeping, not a real count."*

This is exactly the argument `bench-history.py` makes for itself:

    boot-test.sh already tells the reader to "compare against prior runs
    rather than treating this as a hard regression" -- but nothing stored
    the prior runs, so that advice was unfollowable. This script stores them.

Same shape, different axis: `bench-history.py` stores *numbers*, this stores
*outcomes*. `bench/history.jsonl` only gains a record on a `--bench` run that
reached its marker, so it is structurally blind to the runs this file is about.

Three properties that are the whole point
-----------------------------------------

1. **The verdict is derived in one place, from `(exit code, serial file)`.**
   `boot-test.sh` has ~12 exit sites. A recorder called at each of them would
   be wrong the first time someone adds a thirteenth -- and wrong in the
   direction that matters, because the site nobody wired up is a *failure*
   site, so the omission reads as a clean streak. There is one call, in the
   EXIT trap, and it classifies from evidence rather than from where it was
   called.

2. **A failing boot's serial tail is stored.** `build/` is gitignored
   per-worktree scratch and the next run overwrites `serial-test.txt`, so
   today the evidence for a hang survives only if a human pasted it into
   markdown before the next boot. That loss already bit an investigation once
   (`B-FORKEXEC-BOOT-HANG`, cited in boot-test.sh's own comment). Failures
   carry their tail into the record; passes do not, since a passing tail is
   the same 25 lines every time.

3. **An unvalidated fingerprint reports as unvalidated, never as a streak.**
   A matcher that can never fire produces a perfect clean streak, and a
   perfect clean streak is exactly what closes an issue. So every fingerprint
   declares `validated_by`: the occurrences it is known to match. One with an
   empty list prints a warning in place of its streak, because "we have not
   seen this in 90 boots" and "we could not have seen this in 90 boots" are
   indistinguishable from the number alone: *could not verify* must never
   render as *fine*.

   The converse failure is worth naming next to it, because the tree has now
   hit both. `scripts/stamp-ancestry.py` followed exactly this rule and was
   retired anyway (design-decisions.md §277): once the artifacts it watched
   stopped being tracked, "could not verify" became its *only* answer, and a
   warning that fires on every run is read as noise rather than as doubt. So
   the rule holds, with a rider — a check whose could-not-verify branch has
   become unconditional is no longer expressing doubt about the run, it is
   expressing a fact about itself, and belongs in a fixture, not in a banner.

Usage
-----

    python scripts/boot-history.py --exit-code N [--serial PATH] [--label L]
    python scripts/boot-history.py --list          # recent records
    python scripts/boot-history.py --streaks       # per-fingerprint standing
    python scripts/boot-history.py --classify      # verdict only, record nothing

Exit status is about *the recorder*, never about the boot: 0 recorded (or
nothing to record), 1 could not record. `boot-test.sh` keeps its own exit code
regardless -- a broken recorder must not turn a green boot red.
"""

from __future__ import annotations

import argparse
import json
import os
import platform
import re
import socket
import subprocess
import sys
from dataclasses import dataclass, field
from typing import Callable

SCRIPT_DIR = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, SCRIPT_DIR)
import srcload  # noqa: E402

REPO_ROOT = os.path.dirname(SCRIPT_DIR)
DEFAULT_SERIAL = os.path.join(REPO_ROOT, "build", "serial-test.txt")
DEFAULT_HISTORY = os.path.join(REPO_ROOT, "bench", "boot-history.jsonl")
BENCH_HISTORY = os.path.join(SCRIPT_DIR, "bench-history.py")

_BENCH_HISTORY_MODULE = None


def bench_history():
    """Import `bench-history.py` by path; its name is not an identifier.

    Cached because the module compiles several dozen regexes at import and the
    tests parse many logs, while a real run parses one.

    Loaded from source rather than through `importlib`: a `SourceFileLoader`
    consults `__pycache__`, whose staleness check is `(mtime, size)` at
    one-second resolution, so two same-size writes to the sibling inside
    one second leave the second one invisible and this script silently
    runs the previous version of it. See `scripts/srcload.py`.
    """
    global _BENCH_HISTORY_MODULE
    if _BENCH_HISTORY_MODULE is None:
        _BENCH_HISTORY_MODULE = srcload.load(BENCH_HISTORY, "bench_history")
    return _BENCH_HISTORY_MODULE

#: How much of a failing boot's serial log to keep, and how wide.
#:
#: 40 lines is `boot-test.sh`'s own printed tail (25) with room for the lines
#: that precede a freeze; 300 chars is past the longest self-test line we emit
#: and still bounds a record whose last line might be a runaway print. The file
#: is committed, so an unbounded tail would be an unbounded diff.
TAIL_LINES = 40
TAIL_WIDTH = 300


# --------------------------------------------------------------------------
# Serial-log evidence
# --------------------------------------------------------------------------


@dataclass(frozen=True)
class Serial:
    """Everything the classifiers and fingerprints are allowed to look at.

    Parsed once, so that two fingerprints cannot disagree about whether the
    log ended mid-line.
    """

    path: str
    text: str
    lines: tuple[str, ...]
    n_bytes: int
    ends_mid_line: bool
    boot_ok: bool
    marker_ok: bool
    marker: str
    #: `EXCEPTION:` lines a *healthy* boot is not supposed to contain. The
    #: deliberate ones raised by the ring-3 self-tests are filtered out into
    #: `benign_exceptions` -- see `_BENIGN_EXCEPTION_RE`.
    exceptions: tuple[str, ...]
    benign_exceptions: tuple[str, ...]
    has_panic: bool
    #: Which sanitizer the kernel was built with, as the kernel itself reported
    #: it: `"kasan-instrumented"`, `"none"`, or `None` when the log carries no
    #: banner at all.
    #:
    #: The three-way split is the point, and `None` must never be folded into
    #: `"none"`. Every boot recorded before 2026-08-19 predates the banner, and
    #: a good number of those *were* instrumented; treating a missing line as
    #: "not instrumented" would mislabel them all in the direction that makes
    #: the two populations look like one. `None` means "this log cannot say",
    #: which is a thing a consumer can decline to average.
    sanitizer: str | None = None
    #: Which accelerator the kernel ran under, as the kernel itself reported it:
    #: `"QEMU TCG"`, `"Hyper-V/WHPX"`, `"bare metal"`, or `None` when the log
    #: carries no `[hypervisor]` banner at all.
    #:
    #: Here for exactly the reason `sanitizer` is, one variable over. A boot's
    #: wall time is a property of the *pair* (build, accelerator), and this file
    #: already knows it: `wall_populations`' docstring records two WHPX boots at
    #: 168 s and 186 s against a TCG median of ~120 s for the same profile. It
    #: keeps them out today by skipping `experiment` rows -- which works only
    #: because every WHPX boot so far happened to be a tagged probe. That is a
    #: property of how those runs were invoked, not one this file guarantees,
    #: and Q53 is a live proposal to make WHPX the ordinary way to boot. The
    #: first untagged WHPX boot would move the median by ~40% with nothing to
    #: say why.
    #:
    #: Three-valued for the same reason and with the same force: `None` means
    #: "this log cannot say", never "TCG". `bench-history.py`'s `ACCEL_RE` notes
    #: that the conflation is provably wrong -- the first WHPX run on this host
    #: predates the banner -- and the same records are described here.
    accel: str | None = None
    #: The names of the self-test sections that announced `SKIP:` on this boot,
    #: deduplicated and sorted -- `"[mm] Zeroed frame allocation"` and so on.
    #:
    #: Names only, without the parenthesised reason. The reason is a
    #: `&'static str` in the kernel (`fs::selftest::Skips::record`), so it is a
    #: property of the *code* and never of the run: carrying it here would add
    #: bytes to a committed file on every boot and could still only ever
    #: disagree with itself across a code change, which is precisely when a
    #: consumer comparing runs wants the two rows to still line up by name.
    #:
    #: Empty tuple means "this boot skipped nothing", which is a real and useful
    #: answer; the *absent* field on a row means "recorded before this field
    #: existed" -- the same three-way distinction `sanitizer` and `accel` keep
    #: above, and for the same reason. `check-boot-skips.py` counts only rows
    #: that carry the key, so a pre-field row cannot be mistaken for a boot on
    #: which some skip did not fire.
    #:
    #: A section that skipped at one call site and *ran* at another is not here
    #: -- it is in `skips_covered`. See `partition_skips` for why that split is
    #: the difference between a gate and a permanent false positive.
    skips: tuple[str, ...] = ()
    #: Sections that announced SKIP somewhere in this boot and also ran
    #: somewhere else in it: `ipc::io_ring`'s two file-handle cases, which skip
    #: before `/tmp` is mounted and pass after it, and `[mm] Zero-on-free`.
    #:
    #: Recorded rather than discarded because those pre-mount calls are
    #: deliberate tripwires whose whole purpose is to be followed by an OK
    #: later. Keeping them in a field of their own means the day one stops
    #: being followed by an OK, it moves into `skips` and the gate starts
    #: counting it -- which is the alarm the tripwire's own source comment
    #: wishes for and could not have.
    skips_covered: tuple[str, ...] = ()

    @property
    def last_line(self) -> str:
        return self.lines[-1] if self.lines else ""

    def tail(self, n: int = TAIL_LINES) -> list[str]:
        return [ln[:TAIL_WIDTH] for ln in self.lines[-n:]]


#: Anchored, for the reason boot-test.sh spells out at its own grep: the
#: livelock diagnostic prints "...still armed 200s after arming (no BOOT_OK)",
#: which contains the substring BOOT_OK. An unanchored match calls a hung boot
#: a pass -- the single most expensive false answer this script could give.
_BOOT_OK_RE = re.compile(r"^BOOT_OK", re.MULTILINE)
_EXCEPTION_RE = re.compile(r"^EXCEPTION:.*$", re.MULTILINE)

#: Unanchored, for parity with boot-test.sh's post-loop net -- which is itself
#: deliberately wider than its in-loop `kernel_is_dead` check, on the reasoning
#: that by that point the boot has already failed to reach the marker, "so a
#: loose match cannot turn a healthy boot into a failure". This regex is only
#: ever consulted on a run with no marker, which is the same guarantee.
_PANIC_RE = re.compile(r"PANIC|FATAL")

#: An `EXCEPTION:` line a healthy boot is *supposed* to print.
#:
#: Found the hard way, against a live serial log: every green boot contains
#:
#:     EXCEPTION: Invalid Opcode (#UD) at 0x4000000011 in userspace
#:                (deliberate compiler trap)
#:
#: from a ring-3 self-test. Treating it as a fault would have been quietly
#: catastrophic in both directions at once: every non-panic failure would
#: classify as PANIC, *and* the W1 fingerprint -- which requires no exception
#: anywhere -- could never match again. That is the failure mode this whole
#: script exists to prevent, arriving through the front door.
#:
#: Note this is not merely a suppression list. The kernel's real fault reports
#: name the ring in the following `Cause:` line (`... kernel`), and the ones
#: below are exactly those the self-tests announce as intentional.
_BENIGN_EXCEPTION_RE = re.compile(
    r"in userspace|deliberate|intentional|expected|self-test", re.IGNORECASE)

#: Vectors whose handler prints and *returns*, so the line can never be evidence
#: that the kernel died.
#:
#: This is a second, independent guard, and the first one having failed is why
#: it exists. `_BENIGN_EXCEPTION_RE` relies on the kernel annotating deliberate
#: faults, and on 2026-08-19 three that it did not annotate --
#:
#:     [idt] Running direction-flag self-test...
#:     EXCEPTION: Breakpoint (#BP) at 0xffffffff813b56b6
#:     [idt]   DF is clear on exception entry: OK
#:
#: -- turned a boot that merely ran out of clock into a `PANIC` verdict. Note
#: what made that bug survive: `classify()` consults the exception list only on
#: a run with no marker, so the mislabelling is invisible on every green boot
#: and fires exactly on the failed one whose verdict someone needs.
#:
#: Annotating those lines (kernel/src/idt.rs, `ExpectedBreakpoint`) fixes the
#: instance. This fixes the class: `#BP`'s handler is documented "Logged but
#: non-fatal" and structurally returns, so *whatever* raised it, the kernel was
#: still running afterwards. A stray breakpoint is still worth knowing about --
#: it stays in `benign_exceptions` and is still printed -- it just cannot on its
#: own mean "kernel died".
_NONFATAL_VECTOR_RE = re.compile(r"\(#BP\)")

#: The kernel's build-profile banner (kernel/src/main.rs, printed immediately
#: after "=== Kernel booting ===").
#:
#: Matched loosely on the `sanitizer=` key rather than on the whole line, so
#: that adding a second key to the banner later (`opt=`, `lto=`, …) does not
#: silently stop this from matching — a parser that stops matching produces the
#: same `None` as a kernel that never printed, and those must stay
#: distinguishable.
_SANITIZER_RE = re.compile(r"^\[boot\] build profile:.*\bsanitizer=(\S+)",
                           re.MULTILINE)

#: A self-test section announcing that it did not run.
#:
#: The shape is fixed by `fs::selftest::Skips::report`, which prints
#: `"{tag}   SKIP: {section} ({why})"`, and by the hand-rolled skips that
#: predate that type and follow the same shape. Both the tag and the section
#: are captured; the reason is left to `_skip_name` to strip, because it is not
#: a fixed-width field and cannot be split off by this regex -- see there.
#:
#: `SKIPPED` is matched as well as `SKIP:`, with the colon optional, because
#: the kernel says both: `[backtrace] Self-test SKIPPED (no frame pointers)`
#: is the same event as `[mm]   SKIP: Zero-on-free (...)` and would otherwise
#: be invisible to the gate that exists to notice a skip that never stops
#: firing. The two spellings are normalised to one name here rather than in the
#: kernel: renaming ~40 call sites to satisfy a parser is the tail wagging the
#: dog, and the parser is where a reader looks when a name looks wrong anyway.
#: Every quantifier here is `[ \t]` or `[^\n]`, never `\s` or `.`, and that is
#: not style. `\s` matches a newline, so `^(\[tag\])\s*(.*?)SKIP` happily pairs
#: a tag on one line with a `SKIP` five hundred lines later and names the skip
#: after whatever text lay between. The first draft did exactly that and
#: produced `'[mm] Frame allocator self-test PASSED - 2 section(s) [mm] Kernel
#: heap allocator initialized'` as a section name -- wrong, and wrong in the
#: expensive direction, because a name assembled from run-specific text is
#: never equal to itself on the next boot and so the 100%-of-N test can never
#: fire.
_SKIP_RE = re.compile(
    r"^(\[[^\]\n]+\])[ \t]*([^\n]*?)\bSKIP(?:PED)?:?[ \t]*([^\n]*)$",
    re.MULTILINE)

#: Punctuation a section name may end in once its reason is stripped: the
#: kernel writes `name: SKIP (why)`, `Self-test SKIPPED (why)` and
#: `... -- SKIP (why)` interchangeably.
_SKIP_NAME_TRAILING = " \t:-\u2014\u2013."

#: Lines that contain the word SKIPPED but do not *name* a skipped section.
#:
#: Two shapes, both from `fs::selftest::Skips`, and both actively harmful to
#: let through:
#:
#:   * `[mm] Frame allocator self-test PASSED - 2 section(s) SKIPPED` is the
#:     closing *summary* (`SkipSuffix`). It is a count, so the name derived
#:     from it carries the count, so it differs between two boots that skipped
#:     different numbers of sections -- and a name that changes is a name the
#:     100%-of-N test can never accumulate evidence against. Worse, it names
#:     the *suite*, which would double-count every suite that already
#:     contributed its real skips a few lines above.
#:   * `[tag]   SKIP: 3 further section(s) (ledger holds 8)` is the overflow
#:     line: the ledger filled and the names were lost. Also a count, and
#:     excluded for the same reason -- but note it is not a nothing. If this
#:     line ever appears, `MAX_SKIPS` is too small and some skips are invisible
#:     to this gate; the line stays in the log where a reader will see it.
_SKIP_SUMMARY_RE = re.compile(
    r"\d+[ \t]*(?:further[ \t]*)?section\(s\)", re.IGNORECASE)

#: Shortest section key [`partition_skips`] will accept as evidence a section
#: ran. Below this the truncated key is dropped and the full section text is
#: required instead.
#:
#: The truncation exists because the kernel spells a section's parentheses
#: differently between its SKIP line and its result line, so only the text
#: before the first `(` is reliably common to both. That is safe for
#: `Positioned I/O (pread/pwrite)` and useless for a hypothetical
#: `RX (queue 0)`, whose key `RX` occurs in half of a NIC driver's output and
#: would mark the section as having run against an unrelated line.
#:
#: Eight characters is not a tuned number; it is "long enough that a match is
#: unlikely to be an accident, short enough to keep every real section name in
#: the current log". The shortest live one is `Zero-on-free` at 12. Erring high
#: costs nothing but strictness, and strictness here errs toward `uncovered`,
#: which is the recoverable direction -- see [`partition_skips`].
_MIN_COVER_KEY = 8


def _strip_trailing_paren(text: str) -> str:
    """Drop one balanced parenthesised group from the end of `text`.

    Not `text.split(" (")[0]`, and not a non-greedy regex: skip reasons nest
    their own parentheses. The live log carries

        [mm]   SKIP: Zeroed frame allocation (HHDM is not mapped yet (running
        before page_table::init))

    on which splitting at the first `" ("` keeps `Zeroed frame allocation` by
    luck, while splitting at the last one yields the nonsense name
    `Zeroed frame allocation (HHDM is not mapped yet`. Scanning inward from the
    close paren is the only rule that is right on both.

    Unbalanced input is returned unchanged rather than half-trimmed: a name
    that keeps a stray `(` is obviously wrong to a reader, whereas a silently
    truncated one looks like a different section and would split one skip's
    history into two.
    """
    text = text.rstrip()
    if not text.endswith(")"):
        return text
    depth = 0
    for i in range(len(text) - 1, -1, -1):
        if text[i] == ")":
            depth += 1
        elif text[i] == "(":
            depth -= 1
            if depth == 0:
                return text[:i].rstrip()
    return text


def _skip_name(tag: str, before: str, after: str) -> str:
    """One stable name for a skipped section, from a `_SKIP_RE` match.

    `before` is what stood between the tag and the word SKIP (empty for the
    `Skips::report` shape, `"Self-test "` for `[backtrace] Self-test SKIPPED`);
    `after` is the rest of the line. Exactly one of the two carries the section
    name, so they are concatenated and the reason trimmed off the end.

    The name is deliberately coarse -- tag plus section, no reason, no counts.
    Its whole job is to be the same string on two boots a week apart so that
    "this skip has fired on every one of the last N boots" is answerable at
    all; a name that carries anything run-specific answers "no two boots agree"
    instead, which is the same as having no gate.
    """
    section = _strip_trailing_paren(f"{before.strip()} {after.strip()}".strip())
    # Collapse internal runs of whitespace: the kernel's own indentation varies
    # between suites and must not fork one skip into two names.
    section = " ".join(section.split()).strip(_SKIP_NAME_TRAILING)
    return f"{tag} {section}".rstrip()


def parse_skips(text: str) -> tuple[str, ...]:
    """Every distinct self-test skip announced in `text`, sorted.

    Deduplicated because a suite that runs twice in a boot (the ACPI self-test
    runs from either arm of an `if let`) would otherwise contribute the same
    name twice and make one boot look like two pieces of evidence.

    Not used by `read_serial`, which wants the coverage split from
    [`partition_skips`]. This is the *naming* half on its own, and it is kept
    because `test-check-boot-skips.py` tests naming through it: a bug in how a
    section is named then fails as a wrong name rather than surfacing three
    functions away as a section that mysteriously stopped being covered.
    Deleting it would not remove code, it would remove the ability to tell those
    two failures apart.
    """
    return tuple(sorted(n for n, _, _ in _skip_triples(text)))


def _skip_triples(text: str) -> set[tuple[str, str, str]]:
    """`(name, tag, section)` for every skip announcement, deduplicated.

    The tag and section are kept alongside the name because
    [`partition_skips`] has to search the log for the *same* section reported
    by some other line, and the joined name is not the shape that appears
    there: the kernel prints `[io_ring]   SKIP: File handle read/write (...)`
    in one place and `[io_ring]   File handle read/write (1 entry): OK` in
    another, so the two halves have to be matched separately.
    """
    out = set()
    for m in _SKIP_RE.finditer(text):
        if _SKIP_SUMMARY_RE.search(m.group(0)):
            continue
        tag, before, after = m.groups()
        name = _skip_name(tag, before, after)
        if not name:
            continue
        out.add((name, tag, name[len(tag):].strip()))
    return out


def partition_skips(text: str) -> tuple[tuple[str, ...], tuple[str, ...]]:
    """Split this boot's skips into the ones that mean something and the rest.

    Returns `(uncovered, covered)`:

      * **uncovered** -- the section announced SKIP and no other line in the
        log reports that same section under the same tag. On this boot, it did
        not run. This is the set the never-running-self-test gate counts.
      * **covered** -- the section announced SKIP *here* and ran *there*. A
        second call site picked it up later in the boot.

    Why the split has to exist, and why it was not obvious
    -----------------------------------------------------

    The first version of this field recorded every SKIP line, and on the first
    real log that made three of its four findings wrong. `ipc::io_ring` runs
    its two file-handle cases from `self_test()` before `/tmp` is mounted --
    where they skip -- and again from `self_test_fh()` after the mount, where
    they pass. The pre-mount pair is not dead code; it is a **deliberate
    tripwire**, and the source says so:

        The two file-handle sections cannot run here [...] They are still
        reported, because "expected" and "invisible" are different things:
        `self_test_fh` below re-runs them once /tmp is mounted, and if *that*
        call ever stopped happening the only evidence would be these two lines
        never being followed by an OK.

    A gate that flagged those would have been a permanent false positive, and
    the "fix" its message invites -- delete the pre-mount call -- would have
    destroyed the tripwire. `[mm] Zero-on-free` is a third instance of the same
    shape. Only `[mm] Zeroed frame allocation` was a genuine never-run, which
    is one finding out of four.

    So this does not merely avoid the false positives: it turns the tripwire's
    own stated wish into an alarm. "The only evidence would be these two lines
    never being followed by an OK" is exactly the condition computed here, and
    the day `self_test_fh` stops being called, the pair moves from `covered` to
    `uncovered` and the gate starts counting it.

    The matcher, and which way it is allowed to be wrong
    ----------------------------------------------------

    A section counts as having run if some line carries the same tag, contains
    the section's key text, and does not mention skipping in any spelling.
    Three rules, each earned against the live log:

      * **Match on the section text up to its first `(`**, not the whole
        section. The kernel spells a section's own parenthesis differently
        between the skip and the result -- `SKIP: Positioned I/O
        (pread/pwrite)` against `Positioned I/O (pread/pwrite preserve the
        cursor): OK` -- so the full string matches nothing and the case reads
        as never run when it demonstrably ran two lines from its sibling. The
        key must still be substantial (>= `_MIN_COVER_KEY` characters) or the
        full section is used, so a section named `RX` cannot match half the
        subsystem's output.
      * **Reject any line mentioning skip**, not just the `SKIP:`/`SKIPPED`
        shapes this file parses. Two suites narrate the skip in prose on the
        line *above* the machine-readable one -- `[hotplug]   Single-CPU:
        skipping offline/online cycle` -- and a naive "not a SKIP line" test
        reads that as evidence the section ran, which is the exact inversion of
        what it says.
      * **Anything else counts, including `FAIL:`.** The question is whether
        the case executed, not whether it passed; a failure reddens the boot
        through its own machinery, which is not this field's job.

    Where it errs, it errs toward `uncovered`, deliberately. A wrong
    `uncovered` eventually becomes a visible accusation that a human resolves
    with an allowlist entry stating a reason. A wrong `covered` is a section
    quietly excused from the gate forever, with nothing to see. Between a
    recoverable error and an invisible one, take the recoverable.
    """
    triples = _skip_triples(text)
    if not triples:
        return (), ()

    # One pass over the log, bucketed by tag, so nine skips do not each walk
    # 47k lines. Only tags that actually skipped are collected.
    tags = {tag for _, tag, _ in triples}
    other: dict[str, list[str]] = {tag: [] for tag in tags}
    for line in text.split("\n"):
        if "skip" in line.lower():
            continue
        for tag in tags:
            if line.startswith(tag):
                other[tag].append(line)

    uncovered, covered = [], []
    for name, tag, section in triples:
        key = section.split("(")[0].strip()
        if len(key) < _MIN_COVER_KEY:
            key = section
        ran = bool(key) and any(key in line for line in other.get(tag, ()))
        (covered if ran else uncovered).append(name)
    return tuple(sorted(uncovered)), tuple(sorted(covered))


def _can_be_fatal(exc: str) -> bool:
    """Could this `EXCEPTION:` line be evidence that the kernel died?

    Two independent reasons it could not: the kernel said the fault was on
    purpose, or the vector's handler returns and so the kernel outlived it
    either way. Only lines that clear both become `Serial.exceptions`; the rest
    are still recorded (as `benign_exceptions`) and still printed, because "the
    kernel survived it" is not the same claim as "nobody needs to see it".
    """
    if _BENIGN_EXCEPTION_RE.search(exc):
        return False
    if _NONFATAL_VECTOR_RE.search(exc):
        return False
    return True


def read_serial(path: str, marker: str = "BOOT_OK") -> Serial | None:
    """Parse the serial log, or None if it does not exist / is empty.

    Read as bytes and decoded with `errors="replace"`: a wedged UART can leave
    a partial multi-byte sequence at the cut point, and a decode exception here
    would lose the entire record for the one run we most want recorded.
    """
    try:
        with open(path, "rb") as handle:
            raw = handle.read()
    except FileNotFoundError:
        return None
    except OSError as exc:
        print(f"boot-history: cannot read {path}: {exc}", file=sys.stderr)
        return None

    if not raw.strip():
        return None

    text = raw.decode("utf-8", errors="replace").replace("\r\n", "\n")
    # `ends_mid_line` is the discriminator W1's analysis turns on: the UART
    # write is synchronous at ~87us/char, so a CPU that wedged for an unrelated
    # reason wedges *between* lines with the in-flight line already flushed.
    # A cut inside a line means the printing CPU itself stopped mid-write.
    ends_mid_line = not text.endswith("\n")
    lines = tuple(ln.rstrip("\n") for ln in text.split("\n") if ln != "")

    marker_re = re.compile("^" + re.escape(marker), re.MULTILINE)
    all_exc = tuple(_EXCEPTION_RE.findall(text))
    benign = tuple(e for e in all_exc if not _can_be_fatal(e))
    fatal = tuple(e for e in all_exc if _can_be_fatal(e))
    san_match = _SANITIZER_RE.search(text)
    skips, skips_covered = partition_skips(text)
    return Serial(
        path=path,
        text=text,
        lines=lines,
        n_bytes=len(raw),
        ends_mid_line=ends_mid_line,
        boot_ok=bool(_BOOT_OK_RE.search(text)),
        marker_ok=bool(marker_re.search(text)),
        marker=marker,
        exceptions=fatal,
        benign_exceptions=benign,
        has_panic=bool(_PANIC_RE.search(text)),
        sanitizer=san_match.group(1) if san_match else None,
        accel=_parse_accel(path),
        skips=skips,
        skips_covered=skips_covered,
    )


def _parse_accel(path: str) -> str | None:
    """Which accelerator this boot ran under, delegated not reimplemented.

    `bench-history.py` owns the two `[hypervisor]` banner patterns and the
    reasoning about why there have to be two of them (the kernel prints a
    different sentence on bare metal, and a single pattern would render that
    platform as "cannot say"). A second copy here would be a restatement of a
    selector, which is the drift `design-decisions.md` sec 240 exists to forbid
    -- and it would drift *silently*, because a pattern that stopped matching
    returns the same `None` a pre-banner log does.

    Failure to load that module is caught rather than raised, and this is the
    one place in this file where swallowing an error is right. `boot-test.sh`
    calls this script from its EXIT trap with `|| true`, so an exception here
    does not surface -- it silently loses the record of the boot, which for a
    *failing* boot is the most expensive outcome this script has. Losing the
    accelerator label is cheap; losing the boot is not.

    The answer on failure is `None` -- "this row cannot say" -- which is the
    truth, and is a value every consumer here already declines to average.
    It does not distinguish "the kernel did not print a banner" from "the
    recorder could not read one", and deliberately no sentinel is invented for
    that: the warning above names the difference where a human will see it, and
    a `bench-history.py` too broken to import fails loudly within seconds
    anyway, since `boot-test.sh` runs it directly on the same run.
    """
    try:
        return bench_history().parse_accel(path)
    except Exception as exc:                       # noqa: BLE001 - see above
        print(f"boot-history: cannot read the accelerator banner: {exc}",
              file=sys.stderr)
        return None


# --------------------------------------------------------------------------
# Verdict
# --------------------------------------------------------------------------

#: Verdicts that mean the kernel got where it was going. Only these extend a
#: clean streak; everything else is a recurrence candidate.
CLEAN_VERDICTS = frozenset({"PASS", "PASS_TOOLING", "BENCH_INCOMPLETE"})


def is_experiment(rec: dict) -> bool:
    """Whether this row is a deliberate probe rather than a boot of the tree.

    A probe runs the kernel under conditions no checkout reproduces -- foreign
    emulator flags, a hand-patched binary -- so its outcome is evidence about
    the probe, not about the tree. It is recorded (never discarded: the reason
    a thing was tried and what happened is exactly what stops it being tried
    again) but it is kept out of every statistic that describes the tree's
    health.

    **Absent means "not an experiment", deliberately, even though absent is also
    what every row written before this field looked like.** That is the opposite
    of the rule `bench-history.py` applies to `accel` and `text_pad`, where
    absent must never be folded into a known value -- and the difference is the
    direction each error fails in. There, folding absent into a value *widens* a
    band, and a wider band dismisses real regressions silently. Here, treating
    an old probe as a normal boot can only *shorten* a clean streak or *add* a
    failure to the counts, which shows up as a boot someone goes and looks at.
    Under-counting failures would be the dangerous direction, and this cannot
    do it. So the ambiguity is resolved toward the side that fails loudly.
    """
    return bool(rec.get("experiment"))


def describes_tree(rec: dict) -> bool:
    """Whether this row is evidence about the tree's health.

    Two different things disqualify a row, and they are kept as two predicates
    on purpose. `is_experiment` means "invoked deliberately under non-default
    conditions"; a host failure means "invoked normally, and the machine
    underneath fell over". Widening `is_experiment` to cover the second would
    have been one line fewer and is the mistake `known-issues.md` names when it
    specifies this verdict: a flag that means two things can be satisfied by
    whichever of them nobody was thinking about, and a real regression would
    then be excused by a predicate that was never about it.

    Every statistic in this file that claims to describe the tree filters on
    *this* function, so the streak, the counts and the medians cannot come to
    different views of what a boot is.
    """
    return not is_experiment(rec) and rec.get("verdict") != "HOST_FAIL"


#: Substrings that prove the *host*, not the kernel, is what failed -- each one
#: emitted by QEMU or by Cygwin, below the guest, where nothing in the tree can
#: reach it.
#:
#: That last property is the whole design. Detection reads QEMU's stderr rather
#: than the serial log precisely so that a kernel which printed
#: `Failed to CreateFileMapping` cannot excuse itself; matching these words
#: anywhere the guest can write would hand every kernel a way to opt out of
#: being blamed, which is the one failure this file must never allow.
#:
#: Kept deliberately short and literal. A regex over host output would be a
#: matcher whose false-positive rate nobody can bound, and a false positive here
#: does not merely mislabel a row -- it *removes* a real failure from the counts.
HOST_FAIL_SIGNATURES = (
    ("Failed to CreateFileMapping", "QEMU could not map guest memory"),
    ("The paging file is too small", "Windows pagefile could not grow in time"),
    ("cannot set up guest memory", "QEMU could not allocate guest memory"),
    ("cygheap read copy failed", "a Cygwin fork failed under memory pressure"),
)


def read_qemu_stderr(path: str | None) -> str:
    """QEMU's own stderr for this run, or "" when there is none to read.

    Absent is not an error and never will be: the file is written by
    `boot-test.sh` only, so every other caller of this script -- `--classify` on
    an old log, `--list`, a test -- legitimately has nothing to point at. A
    missing file must therefore mean "no host evidence", which is exactly the
    reading that leaves the existing verdict standing.

    Read errors are swallowed for the reason argued at `read_accel`: this runs
    from an EXIT trap under `|| true`, and losing the whole record of a failing
    boot is far more expensive than losing one attribution. Swallowing is safe
    in *this* direction specifically -- an unreadable file yields "", which can
    only leave a failure blamed on the tree, never excuse one.
    """
    if not path:
        return ""
    try:
        with open(path, "r", encoding="utf-8", errors="replace") as handle:
            return handle.read()
    except FileNotFoundError:
        return ""
    except OSError as exc:
        print(f"boot-history: cannot read qemu stderr {path}: {exc}",
              file=sys.stderr)
        return ""


def host_failure(text: str) -> str | None:
    """The reason this run's host failed, or None if it did not say so.

    Returns the human-readable half of the matched signature rather than a bool
    so the row can record *what* went wrong. A row that says only "HOST_FAIL"
    invites the next reader to re-derive the evidence from a build artefact that
    was deleted by the following run.

    First match wins, and the order above is not alphabetical: the two most
    specific Windows messages come before the generic QEMU one, so a run that
    printed both is described by the more informative of the two.
    """
    for needle, reason in HOST_FAIL_SIGNATURES:
        if needle in text:
            return reason
    return None


#: Exit statuses `boot-test.sh` cannot produce, mapped to what they mean.
#:
#: The harness's own vocabulary is 0 (pass), 1 (kernel/self-test failure),
#: 2 (hang/wedge), 3 (booted but produced no artefact) and 4 (boot lock held --
#: nothing was booted). Every one of those comes from an explicit `exit` the
#: script reaches after it has looked at the boot. 126 and 127 come from
#: somewhere else entirely: they are the *shell's* statuses for "found the
#: command but could not execute it" and "could not run the command at all",
#: and on this host the second is what a fork() refused by the Windows commit
#: limit looks like from outside (`dofork: child -1 ... 0xC000012D`).
#:
#: This is HOST_FAIL_SIGNATURES' trick taken from the other end. There the
#: evidence is trustworthy because the guest cannot write to that stream; here
#: it is trustworthy because the guest cannot set the harness's exit status --
#: the number is produced by the shell, above the emulator, after the kernel
#: has had its say. Neither can be forged from inside the tree.
#:
#: Deliberately two literal codes and not a range. "Anything unexpected is a
#: host failure" would be a predicate whose false-positive rate nobody can
#: bound, and a false positive here does not merely mislabel a row -- it
#: *removes* a real failure from the counts, which is the one direction this
#: file must never fail in. 124 is excluded on purpose despite being outside
#: the vocabulary: it is `run-timeout.py`'s expiry, which *is* a statement
#: about the tree (something did not finish in the time allowed).
HARNESS_ABORT_EXITS = {
    126: "the harness could not execute a command it found (exit 126)",
    127: "the harness could not run a command at all (exit 127) -- on this "
         "host that is what a fork() refused by the Windows commit limit "
         "looks like from outside",
}


def harness_abort(exit_code: int) -> str | None:
    """The reason the *harness* died, or None if it exited on its own terms.

    Distinct from `host_failure` in where it looks, identical in what it means:
    neither says anything about the kernel. They stay two functions because the
    evidence is two things -- a run can have host stderr and no harness abort,
    or the reverse -- and merging them would make it impossible to say which
    one actually fired when a row is read back months later.
    """
    return HARNESS_ABORT_EXITS.get(exit_code)


#: Verdicts the serial log establishes on its own, without consulting the exit
#: status.
#:
#: This is the guard that keeps `harness_abort` from deleting real failures,
#: and it exists because the history already contained the counter-example.
#: The boot of 2026-09-01T11:06 exited 127 *and* its log ends
#: `FATAL: virtio-gpu render-resource self-test failed`. Both things happened:
#: the kernel died, and then the harness could not fork on its way out. A rule
#: that excused every 127 would have rewritten that row to HOST_FAIL and
#: removed a genuine FATAL from the counts -- the exact direction this file
#: exists to prevent.
#:
#: The distinction is what the verdict rests on. PANIC is read out of the log:
#: the kernel said it died, and the harness stumbling afterwards does not
#: un-say it. TIMEOUT and SELFTEST_FAIL rest on *absence* -- a marker that
#: never arrived, a gate that never reported -- and absence is precisely what a
#: harness that stopped running cannot testify to. So the harness override
#: applies to those and not to this set.
#:
#: Note this guard is the harness half's alone; the stderr half deliberately
#: *does* override PANIC, because QEMU saying it could not map guest memory is
#: evidence about the cause of that panic, whereas a failed fork minutes later
#: is evidence about nothing but the fork.
LOG_EVIDENCED_VERDICTS = frozenset({"PANIC"})


def not_about_the_tree(qemu_stderr: str, exit_code: int) -> str | None:
    """Why this run is not evidence about the kernel, or None if it is.

    One derivation with two callers -- `classify` and `main` -- for the same
    reason `main` reads QEMU's stderr exactly once: the verdict and the reason
    recorded beside it must be two views of one piece of evidence.

    Answering "why is this run not about the tree" is not the same as deciding
    whether the verdict is overridden, and this function does only the first.
    `classify` owns the second, because the two halves are guarded differently
    (see LOG_EVIDENCED_VERDICTS) -- so a row can legitimately carry a reason
    while keeping a verdict that blames the tree. That is exactly what the boot
    which both panicked and exited 127 needs, in order to record both facts
    instead of choosing between them.

    The host is asked before the harness only so a run with both gets the more
    specific answer: QEMU's own words name the resource that ran out, whereas
    exit 127 can say only that *something* could not be started.
    """
    return host_failure(qemu_stderr) or harness_abort(exit_code)


VERDICT_HELP = {
    "PASS": "marker reached, every gate green",
    "PASS_TOOLING": "kernel booted; the harness failed to produce an artefact",
    "BENCH_INCOMPLETE": "BOOT_OK reached, BENCH_OK did not (known bench livelock)",
    "SELFTEST_FAIL": "marker reached but a self-test / liveness gate failed",
    "PANIC": "kernel died (PANIC / FATAL in the serial log)",
    "WEDGE": "serial output stalled; kernel stopped progressing",
    "TIMEOUT": "marker never arrived, no panic, no stall detected",
    "HOST_FAIL": "the host or the harness failed underneath the boot; "
                 "says nothing about the tree",
}


def classify(serial: Serial | None, exit_code: int,
             qemu_stderr: str = "") -> str:
    """The verdict for one run: evidence first, host override second.

    `qemu_stderr` defaults to empty so every existing caller keeps its exact
    behaviour, and so the stderr half of the override can only ever be reached
    by a caller that went and got the host's own words. The harness half needs
    no such opt-in: `exit_code` is required of every caller already, and a
    status the harness cannot produce means the same thing to all of them.
    """
    verdict = _verdict_from_evidence(serial, exit_code)

    # An override replaces a verdict that blames the tree, and never one that
    # clears it -- which is why this is applied to the result rather than as a
    # branch taken before the evidence is read.
    #
    # Both halves are load-bearing. Downward: a kernel that reached BOOT_OK
    # reached it, and a warning QEMU printed on the way does not un-boot it;
    # rewriting a PASS would *destroy* a real clean boot, and this file's whole
    # bias is that manufacturing a clean streak is the dangerous direction.
    # Upward: NO_BOOT is left alone because it is not a verdict about the tree
    # either -- it means the run produced no serial output at all, which main()
    # declines to record, and promoting it here would file a row with no boot
    # in it.
    if verdict in CLEAN_VERDICTS or verdict == "NO_BOOT":
        return verdict

    # QEMU's own stderr overrides everything below that guard, PANIC included:
    # "cannot set up guest memory" is evidence about *why* the guest died, and
    # the boot this was built for is one where the host OOM surfaced inside the
    # guest as a kernel panic.
    if host_failure(qemu_stderr) is not None:
        return "HOST_FAIL"

    # The harness's exit status does not reach that far. It proves the harness
    # stopped running; it says nothing about what the kernel had already
    # printed. So it may retract a verdict that rests on absence -- TIMEOUT's
    # missing marker, SELFTEST_FAIL's silent gate, neither of which a stopped
    # harness could have observed -- and may not touch one the log establishes
    # on its own. See LOG_EVIDENCED_VERDICTS for the boot that proves the
    # difference matters.
    if (harness_abort(exit_code) is not None
            and verdict not in LOG_EVIDENCED_VERDICTS):
        return "HOST_FAIL"

    return verdict


def _verdict_from_evidence(serial: Serial | None, exit_code: int) -> str:
    """Derive the verdict from evidence, using the exit code only to break ties.

    Deliberately *not* a lookup on the exit code. boot-test.sh reaches exit 1
    from five distinct conditions, and the serial log is what distinguishes
    them; conversely the serial log alone cannot distinguish a stall (exit 2)
    from a plain timeout, because both end with no marker. Each source answers
    the half the other cannot.
    """
    if serial is None:
        return "NO_BOOT"

    if serial.marker_ok:
        if exit_code == 0:
            return "PASS"
        if exit_code == 3:
            # boot-test.sh's own code 3: "the kernel booted but the run did not
            # produce the artefact it was asked for". A tooling failure, and
            # conflating it with a kernel failure sends the reader to the wrong
            # tree -- which is exactly why boot-test.sh made it a distinct code.
            return "PASS_TOOLING"
        return "SELFTEST_FAIL"

    # No marker. In --bench mode BOOT_OK-but-not-BENCH_OK is the documented
    # deferred-benchmark livelock, not a boot hang; counting it as one would
    # reset every hang streak on every bench run.
    if serial.boot_ok and serial.marker != "BOOT_OK":
        return "BENCH_INCOMPLETE"

    if serial.has_panic or serial.exceptions:
        return "PANIC"
    if exit_code == 2:
        return "WEDGE"
    return "TIMEOUT"


# --------------------------------------------------------------------------
# Fingerprints
# --------------------------------------------------------------------------


@dataclass(frozen=True)
class Fingerprint:
    """One known-issues entry, expressed as a predicate over a failed boot.

    `validated_by` is not documentation. It is the guard against the failure
    mode that makes this whole script dangerous: a matcher that cannot fire
    reports a flawless streak, and a flawless streak is what closes an issue.
    An empty tuple means the predicate has never been checked against a real
    occurrence, and its streak is therefore not evidence of anything.
    """

    id: str
    title: str
    match: Callable[[Serial, str], bool]
    validated_by: tuple[str, ...] = ()
    note: str = ""
    #: True when every known occurrence predates this history file, so a
    #: "never seen" streak is expected and says nothing on its own yet.
    historic_only: bool = True


def _is_user_address(addr: int) -> bool:
    """User-half canonical address (below the higher-half split)."""
    return 0 < addr < 0x8000_0000_0000_0000


def _pf_fields(line: str) -> dict[str, int]:
    out: dict[str, int] = {}
    for key in ("address", "error"):
        m = re.search(rf"\b{key}=0x([0-9a-fA-F]+)", line)
        if m:
            out[key] = int(m.group(1), 16)
    m = re.search(r"#PF\) at 0x([0-9a-fA-F]+)", line)
    if m:
        out["rip"] = int(m.group(1), 16)
    return out


def _fp_w1(s: Serial, verdict: str) -> bool:
    # W1 as *retargeted* by the 2026-08-14 analysis: since `1e5c091f4` (cli
    # around the SERIAL critical section) and `58102abca` (per-CPU IN_PRINT +
    # emergency fallback), a console-lock re-entry is expected to *print*
    # rather than go silent. So the fingerprint is not the OOM self-test's
    # location -- that was only whatever happened to be printing -- but the
    # silence itself: a mid-line cut with no diagnostic anywhere. A match
    # falsifies the cured-incidentally analysis, which is the one observation
    # the entry says is worth more than its remaining 83 blind boots.
    return (
        verdict in ("WEDGE", "TIMEOUT")
        and s.ends_mid_line
        and not s.exceptions
        and not s.has_panic
    )


def _fp_kasan_midprint(s: Serial, verdict: str) -> bool:
    # Same silence-shaped wedge, but the cut lands *inside* the exception
    # report -- `EXCEPTION: Page Fault (#PF) at` truncated exactly where
    # `{:#x}` would have formatted `frame.rip`. Disjoint from W1 by
    # construction: W1 requires no exception line at all.
    #
    # The issue is titled "KASAN builds only", and now that the kernel says
    # which build it is, say so here. Note the asymmetry in how the three
    # sanitizer states are treated, which is deliberate and is the whole reason
    # `sanitizer` is three-valued: an explicit `"none"` is a *positive* denial
    # from the kernel and rules the fingerprint out, whereas `None` -- a log
    # from a kernel too old to print the banner -- rules nothing out and must
    # still be allowed to match. Every boot this fingerprint was validated
    # against (2026-08-12) predates the banner, so folding `None` in with
    # `"none"` would un-validate it and reset its streak to a clean one, which
    # is precisely the failure this file exists to prevent.
    if s.sanitizer == "none":
        return False
    return (
        verdict in ("WEDGE", "TIMEOUT", "PANIC")
        and s.ends_mid_line
        and s.last_line.startswith("EXCEPTION:")
    )


def _fp_pthread_teardown_pf(s: Serial, verdict: str) -> bool:
    # Null-ish deref at a small fixed offset while a cloned thread tears down.
    # Matched on (address, task name) rather than on the RIP: the RIP moves
    # with every kernel rebuild, so a RIP-keyed fingerprint would silently
    # stop matching -- a streak that resets to "clean" on recompilation is
    # worse than no streak.
    for line in s.exceptions:
        f = _pf_fields(line)
        if f.get("address", -1) < 0x1000 and "Page Fault" in line:
            window = s.text[s.text.find(line):][:600]
            if "cloned-thread" in window or "pthread" in window:
                return True
    return False


def _fp_forkexec_hang(s: Serial, verdict: str) -> bool:
    # A quiet hang immediately after the last thread of a process is reaped:
    # no exception, no panic, the log simply stops after the zombie lines.
    # Note it does NOT require a mid-line cut -- this one dies between lines,
    # which is what separates it from W1.
    if verdict not in ("WEDGE", "TIMEOUT"):
        return False
    if s.exceptions or s.has_panic:
        return False
    tail = "\n".join(s.lines[-6:])
    return "has no threads left" in tail and "zombie" in tail


def _fp_kernel_cow_write(s: Serial, verdict: str) -> bool:
    # Write-to-present fault (error=0x3) taken by the kernel against a user
    # mapping -- the copy-on-write path failing to break sharing.
    for line in s.exceptions:
        f = _pf_fields(line)
        if f.get("error") == 0x3 and _is_user_address(f.get("address", 0)):
            return True
    return False


FINGERPRINTS: tuple[Fingerprint, ...] = (
    Fingerprint(
        id="W1",
        title="silent mid-print truncation (console-lock wedge)",
        match=_fp_w1,
        validated_by=("2026-06-10", "2026-06-12"),
        note="a match falsifies the 2026-08-14 cured-incidentally analysis; "
             "re-open and bisect rather than adding to the streak",
    ),
    Fingerprint(
        id="B-KASAN-INSTRUMENTED-BOOT-WEDGES-MID-PRINT-ON-A-PAGE-FAULT",
        title="wedge mid-print inside the #PF report (KASAN builds)",
        match=_fp_kasan_midprint,
        validated_by=("2026-08-12",),
        note="did not reproduce on 2026-08-14; KASAN builds only",
    ),
    Fingerprint(
        id="B-PTHREAD-TEARDOWN-PF",
        title="#PF at a small fixed offset during cloned-thread teardown",
        match=_fp_pthread_teardown_pf,
        validated_by=("2026-08-13",),
    ),
    Fingerprint(
        id="B-FORKEXEC-BOOT-HANG",
        title="quiet hang after the last thread is reaped (no diagnostics)",
        match=_fp_forkexec_hang,
        validated_by=("2026-06-12",),
    ),
    Fingerprint(
        id="W-KERNEL-COW-WRITE",
        title="write fault (error=0x3) on a user mapping -- CoW break failed",
        match=_fp_kernel_cow_write,
        validated_by=("2026-07-28",),
        note="not currently reproducible",
    ),
)


def fingerprints_for(serial: Serial | None, verdict: str) -> list[str]:
    """Ids of every fingerprint matching this run, in declaration order.

    All matches are reported, not just the first: two of these describe the
    same *shape* of wedge at different cut points, and being told which of them
    a new occurrence resembles is the entire diagnostic value.
    """
    # HOST_FAIL is skipped for the reason a clean run is, arrived at from the
    # opposite side: there is nothing here to attribute to a known issue.
    # Skipping matters more than it looks. `_fp_pthread_teardown_pf` and its
    # neighbours match on the *shape* of the exception in the log without
    # consulting the verdict, and a host OOM lands on whatever allocation the
    # kernel happened to be making -- so a host-killed boot can easily wear the
    # shape of a known bug. Recording that would file a recurrence of an issue
    # that did not recur, against a run the kernel was not responsible for, in
    # the counter several `known-issues.md` closure bars are written in.
    if serial is None or verdict in CLEAN_VERDICTS or verdict == "HOST_FAIL":
        return []
    out = []
    for fp in FINGERPRINTS:
        try:
            if fp.match(serial, verdict):
                out.append(fp.id)
        except Exception as exc:  # noqa: BLE001 - see below
            # A fingerprint that raises must not lose the record. The record is
            # the durable artefact; the fingerprint is an opinion about it, and
            # an opinion that crashed is worth less than the evidence it was
            # about. Reported loudly so it gets fixed rather than tolerated.
            print(
                f"boot-history: fingerprint {fp.id} raised {exc!r}; "
                f"recording the run without it",
                file=sys.stderr,
            )
    return out


# --------------------------------------------------------------------------
# History file
# --------------------------------------------------------------------------


def git_commit() -> str:
    """Short HEAD hash, or 'unknown' outside a working repo."""
    try:
        out = subprocess.run(
            ["git", "-C", REPO_ROOT, "rev-parse", "--short", "HEAD"],
            capture_output=True, text=True, timeout=15, check=False,
        )
    except (OSError, subprocess.SubprocessError):
        return "unknown"
    if out.returncode != 0:
        return "unknown"
    return out.stdout.strip() or "unknown"


def git_branch() -> str:
    try:
        out = subprocess.run(
            ["git", "-C", REPO_ROOT, "rev-parse", "--abbrev-ref", "HEAD"],
            capture_output=True, text=True, timeout=15, check=False,
        )
    except (OSError, subprocess.SubprocessError):
        return "unknown"
    if out.returncode != 0:
        return "unknown"
    return out.stdout.strip() or "unknown"


def load_history(path: str) -> list[dict]:
    """Read the log in chronological order, skipping records that fail to parse.

    A corrupt line must not destroy the rest: this file is appended to by every
    boot and is the only longitudinal record of outcomes we have, so partial
    recovery beats an exception. (Same rule as bench-history.py's loader --
    and the same reason: the file is written concurrently by three lanes'
    worktrees and merged as text.)

    **The sort at the end is what makes file order non-semantic**, and that is
    a correctness requirement, not tidiness. `bench/boot-history.jsonl` is
    marked `merge=union` in `.gitattributes` so concurrent appends from three
    lanes stop conflicting -- but union merge *concatenates*, it does not sort:
    for a conflicting hunk it emits our lines then theirs, so two lanes booting
    in the same window produce a file whose last record is not the latest boot.

    `tail_clean_streak` walks `reversed(records)` from the end, and its result
    is a published quantity that several `known-issues.md` closure bars are
    written in terms of. Computing it from the wrong end of history would close
    an issue that is still live -- strictly worse than the merge conflict this
    replaces, because a conflict stops you and a wrong number does not.
    (Filed by lane B: requests/b-a-boot-history-jsonl-conflicts-*.)

    Sorted as strings, not parsed: every `ts` this file has ever carried is
    ISO-8601 with a literal `+00:00` offset, because `iso_now()` hardcodes
    `timezone.utc`, and same-offset ISO-8601 sorts correctly lexicographically.
    (`test-boot-history.py` asserts that offset uniformity against the real
    file, so a writer that started emitting local time would be caught here
    rather than by a quietly-misordered streak.) A record with a missing or
    blank `ts` sorts first, which is the safe direction for one too old or too
    damaged to place: it cannot displace the genuinely-latest record from the
    end.

    The sort is **stable**, so records sharing a timestamp keep their relative
    file order -- the only ordering information left for a same-second tie.
    """
    records: list[dict] = []
    try:
        with open(path, "r", encoding="utf-8") as handle:
            for lineno, line in enumerate(handle, 1):
                line = line.strip()
                if not line:
                    continue
                try:
                    rec = json.loads(line)
                except json.JSONDecodeError:
                    print(
                        f"boot-history: skipping malformed record at "
                        f"{path}:{lineno}", file=sys.stderr,
                    )
                    continue
                # Valid JSON is not the same thing as a record: a line that
                # parses to a bare string or number is what a half-written or
                # mis-merged line often is. Dropping it is right, but dropping
                # it *silently* -- as this did -- makes a shrinking history
                # indistinguishable from a history that is simply short, so it
                # is reported like a malformed line, while the lineno is still
                # in hand to say which line to go and look at.
                if not isinstance(rec, dict):
                    print(
                        f"boot-history: skipping non-object record at "
                        f"{path}:{lineno} ({type(rec).__name__})",
                        file=sys.stderr,
                    )
                    continue
                records.append(rec)
    except FileNotFoundError:
        return []
    except OSError as exc:
        print(f"boot-history: cannot read {path}: {exc}", file=sys.stderr)
        return []
    # See the docstring: this is what licenses `merge=union` on the file.
    #
    # `str(... or "")` rather than a bare `r.get("ts", "")`: a record whose
    # `ts` is JSON `null`, or a number, would otherwise make the sort raise
    # TypeError comparing None/int against str -- destroying the whole history
    # over one damaged line, which is precisely what the per-line recovery
    # above exists to prevent. Coercing sorts such a record early instead,
    # the same safe direction as a missing `ts`.
    records.sort(key=lambda r: str(r.get("ts") or ""))
    return records


def append_record(path: str, record: dict) -> bool:
    """Append one JSON-lines record, creating the directory if needed.

    `newline="\\n"` is not incidental: text mode would translate to CRLF on
    Windows, and this file is committed and appended to from three worktrees.
    Mixed line endings in an append-only log produce phantom whole-file diffs
    and, worse, merge conflicts on lines nobody touched.
    """
    try:
        os.makedirs(os.path.dirname(path), exist_ok=True)
        with open(path, "a", encoding="utf-8", newline="\n") as handle:
            handle.write(json.dumps(record, sort_keys=True) + "\n")
    except OSError as exc:
        print(f"boot-history: cannot write {path}: {exc}", file=sys.stderr)
        return False
    return True


def load_gated_markers(path: str) -> tuple[str, ...] | None:
    """The `RAN-IF` literals emitted by `check-self-tests-wired.py`, or None.

    None means "could not be established" and is deliberately distinct from an
    empty tuple, which would mean "there are no gated call sites". The second is
    a claim about the kernel; the first is a statement about this run. Collapsing
    them would let a missing or malformed file read downstream as "nothing is
    gated, so nothing can be un-run" -- an all-clear manufactured out of a
    plumbing failure, which is the one answer a coverage check must never give
    by accident.

    Every diagnostic here goes to stderr and none is fatal: this runs at the end
    of a boot that took twenty minutes, and losing the whole row over a JSON
    parse error would throw away the wall time, the verdict and the skip lists
    to punish a harness bug in a field none of them depend on.
    """
    try:
        with open(path, "r", encoding="utf-8") as handle:
            payload = json.load(handle)
    except FileNotFoundError:
        # Said out loud, unlike read_serial's silent None for the same cause.
        # Nothing passes this argument by accident -- boot-test.sh sets it to a
        # file it generated earlier in the same run -- so the file not being
        # there is a harness fault, and the symptom is a field quietly missing
        # from every row. That is invisible in the history and looks exactly
        # like "this predates the field", which is how a check stops running
        # without anyone noticing it stopped.
        print(f"boot-history: gated markers {path} does not exist -- "
              f"`gated_ran` will be omitted from this row", file=sys.stderr)
        return None
    except (OSError, ValueError) as exc:
        print(f"boot-history: cannot read gated markers {path}: {exc}",
              file=sys.stderr)
        return None
    markers = payload.get("markers") if isinstance(payload, dict) else None
    if not isinstance(markers, dict):
        print(f"boot-history: {path} has no `markers` object -- ignoring",
              file=sys.stderr)
        return None
    return tuple(sorted(str(k) for k in markers))


def gated_ran(serial: Serial, markers: tuple[str, ...]) -> dict[str, bool]:
    """Which gated self-tests announced themselves in this boot.

    A plain substring test, not a regex, and that is the point rather than a
    shortcut: the marker is the exact `serial_println!` literal, and
    `check-self-tests-wired.py` has already refused to emit one that no file
    defining the suite can print. Treating it as a pattern would give `[acpi]`
    and `(ring 3)` meaning they do not have, so the two markers with brackets or
    parentheses in them -- which is all five -- would match nothing, and every
    gated suite in the kernel would be reported as never-run.
    """
    return {m: (m in serial.text) for m in markers}


def build_record(serial: Serial | None, verdict: str, args,
                 host_fail: str | None = None) -> dict:
    # `--commit`/`--branch` win over asking git, and boot-test.sh always passes
    # them.  It reads HEAD once, before the build, and hands that value down;
    # this function runs at the *end* of a run that took ten to twenty minutes,
    # by which time HEAD may well have moved -- committing while a boot test
    # runs is normal here.  Falling back to `git_commit()` keeps a standalone
    # invocation working, but for a real run it would stamp the row with a
    # commit that was never built.
    rec: dict = {
        "ts": _now_iso(),
        "commit": args.commit or git_commit(),
        # Omitted entirely when unavailable rather than stored empty: an absent
        # field reads as unknown, and unknown refuses to group, whereas an
        # empty string would group every such row together. See
        # scripts/src_digest.py.
        **({"src_digest": args.src_digest} if args.src_digest else {}),
        "branch": args.branch or git_branch(),
        # True when the tree carried uncommitted changes at build time, so the
        # `commit` above names the nearest ancestor rather than what ran.  A
        # consumer that diffs against this row must say so; see
        # report_bench_absence() in boot-test.sh.
        "dirty": bool(args.dirty),
        "host": socket.gethostname(),
        "os": platform.system(),
        "verdict": verdict,
        "exit_code": args.exit_code,
        "marker": args.marker,
        "label": args.label,
        "profile": args.profile,
    }
    # Why this run is not a normal boot, or absent when it is one. Set for a
    # deliberate probe -- non-default emulator flags, a hand-patched kernel --
    # and stored for the same reason `bench/history.jsonl` stores it: such a run
    # is not reproducible from a checkout, so it must not be counted as evidence
    # about the tree.
    #
    # This exists because a probe was silently counted as a regression. On
    # 2026-08-19 a one-off `-cpu host` boot, run only to find out whether WHPX
    # could carry SMEP/SMAP/UMIP, died in OVMF before our kernel loaded -- a
    # fact about QEMU, not about us -- and landed here as a TIMEOUT that reset
    # the consecutive-clean streak to 0 after a long run of passes. Four open
    # kernel issues have closure conditions written as counts of consecutive
    # clean boots, so a streak that any experiment can zero is not merely untidy:
    # it postpones closing real issues, and it trains a reader to shrug at
    # failures in this file.
    if args.experiment:
        rec["experiment"] = args.experiment
    # Why the host is being blamed, in the host's own terms, or absent when it
    # is not being blamed. Kept as a separate field from `verdict` rather than
    # encoded into it because a verdict is a closed vocabulary that `--list`
    # aligns in a column, while this is a sentence -- and because a reader who
    # doubts the attribution needs the reason to doubt, which a bare label
    # cannot give them.
    #
    # Passed in rather than re-derived here on purpose: the caller already read
    # qemu's stderr to reach the verdict, and reading it a second time is two
    # answers to one question. They could differ -- the file is on disk and the
    # run is over, but "cannot happen" is how a row ends up saying HOST_FAIL
    # with no reason attached, or carrying a reason under a verdict that blames
    # the kernel.
    if host_fail:
        rec["host_fail"] = host_fail
    if args.wall_seconds is not None:
        rec["wall_seconds"] = args.wall_seconds
    if args.build_seconds is not None:
        rec["build_seconds"] = args.build_seconds
    # Disk pressure, which is a cause of boot-test failures this file could not
    # previously distinguish from kernel failures.
    #
    # On 2026-08-15 the build volume reached zero bytes free and a half-written
    # edit truncated a kernel source file; a part-way link can also leave a
    # stale kernel staged in the ESP, which a later --no-build run boots as if
    # it were current. Both produce rows here that look like the kernel
    # misbehaving. boot-test.sh has measured free space at each phase since
    # then and *printed* it, which helps only someone reading that one run's
    # console -- not someone asking months later why a cluster of boots went
    # red in the same week.
    #
    # Absent rather than zero when unmeasured, for the same reason
    # `build_seconds` is: a run whose floor check was disabled with
    # --min-free-gb=0, or whose `df` was unreadable, did not observe zero GiB
    # free. A missing field is a question the reader can answer; a wrong one is
    # not.
    if args.free_gb_min is not None:
        rec["free_gb_min"] = args.free_gb_min
        if args.free_gb_phase:
            rec["free_gb_phase"] = args.free_gb_phase
    if serial is not None:
        rec["serial_bytes"] = serial.n_bytes
        rec["serial_lines"] = len(serial.lines)
        rec["ends_mid_line"] = serial.ends_mid_line
        rec["boot_ok"] = serial.boot_ok
        # Written unconditionally, `null` included, and *not* folded into
        # `profile`.
        #
        # `profile` is what the harness was told to build; this is what the
        # kernel says it actually is, and until 2026-08-19 the two were not the
        # same question with the same answer. An instrumented boot and an
        # ordinary one both recorded `profile: "debug"`, while their wall times
        # differed by 3.4x (~1100 s against ~330 s on this host) -- so every
        # duration statistic drawn from this file was averaging two populations
        # it had no way to tell apart.
        #
        # Emitting the key even when the value is `null` is what keeps the
        # three states distinguishable *within* the rows that have a serial log
        # at all: absent means "row predates this field", `null` means "the
        # kernel did not say", and a string means it did. Had the key simply
        # been omitted when unknown, those first two would collapse, and a
        # consumer would have to guess -- which, on this file's history, means
        # guess "uninstrumented" and quietly mislabel the slow boots.
        rec["sanitizer"] = serial.sanitizer
        # Written unconditionally, `null` included, for the same reason and by
        # the same rule as `sanitizer` directly above: absent means "this row
        # predates the field", `null` means "the log did not say", and a string
        # means it did. Fold the first two together and a consumer has to guess,
        # and on this file's history the guess would be "TCG" -- which
        # bench-history.py's ACCEL_RE shows is provably wrong, since the first
        # WHPX run on this host predates the banner.
        rec["accel"] = serial.accel
        # Which self-test sections announced that they did not run.
        #
        # Written on every row that has a serial log, empty list included, and
        # this is the field `check-boot-skips.py` reads. The point is not to
        # know that a boot skipped something -- the log says that and nobody
        # reads it -- but to make "has this skip fired on *every* recorded
        # boot?" a question with an answer. A predicate that has never once
        # been true is not a skip, it is a deletion with a log line, and six of
        # them sat in this kernel printing SKIP under suites that printed
        # PASSED (design-decisions.md sec 650). Eye-finding does not scale to the
        # seventh; this does.
        #
        # Empty list rather than an omitted key when nothing skipped, for the
        # reason `sanitizer` gives two fields up: absent must keep meaning
        # "this row predates the field". Fold the two together and every row
        # written before today reads as a boot on which every skip failed to
        # fire, which would make the 100%-of-N test unfalsifiable in the
        # direction that hides the bug.
        rec["skips"] = list(serial.skips)
        # The tripwires: skipped at one call site, ran at another. Kept out of
        # `skips` so the gate does not accuse them, and kept in the file so the
        # day one stops being covered is visible as a name moving between the
        # two lists rather than as a new accusation with no history.
        rec["skips_covered"] = list(serial.skips_covered)
        # Which conditionally-called self-tests announced themselves, keyed by
        # the serial line each declares in main.rs (`// RAN-IF: "..."`).
        #
        # `skips` above answers "did a suite say it was not running"; this
        # answers the harder question next to it, "did a suite that never says
        # anything simply not run". A self-test behind `if fat_ok` prints no SKIP
        # when the condition is false -- it prints nothing at all, which is
        # indistinguishable from a green boot unless something knows what its
        # output would have looked like. Seven suites sat behind that one false
        # condition for a year.
        #
        # Omitted, rather than written empty, when the marker file is absent or
        # unreadable: see load_gated_markers. An empty object here would mean
        # "the kernel has no gated call sites", which is a claim, and one that
        # would read as an all-clear.
        markers = (load_gated_markers(args.gated_markers)
                   if getattr(args, "gated_markers", None) else None)
        if markers is not None:
            rec["gated_ran"] = gated_ran(serial, markers)
        fps = fingerprints_for(serial, verdict)
        if fps:
            rec["fingerprints"] = fps
        if verdict not in CLEAN_VERDICTS:
            # Only failures carry their tail. A passing tail is the same 25
            # lines every time, and this file is committed: paying that on
            # every green boot would bury the failures it exists to preserve.
            rec["tail"] = serial.tail()
            if serial.exceptions:
                rec["exceptions"] = [e[:TAIL_WIDTH] for e in serial.exceptions[:5]]
    return rec


def _now_iso() -> str:
    import datetime
    return datetime.datetime.now(datetime.timezone.utc).isoformat(
        timespec="seconds")


# --------------------------------------------------------------------------
# Streaks
# --------------------------------------------------------------------------


@dataclass
class Streak:
    fp: Fingerprint
    recorded: int = 0          # records considered
    occurrences: int = 0       # times this fingerprint matched, ever
    since_last: int = 0        # records since the most recent match
    last_seen: str = ""        # ts of the most recent match
    last_commit: str = ""


def streaks(records: list[dict]) -> list[Streak]:
    """Per-fingerprint standing over the whole recorded history.

    `since_last` counts *records*, not clean records: a boot that failed for a
    different reason is still a boot in which this fingerprint did not appear,
    which is what the known-issues closure bars mean by "routine boots count".

    Experiment boots are the exception, and excluding them is the whole reason
    this function may be trusted to close an issue. That argument -- "a boot
    that failed differently is still a boot where this did not appear" --
    silently assumes the kernel *ran*. A probe need not have: the `-cpu host`
    boot of 2026-08-19 died in OVMF before our kernel was loaded, so it could
    not have exhibited any kernel fingerprint whatever. Counting it would be
    recording the absence of a symptom in a run that had no opportunity to
    show one, and `since_last` is exactly what several `known-issues.md`
    closure bars are written in terms of. This is the direction this module
    exists to prevent: not a missed failure, but a manufactured clean streak.

    A `HOST_FAIL` row is stepped over by the same argument, and needs it just as
    much. The kernel there did run, so the assumption above holds -- but the run
    was cut short by the host at a moment nothing in the tree chose, so "this
    fingerprint did not appear" says only that the boot ended before it could
    have. Counting those would inflate `since_last` fastest exactly when the
    machine is under load, which is when boots are being repeated most.
    """
    tree = [r for r in records if describes_tree(r)]
    out = []
    for fp in FINGERPRINTS:
        st = Streak(fp=fp, recorded=len(tree))
        for rec in tree:
            hit = fp.id in (rec.get("fingerprints") or [])
            if hit:
                st.occurrences += 1
                st.since_last = 0
                st.last_seen = str(rec.get("ts", ""))
                st.last_commit = str(rec.get("commit", ""))
            else:
                st.since_last += 1
        out.append(st)
    return out


def describe_streak(st: Streak) -> list[str]:
    """One fingerprint's standing, in lines, honest about what it cannot say."""
    lines = [f"  {st.fp.id}"]
    lines.append(f"      {st.fp.title}")

    if not st.fp.validated_by:
        # The load-bearing branch. Never print a streak here: it would be
        # indistinguishable from a matcher that cannot fire.
        lines.append("      UNVALIDATED fingerprint -- never checked against a "
                     "real occurrence.")
        lines.append("      No streak reported: a matcher that never fires and "
                     "a genuinely clean run")
        lines.append("      produce the same number, and only one of them means "
                     "anything.")
        return lines

    if st.occurrences:
        lines.append(f"      {st.since_last} boot(s) since the last match "
                     f"({st.last_seen} @ {st.last_commit}); "
                     f"{st.occurrences} occurrence(s) recorded")
    else:
        lines.append(f"      not seen in {st.recorded} recorded boot(s)")
        if st.fp.historic_only:
            # Say plainly what the number is worth. The known occurrences
            # predate this file, so the streak is a count of boots since the
            # recorder existed -- not since the issue last appeared.
            lines.append(f"      (known occurrence(s) "
                         f"{', '.join(st.fp.validated_by)} predate this file, "
                         f"so the count starts at the recorder, not at the bug)")
    if st.fp.note:
        lines.append(f"      note: {st.fp.note}")
    return lines


#: Label for a record that cannot say which build it was.
#:
#: Spelled as prose rather than as the bare word "unknown" because it is going
#: to be read next to "none", and those two must never look like near-synonyms:
#: one is the kernel saying it was not instrumented, the other is nobody saying
#: anything.
_SAN_UNKNOWN = "unknown (pre-banner)"


def sanitizer_of(rec: dict) -> str:
    """Which population a record belongs to, for statistics that must not mix.

    Collapses the two ways of not knowing -- key absent (row written before the
    field existed) and key present but null (kernel too old to print the
    banner) -- because for the purpose of *grouping* they are the same: neither
    can be put in a bucket. They stay distinct in the file itself, where the
    difference tells you whether it is the recorder or the kernel that is old.
    """
    if "sanitizer" not in rec:
        return _SAN_UNKNOWN
    val = rec["sanitizer"]
    return _SAN_UNKNOWN if val is None else str(val)


#: Label for a record that cannot say which accelerator ran it. Prose, like
#: `_SAN_UNKNOWN`, and for the same reason: it is printed beside real
#: accelerator names and must not read like one of them.
_ACCEL_UNKNOWN = "unknown accel (pre-banner)"


def accel_of(rec: dict) -> str:
    """Which accelerator population a record belongs to.

    The exact twin of `sanitizer_of`, collapsing key-absent and key-null for
    grouping while leaving them distinct in the file. Never folds either into a
    named accelerator: see `bench-history.py`'s `ACCEL_RE`, and the record from
    2026-08-19T16:15:09 that proves it.
    """
    if "accel" not in rec:
        return _ACCEL_UNKNOWN
    val = rec["accel"]
    return _ACCEL_UNKNOWN if val is None else str(val)


#: Third of the three unknown markers, and phrased on the same principle as
#: `_SAN_UNKNOWN` and `_ACCEL_UNKNOWN`: it sits beside `debug` and `release` in
#: a printed label and must not be mistakable for either.
_PROFILE_UNKNOWN = "unknown profile"


def profile_of(rec: dict) -> str:
    """Which build profile a record belongs to.

    The third twin of `sanitizer_of` and `accel_of`, collapsing key-absent and
    key-null for grouping. Never guesses `debug`: the recorder's *argparse*
    default is debug, but a record that does not say is a record that does not
    say, and folding it into the larger population is how a mixture gets
    presented as a measurement.
    """
    if "profile" not in rec:
        return _PROFILE_UNKNOWN
    val = rec["profile"]
    return _PROFILE_UNKNOWN if val is None else str(val)


def population_of(rec: dict) -> str:
    """The full label of the population a boot's duration belongs to.

    A wall time is a property of the *triple* (profile, sanitizer,
    accelerator). Measured on this host: release boots ~2.7x faster than debug
    (395s vs 144s median on QEMU TCG), KASAN costs ~3.4x, and the accelerator
    ~1.4x. No one of those makes the others irrelevant, so the population is
    the triple and not any subset. Kept as one function rather than composed at
    each call site so that the printed label and the grouping key cannot drift:
    a legend that names a different partition from the one the numbers were
    computed over is worse than no legend, because it is believed.

    THE PROFILE AXIS WAS MISSING UNTIL 2026-08-21, and unlike the accelerator
    case it was not a latent risk -- it was already wrong. 100 of the 243
    records in `bench/boot-history.jsonl` are release boots, and every one of
    them had been pooled with the debug boots since the day the file was
    started. The largest population read "155 boot(s), median 331s", which is
    the median of a 95/60 mixture of a 382s population and a 130s one: a
    duration no build on this host has ever taken. A smaller population read
    121s while its three debug boots took 327s, so a reader asking "what does a
    boot cost" got the release answer under a label that did not say release.

    This is precisely the defect `report_wall`'s docstring was written about,
    on the axis that docstring forgot -- which is worth stating plainly, because
    the lesson is that naming a partition is not the same as checking it covers
    every factor that moves the number.
    """
    return f"{profile_of(rec)}/{sanitizer_of(rec)} on {accel_of(rec)}"


def _median(values: list[float]) -> float:
    ordered = sorted(values)
    mid = len(ordered) // 2
    if len(ordered) % 2:
        return ordered[mid]
    return (ordered[mid - 1] + ordered[mid]) / 2.0


def wall_populations(records: list[dict]) -> dict[str, list[float]]:
    """Wall times grouped by build *and accelerator*, never merged.

    Experiment boots are excluded outright rather than given a population of
    their own, because "experiment" is not a build -- the probes have nothing in
    common with each other. Two WHPX boots recorded on 2026-08-19 took 168 s and
    186 s against a TCG median of ~120 s for the same profile, so leaving them
    in silently shifted a number whose entire purpose is to say what a normal
    boot costs.

    That last sentence used to be the *whole* defence, and it was resting on a
    coincidence. Those two boots were kept out because they happened to be
    tagged `experiment`, which is a fact about how they were invoked and not a
    rule this file applies -- and Q53 is a live proposal to make WHPX the
    ordinary way to boot the tree, at which point the tag stops appearing and
    the ~40% shift arrives with nothing to attribute it to. Grouping by the
    accelerator makes the exclusion structural: an untagged WHPX boot now forms
    its own population instead of moving the TCG one.

    `HOST_FAIL` boots are excluded on the same "not a build" ground, with a
    second reason of their own: a run whose host could not hand QEMU the memory
    it asked for was, by construction, competing for the machine, and its
    duration measures that contention rather than what the boot costs.
    """
    out: dict[str, list[float]] = {}
    for rec in records:
        if not describes_tree(rec):
            continue
        wall = rec.get("wall_seconds")
        if not isinstance(wall, (int, float)) or isinstance(wall, bool):
            continue
        out.setdefault(population_of(rec), []).append(float(wall))
    return out


def tail_clean_streak(records: list[dict]) -> int:
    """How many times running the *tree* has booted clean, most recent first.

    A named function rather than a loop inside `report()` because several
    `known-issues.md` closure bars are written in terms of this number, so it is
    a published quantity that has to be testable on its own — and because the
    probe-skipping rule below is the kind that a second, inlined copy would
    quietly fail to acquire.

    Experiment boots are stepped over, not counted and not treated as a break.
    Neither alternative is right: a probe is not a clean boot of the tree, so it
    cannot extend the streak, and it is not a boot of the tree at all, so it
    cannot end one either. Skipping is what makes this number mean "the tree has
    booted clean this many times running", whatever was probed in between.

    A `HOST_FAIL` boot is stepped over by exactly that reasoning, and it is the
    reason the verdict exists: on 2026-09-01 a host that briefly could not grow
    its pagefile turned a nine-boot clean streak into zero, because a run the
    tree had no part in was allowed to end one. It still cannot extend a streak
    -- the kernel did not reach its marker, and pretending otherwise would be
    the manufactured-clean-streak failure this file is built against.
    """
    streak = 0
    for rec in reversed(records):
        if not describes_tree(rec):
            continue
        if rec.get("verdict") in CLEAN_VERDICTS:
            streak += 1
        else:
            break
    return streak


def report_wall(records: list[dict]) -> None:
    """Per-build, per-accelerator wall-time standing.

    Deliberately prints no combined figure, not even when there is only one
    population -- because "only one population" is a fact about the records
    that happen to be loaded, and a single number printed today would be the
    number someone compares against tomorrow, after an instrumented run has
    landed in the same file. The whole defect this replaces was a statistic
    that stayed valid right up until the day the second population appeared,
    and then said nothing about either.
    """
    pops = wall_populations(records)
    if not pops:
        return
    print("[boot-history] wall time by profile, build and accelerator:")
    for name in sorted(pops):
        vals = pops[name]
        print(f"[boot-history]   {name}: {len(vals)} boot(s), "
              f"median {_median(vals):.0f}s, "
              f"range {min(vals):.0f}-{max(vals):.0f}s")
    if len(pops) > 1:
        print("[boot-history]   (reported separately on purpose: a debug boot "
              "runs ~2.7x longer than release, a KASAN-instrumented one "
              "~3.4x longer again, and a hardware-virtualised one ~40% on top "
              "of that, so one median over the mixture describes no build that "
              "exists)")


def build_populations(records: list[dict]) -> dict[str, list[float]]:
    """Build seconds grouped by profile and sanitizer -- NOT by accelerator.

    The partition differs from `wall_populations`' on purpose. What the guest is
    executed by cannot change how long the host spent compiling, so folding the
    accelerator in here would split each profile into two or three populations
    that differ in nothing and shrink every sample for no gain. What *does*
    change a build's cost is the profile (`opt-level = 3, codegen-units = 1` is
    not a cheap build) and the sanitizer (KASAN instruments every memory
    access), so those are the two axes.

    Experiment and `HOST_FAIL` boots are excluded on the same rule as everywhere
    else in this file, and runs that never built are absent rather than zero --
    see `--build-seconds`.

    Excluding a `HOST_FAIL` row costs a build time that was, in itself, real:
    the compile finished before QEMU ever started. It is dropped anyway, because
    the memory pressure that killed the emulator is the same pressure the
    compiler was running under, so the number is a measurement of a contended
    host and not of this profile. Losing a sample understates nothing; keeping a
    contended one inflates the median a future run is judged against.
    """
    out: dict[str, list[float]] = {}
    for rec in records:
        if not describes_tree(rec):
            continue
        secs = rec.get("build_seconds")
        if not isinstance(secs, (int, float)) or isinstance(secs, bool):
            continue
        san = sanitizer_of(rec)
        prof = rec.get("profile") or "unknown"
        key = prof if san != "kasan-instrumented" else f"{prof} + KASAN"
        out.setdefault(key, []).append(float(secs))
    return out


def report_build(records: list[dict]) -> None:
    """Per-profile build-time standing.

    This exists to make one specific claim checkable. `open-questions.md` Q46
    asks whether the non-bench boot test should build release, and prices the
    change as "slower build, faster boot". The boot half has always been
    measured to the second across hundreds of records; the build half was never
    measured at all, so for the entire life of that question one side of the
    comparison was evidence and the other was an assertion.

    READ THE RANGE, NOT THE MEDIAN. Unlike the wall-time populations, this one
    mixes three genuinely different things that the record cannot tell apart: a
    cold build of the whole dependency graph, an incremental rebuild after a
    one-line edit, and a no-op rebuild that compiled nothing. A median over that
    mixture describes no build anyone actually waits for. The bottom of the
    range is the no-op case and the top is the cold case, and the distance
    between them is the honest answer to "what does this profile cost me".
    """
    pops = build_populations(records)
    if not pops:
        return
    print("[boot-history] build time by profile:")
    for name in sorted(pops):
        vals = pops[name]
        print(f"[boot-history]   {name}: {len(vals)} build(s), "
              f"median {_median(vals):.0f}s, "
              f"range {min(vals):.0f}-{max(vals):.0f}s")
    print("[boot-history]   (read the range, not the median: this mixes cold, "
          "incremental and no-op rebuilds, which the record cannot tell apart. "
          "Runs that did not build are absent, not zero.)")


def report(records: list[dict], current: dict | None) -> None:
    if current is not None:
        verdict = current["verdict"]
        hits = current.get("fingerprints") or []
        why = VERDICT_HELP.get(verdict, "")
        print(f"[boot-history] {verdict}"
              + (f" -- {why}" if why else ""))
        # Named as the pair, matching `wall_populations`' key exactly, so the
        # line that says which population this boot is in and the block that
        # prints that population's median cannot disagree about the partition.
        print(f"[boot-history] build: {population_of(current)}")
        # The evidence, not just the label. `build/qemu-stderr.txt` is deleted
        # by the next run, so this line is the last chance to say what the host
        # actually printed -- and without it "HOST_FAIL" is an assertion the
        # reader has no way to check or to disbelieve.
        #
        # The advice depends on the verdict because the field does not imply
        # it. A host signature is recorded on any row that produced one,
        # including a boot that reached its marker anyway -- and telling someone
        # to re-run a boot that passed would be wrong, as well as teaching them
        # to ignore the line on the runs where it matters.
        host_why = current.get("host_fail")
        if host_why:
            if verdict == "HOST_FAIL":
                print(f"[boot-history] host failure: {host_why} "
                      f"-- excluded from the counts below; re-run this boot")
            else:
                # Not "but the kernel booted anyway". Since the harness's exit
                # status became a reason, this branch is also reached by a boot
                # that PANICKED and *then* had its harness die -- and telling
                # the reader that kernel booted anyway would be flatly untrue
                # on exactly the row where the panic needs believing.
                print(f"[boot-history] note: something outside the kernel also "
                      f"went wrong ({host_why}), but this verdict does not "
                      f"rest on it")
        if hits:
            print("[boot-history] matches known issue(s): " + ", ".join(hits))
            for fp in FINGERPRINTS:
                if fp.id in hits and fp.note:
                    print(f"[boot-history]   {fp.id}: {fp.note}")

    # Rows that are not evidence about the tree are set aside before anything is
    # counted, not filtered at each call site: the streak and the totals must
    # agree about what a boot is, and two separate filters are two chances to
    # disagree.
    tree = [r for r in records if describes_tree(r)]
    # Counted separately, and reported separately below, because they are
    # excluded for different reasons and a reader deciding whether to trust the
    # totals needs to know which. "3 boots excluded" would invite the guess that
    # someone had been running probes, when the truth might be that this machine
    # has run out of memory three times.
    probes = sum(1 for r in records if is_experiment(r))
    host_fails = sum(1 for r in records
                     if not is_experiment(r) and r.get("verdict") == "HOST_FAIL")

    clean = sum(1 for r in tree if r.get("verdict") in CLEAN_VERDICTS)
    print(f"[boot-history] {len(tree)} boot(s) recorded, {clean} clean "
          f"({len(tree) - clean} not)")
    if probes:
        print(f"[boot-history] {probes} experiment boot(s) excluded "
              f"(deliberate probes under non-default conditions; they say "
              f"nothing about the tree)")
    if host_fails:
        print(f"[boot-history] {host_fails} boot(s) excluded as host failures "
              f"(the machine running QEMU ran out of resources, or the harness "
              f"itself could not run; the kernel was not what failed)")

    print("[boot-history] current consecutive clean streak: "
          f"{tail_clean_streak(records)}")
    report_wall(records)
    report_build(records)


def cmd_streaks(history_path: str) -> int:
    records = load_history(history_path)
    print(f"boot-history: {display_path(history_path)} "
          f"({len(records)} record(s))")
    report(records, None)
    print()
    for st in streaks(records):
        for line in describe_streak(st):
            print(line)
        print()
    return 0


def cmd_list(history_path: str, limit: int) -> int:
    records = load_history(history_path)
    if not records:
        print(f"boot-history: no records in {display_path(history_path)}")
        return 0
    for rec in records[-limit:]:
        fps = ",".join(rec.get("fingerprints") or []) or "-"
        wall = rec.get("wall_seconds")
        wall_s = f"{wall:.0f}s" if isinstance(wall, (int, float)) else "-"
        # Abbreviated to keep the row one terminal line, but still three-valued:
        # `kasan`, `-` (kernel said "none"), `?` (nothing said). A row whose
        # duration looks wrong is almost always a row from the other build, and
        # this column is what lets you see that without opening the JSON.
        san = sanitizer_of(rec)
        san_s = {"kasan-instrumented": "kasan", "none": "-"}.get(san, "?")
        # Abbreviated on the same three-valued principle as the column beside
        # it, and present for the same reason: a duration that looks wrong is
        # almost always a row from the other *population*, and until this
        # column existed only half of that population was visible. `?` is a row
        # that cannot say, never a row assumed to be TCG.
        accel_s = {"QEMU TCG": "tcg", "Hyper-V/WHPX": "whpx",
                   "bare metal": "metal"}.get(accel_of(rec), "?")
        print(f"{rec.get('ts','?'):<26} {rec.get('commit','?'):<10} "
              f"{rec.get('verdict','?'):<17} {wall_s:>6} {san_s:<5} "
              f"{accel_s:<5} {rec.get('label','') or '-':<12} {fps}")
    return 0


def display_path(path: str) -> str:
    try:
        return os.path.relpath(path, REPO_ROOT).replace("\\", "/")
    except ValueError:
        return path


# --------------------------------------------------------------------------


def main(argv=None) -> int:
    parser = argparse.ArgumentParser(
        description="Record and summarise boot-test outcomes.")
    parser.add_argument("--serial", default=DEFAULT_SERIAL,
                        help="serial log to classify (default: %(default)s)")
    parser.add_argument("--history", default=DEFAULT_HISTORY,
                        help="JSON-lines history file")
    parser.add_argument("--exit-code", type=int, default=0,
                        help="boot-test.sh's exit status for this run")
    parser.add_argument("--marker", default="BOOT_OK",
                        help="the marker the harness waited for")
    parser.add_argument("--gated-markers", default=None, metavar="PATH",
                        help="JSON from `check-self-tests-wired.py "
                             "--emit-markers`. Each key is the serial line a "
                             "conditionally-called self-test prints when its "
                             "condition held; the row records which ones "
                             "appeared. Without it the `gated_ran` field is "
                             "omitted, not emptied -- absent means unknown, "
                             "and unknown must not read as all-clear.")
    parser.add_argument("--qemu-stderr", default=None, metavar="PATH",
                        help="the file QEMU's own stderr was redirected to. "
                             "Searched for host-side failure signatures -- a "
                             "pagefile that could not grow, a mapping QEMU "
                             "could not create -- which turn a verdict that "
                             "blames the tree into HOST_FAIL. It must be this "
                             "stream and not the serial log: the guest writes "
                             "the serial log, so a kernel that printed the "
                             "same words could otherwise excuse itself. "
                             "Without it no run is ever attributed to the "
                             "host, which is the safe default.")
    parser.add_argument("--wall-seconds", type=float, default=None)
    parser.add_argument("--build-seconds", type=float, default=None,
                        help="seconds cargo spent in Step 1, or omitted when "
                             "the run did not build (--no-build/--no-stage). "
                             "Omitted rather than zero on purpose: a run that "
                             "never built is not a run that built instantly, "
                             "and averaging the two would understate every "
                             "profile's cost. This is the half of "
                             "open-questions.md Q46's 'slower build, faster "
                             "boot' tradeoff that had never been measured.")
    parser.add_argument("--free-gb-min", type=int, default=None,
                        metavar="GIB",
                        help="the LOWEST free space seen on the tree's volume "
                             "during this run, in GiB. boot-test.sh already "
                             "measures this several times -- before the build, "
                             "before staging, before queueing to boot -- and "
                             "until now printed each reading and threw it "
                             "away. The minimum rather than the last reading, "
                             "because the question this answers is 'was the "
                             "host short of disk at the worst moment of this "
                             "run', and the worst moment is not usually the "
                             "final one. See --free-gb-phase.")
    parser.add_argument("--free-gb-phase", default="",
                        metavar="PHASE",
                        help="which check produced --free-gb-min, in "
                             "boot-test.sh's own words ('before building', "
                             "'after building, before queueing to boot'). "
                             "Without it the number cannot be acted on: 12 GiB "
                             "before a build and 12 GiB after one are opposite "
                             "situations.")
    parser.add_argument("--label", default="",
                        help="free-form run tag, e.g. 'soak-iter3'")
    parser.add_argument("--experiment", default="",
                        help="why this boot is a deliberate probe rather than "
                             "a boot of the tree (non-default emulator flags, "
                             "a hand-patched kernel). Recorded, then excluded "
                             "from the clean streak and the wall-time medians. "
                             "boot-test.sh sets this automatically whenever "
                             "QEMU_EXTRA or BENCH_EXPERIMENT is set.")
    parser.add_argument("--profile", default="debug")
    parser.add_argument("--commit", default="",
                        help="commit the tested kernel was built from; pass "
                             "the value read BEFORE the build, since HEAD can "
                             "move during a run (default: ask git now)")
    parser.add_argument("--src-digest", default="",
                        help="identity of the source that was built, from "
                             "scripts/src_digest.py; covers the untracked "
                             "binaries the kernel embeds, which `commit` and "
                             "`dirty` between them cannot see")
    parser.add_argument("--branch", default="",
                        help="branch the tested kernel was built from "
                             "(default: ask git now)")
    parser.add_argument("--dirty", action="store_true",
                        help="the tree had uncommitted changes at build time, "
                             "so --commit names an ancestor of what ran")
    parser.add_argument("--no-record", action="store_true",
                        help="classify and report, write nothing")
    parser.add_argument("--classify", action="store_true",
                        help="print the verdict alone and exit")
    parser.add_argument("--list", action="store_true",
                        help="print recent records")
    parser.add_argument("--streaks", action="store_true",
                        help="print per-fingerprint standing")
    parser.add_argument("--limit", type=int, default=25,
                        help="records shown by --list (default: %(default)s)")
    args = parser.parse_args(argv)

    if args.streaks:
        return cmd_streaks(args.history)
    if args.list:
        return cmd_list(args.history, args.limit)

    serial = read_serial(args.serial, args.marker)
    # Read once, used twice: the verdict and the recorded reason must be two
    # views of one piece of evidence, not two readings of one file.
    qemu_stderr = read_qemu_stderr(args.qemu_stderr)
    host_fail = not_about_the_tree(qemu_stderr, args.exit_code)
    verdict = classify(serial, args.exit_code, qemu_stderr)

    if args.classify:
        print(verdict)
        return 0

    if verdict == "NO_BOOT":
        # Not a boot outcome: the build failed, or the harness died before
        # QEMU wrote anything. Recording it would put build breakage into a
        # series that exists to measure kernel behaviour, and would reset every
        # hang streak on every compile error.
        #
        # A host failure does not change that -- there is still no boot to
        # record -- but it does change what the operator should conclude, so it
        # is said out loud rather than left inside the guess below. This is the
        # one place a host failure is reported without a row behind it.
        print("[boot-history] no serial output -- nothing to record "
              "(build or harness failure, not a boot outcome)")
        if host_fail:
            # Not "qemu's stderr says why": the reason may equally have come
            # from the harness's own exit status, and naming the wrong source
            # sends whoever reads this line to a file that will not contain it.
            print(f"[boot-history] and it was not the tree's doing: {host_fail}")
        return 0

    record = build_record(serial, verdict, args, host_fail=host_fail)
    history = load_history(args.history)

    if not args.no_record:
        if not append_record(args.history, record):
            return 1
        history.append(record)

    report(history, record)
    return 0


if __name__ == "__main__":
    sys.exit(main())
