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

import inspect
import json
import os
import random
import sys
import tempfile

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import srcload  # noqa: E402

REPO_ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
SCRIPT = os.path.join(REPO_ROOT, "scripts", "boot-history.py")

_FAILURES: list[str] = []


def load_module():
    """Import boot-history.py by path (its name is not a valid identifier).

    Loaded through `srcload` rather than `importlib`: a `SourceFileLoader`
    consults `__pycache__`, whose staleness check is `(mtime, size)` at
    one-second resolution, so two same-size writes inside one second leave the
    second one invisible and the suite validates bytecode that is not on disk.
    That has actually happened here. See `scripts/srcload.py`.

    `srcload` also registers the module before running its body, which this
    file needs: `@dataclass` resolves the defining module by name while the
    class body runs, and an unregistered module makes that lookup return
    None.
    """
    return srcload.load(SCRIPT, "boot_history")


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

#: The `[hypervisor]` banner, in each of the three shapes the kernel emits.
#: Verbatim from `kernel/src/hypervisor.rs`; `bench-history.py` owns the
#: patterns that read them and this file's parser delegates to it, so these
#: samples are also what keeps that delegation honest -- a copy of the regex
#: here would agree with a broken copy there.
S_BANNER_TCG = '[hypervisor] Detected: QEMU TCG (signature: "TCGTCGTCGTCG")\n'
S_BANNER_WHPX = ('[hypervisor] Detected: Hyper-V/WHPX '
                 '(signature: "Microsoft Hv")\n')
S_BANNER_METAL = ("[hypervisor] Running on bare metal "
                  "(no hypervisor detected)\n")


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


def test_an_uncomputable_source_digest_is_absent_not_empty(bh):
    """Absent means unknown; empty would be a value that every row shares.

    `commit` and `dirty` between them never identified the built source -- the
    kernel `include_bytes!`s six gitignored service binaries that
    `git diff --quiet HEAD` cannot see -- so runs now stamp themselves with a
    `src_digest` instead. boot-test.sh omits the flag entirely when it could
    not compute one, and this pins the half of that contract living here.

    The distinction is the whole safety property. Downstream, `arm_group_key`
    treats an absent digest as unknown and falls back to a key that groups
    nothing new. An empty string is not unknown: it is a value, equal to every
    other empty string, so a fleet of rows that all failed to compute a digest
    would silently band together as though they shared a build.
    """
    args = _Args()
    args.src_digest = "full:0123456789abcdef"
    rec = bh.build_record(_serial(bh, S_PASS), "PASS", args)
    check("a supplied digest is recorded", rec.get("src_digest"),
          "full:0123456789abcdef")

    absent = bh.build_record(_serial(bh, S_PASS), "PASS", _Args())
    check("an uncomputable digest leaves the key out entirely",
          "src_digest" in absent, False)


class _Args:
    exit_code = 1
    marker = "BOOT_OK"
    label = ""
    profile = "debug"
    wall_seconds = None
    # None is the ordinary case: a --no-build/--no-stage run never enters Step 1
    # and boot-test.sh omits the flag, so the key must stay out of the record
    # rather than land as a 0 that reads like an instant build.
    build_seconds = None
    # Same shape, and the same reason: a run whose floor check was disabled with
    # --min-free-gb=0, or whose `df` was unreadable, did not observe zero GiB
    # free. These must match argparse's own defaults (None and "") or the stub
    # stops standing in for a real parse -- which is exactly how it failed:
    # 034dffe2c taught build_record() to read args.free_gb_min without adding
    # it here, so every test calling build_record() died on AttributeError
    # rather than reporting anything about boot history.
    free_gb_min = None
    free_gb_phase = ""
    # Empty, as argparse leaves them when boot-test.sh does not pass them, so
    # build_record() takes the git fallback -- which is the path these tests
    # were written against.
    commit = ""
    branch = ""
    dirty = False
    # Empty is the ordinary case here too: boot-test.sh omits the flag when the
    # digest could not be computed, and build_record() must then leave the key
    # out entirely rather than store an empty one. An absent field reads as
    # unknown downstream and refuses to group; a shared empty string would
    # group every such row together. See scripts/src_digest.py.
    src_digest = ""
    # Empty is the ordinary case -- a boot of the tree, not a probe. The
    # experiment path has its own tests below.
    experiment = ""
    # None is the ordinary case for any caller that is not boot-test.sh: no
    # marker file, so `gated_ran` stays out of the record. Spelled out rather
    # than left to build_record's getattr default, so these tests exercise the
    # same attribute lookup a real argparse namespace provides.
    gated_markers = None


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
          {f"{bh.profile_of(_rec('PASS'))}/{bh.sanitizer_of(_rec('PASS'))}"
           f" on {bh._ACCEL_UNKNOWN}": [120.0, 120.0]})


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
    # The keys are the (build, accelerator) pair. Spelled out from the two
    # constants rather than built with `population_of`, so this asserts the
    # partition instead of restating the code that produces it.
    plain = f"{bh._PROFILE_UNKNOWN}/none on {bh._ACCEL_UNKNOWN}"
    kasan = f"{bh._PROFILE_UNKNOWN}/kasan-instrumented on {bh._ACCEL_UNKNOWN}"
    check("three populations kept apart", sorted(pops), sorted(
        [plain, kasan,
         f"{bh._PROFILE_UNKNOWN}/{bh._SAN_UNKNOWN} on {bh._ACCEL_UNKNOWN}"]))
    check("uninstrumented median", bh._median(pops[plain]), 330.0)
    check("instrumented median", bh._median(pops[kasan]), 1100.0)


def test_wall_populations_ignore_rows_without_a_duration(bh):
    """A missing `wall_seconds` must not become a zero -- a zero would drag the
    median of whichever population it landed in toward a value no boot took."""
    records = [{"verdict": "PASS", "sanitizer": "none"},
               {"verdict": "PASS", "sanitizer": "none", "wall_seconds": None},
               {"verdict": "PASS", "sanitizer": "none", "wall_seconds": 300.0}]
    check("only the row with a duration counts",
          bh.wall_populations(records)[
              f"{bh._PROFILE_UNKNOWN}/none on {bh._ACCEL_UNKNOWN}"],
          [300.0])


def test_build_populations_split_by_profile_not_by_accelerator(bh):
    """Build time is a fact about the host compiler, not about the emulator.

    Folding the accelerator into this key -- which is right for wall time and
    wrong here -- would split each profile into populations that differ in
    nothing, shrinking every sample for no gain. These four records are two
    profiles across two accelerators and must come out as exactly two groups.
    """
    records = [
        {"verdict": "PASS", "sanitizer": "none", "profile": "debug",
         "build_seconds": 100.0, "accelerator": "QEMU TCG"},
        {"verdict": "PASS", "sanitizer": "none", "profile": "debug",
         "build_seconds": 140.0, "accelerator": "Hyper-V/WHPX"},
        {"verdict": "PASS", "sanitizer": "none", "profile": "release",
         "build_seconds": 600.0, "accelerator": "QEMU TCG"},
        {"verdict": "PASS", "sanitizer": "none", "profile": "release",
         "build_seconds": 700.0, "accelerator": "Hyper-V/WHPX"},
    ]
    pops = bh.build_populations(records)
    check("two populations, one per profile", sorted(pops), ["debug", "release"])
    check("debug median", bh._median(pops["debug"]), 120.0)
    check("release median", bh._median(pops["release"]), 650.0)


