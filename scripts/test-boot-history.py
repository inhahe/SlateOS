#!/usr/bin/env python3
"""Regression tests for `scripts/boot-history.py`.

Run: `python scripts/test-boot-history.py` (exit 0 = pass, 1 = fail).
No pytest dependency, for the same reason `test-bench-history.py` has none:
this must run from a bare checkout.

Why this file exists, and why it is the *validation* of the fingerprints
------------------------------------------------------------------------

`boot-history.py`'s fingerprints declare a `validated_by` list -- the real
occurrences they claim to match. That claim is worthless as a comment: the
whole hazard the field exists to guard against is a matcher that cannot fire,
and a comment saying "this matches the 2026-06-12 hang" cannot fire either.

So the serial samples below are **reconstructed verbatim from the evidence
quoted in `known-issues.md`** for each occurrence named in `validated_by`, and
each is asserted to match its own fingerprint *and no other*. If a refactor
breaks a matcher, this suite fails; without it, the only symptom would be a
beautifully clean streak that closes an open kernel bug on no evidence.

The mutual-exclusion half matters as much as the positive half. Three of these
fingerprints describe hangs that all end with "the log stops", and the
distinctions between them (mid-line vs between-lines; exception present vs
absent; where the cut falls) are the entire diagnostic content. A matcher that
claims every silent hang would reset every streak on every hang and tell the
reader nothing about which one they hit.
"""

from __future__ import annotations

import importlib.util
import inspect
import json
import os
import sys
import tempfile

REPO_ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
SCRIPT = os.path.join(REPO_ROOT, "scripts", "boot-history.py")

_FAILURES: list[str] = []


def load_module():
    """Import boot-history.py by path (its name is not a valid identifier).

    Registered in `sys.modules` before execution: `@dataclass` resolves the
    defining module by name while the class body runs, and an unregistered
    module makes that lookup return None.
    """
    spec = importlib.util.spec_from_file_location("boot_history", SCRIPT)
    if spec is None or spec.loader is None:
        raise RuntimeError(f"cannot load {SCRIPT}")
    module = importlib.util.module_from_spec(spec)
    sys.modules["boot_history"] = module
    spec.loader.exec_module(module)
    return module


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
# Reconstructed serial samples
#
# Each is quoted from the known-issues entry for the occurrence it stands for.
# They are short: only the discriminating lines matter, and a full 900-line
# boot log in a test file would hide which line is load-bearing.
# --------------------------------------------------------------------------

_PROLOGUE = (
    "[boot] SlateOS kernel starting\n"
    "[mm] physical memory manager online\n"
    "[sched] scheduler online\n"
)

#: The `EXCEPTION:` line that EVERY green boot prints, verbatim from a live
#: serial log on 2026-08-16. A ring-3 self-test raises it on purpose.
#:
#: It is in the passing sample deliberately. Treating it as a fault broke the
#: classifier in both directions at once -- every non-panic failure became a
#: PANIC, and W1 (which requires no exception anywhere) could never match
#: again -- and nothing in the synthetic samples would have shown it.
S_DELIBERATE_UD = (
    "EXCEPTION: Invalid Opcode (#UD) at 0x4000000011 in userspace "
    "(deliberate compiler trap)\n"
)

#: A clean boot.
S_PASS = _PROLOGUE + S_DELIBERATE_UD + (
    "[oom]   Callback registration and invocation: OK\n"
    "[shell] prompt ready\n"
    "BOOT_OK\n"
)

#: W1, 2026-06-10 and 2026-06-12. "serial output truncated mid-line at
#: `[sysctl] mm.oom_pol...` during `mm::oom::self_test()`". No trailing
#: newline: that is the fingerprint, not the sysctl name.
S_W1 = _PROLOGUE + (
    "[oom] Test 2: handle_oom selects the fattest task: OK\n"
    "[sysctl] mm.oom_pol"
)

#: The KASAN wedge, 2026-08-12: the cut lands exactly where `{:#x}` would
#: have formatted `frame.rip`.
S_KASAN = _PROLOGUE + (
    "[kasan] shadow memory mapped\n"
    "EXCEPTION: Page Fault (#PF) at "
)

#: B-PTHREAD-TEARDOWN-PF, 2026-08-13.
S_PTHREAD = _PROLOGUE + (
    "EXCEPTION: Page Fault (#PF) at 0xffffffff82713dc2, address=0x97, error=0x0\n"
    "  Cause: not-present, read, kernel\n"
    "  Task: 123 (\"cloned-thread\")\n"
    "PANIC: unhandled page fault in kernel mode\n"
)

#: The IDT self-test breakpoints, verbatim from the KASAN boot of 2026-08-19
#: that this pair of samples exists to keep from being misread again.
#:
#: They are UNannotated on purpose: this is the text the kernel emitted before
#: `ExpectedBreakpoint` was added, and it is what proves the host-side guard
#: stands on its own rather than merely agreeing with the kernel-side one.
S_SELFTEST_BP_UNANNOTATED = (
    "[idt] Running direction-flag self-test...\n"
    "EXCEPTION: Breakpoint (#BP) at 0xffffffff813b56b6\n"
    "[idt]   DF is clear on exception entry: OK\n"
    "EXCEPTION: Breakpoint (#BP) at 0xffffffff813b56e7\n"
    "[idt]   iretq restores the caller's DF: OK\n"
    "[idt] Direction-flag self-test PASSED\n"
)

#: The same three lines as the kernel emits them now.
S_SELFTEST_BP_ANNOTATED = S_SELFTEST_BP_UNANNOTATED.replace(
    "(#BP) at 0xffffffff813b56b6\n",
    "(#BP) at 0xffffffff813b56b6 (deliberate self-test)\n",
).replace(
    "(#BP) at 0xffffffff813b56e7\n",
    "(#BP) at 0xffffffff813b56e7 (deliberate self-test)\n",
)

