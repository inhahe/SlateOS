#!/usr/bin/env python3
"""Regression tests for `scripts/layout-sweep.py`.

Run: `python scripts/test-layout-sweep.py` (exit 0 = pass, 1 = fail).
No pytest dependency, for the same reason `test-bench-history.py` has none.

Why this file exists
--------------------
`layout-sweep.py` has a self-test (`--self-test`) that verifies the *padding
mechanism*, and it is thorough -- it builds real kernels and checks that every
shared `.text` symbol moved by exactly the pad. What it does not, and cannot,
cover is everything the sweep does *around* the arms: argument parsing, the
minimum-arm guard, and whether the boot test can be invoked at all. Those are
the parts whose failures are cheapest to prevent and most expensive to
discover, because the sweep spends ten minutes building before it reaches any
of them and hours before it finishes.

The concrete history: the first real release sweep passed a 34-minute
self-test and then died on its first arm with

    /bin/bash: D:\\visual studio projects\\...\\scripts\\boot-test.sh:
        No such file or directory

-- exit 127, from a file that was plainly present. The absolute path was
handed to an MSYS bash, which cannot open a drive-letter path with
backslashes. Nothing in the sweep asked the question early, and the message it
finally produced pointed at the wrong thing.
"""

from __future__ import annotations

import argparse
import importlib.util
import inspect
import os
import sys

REPO_ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
SCRIPT = os.path.join(REPO_ROOT, "scripts", "layout-sweep.py")

_FAILURES = []


def load_module():
    """Import layout-sweep.py by path (its name is not a valid identifier)."""
    spec = importlib.util.spec_from_file_location("layout_sweep", SCRIPT)
    if spec is None or spec.loader is None:
        raise RuntimeError(f"cannot load {SCRIPT}")
    module = importlib.util.module_from_spec(spec)
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


def test_the_sweep_picks_a_bash_that_can_actually_run_the_boot_test(ls):
    """The root cause behind two dead sweeps, and its two separate symptoms.

    Windows `CreateProcess` searches `System32` before `PATH`, so on a machine
    with WSL installed `subprocess.run(["bash", ...])` launches WSL's bash
    regardless of `PATH` -- and regardless of `shutil.which("bash")`, which
    implements a `PATH` search and so answers a different question than the one
    that decides. WSL's bash parses the boot test perfectly and has no QEMU.

    Symptom one was `No such file or directory` on an absolute path; fixing
    that made the error go away without fixing anything, and symptom two was
    `ERROR: qemu-system-x86_64 not found` on the very next attempt. So the test
    asserts the *capability*, not the parse.
    """
    bash, message = ls.find_bash()
    check("a usable bash is found", bash is not None, True)
    check("...and it is named, so the choice is on the record",
          "preflight" in message and (bash or "") in message, True)

    ok, detail = ls.bash_can_run_boot_test(bash)
    check("the chosen bash can find QEMU and OVMF, not merely parse the script",
          ok, True)
    check("...and the receipt says which of the two it checked",
          "QEMU" in detail, True)

    # The constant itself: an absolute path here would reintroduce symptom one
    # for any candidate that cannot open drive-letter paths.
    check("the sweep still names the boot test relatively",
          os.path.isabs(ls.BOOT_TEST), False)

    # `bash` must be the fallback, never the preference: it is the one entry
    # whose meaning is decided by the OS rather than by this list.
    check("bare `bash` is the last candidate, not the first",
          ls.BASH_CANDIDATES[-1], "bash")


def test_a_bash_that_cannot_find_qemu_is_rejected_however_well_it_parses(ls):
    """The distinction the old preflight could not draw.

    A `bash -n` check passes under WSL, and that pass is what let a sweep start
    that could never finish. If both bashes exist on this machine, this asserts
    the rejection directly; if only one does, it asserts the weaker but still
    real invariant that whatever was chosen passes the dependency half.
    """
    wsl = r"C:\Windows\System32\bash.exe"
    if not os.path.exists(wsl):
        print("      (no WSL bash on this machine; the discriminating case "
              "cannot be exercised here)")
        chosen, _ = ls.find_bash()
        ok, _ = ls.bash_can_run_boot_test(chosen)
        check("the chosen bash still passes the dependency probe", ok, True)
        return

    parses = ls.subprocess.run([wsl, "-n", ls.BOOT_TEST], cwd=REPO_ROOT,
                               capture_output=True, text=True)
    check("WSL's bash parses the boot test perfectly", parses.returncode, 0)

    ok, detail = ls.bash_can_run_boot_test(wsl)
    check("...and is still rejected, because it cannot run it", ok, False)
    check("...with the reason naming the dependency, not the syntax",
          "cannot find what it needs" in detail, True)

    chosen, _ = ls.find_bash()
    check("so the sweep does not choose it",
          os.path.normcase(chosen or "") == os.path.normcase(wsl), False)