def test_build_populations_keep_kasan_apart(bh):
    """KASAN instruments every memory access, so it is a different build cost
    and not a slow instance of the same one."""
    records = [
        {"verdict": "PASS", "sanitizer": "none", "profile": "debug",
         "build_seconds": 100.0},
        {"verdict": "PASS", "sanitizer": "kasan-instrumented",
         "profile": "debug", "build_seconds": 400.0},
    ]
    check("split by sanitizer as well as profile",
          sorted(bh.build_populations(records)), ["debug", "debug + KASAN"])


def test_build_populations_ignore_runs_that_did_not_build(bh):
    """A --no-build run has no `build_seconds` at all, and must not be counted
    as a zero-second build -- that would understate every profile's cost while
    looking like an implausibly fast compile rather than like an absent one."""
    records = [{"verdict": "PASS", "profile": "debug"},
               {"verdict": "PASS", "profile": "debug", "build_seconds": None},
               {"verdict": "PASS", "profile": "debug", "build_seconds": 90.0}]
    check("only the row that built counts",
          bh.build_populations(records)["debug"], [90.0])


def test_build_populations_skip_experiments(bh):
    """Same rule as everywhere else in this file: a probe is not a boot of the
    tree, and its build is not a build of the tree either."""
    records = [
        {"verdict": "PASS", "profile": "debug", "build_seconds": 90.0},
        {"verdict": "PASS", "profile": "debug", "build_seconds": 9000.0,
         "experiment": "hand-patched Cargo.toml"},
    ]
    check("the probe's build is excluded",
          bh.build_populations(records)["debug"], [90.0])


def test_report_prints_build_time_and_warns_about_the_mixture(bh):
    import contextlib
    import io
    records = [
        {"verdict": "PASS", "sanitizer": "none", "profile": "debug",
         "wall_seconds": 330.0, "build_seconds": 3.0},
        {"verdict": "PASS", "sanitizer": "none", "profile": "debug",
         "wall_seconds": 340.0, "build_seconds": 900.0},
    ]
    buf = io.StringIO()
    with contextlib.redirect_stdout(buf):
        bh.report(records, None)
    out = buf.getvalue()
    check_true("build time is reported at all", "build time by profile" in out)
    check_true("the range spans no-op to cold", "3-900s" in out)
    # The caveat is load-bearing, not decoration: a median over a mixture of
    # cold, incremental and no-op rebuilds describes no build anyone waits for,
    # so a reader who takes it at face value is worse off than before.
    check_true("and the reader is told to read the range",
               "read the range" in out)


def test_report_omits_build_section_when_nothing_built(bh):
    """Printing an empty section would suggest the data exists and is boring;
    every record predating --build-seconds lacks the field entirely."""
    import contextlib
    import io
    buf = io.StringIO()
    with contextlib.redirect_stdout(buf):
        bh.report([{"verdict": "PASS", "wall_seconds": 330.0}], None)
    check("no build section without build data",
          "build time by profile" in buf.getvalue(), False)


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
        bh.report([], {"verdict": "PASS", "profile": "debug",
                       "sanitizer": "kasan-instrumented"})
    # Asserted as the whole label, not a substring of it: this line's entire
    # job is to say which population the run just recorded belongs to, so it
    # has to name every axis the population is partitioned on.  A test that
    # only checked the sanitizer would have gone on passing through the day
    # the profile axis was missing from it.
    check_true("current run's build is printed",
               "build: debug/kasan-instrumented" in buf.getvalue())


# --------------------------------------------------------------------------
# Accelerator (which emulator/hypervisor the boot ran under)
# --------------------------------------------------------------------------


def test_accel_read_from_the_banner(bh):
    """All three shapes, because the kernel prints three and not two.

    `bench-history.py` matches `Detected: ...` with one pattern and the
    bare-metal sentence with another; a boot-history that exercised only the
    first would still pass while bare metal silently rendered as "cannot say".
    """
    check("TCG banner parsed", _serial(bh, S_BANNER_TCG + S_PASS).accel,
          "QEMU TCG")
    check("WHPX banner parsed", _serial(bh, S_BANNER_WHPX + S_PASS).accel,
          "Hyper-V/WHPX")
    check("bare-metal banner parsed", _serial(bh, S_BANNER_METAL + S_PASS).accel,
          "bare metal")


def test_accel_parsing_is_really_delegated(bh):
    """The delegation is the point, so assert it rather than assume it.

    This file deliberately keeps no copy of the banner patterns
    (`design-decisions.md` sec 240). A copy would be worse than duplication
    here: a pattern that stopped matching returns the same `None` a pre-banner
    log does, so the drift would never announce itself. Reading the answer back
    out of `bench-history.py`'s own constant is what proves there is one
    parser and not two that happen to agree today.
    """
    check("bare metal comes from bench-history's constant, not a literal",
          _serial(bh, S_BANNER_METAL + S_PASS).accel,
          bh.bench_history().ACCEL_BARE_METAL)
    check("and the delegate is reached at all",
          bh.bench_history().parse_accel.__module__, "bench_history")


def test_an_unreadable_delegate_costs_the_label_not_the_boot(bh):
    """The one place in this file where swallowing an error is right.

    `boot-test.sh` calls this script from its EXIT trap with `|| true`, so an
    exception raised here does not surface anywhere -- it silently loses the
    record of the boot. For a *failing* boot that is the most expensive thing
    this script can do. A missing accelerator label costs a row's grouping; a
    missing row costs the evidence.
    """
    import io as _io
    import contextlib
    real = bh.bench_history

    def broken():
        raise RuntimeError("no bench-history today")

    err = _io.StringIO()
    bh.bench_history = broken
    try:
        with contextlib.redirect_stderr(err):
            s = _serial(bh, S_BANNER_WHPX + S_PASS)
            rec = bh.build_record(s, "PASS", _Args())
    finally:
        bh.bench_history = real
    check("the boot is still recorded", rec["verdict"], "PASS")
    check("with an accelerator of 'cannot say', never a guess", rec["accel"],
          None)
    check_true("and the failure is announced where a human sees it",
               "accelerator banner" in err.getvalue())


def test_absent_accel_banner_is_none_not_tcg(bh):
    """The conflation this field exists to prevent, stated as a test.

    A log with no `[hypervisor]` line cannot say what ran it. Reading that as
    "TCG" is not a harmless default: the first WHPX run on this host
    (2026-08-19T16:15:09) predates the banner, so the guess is *known* to be
    wrong for a record already in the file -- and wrong in the direction that
    drops a 168s boot into a population whose median is ~120s.
    """
    s = _serial(bh, S_PASS)
    check("no banner -> None", s.accel, None)
    check("and specifically not the string 'QEMU TCG'", s.accel == "QEMU TCG",
          False)