#: B-FORKEXEC-BOOT-HANG, 2026-06-12: a quiet stop *between* lines, right
#: after the last thread of a process is reaped. No exception, no panic.
S_FORKEXEC = _PROLOGUE + (
    "[exec] replacing image for pid 41\n"
    "[thread] Process 41 has no threads left - now zombie\n"
    "[thread] Process 42 has no threads left - now zombie\n"
    "[sched] Task 130 exiting\n"
)

#: W-KERNEL-COW-WRITE: write-to-present fault against a user mapping.
S_COW = _PROLOGUE + (
    "EXCEPTION: Page Fault (#PF) at 0xffffffff8020a110, "
    "address=0x6000213450, error=0x3\n"
    "  Cause: protection-violation, write, kernel\n"
    "PANIC: kernel wrote a shared CoW page\n"
)

#: A self-test failure: marker reached, gate red.
S_SELFTEST = _PROLOGUE + (
    "[vfs] SELF-TEST FAILED: rename across directories\n"
    "BOOT_OK\n"
)

#: The livelock diagnostic that makes anchoring load-bearing. Contains the
#: substring BOOT_OK, but never prints it as a line.
S_LIVELOCK = _PROLOGUE + (
    "[watchdog] timer still armed 200s after arming (no BOOT_OK yet)\n"
    "[watchdog] still armed\n"
)


#: The build-profile banner, both values, as kernel/src/main.rs prints it
#: immediately after "=== Kernel booting ===".
#:
#: Note none of the samples above carry it. That is deliberate and load-bearing:
#: every occurrence in every `validated_by` list predates the banner, so the
#: pre-banner samples are the *real* evidence and must keep matching. If adding
#: the banner to `_PROLOGUE` had been the easy way to make these tests pass,
#: that would have been the bug -- see `test_kasan_fp_still_matches_a_pre_banner_log`.
S_BANNER_KASAN = "[boot] build profile: sanitizer=kasan-instrumented\n"
S_BANNER_NONE = "[boot] build profile: sanitizer=none\n"


def _serial(bh, text, marker="BOOT_OK"):
    """Parse a sample without touching the filesystem."""
    with tempfile.NamedTemporaryFile("wb", suffix=".txt", delete=False) as fh:
        fh.write(text.encode("utf-8"))
        path = fh.name
    try:
        return bh.read_serial(path, marker)
    finally:
        os.unlink(path)


def _fps(bh, text, exit_code, marker="BOOT_OK"):
    s = _serial(bh, text, marker)
    verdict = bh.classify(s, exit_code)
    return verdict, bh.fingerprints_for(s, verdict)


# --------------------------------------------------------------------------
# Classification
# --------------------------------------------------------------------------


def test_pass(bh):
    check("classify: clean boot -> PASS", _fps(bh, S_PASS, 0)[0], "PASS")


def test_pass_tooling(bh):
    check("classify: marker + exit 3 -> PASS_TOOLING",
          _fps(bh, S_PASS, 3)[0], "PASS_TOOLING")


def test_selftest_fail(bh):
    check("classify: marker + exit 1 -> SELFTEST_FAIL",
          _fps(bh, S_SELFTEST, 1)[0], "SELFTEST_FAIL")


def test_wedge(bh):
    check("classify: no marker + exit 2 -> WEDGE", _fps(bh, S_W1, 2)[0], "WEDGE")


def test_timeout(bh):
    check("classify: no marker + exit 1 -> TIMEOUT",
          _fps(bh, S_FORKEXEC, 1)[0], "TIMEOUT")


def test_panic(bh):
    check("classify: PANIC line beats exit 1 -> PANIC",
          _fps(bh, S_PTHREAD, 1)[0], "PANIC")


def test_no_boot(bh):
    check("classify: missing serial -> NO_BOOT", bh.classify(None, 1), "NO_BOOT")


def test_empty_serial_is_no_boot(bh):
    s = _serial(bh, "   \n\n")
    check("classify: whitespace-only serial -> NO_BOOT",
          bh.classify(s, 1), "NO_BOOT")


def test_bench_incomplete(bh):
    """BOOT_OK reached, BENCH_OK not: the documented bench livelock.

    This must NOT read as a hang. Counting it as one would reset every hang
    streak on every `--bench` run -- and `--bench` runs are the ones we do
    most, so the streaks would never grow.
    """
    verdict, fps = _fps(bh, S_PASS, 1, marker="BENCH_OK")
    check("classify: BOOT_OK without BENCH_OK -> BENCH_INCOMPLETE",
          verdict, "BENCH_INCOMPLETE")
    check("bench livelock matches no hang fingerprint", fps, [])
    check_true("BENCH_INCOMPLETE counts as clean",
               "BENCH_INCOMPLETE" in bh.CLEAN_VERDICTS)


def test_marker_is_anchored(bh):
    """The livelock diagnostic contains the substring BOOT_OK.

    boot-test.sh anchors its own grep for exactly this reason and says so.
    An unanchored match here would call a hung boot a pass -- the most
    expensive wrong answer this script can give, because it is silent.
    """
    s = _serial(bh, S_LIVELOCK)
    check("anchored marker: 'no BOOT_OK yet' is not a pass", s.boot_ok, False)
    check("anchored marker: livelock classifies as TIMEOUT",
          bh.classify(s, 1), "TIMEOUT")


def test_crlf_normalised(bh):
    """QEMU's serial is CRLF on Windows; `\\r` must not defeat the anchor."""
    s = _serial(bh, S_PASS.replace("\n", "\r\n"))
    check("CRLF serial still reaches the marker", s.boot_ok, True)
    check("CRLF serial does not read as mid-line", s.ends_mid_line, False)


def test_undecodable_bytes_do_not_lose_the_record(bh):
    """A wedged UART can cut a multi-byte sequence at the stall point.

    Losing the record for the one run we most want recorded would be the
    worst possible time to raise.
    """
    with tempfile.NamedTemporaryFile("wb", suffix=".txt", delete=False) as fh:
        fh.write(b"[boot] starting\n[sysctl] mm.oom_pol\xe2\x80")
        path = fh.name
    try:
        s = bh.read_serial(path, "BOOT_OK")
    finally:
        os.unlink(path)
    check_true("truncated UTF-8 still parses", s is not None)
    check("truncated UTF-8 reads as mid-line", s.ends_mid_line, True)