def test_the_dependency_probe_is_the_boot_tests_own(ls):
    """A Python translation of the QEMU search would be a second opinion.

    The candidate lists are MSYS- and Windows-style paths that differ per
    machine and per bash flavour. A reimplementation that says "found" while
    the boot test says "not found" is the worst answer a preflight can give: it
    certifies the sweep, and then the sweep dies anyway.
    """
    fragment = ls.extract_dependency_probe()
    check("the probe mentions QEMU", "qemu-system-x86_64" in fragment, True)
    check("...and OVMF, so it is not checking half the dependencies",
          "OVMF" in fragment, True)
    check("...and it is the script's real search, not a rewrite",
          'command -v "$candidate"' in fragment, True)

    # It must refuse rather than silently probe nothing if the block moves.
    import tempfile
    with tempfile.TemporaryDirectory() as tmp:
        decoy = os.path.join(tmp, "boot-test.sh")
        with open(decoy, "w", encoding="utf-8", newline="\n") as handle:
            handle.write("#!/bin/sh\necho no dependency discovery here\n")
        try:
            ls.extract_dependency_probe(decoy)
            outcome = "silently returned something"
        except RuntimeError as exc:
            outcome = "refused" if "Find QEMU" in str(exc) else str(exc)
    check("a boot test whose dependency block moved makes the probe refuse",
          outcome, "refused")


def test_a_missing_boot_test_is_refused_before_any_build(ls):
    """The preflight must fail loudly, not fall through to the arms."""
    bash, message = ls.find_bash(script="scripts/no-such-boot-test.sh")
    check("a boot test that is not there yields no usable bash", bash, None)
    check("...and the message says the sweep is refusing to start",
          "Refusing to start" in message, True)
    check("...and lists what each candidate said, which is the diagnosis",
          "cannot parse" in message, True)


def test_pad_values_are_validated_before_hours_are_spent(ls):
    """`parse_pads` is the last chance to reject a sweep that measures nothing.

    The 4096 case is the one that matters and the one that looks harmless: a
    pad of exactly one guest page shifts every function by a whole page and so
    *preserves every page-straddle relationship*. It is a correct build and a
    worthless sample, and a sweep made of them reports a layout band of zero --
    which would then certify every future layout artifact as a real regression.
    """
    check("ordinary pads parse in order", ls.parse_pads("0,1024,2048"),
          [0, 1024, 2048])
    check("whitespace and empty fields are tolerated",
          ls.parse_pads(" 0 , 1024 ,, 2048 "), [0, 1024, 2048])

    try:
        ls.parse_pads("1024,1024")
        duplicated = "accepted"
    except argparse.ArgumentTypeError as exc:
        duplicated = str(exc)
    check("a duplicate pad is refused", duplicated, "duplicate pad values")

    try:
        ls.parse_pads("-16")
        negative = "accepted"
    except argparse.ArgumentTypeError as exc:
        negative = str(exc)
    check("a negative pad is refused", negative, "pad values must be >= 0")

    # 4096 is accepted -- the self-test needs it as its negative control -- but
    # it must not pass silently.
    import contextlib
    import io
    buf = io.StringIO()
    with contextlib.redirect_stdout(buf):
        pads = ls.parse_pads("0,4096")
    check("a page-multiple pad is still parsed", pads, [0, 4096])
    check("...but is warned about as a sample that measures nothing",
          "measures nothing" in buf.getvalue(), True)
    check("...and the warning explains why (it preserves every straddle)",
          "straddle" in buf.getvalue(), True)


