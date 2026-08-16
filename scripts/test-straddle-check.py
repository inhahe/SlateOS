#!/usr/bin/env python3
"""Regression tests for `scripts/straddle-check.py`.

Run: `python scripts/test-straddle-check.py` (exit 0 = pass, 1 = fail).
No pytest dependency, matching `scripts/test-bench-history.py`: this has to run
from a bare checkout.

Why this file exists
--------------------
`straddle-check.py --compare` exists to answer one question -- "did an
unrelated commit re-roll the TCG page-straddle lottery?" -- and the answer it
gives is used to *dismiss* a benchmark regression as an artifact. A comparison
tool that under-reports therefore does real damage: it converts "layout is not
the cause" from a finding into a default, and a real regression gets filed as
a false alarm.

The obvious smoke test -- compare a binary against itself and expect zero
flips -- passes vacuously. It would pass just as happily if `classify_flips`
returned empty lists unconditionally, if the disassembly regex matched
nothing, or if every function were misclassified as "recompiled". That is
precisely the shape of failure this project keeps rediscovering: "a check that
cannot fire is indistinguishable from a check that passes". So every test here
is built around inputs that *must* produce a specific non-empty answer.

The synthetic-input design also pins the property that actually matters and is
easy to break by accident: **a straddle difference is only attributable to
layout when the machine code is identical.** If a future edit relaxes the
signature check -- say, to "compare functions by name only, they're probably
the same" -- the tool would start reporting recompiled functions as layout
flips, which is exactly the wrong answer in the exactly wrong direction.
"""

from __future__ import annotations

import importlib.util
import inspect
import os
import sys

REPO_ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
SCRIPT = os.path.join(REPO_ROOT, "scripts", "straddle-check.py")

_FAILURES = []


def load_module():
    """Import straddle-check.py by path (its name is not an identifier)."""
    spec = importlib.util.spec_from_file_location("straddle_check", SCRIPT)
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


