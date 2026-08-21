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
   indistinguishable from the number alone. This is the same rule
   `stamp-ancestry.py` follows when a declared source path does not exist:
   *could not verify* must never render as *fine*.

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
import importlib.util
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
REPO_ROOT = os.path.dirname(SCRIPT_DIR)
DEFAULT_SERIAL = os.path.join(REPO_ROOT, "build", "serial-test.txt")
DEFAULT_HISTORY = os.path.join(REPO_ROOT, "bench", "boot-history.jsonl")
BENCH_HISTORY = os.path.join(SCRIPT_DIR, "bench-history.py")

_BENCH_HISTORY_MODULE = None


def bench_history():
    """Import `bench-history.py` by path; its name is not an identifier.

    Cached because the module compiles several dozen regexes at import and the
    tests parse many logs, while a real run parses one.
    """
    global _BENCH_HISTORY_MODULE
    if _BENCH_HISTORY_MODULE is None:
        spec = importlib.util.spec_from_file_location("bench_history",
                                                      BENCH_HISTORY)
        if spec is None or spec.loader is None:
            raise RuntimeError(f"cannot load {BENCH_HISTORY}")
        module = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(module)
        _BENCH_HISTORY_MODULE = module
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

VERDICT_HELP = {
    "PASS": "marker reached, every gate green",
    "PASS_TOOLING": "kernel booted; the harness failed to produce an artefact",
    "BENCH_INCOMPLETE": "BOOT_OK reached, BENCH_OK did not (known bench livelock)",
    "SELFTEST_FAIL": "marker reached but a self-test / liveness gate failed",
    "PANIC": "kernel died (PANIC / FATAL in the serial log)",
    "WEDGE": "serial output stalled; kernel stopped progressing",
    "TIMEOUT": "marker never arrived, no panic, no stall detected",
}


def classify(serial: Serial | None, exit_code: int) -> str:
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
    if serial is None or verdict in CLEAN_VERDICTS:
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
    """Read the log, skipping records that fail to parse.

    A corrupt line must not destroy the rest: this file is appended to by every
    boot and is the only longitudinal record of outcomes we have, so partial
    recovery beats an exception. (Same rule as bench-history.py's loader --
    and the same reason: the file is written concurrently by three lanes'
    worktrees and merged as text.)
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
                if isinstance(rec, dict):
                    records.append(rec)
    except FileNotFoundError:
        return []
    except OSError as exc:
        print(f"boot-history: cannot read {path}: {exc}", file=sys.stderr)
        return []
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


def build_record(serial: Serial | None, verdict: str, args) -> dict:
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
    if args.wall_seconds is not None:
        rec["wall_seconds"] = args.wall_seconds
    if args.build_seconds is not None:
        rec["build_seconds"] = args.build_seconds
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
    """
    tree = [r for r in records if not is_experiment(r)]
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


def population_of(rec: dict) -> str:
    """The full label of the population a boot's duration belongs to.

    A wall time is a property of the *pair* (build, accelerator) -- KASAN costs
    ~3.4x and the accelerator ~1.4x on this host -- and neither factor makes the
    other irrelevant, so the population is the pair and not either half. Kept as
    one function rather than composed at each call site so that the printed
    label and the grouping key cannot drift: a legend that names a different
    partition from the one the numbers were computed over is worse than no
    legend, because it is believed.
    """
    return f"{sanitizer_of(rec)} on {accel_of(rec)}"


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
    """
    out: dict[str, list[float]] = {}
    for rec in records:
        if is_experiment(rec):
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
    """
    streak = 0
    for rec in reversed(records):
        if is_experiment(rec):
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
    print("[boot-history] wall time by build and accelerator:")
    for name in sorted(pops):
        vals = pops[name]
        print(f"[boot-history]   {name}: {len(vals)} boot(s), "
              f"median {_median(vals):.0f}s, "
              f"range {min(vals):.0f}-{max(vals):.0f}s")
    if len(pops) > 1:
        print("[boot-history]   (reported separately on purpose: a "
              "KASAN-instrumented boot runs several times longer and a "
              "hardware-virtualised one ~40% longer again on this host, so "
              "one median over the mixture describes no build that exists)")


def build_populations(records: list[dict]) -> dict[str, list[float]]:
    """Build seconds grouped by profile and sanitizer -- NOT by accelerator.

    The partition differs from `wall_populations`' on purpose. What the guest is
    executed by cannot change how long the host spent compiling, so folding the
    accelerator in here would split each profile into two or three populations
    that differ in nothing and shrink every sample for no gain. What *does*
    change a build's cost is the profile (`opt-level = 3, codegen-units = 1` is
    not a cheap build) and the sanitizer (KASAN instruments every memory
    access), so those are the two axes.

    Experiment boots are excluded on the same rule as everywhere else in this
    file, and runs that never built are absent rather than zero -- see
    `--build-seconds`.
    """
    out: dict[str, list[float]] = {}
    for rec in records:
        if is_experiment(rec):
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
        if hits:
            print("[boot-history] matches known issue(s): " + ", ".join(hits))
            for fp in FINGERPRINTS:
                if fp.id in hits and fp.note:
                    print(f"[boot-history]   {fp.id}: {fp.note}")

    # Probes are set aside before anything is counted, not filtered at each
    # call site: the streak and the totals must agree about what a boot is, and
    # two separate filters are two chances to disagree.
    tree = [r for r in records if not is_experiment(r)]
    probes = len(records) - len(tree)

    clean = sum(1 for r in tree if r.get("verdict") in CLEAN_VERDICTS)
    print(f"[boot-history] {len(tree)} boot(s) recorded, {clean} clean "
          f"({len(tree) - clean} not)")
    if probes:
        print(f"[boot-history] {probes} experiment boot(s) excluded "
              f"(deliberate probes under non-default conditions; they say "
              f"nothing about the tree)")

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
    verdict = classify(serial, args.exit_code)

    if args.classify:
        print(verdict)
        return 0

    if verdict == "NO_BOOT":
        # Not a boot outcome: the build failed, or the harness died before
        # QEMU wrote anything. Recording it would put build breakage into a
        # series that exists to measure kernel behaviour, and would reset every
        # hang streak on every compile error.
        print("[boot-history] no serial output -- nothing to record "
              "(build or harness failure, not a boot outcome)")
        return 0

    record = build_record(serial, verdict, args)
    history = load_history(args.history)

    if not args.no_record:
        if not append_record(args.history, record):
            return 1
        history.append(record)

    report(history, record)
    return 0


if __name__ == "__main__":
    sys.exit(main())