def test_the_arm_minimum_comes_from_the_analyser_not_a_local_copy(ls):
    """A hardcoded 3 here would let a two-arm sweep run to completion.

    Forty minutes of QEMU per arm, producing a history from which no band can
    ever be computed, reported as a successful sweep -- this project's
    recurring failure shape, a check that cannot fire presenting as a check
    that found nothing. The guard therefore reads the minimum out of
    `bench-history.py`, and this asserts that the import path still works and
    still yields a usable number.
    """
    bh = ls.bench_history()
    minimum = bh.MIN_PADS_FOR_LAYOUT_BAND
    check("the analyser's minimum is importable and sane",
          isinstance(minimum, int) and minimum >= 3, True)

    # The regex comes from there too, not a local copy -- and it is asserted
    # against the *kernel's own banner*, not a convenient substring. The whole
    # sweep rests on this one coupling: the kernel prints the pad it was built
    # with, and the analyser recovers it. If either side renames the key, every
    # arm silently becomes an unattributable record and the band is computed
    # from nothing.
    banner = "[boot] build profile: sanitizer=none textpad=1536"
    match = bh.TEXTPAD_RE.search(banner)
    check("the analyser reads the pad out of the kernel's boot banner",
          match.group(1) if match else None, "1536")

    main_rs = os.path.join(REPO_ROOT, "kernel", "src", "main.rs")
    with open(main_rs, "r", encoding="utf-8", errors="replace") as handle:
        source = handle.read()
    check("...and the kernel still prints that banner in that shape",
          '"[boot] build profile: sanitizer={} textpad={}"' in source, True)

    import contextlib
    import io
    buf = io.StringIO()
    with contextlib.redirect_stdout(buf):
        rc = ls.run_sweep(list(range(minimum - 1)), "release", "unused")
    check("a sweep with too few arms is refused up front", rc, 2)
    check("...before bash, cargo or QEMU is touched",
          "preflight" in buf.getvalue(), False)
    check("...and the message names the minimum it is short of",
          f"needs {minimum} distinct layouts" in buf.getvalue(), True)


class FakeHistory:
    """A `bench-history` module whose history is whatever the test says.

    Everything except `load_history`/`DEFAULT_HISTORY` is the *real* module, so
    `layout_arm_rejection` under test is the function `layout_arms` actually
    calls. Stubbing that too would make this suite assert only that the sweep
    calls something -- and the entire value of the guard is that it calls
    *that* something.
    """

    def __init__(self, real, records):
        self._real = real
        self._records = records
        self.DEFAULT_HISTORY = "<fake>"

    def load_history(self, path):
        return self._records

    def __getattr__(self, name):
        return getattr(self._real, name)


def test_an_arm_that_would_not_be_counted_stops_the_sweep(ls):
    """The guard against a third silently-voided sweep.

    The failure it exists to catch is not a failed run. It is a *successful*
    one whose record `layout_arms()` discards -- which is what a dirty tree
    produces, and which cost the second sweep attempt three hours that would
    have ended in `--layout-bands` printing nothing.
    """
    import platform

    bh = ls.bench_history()
    host = platform.node() or "unknown"

    def record(**overrides):
        # The experiment tag is part of what makes a row an *arm*, not
        # decoration: every run reports a `text_pad`, so an untagged row is an
        # ordinary run rather than a deliberately perturbed one.
        base = {"host": host, "profile": "release", "text_pad": 1024,
                "commit": "abc1234", "dirty": False,
                "experiment": f"{bh.LAYOUT_SWEEP_TAG}1024 (identical source)"}
        base.update(overrides)
        return base

    ok, message, _ = ls.check_arm_counts(
        FakeHistory(bh, [record()]), 1024, "release")
    check("a clean, padded, committed arm is accepted", ok, True)
    check("...and says so, so acceptance is a receipt and not a silence",
          "accepted as an arm" in message, True)

    # The regression test for the 2026-08-19 near-miss: an untagged run is not
    # an arm, however clean and however well-padded. The WHPX-vs-TCG probe was
    # unpadded (so `textpad=0`), on a kernel predating the accel banner (so
    # `accel` absent, reading exactly like a TCG arm), and built from source
    # identical to the TCG sweep (so it shared that sweep's digest). Nothing
    # else in the record could have kept it out of the TCG band.
    ok, message, _ = ls.check_arm_counts(
        FakeHistory(bh, [record(experiment=None)]), 1024, "release")
    check("an untagged run is not an arm, however clean", ok, False)
    ok, _, _ = ls.check_arm_counts(
        FakeHistory(bh, [record(experiment="WHPX vs TCG: a different probe")]),
        1024, "release")
    check("a differently-tagged experiment is not an arm either", ok, False)

    ok, message, _ = ls.check_arm_counts(
        FakeHistory(bh, [record(dirty=True)]), 1024, "release")
    check("a dirty arm stops the sweep", ok, False)
    check("...naming the reason rather than just failing",
          "dirty" in message and "commit" in message, True)
    check("...and saying why continuing is pointless",
          "no band" in message, True)

    ok, _, _ = ls.check_arm_counts(
        FakeHistory(bh, [record(text_pad=None)]), 1024, "release")
    check("an arm with no recorded pad stops the sweep", ok, False)

    ok, _, _ = ls.check_arm_counts(
        FakeHistory(bh, [record(commit=None)]), 1024, "release")
    check("an arm with no commit stops the sweep", ok, False)

    ok, message, _ = ls.check_arm_counts(
        FakeHistory(bh, [record(profile="debug")]), 1024, "release")
    check("a debug arm cannot calibrate a release band", ok, False)

    ok, message, _ = ls.check_arm_counts(FakeHistory(bh, []), 1024, "release")
    check("a run that wrote no row at all stops the sweep", ok, False)
    check("...distinguishing 'wrote nothing' from 'wrote something rejected'",
          "wrote no history row" in message, True)

    # The newest row must be *this* arm's. Judging someone else's row would
    # report a verdict about a different run as though it were about this one.
    ok, message, _ = ls.check_arm_counts(
        FakeHistory(bh, [record(text_pad=2048)]), 1024, "release")
    check("a newest row from some other run stops the sweep", ok, False)
    check("...because the arms would no longer be the runs this script did",
          "Something else appended" in message, True)