# --------------------------------------------------------------------------
# Fingerprints -- the positive half
# --------------------------------------------------------------------------


def test_deliberate_exception_is_not_a_fault(bh):
    """Regression: found against a live serial log, not against a sample.

    Every green boot prints a deliberate #UD from a ring-3 self-test. The first
    version of the classifier called any `EXCEPTION:` line a fault, which meant
    a real boot classified as PANIC while still printing self-test results.
    """
    s = _serial(bh, S_PASS)
    check("the deliberate #UD is not counted as a fault", s.exceptions, ())
    check("it is retained as benign", len(s.benign_exceptions), 1)
    check("a clean boot with it still classifies as PASS",
          bh.classify(s, 0), "PASS")


def test_w1_survives_a_deliberate_exception_before_the_wedge(bh):
    """The ordering that makes this more than cosmetic.

    Today `[sysctl] mm.oom_pol...` prints at serial line ~376 and the
    deliberate #UD at ~1353, so a W1 truncation happens to cut before the #UD
    ever appears. That is luck, not design: move one self-test earlier and a
    fingerprint keyed on "no exception anywhere" would silently stop matching
    forever -- reported, of course, as a beautiful clean streak.
    """
    s_text = _PROLOGUE + S_DELIBERATE_UD + (
        "[oom] Test 2: handle_oom selects the fattest task: OK\n"
        "[sysctl] mm.oom_pol"
    )
    _, fps = _fps(bh, s_text, 2)
    check("W1 still matches with a deliberate exception earlier in the log",
          fps, ["W1"])


def test_non_deliberate_exception_still_reads_as_a_fault(bh):
    """The filter must not become a blanket suppression of EXCEPTION lines."""
    s = _serial(bh, S_PTHREAD)
    check("a kernel-mode page fault is still a fault", len(s.exceptions), 1)
    check("and is not filed as benign", s.benign_exceptions, ())


def test_selftest_breakpoints_do_not_read_as_a_kernel_death(bh):
    """Regression, 2026-08-19: a slow boot reported as PANIC.

    A KASAN-instrumented boot ran past its 900s budget while still printing --
    27 841 lines, no PANIC and no FATAL text anywhere -- and was recorded as
    "PANIC: kernel died". The evidence the classifier acted on was three
    `EXCEPTION: Breakpoint (#BP)` lines from the IDT self-tests, which every
    boot prints and which the benign filter did not know about.

    What made it survive so long is worth stating, because it is the shape of
    the next bug of this kind: `classify()` looks at exceptions only when the
    marker was never reached, so on every green boot the mislabelling is
    unreachable, and it fires exactly on the failed boot whose verdict someone
    is relying on.
    """
    s_text = _PROLOGUE + S_SELFTEST_BP_UNANNOTATED + "[shell] prompt ready\n"
    s = _serial(bh, s_text)
    check("an unannotated self-test #BP is not evidence of a death",
          s.exceptions, ())
    check("but it is still retained and reportable",
          len(s.benign_exceptions), 2)
    check("so a boot that merely ran out of clock reads as TIMEOUT",
          bh.classify(s, 1), "TIMEOUT")


def test_annotated_breakpoints_are_benign_by_the_kernels_own_word(bh):
    """The kernel-side half: `ExpectedBreakpoint` marks its own `int3`s.

    Checked separately from the host-side vector rule so that neither can
    quietly become the only thing holding the verdict up. If the `(#BP)` rule
    were deleted tomorrow this test would still pass, and vice versa.
    """
    s_text = _PROLOGUE + S_SELFTEST_BP_ANNOTATED + "[shell] prompt ready\n"
    s = _serial(bh, s_text)
    check("the annotation alone makes them benign", s.exceptions, ())
    check("both are retained", len(s.benign_exceptions), 2)
    check("and the kernel says so in the log itself",
          all("deliberate self-test" in e for e in s.benign_exceptions), True)


def test_a_breakpoint_is_never_fatal_but_a_page_fault_still_is(bh):
    """The vector rule must be about the vector, not about breakpoints in general.

    The danger in exempting a vector is exempting too much: a rule keyed on
    "an EXCEPTION line near a self-test" would swallow a real #PF that happened
    to land in the same neighbourhood.
    """
    s_text = _PROLOGUE + S_SELFTEST_BP_UNANNOTATED + (
        "EXCEPTION: Page Fault (#PF) at 0xffffffff82713dc2, address=0x97, error=0x0\n"
        "  Cause: not-present, read, kernel\n"
    )
    s = _serial(bh, s_text)
    check("the #PF alongside them is still a fault", len(s.exceptions), 1)
    check("and it is the #PF, not a #BP",
          "#PF" in s.exceptions[0] if s.exceptions else False, True)
    check("so the run still classifies as PANIC", bh.classify(s, 1), "PANIC")


def test_fp_w1(bh):
    verdict, fps = _fps(bh, S_W1, 2)
    check("W1 sample matches W1", fps, ["W1"])
    check("W1 sample classifies as WEDGE", verdict, "WEDGE")


def test_fp_kasan(bh):
    _, fps = _fps(bh, S_KASAN, 2)
    check("KASAN sample matches the KASAN fingerprint alone",
          fps, ["B-KASAN-INSTRUMENTED-BOOT-WEDGES-MID-PRINT-ON-A-PAGE-FAULT"])


def test_fp_pthread(bh):
    _, fps = _fps(bh, S_PTHREAD, 1)
    check("pthread teardown sample matches B-PTHREAD-TEARDOWN-PF",
          fps, ["B-PTHREAD-TEARDOWN-PF"])


def test_fp_forkexec(bh):
    _, fps = _fps(bh, S_FORKEXEC, 1)
    check("fork/exec hang sample matches B-FORKEXEC-BOOT-HANG",
          fps, ["B-FORKEXEC-BOOT-HANG"])