def place(sc, base: int, offset: int, span: int):
    """Build a Loop as if a function starting at `base` were disassembled.

    Straddle status is computed the same way the real parser computes it, from
    the two absolute addresses -- so a test cannot accidentally assert a
    straddle bit that the tool would not itself derive.
    """
    target = base + offset
    return sc.Loop(offset=offset, span=span, target=target,
                   straddles=(target // sc.PAGE) != ((target + span) // sc.PAGE))


def test_a_moved_function_is_reported_as_a_flip(sc):
    """The positive control: identical code, different address, both ways.

    The move is modelled the way a real one happens: the function's *base*
    address shifts while the loop keeps its offset and span. Getting this
    backwards -- holding the base fixed and moving the offset -- models a
    recompile, not a move, and the first draft of this test did exactly that
    and "failed" against correct code. Keep the distinction; it is the same
    one `signature()` is built on.

    A 64-byte loop landing at 0x1100 fits inside its page; the identical loop
    landing at 0x1fe0 runs off the end of one. Nothing about the function
    changed but where it was placed, which is the entire artifact this tool
    hunts.
    """
    clean = {"f": [place(sc, 0x1000, 0x100, 64)]}   # loop at 0x1100
    dirty = {"f": [place(sc, 0x1ee0, 0x100, 64)]}   # same loop at 0x1fe0
    check("the modelled move really does change straddle status",
          (clean["f"][0].straddles, dirty["f"][0].straddles), (False, True))

    check("loop that moved onto a boundary is 'gained'",
          [r[0] for r in sc.classify_flips(clean, dirty, [], 0).gained], ["f"])
    check("that same pair reports nothing as 'lost'",
          sc.classify_flips(clean, dirty, [], 0).lost, [])
    # The reverse direction must work too: a build that *removes* a straddle
    # produces a benchmark that got faster, and reading only the "gained" half
    # would leave half the evidence on the floor -- which is how a bidirectional
    # layout re-roll gets mistaken for a one-sided regression.
    check("the reverse pair is 'lost', not 'gained'",
          [r[0] for r in sc.classify_flips(dirty, clean, [], 0).lost], ["f"])
    check("...and reports nothing as 'gained'",
          sc.classify_flips(dirty, clean, [], 0).gained, [])


def test_recompiled_functions_are_never_called_layout(sc):
    """A changed loop signature must be quarantined, not attributed.

    This is the failure that would silently invert the tool's conclusion: if a
    genuinely-recompiled function were compared by name alone, its straddle
    difference would be reported as "moved onto a boundary" and a real code
    regression would be filed as an artifact.
    """
    old = {"f": [place(sc, 0x1000, 0x100, 64)]}
    new = {"f": [place(sc, 0x1ee0, 0x100, 96)]}   # different span => recompiled
    flips = sc.classify_flips(old, new, [], 0)
    check("recompiled function is not reported as gained", flips.gained, [])
    check("recompiled function is not reported as lost", flips.lost, [])
    check("recompiled function IS reported, separately", flips.recompiled,
          ["f"])


def test_loops_longer_than_a_page_are_excluded(sc):
    """A loop that cannot fit in a page straddles at every address.

    There are hundreds of these (every outer dispatch loop in the kernel).
    Counting them would swamp the report with rows whose straddle bit is
    arithmetic, not layout, and none of them can ever flip.
    """
    big_lo = {"f": [place(sc, 0x1000, 0x000, sc.PAGE + 8)]}
    big_hi = {"f": [place(sc, 0x1fe0, 0x000, sc.PAGE + 8)]}
    check("a >4KiB loop straddles in both placements",
          (big_lo["f"][0].straddles, big_hi["f"][0].straddles), (True, True))
    check("a >4KiB loop is marked forced", big_lo["f"][0].forced, True)
    check("a >4KiB loop never appears as a flip",
          sc.classify_flips(big_lo, big_hi, [], 0).gained, [])
    check("a page-sized loop is not marked forced",
          place(sc, 0x1000, 0x100, sc.PAGE - 1).forced, False)


def test_min_span_and_symbol_filters_actually_filter(sc):
    """Both filters must be able to suppress a flip that would otherwise fire.

    Tested against the positive control above, so a filter that silently did
    nothing would show up as a test failure rather than as a quieter report.
    """
    clean = {"hot_loop": [place(sc, 0x1000, 0x100, 64)],
             "cold_loop": [place(sc, 0x1000, 0x100, 64)]}
    dirty = {"hot_loop": [place(sc, 0x1ee0, 0x100, 64)],
             "cold_loop": [place(sc, 0x1ee0, 0x100, 64)]}

    check("with no filter, both functions flip",
          sorted(r[0] for r in sc.classify_flips(clean, dirty, [], 0).gained),
          ["cold_loop", "hot_loop"])
    check("a symbol substring narrows it to one",
          [r[0] for r in sc.classify_flips(clean, dirty, ["hot"], 0).gained],
          ["hot_loop"])
    check("--min-span above the loop's span suppresses it entirely",
          sc.classify_flips(clean, dirty, [], 65).gained, [])
    check("--min-span at exactly the loop's span still reports it",
          len(sc.classify_flips(clean, dirty, [], 64).gained), 2)


def test_added_and_removed_functions_are_counted_not_dropped(sc):
    """A function present in only one build is neither a flip nor invisible.

    Silently dropping them would make a big refactor look like a small one, and
    the counts are the only signal that the two binaries are far enough apart
    that the whole comparison should be distrusted.
    """
    old = {"gone": [place(sc, 0x1000, 0x100, 64)],
           "both": [place(sc, 0x1000, 0x100, 64)]}
    new = {"fresh": [place(sc, 0x1000, 0x100, 64)],
           "both": [place(sc, 0x1000, 0x100, 64)]}
    flips = sc.classify_flips(old, new, [], 0)
    check("a function only in the new build is counted", flips.only_new, 1)
    check("a function only in the old build is counted", flips.only_old, 1)
    check("the unchanged shared function produces no flip", flips.gained, [])


def test_signature_ignores_address_but_not_shape(sc):
    """`signature()` is the load-bearing predicate; pin both directions."""
    at_1000 = [place(sc, 0x1000, 0x100, 64), place(sc, 0x1000, 0x300, 16)]
    at_9000 = [place(sc, 0x9000, 0x100, 64), place(sc, 0x9000, 0x300, 16)]
    reshaped = [place(sc, 0x1000, 0x100, 64), place(sc, 0x1000, 0x300, 17)]
    check("the same code at a different address has one signature",
          sc.signature(at_1000), sc.signature(at_9000))
    check("a one-byte span change breaks the signature",
          sc.signature(at_1000) == sc.signature(reshaped), False)
    # Order of discovery must not matter: the disassembly emits loops in
    # address order, but a future change (e.g. reading a symbol table) could
    # reorder them, and that must not read as a recompile.
    check("signature is order-independent",
          sc.signature(at_1000), sc.signature(list(reversed(at_1000))))


def test_demangle_strips_only_the_build_hash(sc):
    """Names must match across builds, without collapsing distinct symbols."""
    check("the trailing hash is stripped",
          sc.demangle("_ZN6kernel5bench7run_all17h0123456789abcdefE"),
          "_ZN6kernel5bench7run_all")
    check("a name with no hash is untouched",
          sc.demangle("kernel_main"), "kernel_main")
    check("two different functions do not collapse to one name",
          sc.demangle("_ZN1a17h0000000000000000E") ==
          sc.demangle("_ZN1b17h0000000000000000E"), False)


def main():
    """Auto-discover `test_*` in this module, same contract as bench-history.

    The floor assertion is not ceremony: a suite that discovers zero tests
    prints nothing and exits 0, which is indistinguishable from a suite that
    passed -- the exact bug the whole file is about.
    """
    sc = load_module()
    tests = [(name, fn) for name, fn in list(globals().items())
             if name.startswith("test_") and callable(fn)]
    if len(tests) < 7:
        print(f"FATAL: test discovery found only {len(tests)} tests; the "
              f"suite has at least 7. Discovery is broken, not the code.")
        return 1
    for _, fn in tests:
        params = inspect.signature(fn).parameters
        fn(**{p: {"sc": sc}[p] for p in params if p == "sc"})

    print()
    if _FAILURES:
        print(f"{len(_FAILURES)} FAILED: {', '.join(_FAILURES)}")
        return 1
    print("all straddle-check tests passed")
    return 0


if __name__ == "__main__":
    sys.exit(main())