def test_record_carries_the_accel_even_when_unknown(bh):
    """Present-and-null, not absent -- the same three-state rule as `sanitizer`.

    Absent means "this row predates the field"; null means "the log did not
    say". Omitting the key when unknown collapses those, and this file already
    contains rows of both kinds.
    """
    rec = bh.build_record(_serial(bh, S_PASS), "PASS", _Args())
    check_true("accel key always written", "accel" in rec)
    check("unknown accelerator recorded as null", rec["accel"], None)
    rec2 = bh.build_record(_serial(bh, S_BANNER_WHPX + S_PASS), "PASS", _Args())
    check("known accelerator recorded verbatim", rec2["accel"], "Hyper-V/WHPX")
    check("null survives serialisation",
          json.loads(json.dumps(rec))["accel"], None)


def test_accel_of_groups_the_two_ways_of_not_knowing(bh):
    absent = {"verdict": "PASS"}
    null = {"verdict": "PASS", "accel": None}
    known = {"verdict": "PASS", "accel": "QEMU TCG"}
    check("absent key groups as unknown", bh.accel_of(absent), bh.accel_of(null))
    check("and never as a named accelerator",
          bh.accel_of(absent) == bh.accel_of(known), False)
    check("a named accelerator groups as itself", bh.accel_of(known),
          "QEMU TCG")


def test_population_is_the_triple_not_any_subset(bh):
    """Change any one of the three factors and you have a different population.

    Each pairing below merges boots that differ by a measured multiple on this
    host: profile ~2.7x, sanitizer ~3.4x, accelerator ~1.4x. Dropping any axis
    presents one of those mixtures as a measurement.
    """
    tcg_plain = {"profile": "debug", "sanitizer": "none", "accel": "QEMU TCG"}
    whpx_plain = {"profile": "debug", "sanitizer": "none",
                  "accel": "Hyper-V/WHPX"}
    tcg_kasan = {"profile": "debug", "sanitizer": "kasan-instrumented",
                 "accel": "QEMU TCG"}
    tcg_release = {"profile": "release", "sanitizer": "none",
                   "accel": "QEMU TCG"}
    check("same build, different accelerator -> different populations",
          bh.population_of(tcg_plain) == bh.population_of(whpx_plain), False)
    check("same accelerator, different sanitizer -> different populations",
          bh.population_of(tcg_plain) == bh.population_of(tcg_kasan), False)
    check("same everything else, different profile -> different populations",
          bh.population_of(tcg_plain) == bh.population_of(tcg_release), False)
    check_true("and all three factors are named in the label a human reads",
               "debug" in bh.population_of(tcg_plain)
               and "none" in bh.population_of(tcg_plain)
               and "QEMU TCG" in bh.population_of(tcg_plain))


def test_a_release_boot_does_not_move_the_debug_median(bh):
    """The defect this axis was added for, with the file's own real numbers.

    100 of the 243 records in bench/boot-history.jsonl were release boots
    pooled with the debug ones. The largest population printed "155 boot(s),
    median 331s" -- the median of a 95/60 mixture of a 382s population and a
    130s one, and therefore a duration no build on this host has ever taken.
    The numbers below are that shape in miniature.
    """
    def rec(profile, wall):
        return {"verdict": "PASS", "profile": profile, "sanitizer": "none",
                "accel": "QEMU TCG", "wall_seconds": wall}

    records = [rec("debug", 380.0), rec("debug", 390.0), rec("debug", 400.0),
               rec("release", 130.0), rec("release", 140.0)]
    pops = bh.wall_populations(records)
    debug = "debug/none on QEMU TCG"
    release = "release/none on QEMU TCG"
    check("two populations, not one", sorted(pops), [debug, release])
    check("the debug median is a debug boot", bh._median(pops[debug]), 390.0)
    check("the release median is a release boot",
          bh._median(pops[release]), 135.0)
    # 380 is the pooled median of all five -- close enough to the debug figure
    # to look plausible, which is exactly what made this survive unnoticed.
    check("and no population reports the pooled figure",
          any(bh._median(v) == 380.0 for v in pops.values()), False)


def test_profile_of_groups_the_two_ways_of_not_knowing(bh):
    """The third twin of accel_of/sanitizer_of, and it must not guess `debug`
    just because that is the recorder's argparse default -- folding
    does-not-say into the larger population is how a mixture becomes a
    measurement."""
    absent = {"verdict": "PASS"}
    null = {"verdict": "PASS", "profile": None}
    check("absent key groups as unknown",
          bh.profile_of(absent), bh.profile_of(null))
    check("and never as debug",
          bh.profile_of(absent) == "debug", False)
    check("a named profile groups as itself",
          bh.profile_of({"profile": "release"}), "release")


def test_an_untagged_whpx_boot_does_not_move_the_tcg_median(bh):
    """The whole reason this change exists, with the real numbers.

    `wall_populations`' docstring records two WHPX boots at 168s and 186s
    against a TCG median near 120s. They stayed out of the TCG median only
    because they happened to carry an `experiment` tag -- a fact about how they
    were invoked, not a rule this file applies. Q53 proposes making WHPX the
    ordinary way to boot the tree, at which point the tag stops appearing.

    So the fixture is untagged on purpose. Under the old grouping the four rows
    are one population with a median of 144s -- a duration no boot took, and
    ~20% off both real ones, which is twice CLAUDE.md's regression threshold.
    """
    records = [
        {"verdict": "PASS", "profile": "debug", "sanitizer": "none",
         "accel": "QEMU TCG", "wall_seconds": 118.0},
        {"verdict": "PASS", "profile": "debug", "sanitizer": "none",
         "accel": "QEMU TCG", "wall_seconds": 122.0},
        {"verdict": "PASS", "profile": "debug", "sanitizer": "none",
         "accel": "Hyper-V/WHPX", "wall_seconds": 168.0},
        {"verdict": "PASS", "profile": "debug", "sanitizer": "none",
         "accel": "Hyper-V/WHPX", "wall_seconds": 186.0},
    ]
    tcg = "debug/none on QEMU TCG"
    whpx = "debug/none on Hyper-V/WHPX"
    pops = bh.wall_populations(records)
    check("the two accelerators are two populations", sorted(pops),
          sorted([whpx, tcg]))
    check("TCG median is the TCG boots'", bh._median(pops[tcg]), 120.0)
    check("WHPX median is the WHPX boots'", bh._median(pops[whpx]), 177.0)
    check("and no population holds the pooled figure",
          any(bh._median(v) == 144.0 for v in pops.values()), False)


