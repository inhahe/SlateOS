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

Measured 2026-09-15: **14 of 20 apps unpinned.** The four that were pinned --
`dbviewer`, `diagram`, `podcast`, `clipmanager` -- each carry a test asserting
that input while the dialog is up does not reach the window behind it, which
is the cheapest form this coverage takes.

Usage:  python scripts/find-unpinned-picker-routing.py [--apps=kanban,torrent]
"""

import hashlib
import pathlib
import re
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent

# Where a crate's directory name is not its package name.
PACKAGE = {"sysinfo": "sysinfo-app"}

ROUTING = re.compile(r"self\.picker\s*\.?\s*handle\([^;]*?\)\s*\{", re.S)


def apps_with_a_picker():
    return sorted(
        p.parent.parent.name
        for p in (ROOT / "apps").glob("*/src/main.rs")
        if "picker.handle(" in p.read_text(encoding="utf-8", errors="replace")
    )


def main():
    wanted = None
    for arg in sys.argv[1:]:
        if arg.startswith("--apps="):
            wanted = {a for a in arg.split("=", 1)[1].split(",") if a}
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
