#!/usr/bin/env python3
"""The gate that keeps `scripts/reintro-palette.py` from rotting unseen.

Run: `python scripts/test-reintro-palette.py` (0 = pass, 1 = fail).

**What it guards.** The harness proves the desktop's palette tests are real:
it puts each old colour back, one defect at a time, and checks that a test
fails. A full sweep takes hours, so it is run by hand -- and between sweeps
nothing looked at it. By 2026-09-27, 308 of its 1,459 defects no longer
matched the code they were written against, and 31 tests were proven to bite
only by defects that could no longer be applied (known-issues.md,
`TD-C-THE-PALETTE-REINTRODUCTION-HARNESS-HAS-ROTTED`). `--check` answers the
one question that rots -- does every defect still apply, exactly once, and
change something? -- in about a second, with no toolchain, so the boot test's
sweep of `scripts/test-*.py` runs it on every boot.

**When it fails.** A change to `gui/desktop`, `gui/appearance` or
`gui/toolkit` moved or rewrote a line a defect names. Re-derive that defect
against the code as it now reads -- the harness's `Re-derived` notes show the
shape -- or, if what it broke no longer exists, replace it with a
`# RETIRED <date>: <description>.` record saying why. Then run the defect
through the harness (`python scripts/reintro-palette.py <label>`) to confirm
the tests it declares still catch it.

**The controls.** Each drives `check()` on synthetic defects against a
synthetic file, one per way a defect can rot, so a `check()` that quietly
stopped finding anything fails here rather than passing the real tree.
"""

from __future__ import annotations

import contextlib
import importlib.util
import io
import subprocess
import sys
import tempfile
from pathlib import Path

HERE = Path(__file__).resolve().parent
SCRIPT = HERE / "reintro-palette.py"


def load():
    """The harness as a module: its `check`, `snapshot`, `ROOT` and `DEFECTS`."""
    spec = importlib.util.spec_from_file_location("reintro_palette_under_test", SCRIPT)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def checked(rp, defects, snap):
    """`check()`'s status and report for `defects` against `snap`."""
    saved = rp.DEFECTS
    rp.DEFECTS = defects
    out = io.StringIO()
    try:
        with contextlib.redirect_stdout(out):
            status = rp.check(snap)
    finally:
        rp.DEFECTS = saved
    return status, out.getvalue()


def main() -> int:
    failures = []
    passed = 0

    def expect(ok, what, detail=""):
        nonlocal passed
        if ok:
            passed += 1
        else:
            failures.append(what + (f"\n{detail}" if detail else ""))

    # ---- the real tree -------------------------------------------------------
    r = subprocess.run([sys.executable, str(SCRIPT), "--check"], capture_output=True,
                       text=True, check=False)
    last = (r.stdout.strip().splitlines() or [""])[-1]
    report = "\n".join(
        "    " + line for line in r.stdout.splitlines()
        if line.startswith(("PATTERN NOT FOUND", "AMBIGUOUS", "NO-OP", "    edit", "    every"))
    )
    expect(r.returncode == 0 and last.endswith("0 stale, 0 ambiguous, 0 no-op"),
           f"the harness has rotted -- {last or 'no summary line'} (exit {r.returncode}); "
           "re-derive or retire each entry below (see this file's docstring)",
           report or r.stderr.strip())

    # ---- the controls ------------------------------------------------------------
    rp = load()
    snap = {"f.rs": b"alpha\nbeta\nbeta\n"}

    status, out = checked(rp, [("A: applies once", "f.rs", [("alpha\n", "gamma\n")], [], [])], snap)
    expect(status == 0 and "0 stale, 0 ambiguous, 0 no-op" in out,
           "a defect that applies once and changes the file was reported", out)

    status, out = checked(rp, [("B: stale", "f.rs", [("delta\n", "gamma\n")], [], [])], snap)
    expect(status == 1 and "PATTERN NOT FOUND  B: stale" in out,
           "a defect whose text is gone was not reported as stale", out)

    status, out = checked(rp, [("C: ambiguous", "f.rs", [("beta\n", "gamma\n")], [], [])], snap)
    expect(status == 1 and "AMBIGUOUS (2 matches, 1 listed)  C: ambiguous" in out,
           "a defect whose text occurs twice was not reported as ambiguous", out)

    # Listing the same edit twice is how a defect wounds both of an identical
    # pair on purpose; that is not an ambiguity.
    status, out = checked(rp, [("D: both of a pair", "f.rs",
                                [("beta\n", "gamma\n"), ("beta\n", "gamma\n")], [], [])], snap)
    expect(status == 0, "a defect that lists both copies of a pair was reported", out)

    # The first edit creates the text the second looks for, earlier in the
    # file, so the second undoes the first and nothing is introduced at all.
    status, out = checked(rp, [("E: undoes itself", "f.rs",
                                [("alpha\n", "beta\n"), ("beta\n", "alpha\n")], [], [])], snap)
    expect(status == 1 and "NO-OP  E: undoes itself" in out,
           "a defect whose edits cancel was not reported as a no-op", out)

    # A file that is not there at all: `--check` reads it as empty and names
    # the defect, rather than dying on the read with a traceback that names
    # only the file.
    saved_root = rp.ROOT
    with tempfile.TemporaryDirectory(prefix="reintro_palette_test_") as tmp:
        rp.ROOT = Path(tmp)
        try:
            gone = rp.snapshot(["gone.rs"], missing_ok=True)
        finally:
            rp.ROOT = saved_root
    status, out = checked(rp, [("F: file gone", "gone.rs", [("alpha\n", "gamma\n")], [], [])], gone)
    expect(gone == {"gone.rs": b""} and status == 1 and "PATTERN NOT FOUND  F: file gone" in out,
           "a defect aimed at a deleted file was not reported as stale", out)

    if failures:
        print("test-reintro-palette: FAILED")
        for f in failures:
            print("  " + f)
        return 1
    print(f"test-reintro-palette: all {passed} checks passed")
    return 0


if __name__ == "__main__":
    sys.exit(main())
