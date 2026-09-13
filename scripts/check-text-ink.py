#!/usr/bin/env python3
"""Refuse an accent-family colour that reaches a text site without `ink()`.

The other half of `check-overlay0-ink.py`'s question. That one refuses text
drawn in the *disabled* grey; this one refuses text drawn in a role that was
never floored to 4.5:1 against the ground the active theme puts it on.

`accent`, `red`, `green`, `yellow`, `peach`, `blue`, `lavender`, `mauve`,
`sapphire`, `teal` and `sky` are **dual-use**: the same value is a fill, a
bar, a dot or a badge background -- where the contrast floor does not apply --
and a label, where it does. `Palette::ink(colour)` moves an ink only as far as
the floor requires. A fill asks for the raw field; a text site asks for
`ink()`. See `known-issues.md`
TD-C-THIRTEEN-LIGHT-ACCENTS-STILL-FAIL-ON-CARDS and `design-decisions.md`.

**Why this file exists rather than a `run_checker` line naming the tool
directly.** The classifier and the conversion are `gui/appearance/ink-text.py`,
which lives with the crate whose roles it knows about and is run by hand with
`--apply`. Gates live in `scripts/`, and two of this tree's own checkers say so
by construction: `check-gates-are-wired.py` scans `scripts/check-*.py`, so a
gate outside that directory is invisible to the tool that finds unwired gates
-- which is exactly how this property came to have no gate at all -- and
`check-gate-call-sites.py` resolves a gate's script against `scripts/`, so a
`run_checker` line pointing anywhere else is reported as naming a file that
does not exist. Both assumptions are reasonable and neither is worth changing
for one tool. This is the gate; the tool stays where it belongs.

Usage:  python scripts/check-text-ink.py [--self-test]
"""

import importlib.util
import pathlib
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
sys.path.insert(0, str(ROOT / "scripts"))
import selftestflag  # noqa: E402

TOOL = ROOT / "gui" / "appearance" / "ink-text.py"


def load_tool():
    """The conversion tool, imported rather than shelled out to.

    Imported so that a failure to load is a failure of this gate rather than a
    non-zero exit nobody reads, and so the self-test below grades the same code
    the check runs -- not a second copy of its rules, which is how the two
    would drift.
    """
    spec = importlib.util.spec_from_file_location("ink_text", TOOL)
    if spec is None or spec.loader is None:
        print(f"check-text-ink: cannot load {TOOL}", file=sys.stderr)
        sys.exit(2)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def main(argv):
    if not TOOL.exists():
        print(f"check-text-ink: {TOOL} is missing, so nothing was checked", file=sys.stderr)
        return 2
    tool = load_tool()

    if selftestflag.wants_selftest(argv):
        return tool.self_test()

    unknown = selftestflag.unknown_options(argv)
    if unknown:
        for opt in unknown:
            print("check-text-ink: unrecognized option " + repr(opt))
        print("usage: check-text-ink.py [--self-test]")
        return 2

    paths = tool.default_paths()
    if not paths:
        print("check-text-ink: no files to scan; refusing to call that a pass", file=sys.stderr)
        return 2

    inked_already = tool.already_inking(paths)
    findings = 0
    for path in paths:
        n = tool.convert(path, False, inked_already.get(tool.crate_of(path), frozenset()))
        if n:
            print(f"{path.relative_to(ROOT).as_posix()}: {n} text site(s) name a dual-use role")
            findings += n

    if findings:
        print(
            f"\n{findings} text site(s) draw in an accent-family role without "
            "`ink()`, so each is whatever contrast that hue happens to have on "
            "that ground. Run:  python gui/appearance/ink-text.py --apply",
            file=sys.stderr,
        )
        return 1

    # The corpus travels with the verdict: a pass over nothing reads exactly
    # like a pass over everything.
    print(f"ok -- every text site in {len(paths)} file(s) goes through ink().")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
