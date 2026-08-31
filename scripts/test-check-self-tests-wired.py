#!/usr/bin/env python3
"""Regression tests for the self-test wiring gate's `RAN-IF` markers.

Run: `python scripts/test-check-self-tests-wired.py` (0 = pass, 1 = fail). No
pytest dependency, matching the other suites in this directory, so it runs from
a bare checkout and from `scripts/boot-test.sh`.

What is under test, and why it needs a suite of its own
------------------------------------------------------

`check-self-tests-wired.py` answers "is this self-test wired up". For six call
sites in `main.rs` that is not the interesting question, because they sit inside
a conditional: they are wired, and whether they *ran* depends on the boot path.
For a year the gate printed a note asking a human to go and check the serial
log, and for a year nobody did -- seven suites behind one false `fat_ok` never
executed while the gate said, accurately and uselessly, "check each against the
serial log".

The `RAN-IF` marker turns that note into data: each gated site declares the
exact serial line it prints, so a later pass can ask the log instead of asking
the reader.

Every test here is about a way that mechanism can go **quietly** wrong, because
a loud failure needs no suite. The three shapes:

  * a marker that matches nothing (typo, rename, stale copy). Downstream this
    reads as "the suite stopped running" -- a manufactured bug report against
    working code. Caught by the `unfound` check.
  * a marker that outlives its call site. Downstream this reads as "the suite
    ran" for a site that no longer exists. Prevented by requiring contiguity
    with the call.
  * a gated site with no marker at all, which is where this started.

Plus one that is not a defect and must not be reported as one: `acpi::self_test`
is called from both arms of one `if`/`else`, so two sites share one marker and
seeing the line proves an arm ran without saying which. Reporting one arm as
never-seen because the other won would be a false alarm on code that is correct
by construction.
"""

from __future__ import annotations

import os
import sys

SCRIPT_DIR = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, SCRIPT_DIR)

import srcload  # noqa: E402

WIRED = srcload.load(os.path.join(SCRIPT_DIR, "check-self-tests-wired.py"),
                     "check_self_tests_wired")

FAILURES: list[str] = []
CASES = 0


def check(label: str, cond: bool, detail: object = "") -> None:
    global CASES
    CASES += 1
    if cond:
        print("ok   %s" % label)
    else:
        print("FAIL %s%s" % (label, ("  -- %s" % (detail,)) if detail else ""))
        FAILURES.append(label)


def eq(label: str, got, want) -> None:
    check(label, got == want, "got %r, want %r" % (got, want))


# --------------------------------------------------------------------------
# marker_above: the literal must sit in the comment block touching the call
# --------------------------------------------------------------------------

def test_marker_above() -> None:
    src = '\n'.join([
        'fn a() {',                                     # 1
        '    // RAN-IF: "[x] Running self-test..."',     # 2
        '    x::self_test();',                          # 3
        '',                                             # 4
        '    y::self_test();',                          # 5
        '    // RAN-IF: "[z] Running self-test..."',     # 6
        '    // an intervening comment line',           # 7
        '    z::self_test();',                          # 8
        '}',                                            # 9
    ])
    lines = src.split("\n")
    eq("the line directly above is read",
       WIRED.marker_above(lines, 3), "[x] Running self-test...")
    # Line 5's predecessor is blank, so the block above it is empty. A marker
    # further up belongs to a different call and must not be borrowed.
    eq("a blank line ends the block, so nothing is inherited",
       WIRED.marker_above(lines, 5), None)
    eq("a marker higher in the same comment block is still found",
       WIRED.marker_above(lines, 8), "[z] Running self-test...")
    eq("a call on line 1 has nothing above it",
       WIRED.marker_above(lines, 1), None)