def test_report_prints_the_accelerator_beside_the_build(bh):
    import contextlib
    import io
    records = [
        {"verdict": "PASS", "profile": "debug", "sanitizer": "none",
         "accel": "QEMU TCG", "wall_seconds": 120.0},
        {"verdict": "PASS", "profile": "debug", "sanitizer": "none",
         "accel": "Hyper-V/WHPX", "wall_seconds": 177.0},
    ]
    buf = io.StringIO()
    with contextlib.redirect_stdout(buf):
        bh.report(records, {"verdict": "PASS", "profile": "debug",
                            "sanitizer": "none", "accel": "Hyper-V/WHPX"})
    out = buf.getvalue()
    check_true("TCG population reported", "120s" in out)
    check_true("WHPX population reported", "177s" in out)
    check_true("the legend names the accelerator too",
               "accelerator" in out)
    check_true("and the current run is labelled with its own triple",
               "build: debug/none on Hyper-V/WHPX" in out)
    # 148.5 is the mean of the two: the figure the old grouping produced, and
    # one no boot on this host has ever taken.
    check("no figure pooled across accelerators", "148" in out, False)


def test_list_shows_the_accelerator(bh, tmpdir):
    import contextlib
    import io
    path = os.path.join(tmpdir, "h.jsonl")
    accels = ("QEMU TCG", "Hyper-V/WHPX", "bare metal", None)
    with open(path, "w", encoding="utf-8", newline="") as fh:
        for accel in accels:
            # An explicit `sanitizer` so the column beside this one renders as
            # `-`. Leave it out and *it* prints `?`, and a test that merely
            # looked for a `?` somewhere on the line would be satisfied by the
            # neighbour -- which is exactly how a row that cannot say could
            # start claiming TCG without any test noticing.
            rec = {"ts": "2026-08-19T00:00:00", "verdict": "PASS",
                   "commit": "abc1234", "sanitizer": "none", "accel": accel}
            fh.write(json.dumps(rec) + "\n")
    buf = io.StringIO()
    with contextlib.redirect_stdout(buf):
        bh.cmd_list(path, 10)
    rows = [line.split() for line in buf.getvalue().splitlines() if line.strip()]
    check("one row per record", len(rows), len(accels))
    # Column 5: ts, commit, verdict, wall, sanitizer, accel, label, fingerprints.
    check("each accelerator gets its own token, and 'cannot say' is not one of "
          "them", [r[5] for r in rows], ["tcg", "whpx", "metal", "?"])


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


# --------------------------------------------------------------------------
# File order is not chronological order
#
# `bench/boot-history.jsonl` is marked `merge=union` in `.gitattributes` so
# three lanes appending concurrently stop conflicting. Union merge
# *concatenates* -- for a conflicting hunk it emits our lines then theirs --
# so the merged file's last line is not the latest boot. `load_history` sorts
# by `ts` to make file position carry no meaning at all, and these two tests
# are what stop that sort being "tidied away" by someone who reads it as
# cosmetic. (Filed by lane B:
# requests/b-a-boot-history-jsonl-conflicts-*.)
# --------------------------------------------------------------------------


def _streak_records(ts_suffix="+00:00"):
    """Six boots whose *chronological* tail is three clean ones.

    The WEDGE sits at t2 so that the clean tail (t3, t4, t5) is long enough to
    be shortened by a reordering rather than merely permuted.
    """
    verdicts = ["PASS", "PASS", "WEDGE", "PASS", "PASS", "PASS"]
    return [
        {"ts": f"2026-08-2{i}T00:00:00{ts_suffix}", "verdict": v,
         "commit": f"c{i:06d}", "label": "test"}
        for i, v in enumerate(verdicts)
    ]


def _write_jsonl(path, records):
    with open(path, "w", encoding="utf-8", newline="\n") as fh:
        for rec in records:
            fh.write(json.dumps(rec) + "\n")


def test_union_merged_order_yields_the_same_streak_as_a_sorted_one(bh, tmpdir):
    """A union-merged file must produce the streak the timestamps say.

    The order below is exactly what `merge=union` produces from a real
    concurrent session: a common ancestor (t0, t1), then *our* appends
    (t3, t5), then *theirs* (t2, t4). Nothing is corrupt and nothing is
    missing -- the lines are simply not in time order, which is the entire
    hazard, because a wrong streak is a number that closes a live
    `known-issues.md` entry and a merge conflict is not.
    """
    chrono = _streak_records()
    merged = [chrono[0], chrono[1], chrono[3], chrono[5], chrono[2], chrono[4]]

    sorted_path = os.path.join(tmpdir, "sorted.jsonl")
    merged_path = os.path.join(tmpdir, "merged.jsonl")
    _write_jsonl(sorted_path, chrono)
    _write_jsonl(merged_path, merged)

    want = bh.tail_clean_streak(bh.load_history(sorted_path))
    check("the sorted file's streak is the three clean boots at the end",
          want, 3)

    # The fixture has to actually exercise the hazard. If a later edit made the
    # union order incidentally chronological, every other assertion here would
    # still pass while testing nothing -- so pin the wrong answer explicitly.
    # Reading the merged file's *raw* order stops one record in, at t2's
    # WEDGE, and reports a streak of 1 for a tree that has booted clean 3
    # times running.
    check("raw file order really does give the wrong answer (else this test "
          "proves nothing)", bh.tail_clean_streak(merged), 1)

    got = bh.load_history(merged_path)
    check("load_history returns union-merged records in time order",
          [r["commit"] for r in got], [r["commit"] for r in chrono])
    check("and so the streak is the same as the sorted file's",
          bh.tail_clean_streak(got), want)


def test_any_shuffle_of_the_history_gives_the_same_streak(bh, tmpdir):
    """The stronger form: order must carry no information at all.

    The union-merge test above pins one specific reordering. This one asserts
    the general property, over many random permutations, so that a partial fix
    -- a sort that only handles the shapes union merge happens to produce --
    fails here.
    """
    chrono = _streak_records()
    want_commits = [r["commit"] for r in chrono]
    rng = random.Random(20260829)  # Fixed seed: a flaky test proves nothing.
    bad_order_seen = False
    for i in range(50):
        shuffled = chrono[:]
        rng.shuffle(shuffled)
        if bh.tail_clean_streak(shuffled) != 3:
            bad_order_seen = True
        path = os.path.join(tmpdir, f"shuffled{i}.jsonl")
        _write_jsonl(path, shuffled)
        got = bh.load_history(path)
        if [r["commit"] for r in got] != want_commits:
            check(f"shuffle {i} restored to time order", got, chrono)
            return
        if bh.tail_clean_streak(got) != 3:
            check(f"shuffle {i} gives the chronological streak",
                  bh.tail_clean_streak(got), 3)
            return
    check("50 shuffles all load in time order with streak 3", True, True)
    check_true("at least one shuffle would have given the wrong streak "
               "unsorted", bad_order_seen)


