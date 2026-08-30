#!/usr/bin/env python3
"""proctree.py — launch a child process so that its *whole tree* can be killed.

The problem this exists for: neither coreutils `timeout` nor Python's own
`subprocess.run(timeout=...)` kills anything but the direct child. Everything
that child spawned survives — a deadlocked test binary, a shell's `&` job, an
interposed `#!` interpreter — and lingers as an orphan.

On Windows the tree is held together by a Job Object created with
`JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`: every descendant inherits membership, and
closing the last handle to the job terminates all of them atomically — including
when *this* process dies, so nothing can be orphaned by our own crash. On POSIX
a process group plus `SIGKILL` does the same job.

Two entry points, sharing that one mechanism:

* [`Tree`] — a context manager around a `Popen` for callers that want to drive
  the child themselves (streaming its output, printing heartbeats). `run-timeout.py`
  uses this.
* [`run_captured`] — the batch case: feed stdin, capture stdout/stderr, and
  enforce a deadline. Use this instead of `subprocess.run(..., timeout=...)`.

## Why `subprocess.run(timeout=…)` is not enough

Its Windows timeout path is:

```python
except TimeoutExpired as exc:
    process.kill()                                   # the direct child only
    if _mswindows:
        exc.stdout, exc.stderr = process.communicate()   # no timeout
```

The capture threads are blocked in `read()` on the pipes, and a pipe does not
report EOF while *any* process holds its write end. A grandchild that inherited
those handles keeps them open after the child is killed, so that second
`communicate()` — which takes no timeout — never returns. The timeout silently
becomes infinite, and the symptom is a runner sitting at 0% CPU with no
children, having apparently stopped mid-run. Killing the tree first is what
closes the handles and lets the drain finish.
"""

from __future__ import annotations

import os
import shutil
import subprocess
import sys
from dataclasses import dataclass
from pathlib import Path, PurePosixPath

IS_WINDOWS = sys.platform.startswith("win")

# How long to wait for the capture threads once the tree is dead. Their pipes'
# write ends are all closed by then, so this is a guard against the impossible
# rather than a real budget.
DRAIN_GRACE = 10.0

# How long to let the `taskkill` fallback run. It is a belt-and-braces second
# kill — closing the job handle has already terminated the tree — so it is never
# the thing that decides whether the child dies, and it must never be the thing
# that decides how long we wait. Left unbounded it is exactly that: `taskkill`
# blocks while a target sits in an uninterruptible wait, and a caller stuck
# inside it still *holds the job handle open*, which is what keeps a runaway
# case alive. Observed: a sweep wedged at 0% CPU for 23 hours with one of its
# cases spinning at 100%, having escaped the containment meant to reap it.
TASKKILL_GRACE = 10.0


