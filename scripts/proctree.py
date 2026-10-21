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

## What starts outside the job, and how it is brought back

The root is created suspended and joins the job before its first instruction
runs, so nothing it starts is born ahead of the job. After that a child inherits
its parent's job, with one exception that matters on this host: a Store-packaged
program reached through an App Execution Alias (the `python.exe` and
`python3.exe` in `%LOCALAPPDATA%\\Microsoft\\WindowsApps`) is created by a
system broker on its caller's behalf, outside the caller's job, with the caller
recorded as its parent. Measured 2026-10-21 with the Python install manager's
alias run by a member of a tree: the launcher, the runtime it starts and the
runtime's own children were all outside the tree's job, and all outlived the job
being closed. (As a tree's root, the same launcher joins the job like any child.)

So a `Tree` watches for such escapes (`_Escapes`). Every `ESCAPE_POLL` seconds,
and again when the tree is closed or killed, it lists every process on the
machine and adopts each one whose parent is in the tree but which is itself in
none of the tree's jobs. Each goes into a fresh kill-on-close job of its own,
where its later children are then born, so it dies with the tree: on a timeout,
at the end of the `with` block, and when this process dies. One job for all of
them will not do: Windows puts a process that is already in a job only into an
empty job, and refused a shared one to the runtime, which sits in its
launcher's job (error 5). `Tree.adopted` names them, and `Tree.accounting`
counts their cost in full: a job charges a process it takes in with what that
process had already spent (2 s of CPU burned before adoption was all counted,
measured 2026-10-21).

Nothing is adopted without proof that the tree started it, since adopting a
stranger would kill it with the tree. The proof is a handle to each parent,
taken when the job's completion port says the parent joined and kept until a
sweep after it exits: while the handle is open the parent's process id cannot be
reused, so a process that names that id as its parent and started after the
parent did is its child.

Still not contained: an escape for the moments before the sweep that adopts it,
so this process's own death in that window would leave it running; an escape
whose parent left the tree before the port's message about it was heard; a
process Windows refuses a job (named through `warn`, and killed with the tree
by handle); and anything across the WSL boundary (see `run-timeout.py`).
"""

from __future__ import annotations

import os
import shutil
import subprocess
import sys
import threading
import time
from dataclasses import dataclass
from pathlib import Path, PurePosixPath

IS_WINDOWS = sys.platform.startswith("win")


@dataclass
class Accounting:
    """What a tree's processes cost, over its whole life so far: CPU seconds
    in user and kernel mode, and how many processes it ran -- the exited
    ones counted in full. See `job_accounting`."""

    user_s: float
    kernel_s: float
    processes: int

    @property
    def cpu_s(self) -> float:
        return self.user_s + self.kernel_s

    def __add__(self, other: "Accounting") -> "Accounting":
        return Accounting(self.user_s + other.user_s, self.kernel_s + other.kernel_s,
                          self.processes + other.processes)

    def summary(self) -> str:
        """One phrase for a log line, in hours once that reads better."""
        def span(sec: float) -> str:
            # One decimal below ten seconds: a short run that did real work
            # must not read "0 s", which says it cost nothing.
            if sec >= 3600:
                return f"{sec / 3600:.2f} h"
            return f"{sec:.0f} s" if sec >= 10 else f"{sec:.1f} s"
        return (f"CPU {span(self.cpu_s)} (user {span(self.user_s)}, kernel "
                f"{span(self.kernel_s)}) across {self.processes} process(es)")


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

# How often a tree's watch looks for processes started outside its job (see the
# module docstring). Each look lists every process on the machine, about 9 ms
# with 425 processes, nearly all of it in the kernel (measured 2026-10-21): 2% of
# one core for each live tree, for adopting an escape within half a second. A
# tree also looks once as it closes, which is the 9 ms a short `run_captured`
# now costs over what it did.
ESCAPE_POLL = 0.5

# The most sweeps a closing tree makes, each after killing what the last one
# adopted, to catch what those started on their way out.
REAP_ROUNDS = 5


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

    JobObjectBasicAccountingInformation = 1

    class _JOBOBJECT_BASIC_ACCOUNTING_INFORMATION(ctypes.Structure):
        _fields_ = [
            ("TotalUserTime", wintypes.LARGE_INTEGER),
            ("TotalKernelTime", wintypes.LARGE_INTEGER),
            ("ThisPeriodTotalUserTime", wintypes.LARGE_INTEGER),
            ("ThisPeriodTotalKernelTime", wintypes.LARGE_INTEGER),
            ("TotalPageFaultCount", wintypes.DWORD),
            ("TotalProcesses", wintypes.DWORD),
            ("ActiveProcesses", wintypes.DWORD),
            ("TotalTerminatedProcesses", wintypes.DWORD),
        ]

    def job_accounting(job) -> Accounting | None:
        """What the job's processes have cost so far, exited ones included --
        which is the point: a boot test's CPU is mostly thousands of processes
        that live for a fraction of a second, and no sampler taken from outside
        sees them, while the job has counted every one. None when there is no
        job or the query fails."""
        if not job:
            return None
        info = _JOBOBJECT_BASIC_ACCOUNTING_INFORMATION()
        if not _k32.QueryInformationJobObject(
            job,
            JobObjectBasicAccountingInformation,
            ctypes.byref(info),
            ctypes.sizeof(info),
            None,
        ):
            return None
        return Accounting(
            user_s=info.TotalUserTime / 1e7,
            kernel_s=info.TotalKernelTime / 1e7,
            processes=info.TotalProcesses,
        )

    def close_job(job) -> None:
        if job:
            _k32.CloseHandle(job)

    # --- What the tree starts outside its job: see the module docstring ---

    _ntdll = ctypes.WinDLL("ntdll")

    CREATE_SUSPENDED = 0x00000004
    PROCESS_TERMINATE = 0x0001
    PROCESS_SET_QUOTA = 0x0100
    PROCESS_QUERY_LIMITED_INFORMATION = 0x1000
    ERROR_INVALID_PARAMETER = 87  # what `OpenProcess` says of a process that is gone
    JobObjectAssociateCompletionPortInformation = 7
    JOB_OBJECT_MSG_NEW_PROCESS = 6
    JOB_OBJECT_MSG_EXIT_PROCESS = 7
    JOB_OBJECT_MSG_ABNORMAL_EXIT_PROCESS = 8
    SystemProcessInformation = 5
    _STATUS_INFO_LENGTH_MISMATCH = 0xC0000004
    _INVALID_HANDLE_VALUE = wintypes.HANDLE(-1)
    # The completion key `_Escapes.stop` posts. A job's key is its index in the
    # watch, so it never comes near this.
    _STOP_KEY = 0xFFFF_FFFF

    # Prototypes, so a 64-bit handle is never squeezed through a C `int`.
    _k32.OpenProcess.restype = wintypes.HANDLE
    _k32.OpenProcess.argtypes = (wintypes.DWORD, wintypes.BOOL, wintypes.DWORD)
    _k32.CreateIoCompletionPort.restype = wintypes.HANDLE
    _k32.CreateIoCompletionPort.argtypes = (wintypes.HANDLE, wintypes.HANDLE,
                                            ctypes.c_size_t, wintypes.DWORD)
    _k32.GetQueuedCompletionStatus.argtypes = (
        wintypes.HANDLE, ctypes.POINTER(wintypes.DWORD), ctypes.POINTER(ctypes.c_size_t),
        ctypes.POINTER(ctypes.c_void_p), wintypes.DWORD)
    _k32.PostQueuedCompletionStatus.argtypes = (wintypes.HANDLE, wintypes.DWORD,
                                                ctypes.c_size_t, ctypes.c_void_p)
    _k32.GetProcessTimes.argtypes = (wintypes.HANDLE,) + (ctypes.POINTER(wintypes.FILETIME),) * 4
    _k32.IsProcessInJob.argtypes = (wintypes.HANDLE, wintypes.HANDLE, ctypes.POINTER(wintypes.BOOL))
    _k32.AssignProcessToJobObject.argtypes = (wintypes.HANDLE, wintypes.HANDLE)
    _k32.SetInformationJobObject.argtypes = (wintypes.HANDLE, ctypes.c_int, ctypes.c_void_p,
                                             wintypes.DWORD)
    _k32.QueryInformationJobObject.argtypes = (wintypes.HANDLE, ctypes.c_int, ctypes.c_void_p,
                                               wintypes.DWORD, ctypes.POINTER(wintypes.DWORD))
    _k32.TerminateJobObject.argtypes = (wintypes.HANDLE, wintypes.UINT)
    _k32.TerminateProcess.argtypes = (wintypes.HANDLE, wintypes.UINT)
    _k32.CloseHandle.argtypes = (wintypes.HANDLE,)
    _ntdll.NtQuerySystemInformation.restype = ctypes.c_long
    _ntdll.NtQuerySystemInformation.argtypes = (ctypes.c_int, ctypes.c_void_p, wintypes.ULONG,
                                                ctypes.POINTER(wintypes.ULONG))
    _ntdll.NtResumeProcess.restype = ctypes.c_long
    _ntdll.NtResumeProcess.argtypes = (wintypes.HANDLE,)

    class _UNICODE_STRING(ctypes.Structure):
        _fields_ = [("Length", wintypes.USHORT), ("MaximumLength", wintypes.USHORT),
                    ("Buffer", ctypes.c_void_p)]

    class _SYSTEM_PROCESS_INFORMATION(ctypes.Structure):
        # The head of one record, as far as the parent's id. The rest of the
        # record, and the thread records after it, are skipped.
        _fields_ = [
            ("NextEntryOffset", wintypes.ULONG),
            ("NumberOfThreads", wintypes.ULONG),
            ("WorkingSetPrivateSize", ctypes.c_longlong),
            ("HardFaultCount", wintypes.ULONG),
            ("NumberOfThreadsHighWatermark", wintypes.ULONG),
            ("CycleTime", ctypes.c_ulonglong),
            ("CreateTime", ctypes.c_longlong),
            ("UserTime", ctypes.c_longlong),
            ("KernelTime", ctypes.c_longlong),
            ("ImageName", _UNICODE_STRING),
            ("BasePriority", ctypes.c_long),
            ("UniqueProcessId", ctypes.c_void_p),
            ("InheritedFromUniqueProcessId", ctypes.c_void_p),
        ]

    class _JOBOBJECT_ASSOCIATE_COMPLETION_PORT(ctypes.Structure):
        _fields_ = [("CompletionKey", ctypes.c_void_p), ("CompletionPort", wintypes.HANDLE)]

    # The buffer `process_snapshot` starts with; it grows to fit and stays grown.
    _snapshot_bytes = 1 << 20

    def process_snapshot() -> list[tuple[int, int, int]]:
        """`(pid, parent's pid, creation time)` for every process on the
        machine, read in one system call. The creation time, in the units of
        `GetProcessTimes`, is what tells a parent from a later process that has
        been given its id.

        Toolhelp's snapshot holds the same, but walking it costs a ctypes call
        per process: 13 ms for 425 processes against 9 ms for this, measured
        2026-10-21. Raises `OSError` when the system will not say."""
        global _snapshot_bytes
        for _ in range(8):
            buf = ctypes.create_string_buffer(_snapshot_bytes)
            needed = wintypes.ULONG()
            status = _ntdll.NtQuerySystemInformation(
                SystemProcessInformation, buf, _snapshot_bytes, ctypes.byref(needed)) & 0xFFFF_FFFF
            if status != _STATUS_INFO_LENGTH_MISMATCH:
                break
            # Processes start between the two calls: leave room for them.
            _snapshot_bytes = max(_snapshot_bytes * 2, needed.value + (1 << 16))
        if status:
            raise OSError(f"NtQuerySystemInformation failed: NTSTATUS {status:#010x}")
        out, offset = [], 0
        while True:
            rec = _SYSTEM_PROCESS_INFORMATION.from_buffer(buf, offset)
            out.append((rec.UniqueProcessId or 0, rec.InheritedFromUniqueProcessId or 0,
                        rec.CreateTime))
            if not rec.NextEntryOffset:
                return out
            offset += rec.NextEntryOffset

    def _creation_time(handle) -> int | None:
        """When the process behind `handle` was created, in `process_snapshot`'s
        units; None if Windows will not say."""
        created, exited, kernel, user = (wintypes.FILETIME() for _ in range(4))
        if not _k32.GetProcessTimes(handle, ctypes.byref(created), ctypes.byref(exited),
                                    ctypes.byref(kernel), ctypes.byref(user)):
            return None
        return (created.dwHighDateTime << 32) | created.dwLowDateTime

    def _in_job(handle, job) -> bool:
        member = wintypes.BOOL()
        return bool(_k32.IsProcessInJob(handle, job, ctypes.byref(member))) and bool(member.value)

    def resume_process(proc) -> bool:
        """Let a child created with `CREATE_SUSPENDED` run. `Popen` keeps no
        handle to the child's first thread, so this resumes the whole process,
        whose one thread is the one held."""
        return _ntdll.NtResumeProcess(int(proc._handle)) == 0

    class _Escapes:
        """A tree's watch for the processes it starts outside its job. The
        module docstring says why there are any, and what proves one is the
        tree's.

        `start` it once the root is in the job; `stop` it before the tree's
        own job is closed, which ends the periodic sweeps and hears what the
        port still holds; and `reap` after that, which adopts and kills what
        is left and lets go of every handle. Until `stop` only the watch's own
        thread touches its state, except that others read `jobs` and
        `adopted`, which it only ever appends to."""

        def __init__(self, tree_job, warn):
            self._tree_job = tree_job
            self._warn = warn or (lambda _msg: None)
            self.jobs: list = []           # one per adopted process, owned here
            self.adopted: list[int] = []   # those processes' ids, in order
            # Every process a proof can name as parent: pid -> (handle, creation time).
            self._held: dict[int, tuple[int, int]] = {}
            self._unadoptable: list[int] = []   # held escapes Windows refused a job
            # Held pid -> how many sweeps had begun when its exit was heard.
            self._exited: dict[int, int] = {}
            # (pid, creation time) of processes looked at and left alone.
            self._passed: set[tuple[int, int]] = set()
            self._sweeps = 0
            self._said_no_listing = False
            self._thread = None
            self._port = _k32.CreateIoCompletionPort(_INVALID_HANDLE_VALUE, None, 0, 1)
            if not self._port:
                raise OSError(ctypes.get_last_error(), "CreateIoCompletionPort failed")
            if not self._listen(tree_job, 0):
                err = ctypes.get_last_error()
                _k32.CloseHandle(self._port)
                raise OSError(err, "the job will not report to a completion port")

        def _listen(self, job, key) -> bool:
            """Have `job` tell the port, under `key`, of each process that
            joins or leaves it."""
            info = _JOBOBJECT_ASSOCIATE_COMPLETION_PORT(key, self._port)
            return bool(_k32.SetInformationJobObject(
                job, JobObjectAssociateCompletionPortInformation,
                ctypes.byref(info), ctypes.sizeof(info)))

        def start(self, root_pid) -> None:
            """Hold the root, while it is still suspended and so still surely
            the tree's, and begin the periodic sweeps."""
            h = _k32.OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, False, root_pid)
            created = _creation_time(h) if h else None
            if created is not None:
                self._held[root_pid] = (h, created)
            elif h:
                _k32.CloseHandle(h)
            self._thread = threading.Thread(target=self._run, name="proctree-escapes",
                                            daemon=True)
            self._thread.start()

        def _run(self) -> None:
            try:
                due = time.monotonic() + ESCAPE_POLL
                while True:
                    left = due - time.monotonic()
                    if left > 0:
                        got = self._next(int(left * 1000) + 1)
                        if got is not None:
                            if got[0] == _STOP_KEY:
                                return
                            self._hear(*got)
                        continue
                    self._sweep()
                    self._release()
                    due = time.monotonic() + ESCAPE_POLL
            except Exception as e:  # noqa: BLE001 -- said, since a silent end leaves escapes unwatched
                self._warn(f"the watch for processes started outside the job stopped: {e!r}")

        def _next(self, timeout_ms):
            """The port's next message as `(key, message, pid)`, or None when
            `timeout_ms` passes without one."""
            msg, key, pid = wintypes.DWORD(), ctypes.c_size_t(), ctypes.c_void_p()
            if not _k32.GetQueuedCompletionStatus(self._port, ctypes.byref(msg), ctypes.byref(key),
                                                  ctypes.byref(pid), timeout_ms):
                return None  # the port carries no I/O, so a failure is the timeout
            return key.value, msg.value, pid.value or 0

        def _drain(self) -> None:
            while (got := self._next(0)) is not None:
                if got[0] != _STOP_KEY:
                    self._hear(*got)

        def _job_for(self, key):
            if key == 0:
                return self._tree_job
            return self.jobs[key - 1] if 0 < key <= len(self.jobs) else None

        def _jobs_now(self) -> list:
            return ([self._tree_job] if self._tree_job else []) + self.jobs

        def _hear(self, key, msg, pid) -> None:
            """Hold a process the port says joined one of the jobs, and note
            when a held one leaves."""
            if msg == JOB_OBJECT_MSG_NEW_PROCESS:
                job = self._job_for(key)
                if job is None or pid in self._held:
                    return
                h = _k32.OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, False, pid)
                if not h:
                    return  # gone already: the gap the module docstring names
                created = _creation_time(h)
                # The id can have passed to another process since the message.
                if created is None or not _in_job(h, job):
                    _k32.CloseHandle(h)
                    return
                self._held[pid] = (h, created)
            elif msg in (JOB_OBJECT_MSG_EXIT_PROCESS, JOB_OBJECT_MSG_ABNORMAL_EXIT_PROCESS):
                if pid in self._held:
                    self._exited.setdefault(pid, self._sweeps)

        def _sweep(self) -> int:
            """Hold or adopt every process the listing shows a held process
            started; how many of them were escapes."""
            self._sweeps += 1
            try:
                listing = process_snapshot()
            except OSError as e:
                if not self._said_no_listing:
                    self._said_no_listing = True
                    self._warn(f"cannot list processes, so cannot find those started "
                               f"outside the job: {e}")
                return 0
            found, grew = 0, True
            # Round again after any change: one just held can be the parent of
            # another in the same listing.
            while grew:
                grew = False
                for pid, ppid, created in listing:
                    parent = self._held.get(ppid)
                    # Created before the held parent, it is the child of an
                    # earlier process that had the same id.
                    if (parent is None or pid in self._held or created < parent[1]
                            or (pid, created) in self._passed):
                        continue
                    grew = True
                    found += self._take(pid, created)
            return found

        def _take(self, pid, created) -> int:
            """Hold `pid`, which the listing shows a held process started at
            `created`, and adopt it if it is in none of the tree's jobs. 1 if
            it was such an escape, else 0. Either holds it or passes it, so a
            sweep never looks at it twice."""
            h = _k32.OpenProcess(PROCESS_SET_QUOTA | PROCESS_TERMINATE
                                 | PROCESS_QUERY_LIMITED_INFORMATION, False, pid)
            if not h:
                err = ctypes.get_last_error()
                self._passed.add((pid, created))
                if err != ERROR_INVALID_PARAMETER:
                    self._warn(f"process {pid}, started by the tree, cannot be opened "
                               f"(error {err}): if it is outside the job it can outlive "
                               f"the tree")
                return 0
            if _creation_time(h) != created:
                # It exited, and its id passed to an unrelated process, after the listing.
                _k32.CloseHandle(h)
                self._passed.add((pid, created))
                return 0
            self._held[pid] = (h, created)
            if any(_in_job(h, job) for job in self._jobs_now()):
                return 0  # a member whose message is still on its way
            job = create_kill_on_close_job()
            if job and self._listen(job, len(self.jobs) + 1) and _k32.AssignProcessToJobObject(job, h):
                self.jobs.append(job)
                self.adopted.append(pid)
                return 1
            err = ctypes.get_last_error()
            close_job(job)
            self._unadoptable.append(pid)
            self._warn(f"process {pid} was started by the tree outside its job, and Windows "
                       f"refuses it a job of its own (error {err}): it is killed with the "
                       f"tree, but would outlive this process's own death")
            return 1

        def _release(self) -> None:
            """Let go of the held processes that exited before the sweep just
            made began: it saw every child they left."""
            for pid in [p for p, seen in self._exited.items() if seen < self._sweeps]:
                del self._exited[pid]
                h, _ = self._held.pop(pid)
                _k32.CloseHandle(h)

        def stop(self) -> None:
            """End the periodic sweeps and hear what the port still holds. Call
            while the tree's own job is still open: its messages are checked
            against it."""
            if self._thread is not None:
                _k32.PostQueuedCompletionStatus(self._port, 0, _STOP_KEY, None)
                self._thread.join()
                self._thread = None
            self._drain()
            self._tree_job = None

        def reap(self) -> None:
            """Adopt what the tree's last moments started outside its job, kill
            everything in the jobs and handles held here, and let go of them.
            Call after `stop`, once the tree's own job is closed."""
            for _ in range(REAP_ROUNDS):
                found = self._sweep()
                for job in self.jobs:
                    _k32.TerminateJobObject(job, 1)
                for pid in self._unadoptable:
                    _k32.TerminateProcess(self._held[pid][0], 1)
                if not found:
                    break
                self._drain()  # those just killed may have started members of their jobs
            for job in self.jobs:
                close_job(job)
            self.jobs = []
            for h, _ in self._held.values():
                _k32.CloseHandle(h)
            self._held.clear()
            self._exited.clear()
            self._unadoptable.clear()
            _k32.CloseHandle(self._port)
            self._port = None

    def terminate_tree(job, proc) -> None:
        # `taskkill /T` first, *then* the job: order matters. `/T` walks the
        # tree by parentage, so it reaches what is outside the job -- an escape
        # not adopted yet (see the module docstring), or the whole tree when no
        # job could be made -- and it needs the parents alive to do it. Closing
        # the job first kills them and leaves `/T` nothing to enumerate.
        # Closing the job afterwards costs nothing when the tree is already
        # dead.
        #
        # The call is bounded, and its own tree is torn down on expiry — see
        # `TASKKILL_GRACE`. A hang here used to take the caller with it, and the
        # caller is the one holding the job open, which is how a runaway case
        # escaped the containment meant to reap it.
        try:
            with Tree(
                ["taskkill", "/F", "/T", "/PID", str(proc.pid)],
                adopt_escapes=False,  # taskkill starts nothing
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
    import signal

    def create_kill_on_close_job():
        return None  # POSIX uses process groups instead

    def assign_to_job(job, proc) -> bool:
        return True  # child already leads its own group (start_new_session)

    def job_accounting(job) -> Accounting | None:
        # A process group keeps no totals. `getrusage(RUSAGE_CHILDREN)` would
        # count only descendants somebody waited for, and this runner's other
        # children too -- a number that looks like the tree's and is not.
        return None

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

    On Windows the tree includes what it starts outside its Job Object, which
    a watch adopts into jobs of its own (see the module docstring and
    `adopted`). `adopt_escapes=False` leaves that watch out, for a command
    known to start nothing.

    `warn` is called with a one-line message when the containment could not be
    set up, or could not take in a process; the caller decides how loudly to
    say so. Containment failing is not fatal — [`terminate_tree`] still falls
    back to `taskkill /T`.
    """

    def __init__(self, command, warn=None, adopt_escapes=True, **kwargs):
        # Before anything else: make the command launchable at all. See
        # `resolve_command` — on Windows a `.sh` cannot be started directly and
        # a bare `bash` silently resolves to the WSL shim.
        command = resolve_command(command)
        self.job = create_kill_on_close_job()
        if IS_WINDOWS and self.job is None and warn:
            warn("could not create Job Object; relying on taskkill fallback")
        self._escapes = None
        if self.job is not None and adopt_escapes:
            try:
                self._escapes = _Escapes(self.job, warn)
            except OSError as e:
                if warn:
                    warn(f"cannot watch for processes started outside the job ({e}); "
                         f"they would outlive it")
        self._adopted = self._escapes.adopted if self._escapes else []
        # Held until it is in the job, so nothing it starts is born outside it.
        if self.job is not None:
            kwargs["creationflags"] = kwargs.get("creationflags", 0) | CREATE_SUSPENDED
        try:
            self.proc = subprocess.Popen(command, **popen_kwargs(), **kwargs)
        except BaseException:
            self._close()
            raise
        if self.job is not None:
            if not assign_to_job(self.job, self.proc) and warn:
                warn("AssignProcessToJobObject failed; relying on taskkill fallback")
            if not resume_process(self.proc):
                self.proc.kill()
                self._close()
                raise OSError(f"could not start {command!r}: NtResumeProcess failed")
            if self._escapes:
                self._escapes.start(self.proc.pid)

    @property
    def adopted(self) -> list[int]:
        """The ids of the processes the tree started outside its job, and that
        were adopted into jobs of their own, in the order they were found.
        Empty on POSIX, and where no job could be made."""
        return list(self._adopted)

    def kill(self) -> None:
        """Terminate the whole tree, adopted processes included. Safe to call
        more than once."""
        escapes, self._escapes = self._escapes, None
        if escapes:
            escapes.stop()
        terminate_tree(self.job, self.proc)
        self.job = None
        if escapes:
            escapes.reap()

    def accounting(self) -> Accounting | None:
        """What the tree has cost so far (see `job_accounting`), adopted
        processes included (module docstring); None once the job is closed
        -- after `kill` or the end of the `with` block -- or where there is no
        job to ask (POSIX, or a job that could not be created)."""
        acct = job_accounting(self.job)
        if acct is not None and self._escapes is not None:
            for job in list(self._escapes.jobs):
                more = job_accounting(job)
                if more is not None:
                    acct = acct + more
        return acct

    def _close(self) -> None:
        """Close the job, which kills what is left in it, and then what is left
        of the adopted processes."""
        escapes, self._escapes = self._escapes, None
        if escapes:
            escapes.stop()
        close_job(self.job)
        self.job = None
        if escapes:
            escapes.reap()

    def __enter__(self) -> "Tree":
        return self

    def __exit__(self, *_exc) -> None:
        self._close()


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