def test_a_damaged_ts_sorts_early_instead_of_raising(bh, tmpdir):
    """One bad record must not cost us the whole history.

    `ts: null` and a numeric `ts` both compare-fail against `str` under a bare
    `r.get("ts", "")` key, and the resulting TypeError would escape
    `load_history` -- defeating the per-line recovery the loader is built
    around, and taking out `--streaks` entirely over a single damaged line.
    They sort first instead: the safe direction, since a record too damaged to
    place must not be able to displace the genuinely-latest one from the end.
    """
    chrono = _streak_records()
    damaged = [{"ts": None, "verdict": "WEDGE", "commit": "cnull"},
               {"ts": 17, "verdict": "WEDGE", "commit": "cnum"},
               {"verdict": "WEDGE", "commit": "cmissing"}]
    path = os.path.join(tmpdir, "damaged.jsonl")
    # Written *last* in the file, so a loader that failed to sort them would
    # end its walk on them and report a zero streak. The bare string is a
    # separate case: valid JSON, but not a record, so the loader drops it
    # rather than handing a caller something with no `.get`.
    _write_jsonl(path, chrono + damaged + ["not an object at all"])

    got = bh.load_history(path)
    check("the damaged records are kept, the non-object is not",
          len(got), len(chrono) + len(damaged))
    check("nothing that is not a record survives the loader",
          [r for r in got if not isinstance(r, dict)], [])
    check("the damaged records sort to the front",
          sorted(r["commit"] for r in got[:3]), ["cmissing", "cnull", "cnum"])
    check("...leaving the datable records in time order behind them",
          [r["commit"] for r in got[3:]], [r["commit"] for r in chrono])
    check("and the streak is unaffected by them", bh.tail_clean_streak(got), 3)


def test_the_real_history_has_a_uniform_utc_offset(bh):
    """`load_history` sorts `ts` as a string; that needs one offset, not two.

    Same-offset ISO-8601 sorts correctly lexicographically -- `_now_iso()`
    hardcodes `timezone.utc`, so every record ever written carries a literal
    `+00:00`. Mix in a `-04:00` and string order stops being time order for
    the overlapping hours, which would misplace exactly the records written
    around a lane handover. Asserted against the *real* file rather than a
    fixture, because the property being guarded is about the writer, and a
    fixture cannot notice a writer that changed.
    """
    path = bh.DEFAULT_HISTORY
    check_true(f"the real history exists at {bh.display_path(path)}",
               os.path.exists(path))
    if not os.path.exists(path):
        return
    offsets = set()
    untyped = 0
    with open(path, "r", encoding="utf-8") as fh:
        for line in fh:
            line = line.strip()
            if not line:
                continue
            try:
                rec = json.loads(line)
            except json.JSONDecodeError:
                continue
            ts = rec.get("ts") if isinstance(rec, dict) else None
            if isinstance(ts, str):
                offsets.add(ts[-6:])
            else:
                untyped += 1
    check("every real record's ts is a string", untyped, 0)
    check("every real record's ts carries the same UTC offset",
          offsets, {"+00:00"})


# --------------------------------------------------------------------------
# Gated self-tests: which conditionally-called suites announced themselves
#
# `skips` answers "did a suite say it was not running". This answers the harder
# question beside it: "did a suite that says nothing at all simply not run?" A
# self-test behind `if fat_ok` prints no SKIP when the condition is false -- it
# prints nothing, which is indistinguishable from a green boot unless something
# knows what its output would have looked like. Seven suites sat behind that one
# false condition for a year.
# --------------------------------------------------------------------------


def _markers_file(tmpdir, *literals):
    path = os.path.join(tmpdir, "gated-markers.json")
    with open(path, "w", encoding="utf-8", newline="") as fh:
        json.dump({"root": "main.rs",
                   "markers": {lit: {"sites": [1], "tests": ["x::self_test"]}
                               for lit in literals}}, fh)
    return path


def test_gated_ran_reports_present_and_absent(bh):
    s = _serial(bh, "[fat] Running mkfs/format self-test...\n" + S_PASS)
    got = bh.gated_ran(s, ("[fat] Running mkfs/format self-test...",
                           "[swap] Running disk backend self-test..."))
    check("a marker in the log reads true",
          got["[fat] Running mkfs/format self-test..."], True)
    check("a marker absent from the log reads false",
          got["[swap] Running disk backend self-test..."], False)


def test_gated_markers_are_literals_not_patterns(bh):
    """Every real marker contains `[`, and four contain `(` or `)` too.

    Treated as a regex, `[acpi] Running self-test...` is a character class
    followed by a literal, and matches nothing in the log -- so every gated
    suite in the kernel would be reported as never-run, on every boot, forever.
    That is a manufactured accusation against five working suites, which is
    worse than the hole this field was added to close.
    """
    text = ("[spawn] Running netstack DNS-over-IPC (ring 3) integration "
            "test...\n" + S_PASS)
    s = _serial(bh, text)
    got = bh.gated_ran(s, ("[spawn] Running netstack DNS-over-IPC (ring 3) "
                           "integration test...",))
    check("brackets and parentheses match themselves",
          list(got.values()), [True])
    # And the converse: the class the regex reading would have matched must not
    # be mistaken for the marker.
    s2 = _serial(bh, "aciRunning self-test...\n" + S_PASS)
    check("a string that only a regex reading would match does not count",
          bh.gated_ran(s2, ("[acpi] Running self-test...",)),
          {"[acpi] Running self-test...": False})


def test_gated_ran_is_recorded_when_markers_are_given(bh, tmpdir):
    args = _Args()
    args.gated_markers = _markers_file(
        tmpdir, "[fat] Running mkfs/format self-test...",
        "[swap] Running disk backend self-test...")
    rec = bh.build_record(
        _serial(bh, "[fat] Running mkfs/format self-test...\n" + S_PASS),
        "PASS", args)
    check("the field is written", "gated_ran" in rec, True)
    check("with one entry per marker", sorted(rec["gated_ran"]),
          ["[fat] Running mkfs/format self-test...",
           "[swap] Running disk backend self-test..."])
    check("recording which ran",
          rec["gated_ran"]["[fat] Running mkfs/format self-test..."], True)
    check("and which did not",
          rec["gated_ran"]["[swap] Running disk backend self-test..."], False)
    check("it survives the round trip that actually happens -- JSONL",
          json.loads(json.dumps(rec))["gated_ran"], rec["gated_ran"])


def test_gated_ran_omitted_not_emptied_without_markers(bh):
    """Absent means unknown; `{}` would mean "the kernel gates nothing".

    The same three-way distinction `sanitizer`, `accel` and `skips` all keep,
    and for a sharper reason here: an empty object is an all-clear. Downstream
    the gate counts only rows carrying the key, so a row written without a
    marker file must not be countable as a boot on which some gated suite
    failed to appear.
    """
    rec = bh.build_record(_serial(bh, S_PASS), "PASS", _Args())
    check("no marker file -> no field at all", "gated_ran" in rec, False)


def test_a_missing_marker_file_is_unknown_not_all_clear(bh, tmpdir):
    args = _Args()
    args.gated_markers = os.path.join(tmpdir, "was-never-written.json")
    rec = bh.build_record(_serial(bh, S_PASS), "PASS", args)
    check("a marker file that does not exist omits the field",
          "gated_ran" in rec, False)
    check("and the rest of the row is intact -- a harness fault must not "
          "cost the verdict", rec["verdict"], "PASS")


