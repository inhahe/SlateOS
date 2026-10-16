#!/usr/bin/env python3
"""Tests for the interpreter `scripts/hooks/pre-push` runs its gates with.

Run: `python scripts/test-pre-push-python-choice.py` (0 = pass, 1 = fail). No
pytest dependency, matching the other suites in this directory.

What this tests, and why it exists
----------------------------------

The hook used to take `command -v python3` first. On the lanes' host that is
%LOCALAPPDATA%\\Microsoft\\WindowsApps\\python3.exe: an App Execution Alias for the
Store-packaged Python install manager. Measured 2026-09-26 with a 90-second
sleeper under run-timeout's 15-second timeout: the packaged launcher and the
interpreter it starts ran outside the caller's Job Object (the job counted 1
process of 3), so every gate's work was invisible to run-timeout's accounting
-- a 26-minute push reported 28 seconds of CPU -- and beyond its
kill-on-close. It also meant pushes were graded by a different Python install
than boot-test.sh graded boots with.

`find_py` now prefers `python` and skips aliases while a real interpreter
exists. This suite lifts it out of the hook verbatim and runs it under the
hook's own shell with PATHs built from fake interpreters: an alias ahead of a
real one, aliases only, a real `python3` with only an aliased `python`.
"""

from __future__ import annotations

import os
import re
import subprocess
import sys
import tempfile

REPO_ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
HOOK = os.path.join(REPO_ROOT, "scripts", "hooks", "pre-push")
sys.path.insert(0, os.path.join(REPO_ROOT, "scripts"))
import msysbash  # noqa: E402

_FAILURES: list[str] = []


def check(label, got, want):
    if got == want:
        print(f"PASS  {label}")
        return True
    print(f"FAIL  {label}")
    print(f"        got : {got!r}")
    print(f"        want: {want!r}")
    _FAILURES.append(label)
    return False


def lifted_find_py(text):
    match = re.search(r"^find_py\(\)\s*\{.*?^\}\n", text, re.MULTILINE | re.DOTALL)
    if match is None:
        raise RuntimeError("the hook has no `find_py() { ... }`; this suite tests nothing")
    return match.group(0)


def fake(directory, name):
    """An executable the shell will find: a `#!` script, which MSYS runs."""
    os.makedirs(directory, exist_ok=True)
    path = os.path.join(directory, name)
    with open(path, "w", encoding="utf-8", newline="\n") as fh:
        fh.write("#!/bin/sh\nexit 0\n")
    os.chmod(path, 0o755)
    return path


def to_msys(path):
    """C:\\a\\b -> /c/a/b, the form PATH takes inside MSYS."""
    drive, rest = os.path.splitdrive(os.path.abspath(path))
    return "/" + drive.rstrip(":").lower() + rest.replace("\\", "/")


def choose(fn, path_dirs):
    script = f"PATH='{':'.join(to_msys(d) for d in path_dirs)}:/usr/bin'\n{fn}\nfind_py\n"
    proc = subprocess.run([msysbash.bash(), "--posix", "-c", script],
                          capture_output=True, text=True, check=False)
    return proc.stdout.strip()


def main() -> int:
    with open(HOOK, encoding="utf-8", newline="") as fh:
        fn = lifted_find_py(fh.read())

    with tempfile.TemporaryDirectory(prefix="pychoice-", ignore_cleanup_errors=True) as tmp:
        alias = os.path.join(tmp, "Local", "Microsoft", "WindowsApps")
        real = os.path.join(tmp, "python314")
        only3 = os.path.join(tmp, "only3")
        fake(alias, "python3")
        fake(alias, "python")
        fake(real, "python")
        fake(only3, "python3")

        check("a real `python` beats a WindowsApps alias ahead of it on PATH",
              choose(fn, [alias, real]), to_msys(os.path.join(real, "python")))
        check("with aliases only, an alias is still used (a skipped gate is worse)",
              choose(fn, [alias]).startswith(to_msys(alias)), True)
        check("a real `python3` beats an aliased `python`",
              choose(fn, [alias, only3]), to_msys(os.path.join(only3, "python3")))

    if _FAILURES:
        print(f"\n{len(_FAILURES)} check(s) failed")
        return 1
    print("\nall python-choice tests passed")
    return 0


if __name__ == "__main__":
    sys.exit(main())