if IS_WINDOWS:
    import ctypes
    from ctypes import wintypes

    _k32 = ctypes.WinDLL("kernel32", use_last_error=True)

    JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE = 0x2000
    JobObjectExtendedLimitInformation = 9

    class _IO_COUNTERS(ctypes.Structure):
        _fields_ = [
            ("ReadOperationCount", ctypes.c_ulonglong),
            ("WriteOperationCount", ctypes.c_ulonglong),
            ("OtherOperationCount", ctypes.c_ulonglong),
            ("ReadTransferCount", ctypes.c_ulonglong),
            ("WriteTransferCount", ctypes.c_ulonglong),
            ("OtherTransferCount", ctypes.c_ulonglong),
        ]

    class _JOBOBJECT_BASIC_LIMIT_INFORMATION(ctypes.Structure):
        _fields_ = [
            ("PerProcessUserTimeLimit", wintypes.LARGE_INTEGER),
            ("PerJobUserTimeLimit", wintypes.LARGE_INTEGER),
            ("LimitFlags", wintypes.DWORD),
            ("MinimumWorkingSetSize", ctypes.c_size_t),
            ("MaximumWorkingSetSize", ctypes.c_size_t),
            ("ActiveProcessLimit", wintypes.DWORD),
            ("Affinity", ctypes.POINTER(wintypes.ULONG)),
            ("PriorityClass", wintypes.DWORD),
            ("SchedulingClass", wintypes.DWORD),
        ]

    class _JOBOBJECT_EXTENDED_LIMIT_INFORMATION(ctypes.Structure):
        _fields_ = [
            ("BasicLimitInformation", _JOBOBJECT_BASIC_LIMIT_INFORMATION),
            ("IoInfo", _IO_COUNTERS),
            ("ProcessMemoryLimit", ctypes.c_size_t),
            ("JobMemoryLimit", ctypes.c_size_t),
            ("PeakProcessMemoryUsed", ctypes.c_size_t),
            ("PeakJobMemoryUsed", ctypes.c_size_t),
        ]

    def create_kill_on_close_job():
        """Create a Job Object that kills all member processes when the last
        handle to it closes. Returns the job handle, or None on failure."""
        _k32.CreateJobObjectW.restype = wintypes.HANDLE
        job = _k32.CreateJobObjectW(None, None)
        if not job:
            return None
        info = _JOBOBJECT_EXTENDED_LIMIT_INFORMATION()
        info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE
        if not _k32.SetInformationJobObject(
            job,
            JobObjectExtendedLimitInformation,
            ctypes.byref(info),
            ctypes.sizeof(info),
        ):
            _k32.CloseHandle(job)
            return None
        return job

    def assign_to_job(job, proc) -> bool:
        # proc._handle is the process HANDLE on Windows.
        return bool(_k32.AssignProcessToJobObject(job, int(proc._handle)))

    def close_job(job) -> None:
        if job:
            _k32.CloseHandle(job)

    def terminate_tree(job, proc) -> None:
        # `taskkill /T` first, *then* the job: order matters. Assignment to the
        # job happens a moment after `CreateProcess` returns, so a child that
        # forks immediately — an MSYS shell reaching a `&` in its first
        # milliseconds — can be running before the job exists and is then
        # outside it. `/T` is what catches those, and it walks the tree from the
        # parent down, so it needs the parent still alive. Closing the job first
        # kills the parent and leaves `/T` nothing to enumerate; measured, that
        # loses a forked grandchild roughly one run in three, and the survivor
        # spins forever. Closing the job afterwards costs nothing when the tree
        # is already dead.
        #
        # The call is bounded, and its own tree is torn down on expiry — see
        # `TASKKILL_GRACE`. A hang here used to take the caller with it, and the
        # caller is the one holding the job open, which is how a runaway case
        # escaped the containment meant to reap it.
        try:
            with Tree(
                ["taskkill", "/F", "/T", "/PID", str(proc.pid)],
                stdout=subprocess.DEVNULL,
                stderr=subprocess.DEVNULL,
            ) as killer:
                try:
                    killer.proc.wait(timeout=TASKKILL_GRACE)
                except subprocess.TimeoutExpired:
                    pass
        except OSError:
            pass
        # And directly, so the child dies even if taskkill is refused.
        try:
            proc.kill()
        except OSError:
            pass
        # Closing the kill-on-close job handle terminates whatever is left.
        close_job(job)

    def popen_kwargs() -> dict:
        return {}

else:
    import os
    import signal

    def create_kill_on_close_job():
        return None  # POSIX uses process groups instead

    def assign_to_job(job, proc) -> bool:
        return True  # child already leads its own group (start_new_session)

    def close_job(job) -> None:
        return None

    def terminate_tree(job, proc) -> None:
        try:
            os.killpg(proc.pid, signal.SIGKILL)
        except (ProcessLookupError, PermissionError):
            pass

    def popen_kwargs() -> dict:
        return {"start_new_session": True}  # child leads its own process group