def test_malformed_marker_files_do_not_cost_the_row(bh, tmpdir):
    """This runs at the end of a twenty-minute boot.

    Raising here would throw away the wall time, the verdict, the skip lists and
    the failure tail to punish a harness bug in a field none of them depend on.
    """
    bad = os.path.join(tmpdir, "bad.json")
    for content in ("{not json", "[]", '{"markers": "not an object"}', "null"):
        with open(bad, "w", encoding="utf-8", newline="") as fh:
            fh.write(content)
        check(f"{content[:20]!r} yields unknown, not a crash",
              bh.load_gated_markers(bad), None)
    args = _Args()
    args.gated_markers = bad
    rec = bh.build_record(_serial(bh, S_PASS), "PASS", args)
    check("and the record still lands", rec["verdict"], "PASS")


def test_empty_marker_object_is_distinct_from_a_missing_file(bh, tmpdir):
    """A kernel with no gated call sites is a real state, and not an error.

    It is also the state this whole mechanism is trying to reach. It must be
    recordable as `{}` -- "asked, and there was nothing to ask about" -- and
    stay distinguishable from the absent field, which means nobody asked.
    """
    args = _Args()
    args.gated_markers = _markers_file(tmpdir)
    rec = bh.build_record(_serial(bh, S_PASS), "PASS", args)
    check("an empty marker set is still an answer", rec.get("gated_ran"), {})
    check("and the key is present, unlike the no-file case",
          "gated_ran" in rec, True)


# --------------------------------------------------------------------------
# Host failures
# --------------------------------------------------------------------------

#: QEMU's stderr from the boot of 2026-09-01, verbatim.
#:
#: Verbatim matters here more than in most samples. These signatures are the
#: only thing standing between "the host fell over" and "the kernel died", and
#: they are matched as literal substrings -- so a paraphrased sample would test
#: the matcher against words QEMU does not print, and would keep passing after
#: the real message stopped being recognised.
E_HOST_OOM = (
    "C:\\program files\\qemu\\qemu-system-x86_64.exe: warning: "
    "Failed to CreateFileMapping: The paging file is too small for this "
    "operation to complete.\n"
)

#: The Cygwin half of the same event: a helper in the harness died of the same
#: pressure. Recorded separately because it can appear on its own.
E_CYGHEAP = (
    "0 [main] date (85728) child_copy: cygheap read copy failed, "
    "0x0..0x80000CD40, done 0, windows pid 85728, Win32 error 299\n"
)

#: The guest's own log from that run. A boundary self-test refused an
#: allocation the host could not back, and said so in the words of a genuine
#: off-by-one in the bound check -- which is exactly why the verdict cannot be
#: derived from this stream alone.
S_HOST_OOM_GUEST = _PROLOGUE + (
    "[virtio-gpu] RESOURCE SELF-TEST FAILED: a resource exactly at the bound "
    "was rejected: I/O error (-600)\n"
    "FATAL: virtio-gpu render-resource self-test failed: internal kernel "
    "error (-1)\n"
)


def _host(verdict, why="QEMU could not map guest memory"):
    """A boot the host, not the tree, ended."""
    r = _rec(verdict)
    r["host_fail"] = why
    return r


def test_a_host_signature_overrides_a_verdict_that_blames_the_tree(bh):
    """The whole point: the run of 2026-09-01 must not read as a kernel death.

    Both directions are asserted. Without the host's stderr the same serial log
    is still a PANIC -- because that is the safe default and removing it would
    be far worse than the bug being fixed -- and with it, the same log is
    HOST_FAIL. The pair is what proves the override is doing the work rather
    than the sample having been chosen to classify harmlessly.
    """
    s = _serial(bh, S_HOST_OOM_GUEST)
    check("without host evidence the guest log still reads as a kernel death",
          bh.classify(s, 1), "PANIC")
    check("with QEMU's own stderr it is a host failure",
          bh.classify(s, 1, E_HOST_OOM), "HOST_FAIL")
    check("...and the cygwin signature alone is enough on its own",
          bh.classify(s, 1, E_CYGHEAP), "HOST_FAIL")
    check("unrelated host chatter changes nothing",
          bh.classify(s, 1, "qemu: warning: guest updated active QH\n"),
          "PANIC")


def test_a_kernel_cannot_forge_a_host_failure(bh):
    """The guest must not be able to excuse itself.

    `known-issues.md` names this as one of two things to get right, and it is
    the one with teeth: if the signatures were searched for in the serial log,
    any kernel -- or any test fixture, or any string a userspace program
    printed -- could opt out of being blamed by printing eleven words. The
    stream is chosen, not the words.
    """
    forged = _serial(bh, _PROLOGUE + (
        "[gpu] Failed to CreateFileMapping: The paging file is too small\n"
        "PANIC: kernel wrote a shared CoW page\n"
    ))
    check("the signature in the GUEST's log does not excuse the guest",
          bh.classify(forged, 1), "PANIC")
    check("...not even when it is the whole log",
          bh.classify(forged, 1, ""), "PANIC")


def test_a_host_signature_never_rewrites_a_clean_verdict(bh):
    """A boot that got where it was going got there, warning or no warning.

    The override runs downward only. Rewriting a PASS would *destroy* a real
    clean boot, and this file's standing bias is that a manufactured or
    shortened clean streak are not symmetric errors -- one hides failures, the
    other only adds them.
    """
    s = _serial(bh, S_PASS)
    check("a host warning does not un-boot a passing kernel",
          bh.classify(s, 0, E_HOST_OOM), "PASS")
    check("...nor a PASS_TOOLING", bh.classify(s, 3, E_HOST_OOM),
          "PASS_TOOLING")
    check("...nor the documented bench livelock",
          bh.classify(_serial(bh, S_PASS, marker="BENCH_OK"), 1, E_HOST_OOM),
          "BENCH_INCOMPLETE")
    # NO_BOOT is not a verdict about the tree either, and main() declines to
    # record it. Promoting it would file a row with no boot in it.
    check("a run with no serial output at all stays NO_BOOT",
          bh.classify(None, 1, E_HOST_OOM), "NO_BOOT")


#: A boot that got underway, said ordinary things, and never reached its
#: marker: no panic, no stall, nothing wrong with it but the missing marker.
#: Deliberately the blandest possible failing log, so that what the tests below
#: change is the exit status and nothing else.
S_NO_MARKER = _PROLOGUE + "[shell] prompt ready\n"


def test_an_exit_status_the_harness_cannot_produce_is_not_a_boot_outcome(bh):
    """The run of 2026-09-02: exit 127, filed as TIMEOUT, streak reset.

    That boot ran for 395 seconds and then the *harness* died -- bash could not
    fork a child, because the host had reached 96% of its Windows commit limit.
    Nothing about it was a statement about the kernel, yet it was recorded as a
    TIMEOUT, it reset the consecutive-clean streak that four `known-issues.md`
    entries use as their closure bar, and it was matched against a lane-B hang.

    The stderr-based override could not catch it: that reads QEMU's stderr, and
    the fork that failed was not QEMU's. The exit status is the evidence that
    was there all along.
    """
    s = _serial(bh, S_NO_MARKER)
    check("exit 127 with no host stderr is still a host failure",
          bh.classify(s, 127), "HOST_FAIL")
    check("...and so is 126", bh.classify(s, 126), "HOST_FAIL")
    check("the same log under a status the harness DOES produce is a TIMEOUT",
          bh.classify(s, 1), "TIMEOUT")