def test_fp_cow(bh):
    _, fps = _fps(bh, S_COW, 1)
    check("CoW write sample matches W-KERNEL-COW-WRITE",
          fps, ["W-KERNEL-COW-WRITE"])


def test_every_validated_fingerprint_has_a_sample(bh):
    """Discovery floor, in the shape this project keeps rediscovering.

    A fingerprint that claims `validated_by` occurrences but has no sample in
    this file is validated by nothing -- and would report a clean streak
    forever. Assert that the set of fingerprints exercised above is the full
    set of validated ones, so adding a fingerprint without a sample fails the
    suite instead of quietly disarming it.
    """
    exercised = {
        "W1",
        "B-KASAN-INSTRUMENTED-BOOT-WEDGES-MID-PRINT-ON-A-PAGE-FAULT",
        "B-PTHREAD-TEARDOWN-PF",
        "B-FORKEXEC-BOOT-HANG",
        "W-KERNEL-COW-WRITE",
    }
    claimed = {fp.id for fp in bh.FINGERPRINTS if fp.validated_by}
    check("every fingerprint claiming validation has a sample here",
          sorted(claimed - exercised), [])


# --------------------------------------------------------------------------
# Fingerprints -- the mutual-exclusion half
# --------------------------------------------------------------------------


def test_clean_boot_matches_nothing(bh):
    check("a clean boot matches no fingerprint", _fps(bh, S_PASS, 0)[1], [])


def test_selftest_failure_matches_no_hang(bh):
    """A red self-test is a failure, but not any of these hangs.

    Without this, every unrelated test regression would reset every streak.
    """
    check("a self-test failure matches no hang fingerprint",
          _fps(bh, S_SELFTEST, 1)[1], [])


def test_w1_requires_no_exception(bh):
    """W1's whole retargeted meaning is *silence*.

    Per known-issues' 2026-08-14 analysis, the console-lock fixes mean a
    re-entry now prints instead of spinning. A W1 that also matched a hang
    *with* diagnostics would fire on every future occurrence of anything and
    the falsification test would be lost.
    """
    _, fps = _fps(bh, S_KASAN, 2)
    check("W1 does not claim the mid-print-with-exception wedge",
          "W1" in fps, False)
    _, fps2 = _fps(bh, S_PTHREAD, 1)
    check("W1 does not claim a page fault", "W1" in fps2, False)


def test_w1_requires_mid_line(bh):
    """The between-lines hang is B-FORKEXEC's, not W1's.

    Mid-line vs between-lines is the discriminator the analysis rests on: the
    UART is synchronous at ~87us/char, so an unrelated wedge stops between
    lines with the in-flight line flushed.
    """
    _, fps = _fps(bh, S_FORKEXEC, 2)
    check("a between-lines hang is not W1", "W1" in fps, False)


def test_forkexec_requires_the_zombie_lines(bh):
    """A generic between-lines timeout must not be claimed by B-FORKEXEC."""
    generic = _PROLOGUE + "[net] bringing up loopback\n"
    _, fps = _fps(bh, generic, 1)
    check("a generic quiet timeout matches nothing", fps, [])


def test_forkexec_zombie_lines_must_be_recent(bh):
    """The zombie lines must be at the *stop point*, not anywhere in the log.

    Every boot reaps processes; matching them anywhere would make this
    fingerprint fire on every hang in the tree.
    """
    stale = _PROLOGUE + (
        "[thread] Process 41 has no threads left - now zombie\n"
        "[init] starting service manager\n"
        "[vfs] mounting rootfs\n"
        "[net] bringing up loopback\n"
        "[gui] compositor online\n"
        "[shell] spawning login\n"
        "[usb] enumerating\n"
    )
    _, fps = _fps(bh, stale, 1)
    check("zombie lines far from the stop point do not match", fps, [])


def test_cow_requires_a_user_address(bh):
    """error=0x3 against a *kernel* address is a different bug entirely."""
    kern = _PROLOGUE + (
        "EXCEPTION: Page Fault (#PF) at 0xffffffff8020a110, "
        "address=0xffff800000012340, error=0x3\n"
    )
    _, fps = _fps(bh, kern, 1)
    check("a kernel-address write fault is not W-KERNEL-COW-WRITE",
          "W-KERNEL-COW-WRITE" in fps, False)


def test_pthread_not_matched_by_a_high_address_fault(bh):
    """The fingerprint is the small offset, not merely 'a page fault'."""
    other = _PROLOGUE + (
        "EXCEPTION: Page Fault (#PF) at 0xffffffff82713dc2, "
        "address=0xdeadbeef, error=0x0\n"
        "  Task: 123 (\"cloned-thread\")\n"
    )
    _, fps = _fps(bh, other, 1)
    check("a high-address fault is not B-PTHREAD-TEARDOWN-PF",
          "B-PTHREAD-TEARDOWN-PF" in fps, False)


def test_pthread_is_not_rip_keyed(bh):
    """The same fault at a different RIP must still match.

    The RIP moves with every kernel rebuild. A RIP-keyed fingerprint stops
    matching after any recompilation -- and a streak that resets to clean on
    recompilation is worse than no streak, because it looks like progress.
    """
    moved = S_PTHREAD.replace("0xffffffff82713dc2", "0xffffffff81044c01")
    _, fps = _fps(bh, moved, 1)
    check("B-PTHREAD-TEARDOWN-PF survives a moved RIP",
          fps, ["B-PTHREAD-TEARDOWN-PF"])


def test_fingerprints_skipped_on_clean_verdicts(bh):
    """Cheap, and prevents a matcher bug from resetting a streak on a pass."""
    s = _serial(bh, S_PASS)
    check("no fingerprinting on PASS", bh.fingerprints_for(s, "PASS"), [])


def test_a_raising_fingerprint_does_not_lose_the_record(bh):
    """The record is the durable artefact; a fingerprint is an opinion on it."""
    def boom(_s, _v):
        raise ValueError("deliberate")

    bad = bh.Fingerprint(id="BOOM", title="explodes", match=boom,
                         validated_by=("never",))
    saved = bh.FINGERPRINTS
    try:
        bh.FINGERPRINTS = saved + (bad,)
        _, fps = _fps(bh, S_W1, 2)
        check("a raising fingerprint is skipped, others still report",
              fps, ["W1"])
    finally:
        bh.FINGERPRINTS = saved