# Shell-script launching on Windows.
#
# Two separate traps, both of which produce a *misleading* run rather than an
# error, which is why they are handled here at the single launch point rather
# than left to each caller:
#
#  1. `CreateProcess` cannot execute a `.sh` at all — there is no shebang
#     handling on Windows — so `Tree(["./scripts/boot-test.sh"])` dies with
#     "%1 is not a valid Win32 application". Harmless on its own, but a caller
#     that appends `; echo $?` (or otherwise reports the *wrapper's* status)
#     turns "never ran" into a green result.
#
#  2. Naming `bash` explicitly is worse, because it appears to work. From a
#     native-Windows parent, `CreateProcess` searches System32 *before* PATH,
#     and `C:\Windows\System32\bash.exe` is the WSL launcher. So the child is a
#     Linux bash in an entirely different filesystem namespace: our scripts'
#     `/c/Program Files/qemu`, `cygpath` and `taskkill //F` are all absent, and
#     the script fails for reasons that have nothing to do with the code under
#     test. Note that `shutil.which("bash")` does NOT reproduce this — it walks
#     PATH and answers Git Bash — so the PATH lookup and the actual launch
#     disagree, and only the launch is authoritative.
#
# The fix for both is to resolve an *absolute* path to an MSYS-family bash
# (Git Bash / MSYS2) and invoke the script through it. Anything else — a real
# `.exe`, `cargo`, `taskkill` — passes through untouched.
_MSYS_BASH_CANDIDATES = (
    r"C:\Program Files\Git\bin\bash.exe",
    r"C:\Program Files\Git\usr\bin\bash.exe",
    r"C:\Program Files (x86)\Git\bin\bash.exe",
    r"C:\msys64\usr\bin\bash.exe",
    r"C:\cygwin64\bin\bash.exe",
)

_SHELL_NAMES = {"bash", "sh", "bash.exe", "sh.exe"}

# Shebang interpreters we are willing to run under bash. Deliberately only the
# POSIX-shell family: a `#!` naming python/perl/node is a script we must NOT
# interpose a shell on.
_SHELL_INTERPRETERS = {"sh", "bash", "dash", "ksh", "zsh", "ash"}


def _is_wsl_launcher(path: str) -> bool:
    """Is this the WSL shim in System32 rather than a real Unix shell?

    Matched by location, not by name: System32/SysWOW64/Sysnative hold the
    `bash.exe` and `wsl.exe` stubs that hand off to a Linux distribution.
    """
    try:
        parent = Path(path).resolve().parent.name.lower()
    except OSError:
        return False
    return parent in {"system32", "syswow64", "sysnative"}


def find_unix_shell() -> str | None:
    """Absolute path to an MSYS-family bash, or None if none is installed.

    `SLATE_BASH` overrides the search, for a host whose shell lives somewhere
    unusual. The PATH lookup is tried first so a deliberately-installed shell
    wins over a stock one, but its answer is rejected if it is the WSL shim.
    """
    override = os.environ.get("SLATE_BASH")
    if override and Path(override).is_file():
        return override
    found = shutil.which("bash")
    if found and not _is_wsl_launcher(found):
        return found
    for cand in _MSYS_BASH_CANDIDATES:
        if Path(cand).is_file():
            return cand
    return None


def _is_shell_script(path: str) -> bool:
    """Does this name a shell script (by extension, or by `#!` line)?"""
    if path.lower().endswith((".sh", ".bash")):
        return True
    # Extensionless scripts are common too; ask the file itself.
    try:
        with open(path, "rb") as fh:
            first = fh.read(256)
    except OSError:
        return False
    if not first.startswith(b"#!"):
        return False
    line = first.split(b"\n", 1)[0][2:].strip()
    try:
        tokens = line.decode("utf-8").split()
    except UnicodeDecodeError:
        return False
    if not tokens:
        return False
    # `#!/usr/bin/env bash` — the interpreter is the argument, not `env`.
    # Resolve that one level so the basename we test is the real interpreter.
    interp = tokens[0]
    if PurePosixPath(interp).name == "env" and len(tokens) > 1:
        interp = tokens[1]
    # Match on the interpreter's *basename*, not a substring of the whole
    # line: `#!/home/shared/bin/python` contains "sh" but is not a shell
    # script, and running it under bash would be a confusing failure.
    return PurePosixPath(interp).name in _SHELL_INTERPRETERS