def test_marker_quoting() -> None:
    def one(line: str):
        return WIRED.marker_above([line, "call();"], 2)

    eq("quotes delimit the literal exactly",
       one('// RAN-IF: "[a] b"'), "[a] b")
    eq("indentation before the comment is fine",
       one('        // RAN-IF: "[a] b"'), "[a] b")
    eq("a trailing space outside the quotes is not part of the marker",
       one('// RAN-IF: "[a] b" '), "[a] b")
    eq("a trailing space INSIDE the quotes is preserved, because it is real",
       one('// RAN-IF: "[a] b "'), "[a] b ")
    # Without quotes there is no way to tell where the literal stops, and a
    # marker with an accidental trailing period matches nothing forever.
    eq("an unquoted marker is refused rather than guessed at",
       one('// RAN-IF: [a] b'), None)
    eq("an empty marker is refused -- it would match every line",
       one('// RAN-IF: ""'), None)
    eq("a doc comment is not a marker",
       one('/// RAN-IF: "[a] b"'), None)


# --------------------------------------------------------------------------
# collect_markers: grouping, and the two hard failures
# --------------------------------------------------------------------------

def _gated(lineno: int, sym: str, text: str = "call();"):
    """One `find_gated_calls` finding, in the shape collect_markers consumes."""
    return (lineno, text, [sym], [])


def test_collect_two_sites_one_marker() -> None:
    src = '\n'.join([
        'fn a() {',                                       # 1
        '    if p {',                                     # 2
        '        // RAN-IF: "[acpi] Running self-test..."',  # 3
        '        acpi::self_test();',                     # 4
        '    } else {',                                   # 5
        '        // RAN-IF: "[acpi] Running self-test..."',  # 6
        '        acpi::self_test();',                     # 7
        '    }',                                          # 8
        '}',                                              # 9
    ])
    defs = {"acpi::self_test": ("acpi/mod.rs", "acpi", "self_test")}
    files = {"main.rs": src,
             "acpi/mod.rs": 'serial_println!("[acpi] Running self-test...");'}
    markers, problems = WIRED.collect_markers(
        [_gated(4, "acpi::self_test"), _gated(7, "acpi::self_test")],
        src, files, defs)
    eq("both arms of one if/else are one marker, not two", len(markers), 1)
    eq("and both sites are recorded under it",
       markers["[acpi] Running self-test..."]["sites"], [4, 7])
    eq("the tested symbol is listed once, not twice",
       markers["[acpi] Running self-test..."]["tests"], ["acpi::self_test"])
    eq("mutually exclusive arms are not a problem", problems, [])


def test_collect_missing_marker() -> None:
    src = 'fn a() {\n    if p {\n        fat::self_test();\n    }\n}'
    defs = {"fs::fat::self_test": ("fs/fat.rs", "fs::fat", "self_test")}
    files = {"main.rs": src, "fs/fat.rs": "anything"}
    markers, problems = WIRED.collect_markers(
        [_gated(3, "fs::fat::self_test")], src, files, defs)
    eq("an undeclared gated site yields no marker", markers, {})
    eq("and exactly one problem", len(problems), 1)
    eq("classified as missing", problems[0][0], "missing")
    eq("naming the line", problems[0][1], 3)


def test_collect_unfound_marker() -> None:
    """A marker the kernel cannot print is worse than no marker at all.

    No marker fails loudly, here, now. A marker that matches nothing passes this
    gate and then reports the suite as never-run on every boot forever -- an
    accusation against code that is working, which is the failure mode most
    likely to be believed and acted on.
    """
    src = '\n'.join([
        'fn a() {',
        '    if p {',
        '        // RAN-IF: "[fat] Runing mkfs self-test..."',   # typo: Runing
        '        fat::self_test();',
        '    }',
        '}',
    ])
    defs = {"fs::fat::self_test": ("fs/fat.rs", "fs::fat", "self_test")}
    files = {"main.rs": src,
             "fs/fat.rs": 'serial_println!("[fat] Running mkfs self-test...");'}
    markers, problems = WIRED.collect_markers(
        [_gated(4, "fs::fat::self_test")], src, files, defs)
    eq("a marker no source line can produce is rejected", markers, {})
    eq("classified as unfound", problems[0][0], "unfound")
    eq("and the bad literal is reported back", problems[0][4],
       "[fat] Runing mkfs self-test...")