# --------------------------------------------------------------------------
# History file
# --------------------------------------------------------------------------


def test_roundtrip(bh, tmpdir):
    hist = os.path.join(tmpdir, "sub", "boot-history.jsonl")
    rec = {"ts": "2026-08-16T00:00:00+00:00", "verdict": "PASS", "commit": "abc"}
    check_true("append creates the directory", bh.append_record(hist, rec))
    check("append/load roundtrip", bh.load_history(hist), [rec])


def test_malformed_line_is_skipped_not_fatal(bh, tmpdir):
    """A corrupt line must not destroy the rest.

    This file is appended to from three worktrees and merged as text; a loader
    that raises on one bad line loses every outcome ever recorded.
    """
    hist = os.path.join(tmpdir, "h.jsonl")
    with open(hist, "w", encoding="utf-8", newline="\n") as fh:
        fh.write('{"verdict": "PASS"}\n')
        fh.write("{not json\n")
        fh.write("\n")
        fh.write('{"verdict": "WEDGE"}\n')
    got = [r["verdict"] for r in bh.load_history(hist)]
    check("malformed line skipped, neighbours survive", got, ["PASS", "WEDGE"])


def test_non_dict_record_is_skipped(bh, tmpdir):
    hist = os.path.join(tmpdir, "h.jsonl")
    with open(hist, "w", encoding="utf-8", newline="\n") as fh:
        fh.write("[1, 2, 3]\n")
        fh.write('{"verdict": "PASS"}\n')
    check("a valid-JSON non-record is skipped",
          [r["verdict"] for r in bh.load_history(hist)], ["PASS"])


def test_missing_history_is_empty_not_an_error(bh, tmpdir):
    check("absent history reads as empty",
          bh.load_history(os.path.join(tmpdir, "nope.jsonl")), [])


def test_lf_line_endings(bh, tmpdir):
    """CRLF in an append-only committed file causes phantom whole-file diffs."""
    hist = os.path.join(tmpdir, "h.jsonl")
    bh.append_record(hist, {"verdict": "PASS"})
    with open(hist, "rb") as fh:
        raw = fh.read()
    check("records are written LF-only", b"\r\n" in raw, False)


def test_failure_carries_a_tail_and_pass_does_not(bh):
    """`build/` is gitignored and the next run overwrites the serial file.

    Without the tail, the evidence for a hang survives only if a human pasted
    it into markdown before the next boot -- a loss that already cost one
    investigation (B-FORKEXEC-BOOT-HANG). Passes get none: the same 25 lines
    every time, in a committed file.
    """
    args = _Args()
    s_fail = _serial(bh, S_W1)
    rec_fail = bh.build_record(s_fail, "WEDGE", args)
    check_true("a failed boot records its tail", rec_fail.get("tail"))
    check("the tail ends at the freeze point",
          rec_fail["tail"][-1], "[sysctl] mm.oom_pol")

    s_pass = _serial(bh, S_PASS)
    rec_pass = bh.build_record(s_pass, "PASS", args)
    check("a passing boot records no tail", "tail" in rec_pass, False)


def test_record_is_json_serialisable(bh):
    """A record that cannot be serialised is a boot outcome silently lost."""
    rec = bh.build_record(_serial(bh, S_PTHREAD), "PANIC", _Args())
    try:
        json.dumps(rec, sort_keys=True)
        ok = True
    except (TypeError, ValueError):
        ok = False
    check_true("record serialises", ok)
    check("record carries its fingerprint",
          rec.get("fingerprints"), ["B-PTHREAD-TEARDOWN-PF"])


def test_tail_is_bounded(bh):
    """The file is committed; an unbounded tail is an unbounded diff."""
    big = _PROLOGUE + "".join(f"[spam] line {i}\n" for i in range(500))
    big += "x" * 5000
    rec = bh.build_record(_serial(bh, big), "TIMEOUT", _Args())
    check("tail line count bounded", len(rec["tail"]) <= bh.TAIL_LINES, True)
    check("tail line width bounded",
          max(len(ln) for ln in rec["tail"]) <= bh.TAIL_WIDTH, True)


def test_caller_supplied_commit_wins_over_git(bh):
    """The row must name the tree that was *built*, not HEAD at record time.

    This runs from the EXIT trap of a boot test that took ten to twenty
    minutes, and committing during one is normal here -- so asking git for HEAD
    now can stamp the row with a commit made while QEMU was already running.
    That happened on 2026-08-18: a PASS was filed against a commit whose entire
    content was a paragraph of markdown.  It is not a cosmetic mislabel, because
    boot-test.sh's report_bench_absence() diffs HEAD against the last recorded
    commit to decide whether perf-critical code still needs benchmarking, and a
    row stamped too *new* hides exactly the changes that check exists to find.
    """
    args = _Args()
    args.commit = "deadbee"
    args.branch = "lane-z"
    args.dirty = True
    rec = bh.build_record(_serial(bh, S_PASS), "PASS", args)
    check("the supplied commit is recorded", rec["commit"], "deadbee")
    check("the supplied branch is recorded", rec["branch"], "lane-z")
    check("a dirty build tree is recorded", rec["dirty"], True)

    clean = bh.build_record(_serial(bh, S_PASS), "PASS", _Args())
    check("a clean build tree is recorded as such", clean["dirty"], False)
    check_true("no supplied commit falls back to git", clean["commit"])


class _Args:
    exit_code = 1
    marker = "BOOT_OK"
    label = ""
    profile = "debug"
    wall_seconds = None
    # Empty, as argparse leaves them when boot-test.sh does not pass them, so
    # build_record() takes the git fallback -- which is the path these tests
    # were written against.
    commit = ""
    branch = ""
    dirty = False
    # Empty is the ordinary case -- a boot of the tree, not a probe. The
    # experiment path has its own tests below.
    experiment = ""


