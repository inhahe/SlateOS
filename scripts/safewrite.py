#!/usr/bin/env python3
"""Write a file without destroying it when the write goes wrong.

`io.open(path, "w")` truncates the file and *then* validates its arguments, so
a typo in `newline=` empties the target before raising. On 2026-09-17 that
emptied `scripts/hooks/pre-push` -- 5,591 lines, the hook every lane pushes
through -- from a script whose only fault was `newline="\\\\n"` where it meant
`newline="\\n"`. An empty hook passes `sh -n`, git runs it, it exits 0, and
every gate silently does not run.

Build the text first, write it beside the target, then rename over it. A
rename within a directory is atomic, so a failure anywhere before it leaves
the original exactly as it was.

## Why this file is in `scripts/` and not in `build/`

It was written in `build/`, and `build/` is in `.gitignore`. So the remedy for
a data-loss defect existed only in one working tree: `known-issues.md` ->
`TD-C-A-BAD-ARGUMENT-TO-OPEN-EMPTIES-THE-FILE-BEFORE-IT-COMPLAINS` told the
reader to use `build/safewrite.py`, a fresh clone had no such file, and the
hundred-odd scripts that imported it were themselves ignored scratch. Adoption
looked broad and was zero.

The scripts that most need this are the durable gates here -- the ones every
lane runs repeatedly, several of which rewrite tracked files in place
(`check-collapsed-messages.py --apply` rewrites arbitrary `.rs`; the baseline
files are how four gates remember what they have already seen). A one-shot
generator that has already run is the least important user it has.

## The interaction worth knowing about

`check-text-mode-writes.py` requires every text-mode write under `scripts/` to
pass `newline=`, because the Windows default silently turns `\\n` into `\\r\\n`.
That rule is right and it is also what put a hand-typed escape at every write
site in the tree -- and a mistyped one is what emptied the hook. A gate that
demands an argument raises the odds of a bad argument, and a destructive
default turns a bad argument into data loss. This function is what makes the
two rules compose: the newline is passed once, here, and a bad one costs a
temporary file.

Usage:

    import pathlib, sys
    sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))
    from safewrite import write_text

    write_text(path, text)                  # LF, the default
    write_text(path, text, newline="")      # no translation at all

The `sys.path` line is `__file__`-relative on purpose. Spelling it
`sys.path.insert(0, "scripts")` works only when the process happens to have
been started from the repository root, and a tool that writes files is the
wrong place to find out that it was not.

Self-test:

    python scripts/safewrite.py --self-test
"""

import io
import os
import pathlib
import sys
import tempfile

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))
import selftestflag  # noqa: E402

#: The suffix the in-progress copy is given, beside the target.
TMP_SUFFIX = ".tmp-safewrite"


def write_text(path, text, newline="\n"):
    """Replace `path` with `text`, or leave it exactly as it was.

    `newline` is passed to the temporary file, so an illegal value raises
    against a file nothing is reading and the target is never opened at all.
    """
    tmp = f"{path}{TMP_SUFFIX}"
    try:
        # Any argument error raises here, against a file nothing is reading.
        with io.open(tmp, "w", encoding="utf-8", newline=newline) as handle:
            handle.write(text)
        os.replace(tmp, path)
    finally:
        # Reached with `tmp` gone on the success path, since `os.replace`
        # consumed it. On every failure path it is a partial file beside a
        # target that still holds the old contents, and leaving it would be a
        # second defect: the next run's `--list` would scan it.
        if os.path.exists(tmp):
            os.remove(tmp)


def self_test():
    """Check the property this file exists for, rather than that it runs."""
    bad = 0
    with tempfile.TemporaryDirectory() as d:
        target = os.path.join(d, "target.txt")

        # 1. A bad `newline=` must leave the original untouched. This is the
        #    whole point, and it is the case that cost 5,591 lines.
        write_text(target, "original\n")
        raised = False
        try:
            write_text(target, "replacement\n", newline=chr(92) + "n")
        except ValueError:
            raised = True
        survived = io.open(target, encoding="utf-8", newline="").read()
        ok = raised and survived == "original\n"
        bad += not ok
        print(
            f"{'ok  ' if ok else 'FAIL'}  an illegal newline leaves the file "
            f"as it was: raised={raised} content={survived!r}"
        )

        # 2. The positive control. Without it, a `write_text` that did nothing
        #    at all would pass the case above.
        write_text(target, "replacement\n")
        got = io.open(target, encoding="utf-8", newline="").read()
        ok = got == "replacement\n"
        bad += not ok
        print(f"{'ok  ' if ok else 'FAIL'}  a good write replaces it: {got!r}")

        # 3. No temporary file is left beside the target, on either path.
        leftovers = [n for n in os.listdir(d) if n.endswith(TMP_SUFFIX)]
        ok = not leftovers
        bad += not ok
        print(f"{'ok  ' if ok else 'FAIL'}  nothing left beside it: {leftovers}")

        # 4. The newline is honoured rather than merely accepted -- a writer
        #    that ignored it would satisfy every case above while still
        #    producing the CRLF that `check-text-mode-writes.py` exists to
        #    prevent.
        write_text(target, "a\nb\n")
        raw = io.open(target, "rb").read()
        ok = raw == b"a\nb\n"
        bad += not ok
        print(f"{'ok  ' if ok else 'FAIL'}  LF is written as LF: {raw!r}")

    print(f"\n4 cases, {bad} failed")
    return 1 if bad else 0


if __name__ == "__main__":
    if selftestflag.wants_selftest(sys.argv[1:]):
        sys.exit(self_test())
    print(__doc__)
    sys.exit(0)
