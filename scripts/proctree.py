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

import subprocess
import sys
from dataclasses import dataclass

IS_WINDOWS = sys.platform.startswith("win")

# How long to wait for the capture threads once the tree is dead. Their pipes'
# write ends are all closed by then, so this is a guard against the impossible
# rather than a real budget.
DRAIN_GRACE = 10.0


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
        # Closing the kill-on-close job handle terminates the whole tree.
        close_job(job)
        # Belt-and-suspenders: also taskkill the tree in case assignment
        # raced and missed an early grandchild.
        try:
            subprocess.run(
                ["taskkill", "/F", "/T", "/PID", str(proc.pid)],
                stdout=subprocess.DEVNULL,
                stderr=subprocess.DEVNULL,
                check=False,
            )
        except OSError:
            pass

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
