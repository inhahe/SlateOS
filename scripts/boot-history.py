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
    benign = tuple(e for e in all_exc if _BENIGN_EXCEPTION_RE.search(e))
    fatal = tuple(e for e in all_exc if e not in benign)
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
    )


# --------------------------------------------------------------------------
# Verdict
# --------------------------------------------------------------------------

#: Verdicts that mean the kernel got where it was going. Only these extend a
#: clean streak; everything else is a recurrence candidate.
CLEAN_VERDICTS = frozenset({"PASS", "PASS_TOOLING", "BENCH_INCOMPLETE"})

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
    rec: dict = {
        "ts": _now_iso(),
        "commit": git_commit(),
        "branch": git_branch(),
        "host": socket.gethostname(),
        "os": platform.system(),
        "verdict": verdict,
        "exit_code": args.exit_code,
        "marker": args.marker,
        "label": args.label,
        "profile": args.profile,
    }
    if args.wall_seconds is not None:
        rec["wall_seconds"] = args.wall_seconds
    if serial is not None:
        rec["serial_bytes"] = serial.n_bytes
        rec["serial_lines"] = len(serial.lines)
        rec["ends_mid_line"] = serial.ends_mid_line
        rec["boot_ok"] = serial.boot_ok
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
    """
    out = []
    for fp in FINGERPRINTS:
        st = Streak(fp=fp, recorded=len(records))
        for rec in records:
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


def report(records: list[dict], current: dict | None) -> None:
    if current is not None:
        verdict = current["verdict"]
        hits = current.get("fingerprints") or []
        why = VERDICT_HELP.get(verdict, "")
        print(f"[boot-history] {verdict}"
              + (f" -- {why}" if why else ""))
        if hits:
            print("[boot-history] matches known issue(s): " + ", ".join(hits))
            for fp in FINGERPRINTS:
                if fp.id in hits and fp.note:
                    print(f"[boot-history]   {fp.id}: {fp.note}")

    clean = sum(1 for r in records if r.get("verdict") in CLEAN_VERDICTS)
    print(f"[boot-history] {len(records)} boot(s) recorded, {clean} clean "
          f"({len(records) - clean} not)")

    tail_clean = 0
    for rec in reversed(records):
        if rec.get("verdict") in CLEAN_VERDICTS:
            tail_clean += 1
        else:
            break
    print(f"[boot-history] current consecutive clean streak: {tail_clean}")


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
        print(f"{rec.get('ts','?'):<26} {rec.get('commit','?'):<10} "
              f"{rec.get('verdict','?'):<17} {wall_s:>6}  "
              f"{rec.get('label','') or '-':<12} {fps}")
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
    parser.add_argument("--label", default="",
                        help="free-form run tag, e.g. 'soak-iter3'")
    parser.add_argument("--profile", default="debug")
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
