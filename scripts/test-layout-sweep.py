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


def test_bash_can_actually_open_the_boot_test_this_sweep_will_invoke(ls):
    """The regression that cost a 34-minute self-test and the first arm.

    Both halves are the test. The repo-relative name must work, *and* the
    absolute one must be shown to fail -- otherwise this passes on a platform
    where both happen to work and stops defending anything on the one where it
    matters.
    """
    ok, message = ls.preflight_boot_test()
    check("bash can find, read and parse the boot test as the sweep names it",
          ok, True)
    check("...and says so, so a passing preflight leaves a receipt",
          "preflight" in message, True)

    # The exact string the old code built. On Linux this resolves fine and the
    # check below is vacuous; on Windows it is the bug. Asserted as "not worse
    # than the relative form" rather than "fails", so the test states a real
    # invariant on both platforms.
    absolute = os.path.join(REPO_ROOT, "scripts", "boot-test.sh")
    abs_ok, _ = ls.preflight_boot_test(absolute)
    check("the relative form is never the worse of the two",
          ok or not abs_ok, True)
    if not abs_ok:
        print("      (this platform's bash cannot open the absolute path -- "
              "the original bug, still present and still avoided)")

    # And the constant itself: an absolute path here would reintroduce it.
    check("the sweep names the boot test relatively",
          os.path.isabs(ls.BOOT_TEST), False)


def test_a_missing_boot_test_is_refused_before_any_build(ls):
    """The preflight must fail loudly, not fall through to the arms."""
    ok, message = ls.preflight_boot_test("scripts/no-such-boot-test.sh")
    check("a boot test that is not there fails the preflight", ok, False)
    check("...and the message says the sweep is refusing to start",
          "Refusing to start" in message or "cannot run bash" in message, True)


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


def main():
    """Run every `test_*` in this file, in definition order.

    Same discovery-with-a-floor as `test-bench-history.py`: a discovery
    mechanism that discovers nothing looks exactly like a suite that passes.
    """
    ls = load_module()
    tests = [(name, fn) for name, fn in list(globals().items())
             if name.startswith("test_") and callable(fn)]
    if len(tests) < 4:
        print(f"FATAL: test discovery found only {len(tests)} tests; the "
              f"suite has at least 4. Discovery is broken, not the code.")
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