# --------------------------------------------------------------------------
# Streaks
# --------------------------------------------------------------------------


def _rec(verdict, fps=(), ts="2026-08-16T00:00:00+00:00"):
    r = {"ts": ts, "commit": "abc1234", "verdict": verdict}
    if fps:
        r["fingerprints"] = list(fps)
    return r


def test_streak_counts_all_boots_not_only_clean_ones(bh):
    """A boot that failed for another reason is still a boot without this bug.

    known-issues' closure bars say routine boots count toward the streak.
    """
    records = [_rec("PASS"), _rec("SELFTEST_FAIL"), _rec("PASS")]
    st = {s.fp.id: s for s in bh.streaks(records)}["W1"]
    check("streak counts every recorded boot", st.since_last, 3)
    check("no occurrences recorded", st.occurrences, 0)


def test_streak_resets_on_a_match(bh):
    records = [_rec("PASS"), _rec("WEDGE", ["W1"]), _rec("PASS"), _rec("PASS")]
    st = {s.fp.id: s for s in bh.streaks(records)}["W1"]
    check("streak resets at the match", st.since_last, 2)
    check("occurrence counted", st.occurrences, 1)


def _probe(verdict, why="QEMU_EXTRA=-accel whpx (non-default emulator flags)"):
    """A boot run under conditions no checkout reproduces."""
    r = _rec(verdict)
    r["experiment"] = why
    return r


def test_an_experiment_boot_is_not_evidence_about_the_tree(bh):
    """A deliberate probe must not touch any statistic describing the tree.

    The failure this prevents is not hypothetical. On 2026-08-19 a one-off
    `-cpu host` boot -- run purely to find out whether WHPX could carry
    SMEP/SMAP/UMIP -- died inside OVMF before our kernel was loaded, and landed
    in the history as a plain TIMEOUT. It reset a long consecutive-clean streak
    to zero, and four open kernel issues have closure conditions written as
    counts of consecutive clean boots.

    Both directions are asserted, because only one of them is safe. Excluding
    the probe from the *streak* is a correction; excluding it from
    `since_last` is a correctness requirement, since a boot that never reached
    our kernel cannot be evidence that a kernel bug failed to reappear.
    """
    records = [_rec("PASS"), _probe("TIMEOUT"), _rec("PASS")]

    # A probe in the middle is stepped over, not treated as a break: the tree
    # booted clean twice running, and nothing about the tree happened between.
    check("a probe does not break the clean streak",
          bh.tail_clean_streak(records), 2)
    check("...and the same records without the tag do break it",
          bh.tail_clean_streak([_rec("PASS"), _rec("TIMEOUT"), _rec("PASS")]),
          1)

    # The dangerous direction: a probe must not advance a closure bar.
    st = {s.fp.id: s for s in bh.streaks(records)}["W1"]
    check("a probe is not counted toward a fingerprint's clean run",
          st.since_last, 2)
    check("...and it is not counted among the records considered",
          st.recorded, 2)


def test_a_probe_that_passed_is_excluded_just_the_same(bh):
    """Exclusion follows from being a probe, never from having failed.

    A rule that only skipped *failed* experiments would be worse than none: it
    would quietly inflate every clean streak with boots that never tested the
    tree, which is the one error this module exists to prevent. The two WHPX
    boots of 2026-08-19 both passed, and both are equally uninformative.
    """
    records = [_rec("PASS"), _probe("PASS"), _probe("PASS")]
    check("passing probes do not pad the streak",
          bh.tail_clean_streak(records), 1)

    st = {s.fp.id: s for s in bh.streaks(records)}["W1"]
    check("...nor a fingerprint's clean run", st.since_last, 1)


def test_experiment_wall_times_do_not_move_the_median(bh):
    """`wall time by build` must describe a boot someone can actually run.

    Measured, not supposed: the two WHPX boots took 168 s and 186 s against a
    TCG median near 120 s for the same profile, so leaving them in shifts the
    only number that answers "how long should this take?".
    """
    def timed(wall, probe=False):
        r = _probe("PASS") if probe else _rec("PASS")
        r["wall_seconds"] = wall
        return r

    records = [timed(120), timed(120), timed(186, probe=True)]
    pops = bh.wall_populations(records)
    check("only one population, and the probe is not in it",
          {k: sorted(v) for k, v in pops.items()},
          {bh.sanitizer_of(_rec("PASS")): [120.0, 120.0]})


def test_unvalidated_fingerprint_reports_no_streak(bh):
    """The load-bearing honesty property.

    A matcher that never fires and a genuinely clean run produce the same
    number. If an unvalidated fingerprint printed a streak, that number would
    be read as evidence and could close an open kernel bug.
    """
    fake = bh.Fingerprint(id="UNTESTED", title="t", match=lambda s, v: False,
                          validated_by=())
    st = bh.Streak(fp=fake, recorded=90, occurrences=0, since_last=90)
    text = "\n".join(bh.describe_streak(st))
    check_true("unvalidated fingerprint says so", "UNVALIDATED" in text)
    check("unvalidated fingerprint prints no count", "90" in text, False)


def test_validated_fingerprint_reports_its_streak(bh):
    real = bh.Fingerprint(id="REAL", title="t", match=lambda s, v: False,
                          validated_by=("2026-06-12",))
    st = bh.Streak(fp=real, recorded=90, occurrences=0, since_last=90)
    text = "\n".join(bh.describe_streak(st))
    check_true("validated fingerprint reports its count", "90" in text)
    check_true("and says the known occurrence predates the file",
               "predate this file" in text)


def test_every_fingerprint_is_validated(bh):
    """Not a rule against unvalidated fingerprints -- a rule against *quiet* ones.

    Adding one is fine; it just has to report as unvalidated. This test exists
    so that the day someone adds one, the fact is visible here rather than
    only in a streak nobody re-reads.
    """
    unvalidated = [fp.id for fp in bh.FINGERPRINTS if not fp.validated_by]
    check("shipped fingerprints are all validated", unvalidated, [])


