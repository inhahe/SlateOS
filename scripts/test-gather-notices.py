#!/usr/bin/env python3
"""The gate that keeps the image's third-party notices complete (design-decisions §1433).

Run: `python scripts/test-gather-notices.py` (0 = pass, 1 = fail).

The boot test runs every `scripts/test-*.py`, so this holds three things on
every boot:

1. **`gather-notices.py`'s own self-test passes** -- each way a notice can fail
   to be gathered is still an error, not a quiet gap.
2. **The tree's notices gather.** Every `licenses/notices.yaml` parses and
   names texts that are there; every vendored crate names its licence; every
   crates.io package in `Cargo.lock` is in the registry cache with its licence
   files. A failure here names the manifest or crate, and is fixed where it
   points: a text path corrected, a licence named, or -- for a crates.io
   package -- a build run once so cargo fetches it.
3. **The golden bundle is still what the gatherer writes.**
   `gui/notices/tests/fixtures/bundle` is the gatherer's output for its own
   fixture tree, committed, and `gui/notices`'s tests read it. If the writer
   changes the format, this fails on the Python side, as `gui/notices`'s tests
   fail on the Rust side if the reader does. Regenerate it with
   `python scripts/test-gather-notices.py --regenerate` once both sides agree.

In-process throughout, and starting no other process, so a gate cache that
traces what a suite reads can see all of it
(`requests/c-a-lane-c-gates-start-no-process.md`).
"""

from __future__ import annotations

import contextlib
import importlib.util
import io
import sys
import tempfile
from pathlib import Path

HERE = Path(__file__).resolve().parent
SCRIPT = HERE / "gather-notices.py"
GOLDEN = HERE.parent / "gui" / "notices" / "tests" / "fixtures" / "bundle"


def load():
    spec = importlib.util.spec_from_file_location("gather_notices_under_test", SCRIPT)
    module = importlib.util.module_from_spec(spec)
    # `dataclasses` looks the module up by name while it builds the class.
    sys.modules[spec.name] = module
    spec.loader.exec_module(module)
    return module


def files_under(directory: Path) -> dict[str, bytes]:
    return {p.relative_to(directory).as_posix(): p.read_bytes()
            for p in sorted(directory.rglob("*")) if p.is_file()}


def main(argv: list[str]) -> int:
    gn = load()

    if argv == ["--regenerate"]:
        with tempfile.TemporaryDirectory(prefix="gather_notices_golden_") as tmp:
            root, home = gn.build_fixture(Path(tmp))
            fresh = Path(tmp) / "bundle"
            gn.write_bundle(gn.gather(root, home), fresh)
            for rel, data in files_under(fresh).items():
                target = GOLDEN / rel
                target.parent.mkdir(parents=True, exist_ok=True)
                target.write_bytes(data)
        print(f"test-gather-notices: rewrote {GOLDEN}")
        return 0
    if argv:
        print("usage: test-gather-notices.py [--regenerate]", file=sys.stderr)
        return 2

    failures = []
    passed = 0

    def expect(ok, what, detail=""):
        nonlocal passed
        if ok:
            passed += 1
        else:
            failures.append(what + (f"\n{detail}" if detail else ""))

    # 1. The self-test.
    out = io.StringIO()
    with contextlib.redirect_stdout(out):
        status = gn.self_test()
    expect(status == 0, "gather-notices' self-test failed", out.getvalue())

    # 2. The tree.
    try:
        notices = gn.gather(gn.ROOT, gn.cargo_home())
    except gn.NoticeError as exc:
        expect(False, f"the tree's notices do not gather: {exc}")
    else:
        kinds = {n.kind for n in notices}
        expect({"vendored", "crates.io"} <= kinds,
               f"the tree gathered no {({'vendored', 'crates.io'} - kinds)} notices -- "
               "the walk or the lock file reading is broken, not the tree")

    # 3. The golden bundle.
    with tempfile.TemporaryDirectory(prefix="gather_notices_golden_") as tmp:
        root, home = gn.build_fixture(Path(tmp))
        fresh = Path(tmp) / "bundle"
        gn.write_bundle(gn.gather(root, home), fresh)
        written, golden = files_under(fresh), files_under(GOLDEN)
    expect(written.keys() == golden.keys(),
           "the golden bundle's files are not the ones the gatherer writes",
           f"    written: {sorted(written)}\n    golden:  {sorted(golden)}")
    for rel in sorted(written.keys() & golden.keys()):
        expect(written[rel] == golden[rel],
               f"the gatherer no longer writes {rel} as gui/notices reads it "
               "(see this file's docstring before regenerating)")

    if failures:
        print("test-gather-notices: FAILED")
        for f in failures:
            print("  " + f)
        return 1
    print(f"test-gather-notices: all {passed} checks passed")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