def test_an_arm_that_lands_in_a_different_group_stops_the_sweep(ls):
    """The check no per-record predicate can perform.

    Every arm of the 2026-08-19 sweep passed `layout_arm_rejection` on its own
    merits, and the sweep still yielded nothing, because each landed in a group
    of one. `arm_group_key` removed that sweep's specific cause, but "each arm
    is individually valid" still does not imply "the arms are comparable to
    each other" -- grouping is a relation between two records, and a predicate
    shown one record at a time cannot evaluate a relation.

    So this asserts the relation directly: two arms whose keys differ must stop
    the sweep, and the message must say which two and what changed, because the
    operator's next question is always "changed how?".
    """
    import platform

    bh = ls.bench_history()
    host = platform.node() or "unknown"

    def record(pad, **overrides):
        base = {"host": host, "profile": "release", "text_pad": pad,
                "commit": "abc1234", "dirty": False,
                "src_digest": "full:1111111111111111", "accel": "TCG",
                "experiment": f"{bh.LAYOUT_SWEEP_TAG}{pad} (identical source)"}
        base.update(overrides)
        return base

    ok, message, first = ls.check_arm_counts(
        FakeHistory(bh, [record(0)]), 0, "release")
    check("the first arm has no predecessor to disagree with", ok, True)
    check("...and it reports its group so the next arm can be compared",
          first, ("full:1111111111111111", "TCG"))
    check("...naming the group in the receipt, not just computing it",
          "full:1111111111111111" in message, True)

    ok, message, _ = ls.check_arm_counts(
        FakeHistory(bh, [record(1024)]), 1024, "release", (0, first))
    check("an arm matching the previous one's group continues the sweep",
          ok, True)

    # The dangerous case: individually flawless, collectively useless.
    moved = record(2048, src_digest="full:2222222222222222")
    ok, message, _ = ls.check_arm_counts(
        FakeHistory(bh, [moved]), 2048, "release", (1024, first))
    check("an arm whose source moved under the sweep stops it", ok, False)
    check("...even though that arm is individually a valid arm",
          bh.layout_arm_rejection(moved, host, "release"), None)
    check("...naming both pads, so the divergence can be located in time",
          "pad=1024" in message and "pad=2048" in message, True)
    check("...and showing both keys rather than only asserting they differ",
          "1111111111111111" in message and "2222222222222222" in message,
          True)

    # The accelerator is half the key, and the half most likely to change
    # without anyone touching the tree: QEMU silently falls back to TCG when
    # WHPX is unavailable, and a TCG arm is ~3.5x slower than a WHPX one. Two
    # such arms in one band would report a code-placement sensitivity that is
    # really a hypervisor difference.
    ok, message, _ = ls.check_arm_counts(
        FakeHistory(bh, [record(2048, accel="Hyper-V/WHPX")]),
        2048, "release", (1024, first))
    check("an arm that ran on a different accelerator stops the sweep",
          ok, False)
    check("...and says the accelerator rescales the measurements",
          "accelerator" in message, True)