def test_fingerprint_ids_are_unique(bh):
    ids = [fp.id for fp in bh.FINGERPRINTS]
    check("fingerprint ids unique", len(set(ids)), len(ids))


def test_clean_verdicts_are_all_known(bh):
    """A typo'd verdict in CLEAN_VERDICTS silently makes nothing clean."""
    unknown = sorted(bh.CLEAN_VERDICTS - set(bh.VERDICT_HELP))
    check("every clean verdict is a documented verdict", unknown, [])


# --------------------------------------------------------------------------
# Build profile (which sanitizer the kernel was built with)
# --------------------------------------------------------------------------


def test_sanitizer_read_from_the_banner(bh):
    kasan = _serial(bh, S_BANNER_KASAN + S_PASS)
    plain = _serial(bh, S_BANNER_NONE + S_PASS)
    check("instrumented banner parsed", kasan.sanitizer, "kasan-instrumented")
    check("uninstrumented banner parsed", plain.sanitizer, "none")


def test_absent_banner_is_none_the_object_not_none_the_string(bh):
    """The distinction the whole three-valued field exists for.

    A log with no banner cannot say which build it was; a log that says
    `sanitizer=none` says it was not instrumented. Folding the first into the
    second would relabel every boot recorded before 2026-08-19 -- a population
    that certainly includes instrumented ones -- as definitely-not-instrumented,
    in exactly the direction that makes the two populations look like one.
    """
    s = _serial(bh, S_PASS)
    check("no banner -> None", s.sanitizer, None)
    check("and specifically not the string 'none'", s.sanitizer == "none", False)


def test_banner_survives_a_second_key_being_added(bh):
    """A parser that quietly stops matching produces the same answer as a kernel
    that never printed, and those must stay distinguishable -- so the regex keys
    off `sanitizer=`, not off the whole line."""
    text = "[boot] build profile: sanitizer=none opt=3 lto=thin\n" + S_PASS
    check("extra keys do not break the match", _serial(bh, text).sanitizer,
          "none")
    # No longer hypothetical: `textpad=` was appended on 2026-08-19 for the
    # layout-sensitivity sweep (kernel/src/layout_pad.rs). Assert the *real*
    # banner, not just a stand-in for it -- the stand-in would have kept
    # passing whatever the kernel actually printed.
    real = "[boot] build profile: sanitizer=kasan-instrumented textpad=3072\n"
    check("the real two-key banner still yields the sanitizer",
          _serial(bh, real + S_PASS).sanitizer, "kasan-instrumented")


def test_kasan_fp_still_matches_a_pre_banner_log(bh):
    """Scoping the fingerprint by build must not un-validate its own evidence.

    S_KASAN is the 2026-08-12 occurrence, from a kernel that could not print a
    banner. If "unknown" were treated as "not instrumented", this fingerprint's
    single validated occurrence would stop matching and its streak would reset
    to a perfect clean one -- the precise failure this suite exists to catch.
    """
    verdict, fps = _fps(bh, S_KASAN, 2)
    check_true("pre-banner KASAN wedge still matches",
               "B-KASAN-INSTRUMENTED-BOOT-WEDGES-MID-PRINT-ON-A-PAGE-FAULT"
               in fps)


def test_kasan_fp_matches_an_instrumented_log(bh):
    _, fps = _fps(bh, S_BANNER_KASAN + S_KASAN, 2)
    check_true("instrumented KASAN wedge matches",
               "B-KASAN-INSTRUMENTED-BOOT-WEDGES-MID-PRINT-ON-A-PAGE-FAULT"
               in fps)


def test_kasan_fp_declines_an_explicitly_uninstrumented_log(bh):
    """The issue is titled "KASAN builds only"; now the kernel can say so.

    Only an explicit denial rules it out. This is the one direction where
    narrowing is safe, because the kernel positively asserted the build.
    """
    _, fps = _fps(bh, S_BANNER_NONE + S_KASAN, 2)
    check("uninstrumented build declines the KASAN fingerprint",
          "B-KASAN-INSTRUMENTED-BOOT-WEDGES-MID-PRINT-ON-A-PAGE-FAULT" in fps,
          False)
    check_true("and the record is not lost -- W1 is disjoint, so nothing "
               "silently absorbs it", isinstance(fps, list))


def test_record_carries_the_sanitizer_even_when_unknown(bh):
    """Present-and-null, not absent.

    Within rows that have a serial log at all, an absent key must mean "written
    before the field existed" and null must mean "the kernel did not say". Omit
    the key when unknown and those collapse into one, leaving a consumer to
    guess -- which on this file's history means guess "uninstrumented".
    """
    rec = bh.build_record(_serial(bh, S_PASS), "PASS", _Args())
    check_true("sanitizer key always written", "sanitizer" in rec)
    check("unknown build recorded as null", rec["sanitizer"], None)
    rec2 = bh.build_record(_serial(bh, S_BANNER_KASAN + S_PASS), "PASS",
                           _Args())
    check("known build recorded verbatim", rec2["sanitizer"],
          "kasan-instrumented")
    # It must survive the round trip that actually happens: the file is JSONL.
    check("null survives serialisation", json.loads(json.dumps(rec))["sanitizer"],
          None)


def test_sanitizer_of_groups_the_two_ways_of_not_knowing(bh):
    absent = {"verdict": "PASS"}
    null = {"verdict": "PASS", "sanitizer": None}
    known = {"verdict": "PASS", "sanitizer": "none"}
    check("absent key groups as unknown", bh.sanitizer_of(absent),
          bh.sanitizer_of(null))
    check("and never as the kernel's own 'none'",
          bh.sanitizer_of(absent) == bh.sanitizer_of(known), False)
    check("an explicit 'none' groups as itself", bh.sanitizer_of(known), "none")


