#!/usr/bin/env python3
"""Refuse a tracked shell script that carries CRLF **in the working tree**.

WHY THIS EXISTS, and it happened rather than being imagined. Editing
`scripts/patch-diff.sh` from a Python helper on 2026-09-12 rewrote it with
`Path.write_text`, which on Windows translates every `\\n` into `\\r\\n`. The
next run under WSL died at line 69 with

    cd: $'/work\\r': No such file or directory
    exit: 1: numeric argument required

-- an error naming a directory nobody typed, in a script that had been working
a minute earlier. A carriage return at the end of a line becomes part of the
last word on it: a path, a variable, a numeric argument to `exit`.

WHY GIT DOES NOT CATCH IT, which is the whole point of reading the working
tree. This repository normalises line endings on commit, so the *committed*
bytes are LF no matter what the working tree holds. `git diff` shows nothing.
A pre-push gate reading the pushed revision would see a clean file. The damage
exists only on disk, which is precisely where `bash` reads it from -- so this
check must read the working tree, and it is the rare gate for which reading
anything else would be reading the wrong thing.

Found `scripts/bootstrap-worktree.sh` in the same state on the first run: 486
CRLF line endings on disk, zero in `HEAD`, put there by something long before
today. It is invoked with `--check` before the build and before the boot lock,
so it is not a file anyone would want to discover this way.

WHAT IT INSPECTS. Every tracked file that a shell will execute: anything named
`*.sh`, plus anything whose first line is a `sh`/`bash`/`dash` shebang whatever
it is called -- `scripts/hooks/pre-push` has no extension and is exactly the
kind of file this would otherwise miss.

WHAT IT DOES NOT CLAIM. Nothing about `.py`, `.rs` or `.md`: Python, rustc and
markdown all read CRLF without complaint, and flagging them would make this a
style gate that gets switched off rather than a correctness gate that gets
believed. And nothing about a *lone* CR inside a line, which is a different
defect and belongs to `check-control-bytes`.
"""

import re
import subprocess
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import gitenv
import selftestflag

ROOT = Path(__file__).resolve().parent.parent

SHEBANG = re.compile(rb"^#!\s*\S*/(?:env\s+)?(?:ba|da|a|k)?sh\b")

# A FLOOR, because "0 offenders" and "found nothing to inspect" print the same
# word. This tree has ~780 tracked shell scripts; a run that finds fewer than
# this has lost its file list -- a bad `git ls-files`, a foreign GIT_DIR, a
# checkout that is not this repository -- and its clean verdict is about
# nothing. Set well below the real count so it fires on breakage, not on churn.
FLOOR = 400


def tracked_files() -> list[str]:
    """Every tracked path, from git rather than from a directory walk.

    `git ls-files` and not `rglob`, because a walk would also find build
    output, other lanes' scratch and anything a `.gitignore` exists to keep out
    of a verdict.
    """
    out = subprocess.run(
        ["git", "ls-files", "-z"],
        cwd=ROOT,
        env=gitenv.clean_env(),
        capture_output=True,
        check=False,
    )
    if out.returncode != 0:
        raise SystemExit(
            "check-shell-crlf: `git ls-files` failed:\n"
            + out.stderr.decode("utf-8", "replace")
        )
    return [p for p in out.stdout.decode("utf-8", "replace").split("\0") if p]


def is_shell(path: Path, raw: bytes) -> bool:
    """Will a shell execute this file?  By name, or by shebang."""
    if path.name.endswith(".sh"):
        return True
    return bool(SHEBANG.match(raw.split(b"\n", 1)[0]))


def scan(paths: list[str]) -> tuple[list[tuple[str, int]], int]:
    """Return (offenders, number of shell scripts inspected)."""
    offenders = []
    inspected = 0
    for rel in paths:
        full = ROOT / rel
        try:
            raw = full.read_bytes()
        except OSError:
            # A tracked path that is not readable here -- a submodule, a
            # symlink to nowhere, a file another lane is mid-write on. Not
            # evidence either way, and not this gate's business to report.
            continue
        if not is_shell(full, raw):
            continue
        inspected += 1
        n = raw.count(b"\r\n")
        if n:
            offenders.append((rel, n))
    return offenders, inspected


