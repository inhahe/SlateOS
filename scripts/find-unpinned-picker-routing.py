r"""Whose tests would notice if the file picker stopped receiving events?

A door is three things, and a test can cover the first two while missing the
third entirely:

    1. a control that opens the picker          `Ctrl+O` -> `picker.is_open()`
    2. a writer that puts bytes on the disk     `write_report(&path)`
    3. the routing that connects them           `picker.handle(event, ..)`

Almost every door test in this tree covers (1) and (2). (1) asserts that the
key handler opened the dialog -- but the KEY HANDLER is what opens it, so that
assertion holds whether or not the picker is ever handed another event. (2)
calls the writer with a path directly, which is the right way to test a writer
and never touches the dialog at all.

So the user's actual path -- open the picker, choose a file, get a file -- can
be untested while both halves look covered. Cutting `picker.handle` entirely
leaves such a suite green.

WHAT THE ROUTING ACTUALLY DECIDES, and why a missing test here is not
cosmetic:

  * **`Picked::Chose` never fires**, so choosing a file does nothing at all.
    The door is shut and every test still passes.
  * **The dialog stops being modal.** Keys meant for the filename box land in
    the document behind it. `apps/dbviewer` has the test worth copying:
    typing while the picker is up must not reach the SQL editor.

HOW IT DECIDES. It cuts the routing -- rewrites `self.picker.handle(..) {` to
`Picked::Ignored {` -- runs that crate's tests, and reports whether anything
went red. A crate whose tests all pass without the picker ever receiving an
event has no coverage of (3).

It restores every file it touches and verifies the restore by SHA-256 before
exiting. It is slow: one `cargo test` per app.

Measured 2026-09-15: **16 of 20 apps unpinned.** The four that were pinned --
`dbviewer`, `diagram`, `podcast`, `clipmanager` -- each carry a test asserting
that input while the dialog is up does not reach the window behind it, which
is the cheapest form this coverage takes.

The first run of this said 14, and reported `calendar` and `musicplayer` as
"could not cut the routing". That was this file's bug, not theirs: the pattern
began with a literal backspace, because a backslash-b written into an unquoted
heredoc arrived as the control character rather than as a word-boundary
escape. **A checker that excludes what it cannot parse, and files that under a
word meaning "not evidence", will under-report exactly as quietly as one that
scans the wrong directory.** The two apps route through `state.picker` rather
than `self.picker`, which is what the widened pattern is for.

WHY IT HAS A SELF-TEST. Every app is pinned now, so this reports zero and will
go on reporting zero -- which is indistinguishable from a pattern that has
stopped matching. **That is not hypothetical: it already happened.** The first
version of this file could not match `state.picker.handle(..)` at all, because
its pattern began with a literal backspace, and it filed the two apps it could
not read under "could not cut the routing; not evidence" -- a phrase that reads
as a fact about those apps.

So `--self-test` checks the pattern against the shapes the tree actually
contains, including the ones rustfmt has split across lines. A checker whose
only output is a zero needs a way to prove it can still find something.

Usage:  python scripts/find-unpinned-picker-routing.py [--apps=kanban,torrent]
        python scripts/find-unpinned-picker-routing.py --self-test
"""

import hashlib
import pathlib
import re
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent

# Where a crate's directory name is not its package name.
PACKAGE = {"sysinfo": "sysinfo-app"}

# `self.picker.handle(..)` and `state.picker.handle(..)` -- the second is
# what an app whose event handler is a free function writes, and matching
# only the first quietly skipped `calendar` and `musicplayer`.
ROUTING = re.compile(r"\w+\s*\.\s*picker\s*\.?\s*handle\([^;]*?\)\s*\{", re.S)


def apps_with_a_picker():
    return sorted(
        p.parent.parent.name
        for p in (ROOT / "apps").glob("*/src/main.rs")
        if "picker.handle(" in p.read_text(encoding="utf-8", errors="replace")
    )


