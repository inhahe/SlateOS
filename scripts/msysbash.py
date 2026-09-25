"""The bash this repository's shell scripts actually run under -- found, not assumed.

On Windows, `subprocess.run(["bash", ...])` does not search PATH first.
`CreateProcess` looks in System32 before PATH, and with WSL installed
`C:\\Windows\\System32\\bash.exe` is the WSL launcher, so a bare "bash" runs a
script under Ubuntu's bash and git with a `/mnt/<drive>` view of the disk --
which is never what `boot-test.sh` or the push hook run under: they run under
Git for Windows' MSYS bash. It also fails whenever WSL is unwell, because
WSL's startup warnings and `Catastrophic failure` arrive as the script's own
output. `shutil.which("bash")` does not help: it is a PATH search, and PATH
lists System32 first on this machine, so it can answer WSL too.

This module is the one place that decides. Candidates are verified with
`uname -o`, which must answer `Msys`, so a fallback can never quietly become
WSL again.

Several suites carried a copy of this search (`test-canary-load.py`'s
`find_msys_bash`, from which this one is taken; `selftest-boot-gate-identity.py`'s
`_find_bash`; `layout-sweep.py`'s) and some carried none. That is how
`test-boot-history-commit.py` and three harness runners in `test-boot-test.py`
ran `boot-test.sh`'s own functions under WSL's bash: they passed while WSL was
warm and failed when it was starting up or sick (lane F, 2026-09-25).
"""
from __future__ import annotations

import functools
import os
import shutil
import subprocess

#: How long `uname -o` may take to answer. It needs a fraction of a second; the
#: allowance is for a host so starved that nothing does (see `find_msys_bash`).
VERIFY_TIMEOUT_S = 60


def _is_git_install_path(path: str) -> bool:
    """Whether `path` is a bash.exe inside a Git-for-Windows install tree.

    Structural evidence rather than behavioural: WSL's launcher lives in
    System32 and nowhere else, so a bash.exe under `...\\Git\\bin` or
    `...\\Git\\usr\\bin` cannot be it.
    """
    low = os.path.normcase(os.path.abspath(path)).replace("/", "\\")
    if "\\system32\\" in low:
        return False
    return low.endswith(("\\git\\bin\\bash.exe", "\\git\\usr\\bin\\bash.exe"))


@functools.cache
def find_msys_bash() -> str | None:
    """Git-for-Windows' bash, located explicitly rather than taken from PATH.

    `MSYS_BASH` overrides; then the install roots implied by Git's PATH
    entries; then the standard install locations. Each candidate is verified
    with `uname -o`. A candidate whose verification *times out* is accepted
    only if its path is structurally a Git install (`_is_git_install_path`):
    on a host starved enough that bash cannot answer in a minute, refusing it
    would report "no MSYS bash" for a bash that exists, and accepting an
    unverified path anywhere else could be WSL's. Returns `None` if nothing
    qualifies.
    """
    candidates = []
    override = os.environ.get("MSYS_BASH")
    if override:
        candidates.append(override)
    # Git bash usually reaches the Windows PATH via its own `usr\bin` or
    # `mingw64\bin`; both sit beside a `bin\bash.exe` under the install root.
    for entry in os.environ.get("PATH", "").split(os.pathsep):
        low = entry.lower().replace("/", "\\")
        for marker in ("\\git\\usr\\bin", "\\git\\mingw64\\bin", "\\git\\bin"):
            if low.endswith(marker):
                root = entry
                for _ in range(marker.count("\\")):
                    root = os.path.dirname(root)
                candidates.append(os.path.join(root, "bin", "bash.exe"))
                candidates.append(os.path.join(root, "usr", "bin", "bash.exe"))
    candidates += [
        r"C:\Program Files\Git\bin\bash.exe",
        r"C:\Program Files\Git\usr\bin\bash.exe",
        r"C:\Program Files (x86)\Git\bin\bash.exe",
    ]
    seen = set()
    for candidate in candidates:
        key = os.path.normcase(os.path.abspath(candidate))
        if key in seen or not os.path.exists(candidate):
            continue
        seen.add(key)
        try:
            probe = subprocess.run([candidate, "-c", "uname -o"],
                                   capture_output=True, text=True,
                                   timeout=VERIFY_TIMEOUT_S)
        except subprocess.TimeoutExpired:
            if _is_git_install_path(candidate):
                return candidate
            continue
        except OSError:
            continue
        if probe.returncode == 0 and "msys" in probe.stdout.strip().lower():
            return candidate
    return None


def bash() -> str:
    """The bash to run a repository shell script under.

    On Windows, Git's MSYS bash, verified; any other bash there is the wrong
    one, so its absence raises rather than falling back. Elsewhere, the bash
    on PATH, which is what the scripts run under there.

    Raises:
        RuntimeError: on Windows, if no Git-for-Windows bash can be found.
    """
    if os.name != "nt":
        return shutil.which("bash") or "/bin/bash"
    found = find_msys_bash()
    if found is None:
        raise RuntimeError(
            "no Git-for-Windows (MSYS) bash found; set MSYS_BASH to its "
            "bash.exe. A bare `bash` here is WSL's, which is not the shell "
            "these scripts run under.")
    return found