def test_a_harness_abort_does_not_erase_a_panic_the_log_recorded(bh):
    """The counter-example that was already in the history: 2026-09-01T11:06.

    That boot exited 127 *and* its log ends
    `FATAL: virtio-gpu render-resource self-test failed`. Both happened -- the
    kernel died, then the harness could not fork on its way out -- and the
    first draft of the exit-status override would have rewritten it to
    HOST_FAIL, deleting a real FATAL from the counts. That is the direction
    this whole file exists to prevent, so it is asserted rather than trusted.

    The line the override must not cross: PANIC is read out of the log, and a
    harness that stumbled afterwards cannot un-say what the kernel already
    said. TIMEOUT and SELFTEST_FAIL rest on absence -- a marker that never
    arrived, a gate that never reported -- which is precisely what a harness
    that stopped running is in no position to testify to.
    """
    panicked = _serial(bh, _PROLOGUE + (
        "[virtio-gpu] RESOURCE SELF-TEST FAILED: a resource exactly at the "
        "bound was rejected: I/O error (-600)\n"
        "FATAL: virtio-gpu render-resource self-test failed: internal kernel "
        "error (-1)\n"
    ))
    check("a panic in the log survives the harness dying after it",
          bh.classify(panicked, 127), "PANIC")
    check("...and 126 likewise", bh.classify(panicked, 126), "PANIC")
    # The stderr half is guarded differently on purpose, and the pair is what
    # shows the difference is deliberate rather than an oversight in one of
    # them: QEMU saying it could not get memory explains the panic; a fork
    # that failed minutes later explains nothing but itself.
    check("QEMU's own stderr still does override a panic",
          bh.classify(panicked, 127, E_HOST_OOM), "HOST_FAIL")
    # A gate verdict rests on the gate having reported. Exit 127 means it did
    # not, so the accusation is retracted even though the marker was reached.
    check("a verdict resting on a gate that never ran is retracted",
          bh.classify(_serial(bh, S_PASS), 127), "HOST_FAIL")
    check("...where the same log with a status the harness produces accuses",
          bh.classify(_serial(bh, S_PASS), 1), "SELFTEST_FAIL")


def test_run_timeout_expiry_is_a_statement_about_the_tree(bh):
    """124 is outside the harness's vocabulary and must NOT be excused.

    The tempting rule -- "any status boot-test.sh does not emit means the host
    failed" -- would swallow this one, and 124 is exactly the case where that
    is wrong: `run-timeout.py` returns it when the whole tree overran its
    budget, which is a fact about something not finishing. Excusing it would
    delete the loudest hang signal the harness has.
    """
    s = _serial(bh, S_NO_MARKER)
    check("run-timeout's expiry is not a harness abort",
          bh.harness_abort(124), None)
    check("...and does not become a HOST_FAIL", bh.classify(s, 124), "TIMEOUT")


def test_a_kernel_cannot_forge_a_harness_abort(bh):
    """The exit status is chosen for the same reason QEMU's stderr is.

    It is set by the shell, above the emulator, after the kernel has had its
    say -- so no string the guest prints can reach it. Asserted explicitly
    because the value of both overrides is entirely in this property: an
    excuse the accused can issue itself is not an excuse.
    """
    forged = _serial(bh, _PROLOGUE + (
        "[test] harness exit 127: could not run a command at all\n"
        "PANIC: kernel wrote a shared CoW page\n"
    ))
    check("the words in the guest's log do not excuse the guest",
          bh.classify(forged, 1), "PANIC")


def test_a_harness_abort_cannot_collide_with_a_clean_verdict(bh):
    """The harness half reaches the downward guard differently, and must.

    The stderr override needs the guard: QEMU can print a warning on a boot
    that then passes, so PASS and a host signature genuinely co-occur. The
    harness override cannot -- the exit status is *both* the trigger and the
    thing every clean verdict is derived from, and no clean verdict is derived
    from 126 or 127. So `classify(passing log, 127)` is HOST_FAIL, and that is
    the right answer rather than a hole in the guard: a harness that could not
    run a command did not run its gates either, so nothing about that run had
    yet been checked. The marker in the log says the kernel got somewhere; it
    does not say the run finished.

    Asserted as a property of the two sets, not of one sample, so that adding
    a clean verdict on some future exit status cannot quietly make a
    harness-aborted run readable as clean.
    """
    for code in bh.HARNESS_ABORT_EXITS:
        check_true(f"exit {code} derives no clean verdict from a passing log",
                   bh._verdict_from_evidence(_serial(bh, S_PASS), code)
                   not in bh.CLEAN_VERDICTS)
        check(f"...so exit {code} on a passing log reads as HOST_FAIL",
              bh.classify(_serial(bh, S_PASS), code), "HOST_FAIL")
    # The upward guard is real for both halves and is checked for both: a run
    # with no serial output at all has no boot in it to file.
    check("a run with no serial output at all stays NO_BOOT",
          bh.classify(None, 127), "NO_BOOT")


def test_the_recorded_reason_and_the_verdict_come_from_one_derivation(bh):
    """A HOST_FAIL row must never carry an empty `host_fail`.

    `classify` and `main` decide separately whether to override and what to
    write beside it. `not_about_the_tree` is the single derivation they share;
    if it were ever forked into two, the failure would be silent -- a row that
    says HOST_FAIL and cannot say why, which is precisely the assertion the
    reader has no way to check.
    """
    s = _serial(bh, S_NO_MARKER)
    for stderr, code in ((E_HOST_OOM, 1), ("", 127), (E_CYGHEAP, 127)):
        why = bh.not_about_the_tree(stderr, code)
        check_true(f"stderr={bool(stderr)} exit={code}: a reason exists",
                   bool(why))
        check(f"stderr={bool(stderr)} exit={code}: and the verdict agrees",
              bh.classify(s, code, stderr), "HOST_FAIL")
    check("an ordinary failing boot is left alone",
          bh.not_about_the_tree("", 1), None)
    check("QEMU's words win over the exit status when both are present",
          bh.not_about_the_tree(E_HOST_OOM, 127),
          "QEMU could not map guest memory")


def test_every_signature_is_matched_and_named(bh):
    """Each signature fires, and each names a reason a human can act on.

    A signature nobody exercises is a signature that may already have been
    broken by a typo -- and this list is short precisely so that every entry
    can be checked, which is worth nothing unless it is.
    """
    for needle, reason in bh.HOST_FAIL_SIGNATURES:
        check(f"{needle!r} matches", bh.host_failure(f"noise\n{needle}\nmore"),
              reason)
        check_true(f"...and {needle!r} names a reason", bool(reason.strip()))
    check("nothing matches an empty stream", bh.host_failure(""), None)
    check("nothing matches ordinary QEMU chatter",
          bh.host_failure("qemu-system-x86_64: warning: TCG doesn't support "
                          "requested feature\n"), None)