def test_the_group_guard_shares_its_key_with_the_analyser(ls):
    """A second statement of the grouping rule would drift from the first.

    Same argument as the predicate coupling below: the sweep must ask
    `bench-history.py` what group a record is in, not decide for itself, or the
    sweep and the analyser can come to disagree about which arms are
    comparable -- and the sweep's answer is the one nobody ever checks.
    """
    bh = ls.bench_history()
    check("the analyser exposes the grouping key",
          callable(getattr(bh, "arm_group_key", None)), True)

    source = inspect.getsource(ls.check_arm_counts)
    check("and the sweep calls it rather than restating it",
          "arm_group_key" in source, True)


def test_the_sweep_actually_threads_each_arms_key_to_the_next(ls):
    """A correct guard that nobody calls is not a guard.

    Written because the first version of this file tested the threading by
    grepping `run_sweep` for the word "previous" -- which its own comment and
    its `previous = None` initialiser satisfy. Deleting the line that advances
    it (`previous = (pad, key)`) left `previous` as `None` for the whole sweep,
    disarming the guard completely, and the suite stayed green. So this drives
    `run_sweep` itself and records what each arm was actually told about its
    predecessor.

    Everything below `run_sweep` is stubbed -- bash, QEMU, the recorder -- but
    `run_sweep`'s own control flow is the real thing, which is the only part
    under test here.
    """
    import contextlib
    import io
    import tempfile
    import types

    handle = tempfile.NamedTemporaryFile(
        suffix=".txt", delete=False, mode="w", encoding="utf-8")
    handle.close()
    serial = handle.name
    seen = []

    def fake_check(bh, pad, profile, previous=None):
        seen.append((pad, previous))
        return True, f"[stub] pad={pad}", f"key-{pad}"

    def fake_run(argv, cwd=None, env=None, check=False):
        # Stand in for the build+boot: emit the banner the sweep re-reads to
        # confirm the kernel it got is the kernel it asked for.
        pad = env["SLATEOS_TEXT_PAD"]
        with open(serial, "w", encoding="utf-8") as out:
            out.write(f"[boot] build profile: sanitizer=off textpad={pad}\n")
        return types.SimpleNamespace(returncode=0)

    saved = (ls.find_bash, ls.check_arm_counts, ls.subprocess.run)
    ls.find_bash = lambda script: ("bash", "[stub] preflight")
    ls.check_arm_counts = fake_check
    ls.subprocess.run = fake_run
    try:
        with contextlib.redirect_stdout(io.StringIO()):
            rc = ls.run_sweep([0, 1024, 2048], "release", serial)
    finally:
        ls.find_bash, ls.check_arm_counts, ls.subprocess.run = saved
        try:
            os.unlink(serial)
        except OSError:
            pass

    check("the stubbed sweep completes", rc, 0)
    check("every arm is checked", [pad for pad, _ in seen], [0, 1024, 2048])
    check("the first arm is told it has no predecessor", seen[0][1], None)
    check("the second arm is told the first arm's pad and key",
          seen[1][1], (0, "key-0"))
    check("...and the third the second's, so the key advances every arm",
          seen[2][1], (1024, "key-1024"))


def test_the_guard_shares_its_predicate_with_the_analyser(ls):
    """A copy of the rule would agree today and drift tomorrow.

    This asserts the coupling directly: the acceptance test the sweep runs is
    `bench-history.py`'s own, so the two cannot disagree about what an arm is.
    """
    bh = ls.bench_history()
    check("the analyser exposes the per-record predicate",
          callable(getattr(bh, "layout_arm_rejection", None)), True)

    source = inspect.getsource(ls.check_arm_counts)
    check("and the sweep calls it rather than restating it",
          "layout_arm_rejection" in source, True)