# Every routing shape in the tree on 2026-09-15, plus the two that broke the
# first version. Kept verbatim rather than paraphrased: the point is that these
# are what the files really say.
SELF_TEST = [
    ("self.picker.handle(event, self.width, self.height) {", True),
    ("state.picker.handle(event, state.width, state.height) {", True),
    ("match self.picker.handle(event, self.win_width, self.win_height) {", True),
    # rustfmt splits a long call, and the pattern has to survive it.
    ("match self\n            .picker\n            .handle(event, self.window_width, self.window_height)\n        {", True),
    ("match self.picker.handle(event, size.0, size.1) {", True),
    # Not the routing: a call that opens the dialog, and one that draws it.
    ("self.picker.open_to_read();", False),
    ("self.picker.render(&self.palette, width, height)", False),
    # Not a picker at all.
    ("self.dialog.handle(event, w, h) {", False),
]


def self_test():
    bad = 0
    for text, should in SELF_TEST:
        hit = bool(ROUTING.search(text))
        if hit != should:
            bad += 1
            verb = "matched" if hit else "did not match"
            print(f"  FAIL  {verb}, expected the opposite: {text[:60]!r}")
    print(f"{len(SELF_TEST) - bad}/{len(SELF_TEST)} self-test case(s) as expected")
    return 1 if bad else 0


def main():
    wanted = None
    for arg in sys.argv[1:]:
        if arg.startswith("--apps="):
            wanted = {a for a in arg.split("=", 1)[1].split(",") if a}
        elif arg in ("--self-test", "--selftest"):
            return self_test()
        else:
            print(f"unknown argument: {arg}", file=sys.stderr)
            return 2

    apps = [a for a in apps_with_a_picker() if wanted is None or a in wanted]
    if not apps:
        print("no apps route events to a picker -- refusing to call that a pass",
              file=sys.stderr)
        return 2

    print(f"{len(apps)} app(s) route events to a picker\n")
    unpinned, pinned, skipped = [], [], []

    for app in apps:
        f = ROOT / "apps" / app / "src" / "main.rs"
        original = f.read_bytes()
        before = hashlib.sha256(original).hexdigest()
        try:
            cut, n = ROUTING.subn("Picked::Ignored {", f.read_text(encoding="utf-8"), count=1)
            if n == 0:
                skipped.append(app)
                print(f"--   {app}: could not cut the routing; not evidence")
                continue
            f.write_text(cut, encoding="utf-8", newline="\n")

            r = subprocess.run(
                [sys.executable, str(ROOT / "scripts" / "run-timeout.py"), "600",
                 "cargo", "test", "-p", PACKAGE.get(app, app),
                 "--target", "x86_64-pc-windows-gnu"],
                capture_output=True, text=True, errors="replace", cwd=ROOT,
            )
            out = r.stdout + r.stderr
            # A build error is not a red test: it shows the compiler rejected
            # the edit, not that any assertion noticed anything.
            if "error[E" in out or "error: could not compile" in out:
                skipped.append(app)
                print(f"--   {app}: did not compile; not evidence")
                continue
            red = [ln.split()[1] for ln in out.splitlines()
                   if ln.startswith("test ") and "FAILED" in ln]
            if red:
                pinned.append(app)
                print(f"ok   {app}: {len(red)} test(s) noticed")
            else:
                unpinned.append(app)
                print(f"!!   {app}: NOTHING noticed")
        finally:
            f.write_bytes(original)
            assert hashlib.sha256(f.read_bytes()).hexdigest() == before, (
                f"RESTORE FAILED for {app}"
            )

    print()
    print(f"pinned:   {len(pinned):>2}  {' '.join(pinned)}")
    print(f"unpinned: {len(unpinned):>2}  {' '.join(unpinned)}")
    if skipped:
        print(f"skipped:  {len(skipped):>2}  {' '.join(skipped)}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
