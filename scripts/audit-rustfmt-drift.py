#!/usr/bin/env python3
"""Report every tracked .rs file that rustfmt would reformat.

Run: `python scripts/audit-rustfmt-drift.py [--rev HEAD] [paths...]`.
Exit 0 if the tree is clean, 1 if anything drifted, 2 if the audit itself
could not run.

Why this is a script and not a one-liner
----------------------------------------

Two things make `rustfmt --check <every file>` not work here, both measured
rather than guessed:

* Handing rustfmt all 5214 of lane B's .rs paths at once fails with exit 126.
  It has to be batched, and pre-push gate 7 batches at 64 for the same reason.
* rustfmt builds its `--check` diff with an O(n*m)-memory algorithm, so on
  `kernel/src/kshell.rs` (120k lines) it tries to allocate 36 GB and aborts,
  printing an allocation-failure stack trace where the diff should have been.
  It only does this when the file has drifted -- a clean file of the same size
  checks in 2.5 s -- so the unusable report appears precisely when it is needed.

So this asks for a *verdict* in batches and never for a diff, then narrows a
failing batch one file at a time by formatting a copy and comparing bytes,
which never reaches the diff emitter at all.

Why it exists at all
--------------------

Pre-push gate 7 was supposed to stop formatting drift reaching origin, and for
its whole life it read the working tree rather than the commit being pushed
(fixed 2026-09-02, `dcdd711fe`). Whatever drift it let through in the meantime
is still there, and nothing else in the tree looks. This is how to find out.
"""

from __future__ import annotations

import argparse
import os
import shutil
import subprocess
import sys
import tempfile

BATCH = 64
EDITION = "2024"

# `mod name;` -- the form that makes rustfmt go looking for a sibling file.
# Only used to decide whether a lone copy can stand in for the real file, so a
# false positive costs a slower path and a false negative is caught anyway by
# the copy failing to parse.
import re  # noqa: E402

MOD_DECL = re.compile(
    r"^[ \t]*(?:pub(?:\([^)]*\))?[ \t]+)?mod[ \t]+([A-Za-z_][A-Za-z0-9_]*)[ \t]*;",
    re.MULTILINE,
)


def tracked_rs(rev: str, paths: list[str]) -> list[str]:
    args = ["git", "ls-files", "-z", "--"]
    args += [p for p in paths] if paths else ["*.rs"]
    out = subprocess.run(args, capture_output=True, check=True).stdout
    names = [n.decode("utf-8", "surrogateescape")
             for n in out.split(b"\0") if n]
    return sorted(n for n in names if n.endswith(".rs"))


def batch_is_clean(root: str, files: list[str]) -> bool:
    """A verdict for a whole batch. Output discarded -- see the module docstring."""
    proc = subprocess.run(
        ["rustfmt", "--edition", EDITION, "--check",
         *[os.path.join(root, f) for f in files]],
        capture_output=True, cwd=root, check=False,
    )
    return proc.returncode == 0


def file_is_clean(root: str, rel: str) -> bool | None:
    """Format a copy and compare bytes. None if no verdict could be reached."""
    src = os.path.join(root, rel)
    try:
        with open(src, "rb") as fh:
            original = fh.read()
    except OSError:
        return None

    with tempfile.TemporaryDirectory() as tmp:
        work = os.path.join(tmp, os.path.basename(rel))
        with open(work, "wb") as fh:
            fh.write(original)
        # Give rustfmt the submodules it will look for. A stub is "\n" and not
        # an empty file because rustfmt reports a diff on a zero-byte file.
        for name in set(MOD_DECL.findall(
                original.decode("utf-8", "replace"))):
            stub = os.path.join(tmp, name + ".rs")
            if not os.path.exists(stub):
                with open(stub, "w", encoding="utf-8", newline="") as fh:
                    fh.write("\n")
        proc = subprocess.run(["rustfmt", "--edition", EDITION, work],
                              capture_output=True, check=False)
        if proc.returncode != 0:
            return None
        with open(work, "rb") as fh:
            return fh.read() == original


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--rev", default="HEAD",
                    help="recorded in the report; the audit reads the working "
                         "tree, so run it on a clean checkout")
    ap.add_argument("paths", nargs="*", help="pathspecs (default: all .rs)")
    args = ap.parse_args()

    if shutil.which("rustfmt") is None:
        print("audit: rustfmt is not on PATH", file=sys.stderr)
        return 2

    root = subprocess.run(["git", "rev-parse", "--show-toplevel"],
                          capture_output=True, text=True, check=True
                          ).stdout.strip()
    files = tracked_rs(args.rev, args.paths)
    print(f"audit: {len(files)} tracked .rs file(s) under {args.rev}",
          flush=True)

    drifted: list[str] = []
    unknown: list[str] = []
    for start in range(0, len(files), BATCH):
        batch = files[start:start + BATCH]
        if batch_is_clean(root, batch):
            continue
        for rel in batch:
            verdict = file_is_clean(root, rel)
            if verdict is False:
                drifted.append(rel)
                print(f"DRIFT   {rel}", flush=True)
            elif verdict is None:
                unknown.append(rel)
                print(f"NOVERDICT {rel}", flush=True)
        print(f"audit: ...{start + len(batch)}/{len(files)}", flush=True)

    print(f"\naudit: {len(drifted)} drifted, {len(unknown)} without a verdict, "
          f"{len(files)} checked")
    if unknown:
        print("\nNo verdict means rustfmt could not format a lone copy -- "
              "usually an unhandled `mod` form. Check by hand:")
        for rel in unknown:
            print(f"    rustfmt --edition {EDITION} --check {rel}")
    return 1 if drifted else 0


if __name__ == "__main__":
    sys.exit(main())
