#!/usr/bin/env python3
"""run-timeout.py — run a command with a hard timeout and guaranteed
process-tree cleanup.

The motivating problem: wrapping `cargo test` in coreutils `timeout` only
kills `cargo` itself; the spawned test binaries survive as orphans and can
linger for hours (a deadlocked test never exits on its own). This runner
fixes that on Windows by assigning the child — and every descendant it
spawns — to a Job Object created with JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE.
Killing the job (on timeout, on Ctrl-C, or when this runner itself dies)
tears down the entire tree atomically, so nothing is ever orphaned. On
POSIX it uses a process group + SIGKILL for the same effect.

While the child runs, the runner prints a heartbeat line every
`--poll` seconds ("[run-timeout] still running, Ns elapsed") so a long
build never looks like a silent hang, and it reports a clear final status.

Usage:
    python run-timeout.py [--poll SECS] <timeout_secs> <command> [args...]

Exit codes:
    <child code>  child completed on its own (its exit code is passed through)
    124           timed out — the whole process tree was killed
    125           failed to launch the child
    130           interrupted (Ctrl-C) — tree killed
"""

import subprocess
import sys
import threading
import time

POLL_DEFAULT = 15.0
EXIT_TIMEOUT = 124
EXIT_LAUNCH = 125
EXIT_INTERRUPT = 130

IS_WINDOWS = sys.platform.startswith("win")


def _log(msg: str) -> None:
    print(f"[run-timeout] {msg}", flush=True)


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

    def _create_kill_on_close_job():
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

    def _assign_to_job(job, proc) -> bool:
        # proc._handle is the process HANDLE on Windows.
        return bool(_k32.AssignProcessToJobObject(job, int(proc._handle)))

    def _terminate_tree(job, proc) -> None:
        # Closing the kill-on-close job handle terminates the whole tree.
        if job:
            _k32.CloseHandle(job)
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

else:
    import os
    import signal

    def _create_kill_on_close_job():
        return None  # POSIX uses process groups instead

    def _assign_to_job(job, proc) -> bool:
        return True  # child already leads its own group (start_new_session)

    def _terminate_tree(job, proc) -> None:
        try:
            os.killpg(proc.pid, signal.SIGKILL)
        except (ProcessLookupError, PermissionError):
            pass


def main(argv: list[str]) -> int:
    poll = POLL_DEFAULT
    args = argv[1:]
    if args and args[0] == "--poll":
        if len(args) < 2:
            _log("--poll requires a value")
            return EXIT_LAUNCH
        try:
            poll = float(args[1])
        except ValueError:
            _log(f"invalid --poll value: {args[1]!r}")
            return EXIT_LAUNCH
        args = args[2:]

    if len(args) < 2:
        _log("usage: run-timeout.py [--poll SECS] <timeout_secs> <command> [args...]")
        return EXIT_LAUNCH

    try:
        timeout = float(args[0])
    except ValueError:
        _log(f"invalid timeout: {args[0]!r}")
        return EXIT_LAUNCH
    command = args[1:]

    job = _create_kill_on_close_job()
    if IS_WINDOWS and job is None:
        _log("warning: could not create Job Object; relying on taskkill fallback")

    popen_kwargs = {}
    if not IS_WINDOWS:
        popen_kwargs["start_new_session"] = True  # child leads its own process group

    _log(f"launching (timeout {timeout:g}s): {' '.join(command)}")
    try:
        proc = subprocess.Popen(command, **popen_kwargs)
    except OSError as e:
        _log(f"failed to launch: {e}")
        if job:
            _k32.CloseHandle(job)
        return EXIT_LAUNCH

    if job is not None and not _assign_to_job(job, proc):
        _log("warning: AssignProcessToJobObject failed; relying on taskkill fallback")

    start = time.monotonic()
    deadline = start + timeout
    stop_heartbeat = threading.Event()

    def _heartbeat() -> None:
        while not stop_heartbeat.wait(poll):
            elapsed = time.monotonic() - start
            if elapsed >= timeout:
                return
            _log(f"still running, {elapsed:.0f}s elapsed")

    hb = threading.Thread(target=_heartbeat, daemon=True)
    hb.start()

    try:
        while True:
            remaining = deadline - time.monotonic()
            if remaining <= 0:
                stop_heartbeat.set()
                _log(f"TIMEOUT after {timeout:g}s -- killing process tree")
                _terminate_tree(job, proc)
                try:
                    proc.wait(timeout=10)
                except subprocess.TimeoutExpired:
                    _log("warning: tree did not exit within 10s of kill")
                return EXIT_TIMEOUT
            try:
                code = proc.wait(timeout=min(remaining, 1.0))
            except subprocess.TimeoutExpired:
                continue
            stop_heartbeat.set()
            elapsed = time.monotonic() - start
            status = "PASS" if code == 0 else f"FAIL (exit {code})"
            _log(f"child exited: {status}, {elapsed:.0f}s elapsed")
            if job:
                _k32.CloseHandle(job)
            return code
    except KeyboardInterrupt:
        stop_heartbeat.set()
        _log("interrupted -- killing process tree")
        _terminate_tree(job, proc)
        return EXIT_INTERRUPT


if __name__ == "__main__":
    sys.exit(main(sys.argv))