def test_marker_must_live_in_the_tested_file() -> None:
    """The tag is what makes a marker specific, so it is checked, not assumed.

    `Running self-test...` untagged is printed by dozens of modules in this
    kernel. A marker matching some *other* module's line would report coverage
    this suite does not have -- and would keep reporting it long after this
    suite stopped running, since the other module's line is still there.
    """
    src = '\n'.join([
        'fn a() {',
        '    if p {',
        '        // RAN-IF: "[ahci] Running self-test..."',
        '        fat::self_test();',
        '    }',
        '}',
    ])
    defs = {"fs::fat::self_test": ("fs/fat.rs", "fs::fat", "self_test")}
    files = {
        "main.rs": src,
        "fs/fat.rs": 'serial_println!("[fat] Running self-test...");',
        # The literal exists in the tree -- just not in the file under test.
        "ahci.rs": 'serial_println!("[ahci] Running self-test...");',
    }
    markers, problems = WIRED.collect_markers(
        [_gated(4, "fs::fat::self_test")], src, files, defs)
    eq("another module's line does not vouch for this one", markers, {})
    eq("reported as unfound", problems[0][0], "unfound")


def test_collect_good_marker_passes() -> None:
    src = '\n'.join([
        'fn a() {',
        '    if p {',
        '        // RAN-IF: "[swap] Running disk backend self-test..."',
        '        swap::self_test_disk();',
        '    }',
        '}',
    ])
    defs = {"mm::swap::self_test_disk": ("mm/swap.rs", "mm::swap",
                                         "self_test_disk")}
    files = {
        "main.rs": src,
        "mm/swap.rs":
            'serial_println!("[swap] Running disk backend self-test...");',
    }
    markers, problems = WIRED.collect_markers(
        [_gated(4, "mm::swap::self_test_disk")], src, files, defs)
    eq("a correct declaration is accepted", problems, [])
    eq("and recorded", sorted(markers), ["[swap] Running disk backend "
                                         "self-test..."])


# --------------------------------------------------------------------------
# The real tree: the gate must be green on it, and every site declared
# --------------------------------------------------------------------------

def test_real_tree() -> None:
    root = os.path.normpath(os.path.join(SCRIPT_DIR, "..", "kernel", "src"))
    if not os.path.isdir(root):
        # A bare checkout without the kernel is a legitimate place to run the
        # unit tests above; announce the gap rather than passing silently.
        print("SKIP the real tree (no %s)" % root)
        return
    files = WIRED.load_tree(root)
    defs = WIRED.index_definitions(files)
    by_qualified, by_bare = WIRED.index_call_spellings(defs)
    WIRED.add_reexport_aliases(files, by_qualified)
    gated, gated_ok = WIRED.find_gated_calls(
        files[WIRED.BOOT_ROOT], defs, by_qualified, by_bare, WIRED.BOOT_ROOT)
    check("main.rs brace-matches, so the gated list is trustworthy", gated_ok)
    markers, problems = WIRED.collect_markers(
        gated, files[WIRED.BOOT_ROOT], files, defs)
    eq("every gated site in the real tree declares a usable marker",
       [(p[0], p[1], p[4]) for p in problems], [])
    check("and at least one marker was collected", bool(markers), markers)
    # Not an assertion about *which* sites are gated -- that is a live property
    # and hardcoding it here would be the stale-tally defect this gate's own
    # docstring warns about. Only the invariant is asserted: declared, and
    # findable in the file that defines the suite.
    for lit, slot in markers.items():
        homes = {defs[s][0] for s in slot["tests"]}
        check("marker %r is printable by %s" % (lit[:40], sorted(homes)),
              any(lit in files[h] for h in homes))


def main() -> int:
    test_marker_above()
    test_marker_quoting()
    test_collect_two_sites_one_marker()
    test_collect_missing_marker()
    test_collect_unfound_marker()
    test_marker_must_live_in_the_tested_file()
    test_collect_good_marker_passes()
    test_real_tree()

    print("")
    if FAILURES:
        print("%d of %d case(s) FAILED:" % (len(FAILURES), CASES))
        for f in FAILURES:
            print("  %s" % f)
        return 1
    print("all %d check-self-tests-wired tests passed" % CASES)
    return 0


if __name__ == "__main__":
    sys.exit(main())