def test_wall_times_are_never_averaged_across_builds(bh):
    """The defect this change fixes, stated as a test.

    Two populations whose wall times differ by ~3.4x were both recorded
    `profile: "debug"`, so any median drawn over the file described neither.
    """
    records = [
        {"verdict": "PASS", "sanitizer": "none", "wall_seconds": 320.0},
        {"verdict": "PASS", "sanitizer": "none", "wall_seconds": 340.0},
        {"verdict": "PASS", "sanitizer": "kasan-instrumented",
         "wall_seconds": 1100.0},
        {"verdict": "PASS", "wall_seconds": 900.0},
    ]
    pops = bh.wall_populations(records)
    check("three populations kept apart", sorted(pops), sorted(
        ["none", "kasan-instrumented", bh._SAN_UNKNOWN]))
    check("uninstrumented median", bh._median(pops["none"]), 330.0)
    check("instrumented median", bh._median(pops["kasan-instrumented"]), 1100.0)


def test_wall_populations_ignore_rows_without_a_duration(bh):
    """A missing `wall_seconds` must not become a zero -- a zero would drag the
    median of whichever population it landed in toward a value no boot took."""
    records = [{"verdict": "PASS", "sanitizer": "none"},
               {"verdict": "PASS", "sanitizer": "none", "wall_seconds": None},
               {"verdict": "PASS", "sanitizer": "none", "wall_seconds": 300.0}]
    check("only the row with a duration counts",
          bh.wall_populations(records)["none"], [300.0])


def test_report_prints_each_build_separately_and_no_combined_figure(bh):
    import contextlib
    import io
    records = [
        {"verdict": "PASS", "sanitizer": "none", "wall_seconds": 330.0},
        {"verdict": "PASS", "sanitizer": "kasan-instrumented",
         "wall_seconds": 1100.0},
    ]
    buf = io.StringIO()
    with contextlib.redirect_stdout(buf):
        bh.report(records, None)
    out = buf.getvalue()
    check_true("uninstrumented population reported", "330s" in out)
    check_true("instrumented population reported", "1100s" in out)
    check_true("and the reader is told why they are apart",
               "separately on purpose" in out)
    # 715 is the mean of 330 and 1100: the number the old code would have
    # produced, and the one no boot on this host has ever taken.
    check("no figure averaged across builds", "715" in out, False)


def test_report_names_the_build_of_the_run_it_just_recorded(bh):
    import contextlib
    import io
    buf = io.StringIO()
    with contextlib.redirect_stdout(buf):
        bh.report([], {"verdict": "PASS", "sanitizer": "kasan-instrumented"})
    check_true("current run's build is printed",
               "build: kasan-instrumented" in buf.getvalue())


# --------------------------------------------------------------------------
# End-to-end
# --------------------------------------------------------------------------


def test_main_records_and_is_idempotent_about_exit_status(bh, tmpdir):
    """The recorder's exit status is about the recorder, never about the boot.

    boot-test.sh calls this from its EXIT trap; a non-zero return that leaked
    into the harness's status would turn a wedged boot into a *differently*
    wedged boot and, worse, could turn a green boot red.
    """
    serial = os.path.join(tmpdir, "serial.txt")
    with open(serial, "w", encoding="utf-8", newline="\n") as fh:
        fh.write(S_W1)
    hist = os.path.join(tmpdir, "boot-history.jsonl")

    rc = bh.main(["--serial", serial, "--history", hist, "--exit-code", "2",
                  "--label", "test"])
    check("main returns 0 after recording a wedge", rc, 0)
    recs = bh.load_history(hist)
    check("one record written", len(recs), 1)
    check("verdict recorded", recs[0]["verdict"], "WEDGE")
    check("fingerprint recorded", recs[0]["fingerprints"], ["W1"])
    check("label recorded", recs[0]["label"], "test")


def test_main_does_not_record_a_missing_serial(bh, tmpdir):
    """A build failure is not a boot outcome.

    Recording it would reset every hang streak on every compile error -- and
    compile errors are far more common than hangs, so the streaks would never
    grow past one.
    """
    hist = os.path.join(tmpdir, "boot-history.jsonl")
    rc = bh.main(["--serial", os.path.join(tmpdir, "absent.txt"),
                  "--history", hist, "--exit-code", "1"])
    check("main returns 0 with no serial", rc, 0)
    check("nothing recorded for a missing serial", bh.load_history(hist), [])


def test_no_record_flag_writes_nothing(bh, tmpdir):
    serial = os.path.join(tmpdir, "serial.txt")
    with open(serial, "w", encoding="utf-8", newline="\n") as fh:
        fh.write(S_PASS)
    hist = os.path.join(tmpdir, "boot-history.jsonl")
    bh.main(["--serial", serial, "--history", hist, "--exit-code", "0",
             "--no-record"])
    check("--no-record writes nothing", os.path.exists(hist), False)


def test_streaks_and_list_run_on_an_empty_history(bh, tmpdir):
    """The first invocation ever must not crash on an absent file."""
    hist = os.path.join(tmpdir, "boot-history.jsonl")
    check("--streaks on an empty history", bh.main(["--history", hist,
                                                    "--streaks"]), 0)
    check("--list on an empty history", bh.main(["--history", hist,
                                                 "--list"]), 0)


def main():
    bh = load_module()
    tests = [(name, fn) for name, fn in list(globals().items())
             if name.startswith("test_") and callable(fn)]
    # A discovery mechanism that discovers nothing looks exactly like a suite
    # that passes -- the failure mode this whole script is about. Assert a floor.
    if len(tests) < 60:
        print(f"FATAL: test discovery found only {len(tests)} tests; the suite "
              f"has at least 60. Discovery is broken, not the code.")
        return 1
    for name, fn in tests:
        params = inspect.signature(fn).parameters
        with tempfile.TemporaryDirectory() as tmpdir:
            avail = {"bh": bh, "tmpdir": tmpdir}
            fn(**{p: avail[p] for p in params if p in avail})

    print()
    if _FAILURES:
        print(f"{len(_FAILURES)} FAILED: {', '.join(_FAILURES)}")
        return 1
    print(f"all {len(tests)} boot-history tests passed")
    return 0


if __name__ == "__main__":
    sys.exit(main())
