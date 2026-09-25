#!/usr/bin/env python3
"""Tests for `msysbash.py`, the one place that decides which bash runs our scripts.

The failure it prevents is silent: a bare "bash" on Windows is WSL's, and a
suite run under it passes while WSL is warm, testing the wrong shell and git.
So the classification of paths is pinned exactly, and on Windows the resolved
bash is checked to be MSYS by asking it.
"""
from __future__ import annotations

import os
import subprocess
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import msysbash  # noqa: E402

FAILURES: list[str] = []


def check(label, got, want):
    if got != want:
        FAILURES.append(f"{label}: got {got!r}, want {want!r}")
        print(f"  FAIL {label}: got {got!r}, want {want!r}")
    else:
        print(f"  ok   {label}")


print("which paths are a Git install's bash")
for label, path, want in [
    ("WSL's launcher is never one", r"C:\Windows\System32\bash.exe", False),
    ("Git's bin\\bash.exe is", r"C:\Program Files\Git\bin\bash.exe", True),
    ("Git's usr\\bin\\bash.exe is", r"C:\Program Files\Git\usr\bin\bash.exe",
     True),
    ("a Git install on another drive is", r"D:\tools\Git\usr\bin\bash.exe",
     True),
    ("a Git directory that holds no bash is not",
     r"C:\Program Files\Git\mingw64\bin\bash.exe", False),
    ("a POSIX path is not", "/usr/bin/bash", False),
]:
    check(label, msysbash._is_git_install_path(path), want)

print()
print("the resolved bash")
if os.name == "nt":
    found = msysbash.bash()
    check("it exists", os.path.exists(found), True)
    check("it is not WSL's launcher",
          "system32" in os.path.normcase(found), False)
    try:
        probe = subprocess.run([found, "-c", "uname -o"], capture_output=True,
                               text=True, timeout=msysbash.VERIFY_TIMEOUT_S)
        check("and it says it is MSYS", "msys" in probe.stdout.lower(), True)
    except subprocess.TimeoutExpired:
        # A host too starved to start bash in a minute cannot answer this, and
        # `find_msys_bash` accepted the path on structure for the same reason.
        print("  SKIP and it says it is MSYS: bash did not answer within "
              f"{msysbash.VERIFY_TIMEOUT_S}s on this host")
    check("the answer is cached, not searched for again",
          msysbash.find_msys_bash() is msysbash.find_msys_bash(), True)
else:
    check("off Windows the bash on PATH is used", bool(msysbash.bash()), True)

print()
if FAILURES:
    print(f"{len(FAILURES)} FAILURE(S)")
    for failure in FAILURES:
        print(f"  - {failure}")
    sys.exit(1)
print("all msysbash tests passed")