def test_the_on_target_messages_are_read_from_the_kernel_not_copied(ls):
    """The branch-survival check must track the source it is checking.

    Background: on 2026-08-19 LLVM folded `self_test_pad_is_first_in_text`'s
    comparison of two linker symbols to a constant -- it guarantees distinct
    globals have distinct addresses, and `linker.ld` deliberately makes these
    two alias -- then deleted the success branch as unreachable. Every padded
    release kernel halted at boot. `--self-test` passed throughout, because
    every claim it made was about the ELF rather than about the code that
    inspects the ELF at run time.

    The new claim looks for each branch's message in the image. That only
    works if the messages it looks for are the kernel's current ones, so they
    are extracted from `layout_pad.rs` rather than written down twice.
    """
    fragments = ls.on_target_messages()
    check("all three branches of the on-target check are found",
          len(fragments), 3)
    check("...and they are distinct, so a hit identifies which branch survived",
          len(set(fragments)), 3)
    check("...and none is a bare placeholder-free prefix shared by the others",
          any("[layout_pad] " == f for f in fragments), False)
    for fragment in fragments:
        check(f"...and {fragment[:34]!r}... is long enough to be unique",
              len(fragment) >= 20, True)

    import tempfile
    with open(os.path.join(REPO_ROOT, ls.LAYOUT_PAD_SRC), encoding="utf-8") as h:
        real = h.read()

    with tempfile.TemporaryDirectory() as tmp:
        # A source whose function was renamed away must make the extractor
        # refuse, not quietly check zero branches.
        renamed = os.path.join(tmp, "renamed.rs")
        with open(renamed, "w", encoding="utf-8", newline="\n") as handle:
            handle.write(real.replace(f"pub fn {ls.LAYOUT_PAD_FN}(",
                                      "pub fn something_else("))
        try:
            ls.on_target_messages(renamed)
            outcome = "silently returned something"
        except RuntimeError as exc:
            outcome = "refused" if ls.LAYOUT_PAD_FN in str(exc) else str(exc)
        check("a renamed on-target check makes the extractor refuse",
              outcome, "refused")

        # And a source with a branch deleted must make it refuse too -- that
        # is the shape the bug takes if it ever reaches the source.
        cut = os.path.join(tmp, "cut.rs")
        marker = "[layout_pad] no padding in this build"
        line = next(l for l in real.splitlines() if marker in l)
        with open(cut, "w", encoding="utf-8", newline="\n") as handle:
            handle.write(real.replace(line + "\n", ""))
        try:
            ls.on_target_messages(cut)
            outcome = "silently returned something"
        except RuntimeError as exc:
            outcome = "refused" if "found 2" in str(exc) else str(exc)
        check("a missing branch makes the extractor refuse rather than check "
              "the remaining two", outcome, "refused")


def test_a_binary_missing_a_branch_is_reported_as_missing(ls):
    """`branches_present` must find absence, and must not be fooled by UTF-8.

    Two of the three messages contain an em dash. A check that compared `str`
    against a `bytes` image, or decoded the image as ASCII, would report every
    branch missing and be dismissed as noise -- which is how a real failure
    becomes an ignored one.
    """
    import tempfile
    fragments = ls.on_target_messages()
    with tempfile.TemporaryDirectory() as tmp:
        image = os.path.join(tmp, "fake-kernel")
        with open(image, "wb") as handle:
            handle.write(b"\x7fELF" + b"\x00" * 64)
            for fragment in fragments:
                handle.write(fragment.encode("utf-8") + b"\x00")
        check("a binary containing every message reports nothing missing",
              ls.branches_present(image, fragments), [])

        dropped = fragments[-1]
        with open(image, "wb") as handle:
            handle.write(b"\x7fELF" + b"\x00" * 64)
            for fragment in fragments:
                if fragment != dropped:
                    handle.write(fragment.encode("utf-8") + b"\x00")
        check("a binary whose success branch was folded away reports it",
              ls.branches_present(image, fragments), [dropped])


def main():
    """Run every `test_*` in this file, in definition order.

    Same discovery-with-a-floor as `test-bench-history.py`: a discovery
    mechanism that discovers nothing looks exactly like a suite that passes.
    """
    ls = load_module()
    tests = [(name, fn) for name, fn in list(globals().items())
             if name.startswith("test_") and callable(fn)]
    if len(tests) < 13:
        print(f"FATAL: test discovery found only {len(tests)} tests; the "
              f"suite has at least 13. Discovery is broken, not the code.")
        return 1
    for name, fn in tests:
        params = inspect.signature(fn).parameters
        fn(**{p: {"ls": ls}[p] for p in params if p == "ls"})

    print()
    if _FAILURES:
        print(f"{len(_FAILURES)} FAILED: {', '.join(_FAILURES)}")
        return 1
    print("all layout-sweep tests passed")
    return 0


if __name__ == "__main__":
    sys.exit(main())