def test_a_missing_qemu_stderr_is_absence_of_evidence(bh, tmpdir):
    """No file must mean "no host evidence", never an error and never a match.

    Every caller of this script other than boot-test.sh legitimately has
    nothing to point at -- `--classify` on an old log, `--list`, this suite --
    and the reading that leaves the existing verdict standing is the only safe
    one. An unreadable file has to behave the same way: swallowing there can
    only leave a failure blamed on the tree, never excuse one.
    """
    check("no path at all", bh.read_qemu_stderr(None), "")
    check("empty path", bh.read_qemu_stderr(""), "")
    check("a path that does not exist",
          bh.read_qemu_stderr(os.path.join(tmpdir, "nope.txt")), "")
    # A directory: OSError rather than FileNotFoundError, i.e. the other arm.
    check("a path that is not a file", bh.read_qemu_stderr(tmpdir), "")

    real = os.path.join(tmpdir, "qemu-stderr.txt")
    with open(real, "w", encoding="utf-8", newline="") as fh:
        fh.write(E_HOST_OOM)
    check("and a real file is read", bh.read_qemu_stderr(real), E_HOST_OOM)


def test_the_host_reason_is_recorded_on_the_row(bh):
    """`HOST_FAIL` alone is an assertion the reader cannot check.

    `build/qemu-stderr.txt` is deleted by the next run, so the row is the last
    surviving copy of why the host was blamed. A row that says only the verdict
    invites the next reader to take it on trust or to disbelieve it, with no
    way to do either.
    """
    args = _Args()
    rec = bh.build_record(_serial(bh, S_HOST_OOM_GUEST), "HOST_FAIL", args,
                          host_fail="QEMU could not map guest memory")
    check("the reason is stored", rec.get("host_fail"),
          "QEMU could not map guest memory")
    plain = bh.build_record(_serial(bh, S_PASS), "PASS", args)
    check("and an ordinary boot carries no such key",
          "host_fail" in plain, False)


def test_a_host_failure_is_not_evidence_about_the_tree(bh):
    """It must be stepped over exactly as a probe is, and for the same reason.

    This is the bug being fixed, stated as a test: on 2026-09-01 a host that
    could not grow its pagefile in time turned a nine-boot clean streak into
    zero. The streak is a published quantity -- four open `known-issues.md`
    entries have closure conditions written as counts of consecutive clean
    boots -- so a run the tree had no part in must neither end one nor extend
    one.
    """
    records = [_rec("PASS"), _host("HOST_FAIL"), _rec("PASS")]
    check("a host failure does not break the clean streak",
          bh.tail_clean_streak(records), 2)
    check("...and the same records without the host verdict do break it",
          bh.tail_clean_streak([_rec("PASS"), _rec("PANIC"), _rec("PASS")]), 1)
    check("...and it cannot extend one either",
          bh.tail_clean_streak([_rec("PASS"), _host("HOST_FAIL")]), 1)

    st = {s.fp.id: s for s in bh.streaks(records)}["W1"]
    check("a host failure is not counted toward a fingerprint's clean run",
          st.since_last, 2)
    check("...nor among the records considered", st.recorded, 2)


def test_describes_tree_keeps_the_two_exclusions_apart(bh):
    """Two reasons to exclude a row, deliberately not merged into one flag.

    `known-issues.md` names widening `is_experiment` as the mistake to avoid:
    a flag meaning "invoked deliberately under non-default conditions" that
    also meant "the host hiccupped" could be satisfied by whichever of the two
    nobody was thinking about, and a real regression would then be excused by a
    predicate that was never about it.
    """
    check("a host failure is not an experiment",
          bh.is_experiment(_host("HOST_FAIL")), False)
    check("...but it is still not evidence about the tree",
          bh.describes_tree(_host("HOST_FAIL")), False)
    check("a probe is excluded by the other half",
          bh.describes_tree(_probe("TIMEOUT")), False)
    check("an ordinary failing boot is evidence, and stays counted",
          bh.describes_tree(_rec("PANIC")), True)


def test_host_failure_wall_and_build_times_are_not_measurements(bh):
    """A contended host measures contention, not the tree.

    Both numbers go: the boot that could not get memory was competing for the
    machine, and the compile that preceded it was competing for the same
    machine. Keeping either inflates a median that a later run is judged
    against.
    """
    def timed(wall, build, host=False):
        r = _host("HOST_FAIL") if host else _rec("PASS")
        r["wall_seconds"] = wall
        r["build_seconds"] = build
        return r

    records = [timed(120, 50), timed(120, 50), timed(900, 400, host=True)]
    check("the host failure is in no wall-time population",
          sorted(v for vals in bh.wall_populations(records).values()
                 for v in vals),
          [120.0, 120.0])
    check("...nor in any build-time population",
          sorted(v for vals in bh.build_populations(records).values()
                 for v in vals),
          [50.0, 50.0])


def test_a_host_failure_matches_no_known_issue(bh):
    """A host OOM lands on whatever allocation the kernel was making.

    So a host-killed boot can wear the shape of a real bug: the fingerprints
    match on the shape of the exception in the log and do not consult the
    verdict. Recording that would file a recurrence of an issue that did not
    recur, against a run the kernel was not responsible for, in the very
    counter several closure bars are written in.

    The second assertion is what makes the first mean anything: the identical
    serial log, classified without host evidence, *does* match.
    """
    s = _serial(bh, S_PTHREAD)
    check("the same log does match when the kernel is to blame",
          bh.fingerprints_for(s, bh.classify(s, 1)), ["B-PTHREAD-TEARDOWN-PF"])
    check("but a host-killed boot records no recurrence",
          bh.fingerprints_for(s, bh.classify(s, 1, E_HOST_OOM)), [])


def test_the_host_verdict_is_documented_for_the_reader(bh):
    """Every verdict this script can emit must explain itself in `report`.

    A bare `HOST_FAIL` in the console output of a twenty-minute boot is a word
    the reader has to go and look up, at the moment they are least inclined to.
    """
    check_true("HOST_FAIL has help text", bool(bh.VERDICT_HELP.get("HOST_FAIL")))
    check("and it is not counted as clean",
          "HOST_FAIL" in bh.CLEAN_VERDICTS, False)


def main():
    bh = load_module()
    tests = [(name, fn) for name, fn in list(globals().items())
             if name.startswith("test_") and callable(fn)]
    # A discovery mechanism that discovers nothing looks exactly like a suite
    # that passes -- the failure mode this whole script is about. Assert a floor.
    if len(tests) < 100:
        print(f"FATAL: test discovery found only {len(tests)} tests; the suite "
              f"has at least 100. Discovery is broken, not the code.")
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