def resolve_command(command):
    """Rewrite `command` so a shell script actually runs, under a real bash.

    Returns the command unchanged on POSIX, when it is not a list, or when
    argv[0] is neither a shell script nor a bare `bash`/`sh`. Raises `OSError`
    when a shell is genuinely required but none can be found — silently falling
    back to WSL, or to the un-launchable script, is precisely the failure mode
    this exists to prevent, so it must be loud.
    """
    if not IS_WINDOWS or not isinstance(command, (list, tuple)) or not command:
        return command
    command = list(command)
    argv0 = str(command[0])

    # `bash script.sh` / `sh -c ...`: keep the arguments, fix the interpreter.
    if Path(argv0).name.lower() in _SHELL_NAMES and os.sep not in argv0 and "/" not in argv0:
        shell = find_unix_shell()
        if shell is None:
            raise OSError(
                f"cannot run {argv0!r}: no MSYS/Git Bash found (only the WSL "
                f"shim in System32, which cannot run this project's scripts). "
                f"Install Git for Windows or set SLATE_BASH."
            )
        command[0] = shell
        return command

    # A script path: `CreateProcess` cannot start it, so interpose bash.
    if _is_shell_script(argv0):
        shell = find_unix_shell()
        if shell is None:
            raise OSError(
                f"cannot run shell script {argv0!r}: no MSYS/Git Bash found. "
                f"Install Git for Windows or set SLATE_BASH."
            )
        return [shell, argv0, *command[1:]]

    return command


class Tree:
    """A child process and every descendant it spawns, killable as a unit.

    Used as a context manager: leaving the block closes the job handle, which on
    Windows is itself the kill — so an exception on the way out cannot leave a
    tree behind.

    `warn` is called with a one-line message when the containment could not be
    set up; the caller decides how loudly to say so. Containment failing is not
    fatal — [`terminate_tree`] still falls back to `taskkill /T`.
    """

    def __init__(self, command, warn=None, **kwargs):
        # Before anything else: make the command launchable at all. See
        # `resolve_command` — on Windows a `.sh` cannot be started directly and
        # a bare `bash` silently resolves to the WSL shim.
        command = resolve_command(command)
        self.job = create_kill_on_close_job()
        if IS_WINDOWS and self.job is None and warn:
            warn("could not create Job Object; relying on taskkill fallback")
        try:
            self.proc = subprocess.Popen(command, **popen_kwargs(), **kwargs)
        except BaseException:
            close_job(self.job)
            raise
        if self.job is not None and not assign_to_job(self.job, self.proc):
            if warn:
                warn("AssignProcessToJobObject failed; relying on taskkill fallback")

    def kill(self) -> None:
        """Terminate the whole tree. Safe to call more than once."""
        terminate_tree(self.job, self.proc)
        self.job = None

    def __enter__(self) -> "Tree":
        return self

    def __exit__(self, *_exc) -> None:
        close_job(self.job)
        self.job = None


@dataclass
class Captured:
    """The result of [`run_captured`].

    On a timeout the tree is already dead and `stdout`/`stderr` hold whatever it
    had written by then — partial output, which is why `timed_out` is reported
    separately rather than being inferred from an empty capture. `returncode` is
    `None` in that case: the child was killed, so it never chose a status.
    """

    stdout: bytes
    stderr: bytes
    returncode: int | None
    timed_out: bool


def run_captured(command, *, timeout=None, input=b"", **kwargs) -> Captured:
    """`subprocess.run(capture_output=True, timeout=…)`, but the timeout is real.

    On expiry the *whole tree* is killed before the pipes are drained, so a
    grandchild holding the write end cannot wedge the drain (see the module
    docstring). Extra keyword arguments go to `Popen` — `cwd`, `env`, and so on.
    """
    with Tree(
        command,
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        **kwargs,
    ) as tree:
        try:
            out, err = tree.proc.communicate(input=input, timeout=timeout)
            return Captured(out, err, tree.proc.returncode, timed_out=False)
        except subprocess.TimeoutExpired:
            tree.kill()
            try:
                # Now that every writer is dead the pipes report EOF, so this
                # returns. The grace is a guard, not a budget.
                out, err = tree.proc.communicate(timeout=DRAIN_GRACE)
            except subprocess.TimeoutExpired:
                out, err = b"", b""
            return Captured(out, err, None, timed_out=True)
        except BaseException:
            # Ctrl-C, or anything else on the way out: do not leave a tree.
            tree.kill()
            raise