SELFTEST = [
    ("a .sh with LF only is fine", "a.sh", b"#!/bin/sh\necho hi\n", False, True),
    ("a .sh with CRLF is an offender", "a.sh", b"#!/bin/sh\r\necho hi\r\n", True, True),
    (
        "one CRLF among many LF still counts",
        "a.sh",
        b"#!/bin/sh\necho one\r\necho two\n",
        True,
        True,
    ),
    (
        "an extensionless file with a sh shebang IS inspected",
        "hooks/pre-push",
        b"#!/bin/sh\r\necho hi\r\n",
        True,
        True,
    ),
    (
        "...and with a bash shebang",
        "hooks/pre-push",
        b"#!/usr/bin/env bash\r\ntrue\r\n",
        True,
        True,
    ),
    (
        "a .py with CRLF is NOT this gate's business",
        "a.py",
        b"#!/usr/bin/env python3\r\nprint(1)\r\n",
        False,
        False,
    ),
    (
        "a python shebang is not a shell shebang",
        "tool",
        b"#!/usr/bin/env python3\r\nprint(1)\r\n",
        False,
        False,
    ),
    (
        "a .rs with CRLF is not either",
        "a.rs",
        b"fn main() {}\r\n",
        False,
        False,
    ),
    (
        "a lone CR inside a line is check-control-bytes' job, not this one",
        "a.sh",
        b"#!/bin/sh\necho a\rb\n",
        False,
        True,
    ),
]


def selftest(tmp: Path) -> int:
    bad = 0
    for name, rel, body, want_offender, want_inspected in SELFTEST:
        path = tmp / rel
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(body)
        inspected = is_shell(path, body)
        offender = inspected and b"\r\n" in body
        ok = (offender == want_offender) and (inspected == want_inspected)
        bad += 0 if ok else 1
        print("%-4s %s" % ("ok" if ok else "FAIL", name))
        if not ok:
            print(
                "       wanted offender=%s inspected=%s, got offender=%s inspected=%s"
                % (want_offender, want_inspected, offender, inspected)
            )

    # The floor, proved rather than asserted: a run over a tiny file list must
    # refuse even when it finds no offender, because a clean verdict over four
    # files is not a clean verdict over the tree.
    _offenders, inspected = scan(["a.sh"])
    ok = inspected < FLOOR
    bad += 0 if ok else 1
    print("%-4s a run that inspects almost nothing is below the floor"
          % ("ok" if ok else "FAIL"))

    # ...and the real tree is above it, so the floor is not set so high that it
    # fires on the healthy case. This is the half that a floor usually lacks.
    _offenders, real = scan(tracked_files())
    ok = real >= FLOOR
    bad += 0 if ok else 1
    print("%-4s the real tree is above the floor (%d >= %d)"
          % ("ok" if ok else "FAIL", real, FLOOR))

    print()
    print("check-shell-crlf selftest: %d case(s), %d failed" % (len(SELFTEST) + 2, bad))
    return 1 if bad else 0


def main(argv=None) -> int:
    argv = sys.argv[1:] if argv is None else argv
    if selftestflag.wants_selftest(argv):
        import tempfile

        with tempfile.TemporaryDirectory(prefix="shell-crlf-") as tmp:
            return selftest(Path(tmp))

    offenders, inspected = scan(tracked_files())

    if inspected < FLOOR:
        print(
            "check-shell-crlf: only %d shell script(s) found, below the floor of "
            "%d.\n"
            "  A clean verdict over that few is a verdict about nothing. Suspect\n"
            "  the file list rather than the tree: a foreign GIT_DIR, a checkout\n"
            "  that is not this repository, or `git ls-files` returning short."
            % (inspected, FLOOR),
            file=sys.stderr,
        )
        return 2

    for rel, n in offenders:
        print("%s: %d CRLF line ending(s)" % (rel, n))
    if offenders:
        print()
        print("A carriage return at the end of a line becomes part of the last")
        print("word on it, so `cd $dir` looks for a directory whose name ends in")
        print("a CR and `exit $n` gets a non-numeric argument. The script worked")
        print("until something rewrote it.")
        print()
        print("GIT WILL NOT SHOW YOU THIS. Line endings are normalised on commit,")
        print("so HEAD is already LF and `git diff` is empty. The damage is on")
        print("disk only, which is where bash reads it from.")
        print()
        print("Usual cause: a Python helper using `Path.write_text` or `open(...,")
        print("'w')` on Windows, both of which translate every newline. Read and")
        print("write bytes instead:")
        print("    p.write_bytes(p.read_bytes().replace(b'\\r\\n', b'\\n'))")
        return 1

    print(
        "check-shell-crlf: clean -- %d shell script(s) inspected (of %d tracked "
        "file(s)), 0 with CRLF." % (inspected, len(tracked_files()))
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
