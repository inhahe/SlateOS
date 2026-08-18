#!/usr/bin/env python3
"""Report which processes are keeping a directory un-renamable, without admin.

Written for `known-issues.md` ->
`A-RECLAIM-SPACE-CANNOT-FREE-A-LANE'S-OWN-TARGET`, where
`target/x86_64-pc-windows-gnu` could not be renamed and the holder could not be
attributed with the tools that were reachable:

* `Get-Process | Where-Object Path -like ...` matches only the process **image**,
  so it misses a loaded **DLL** entirely, and it silently reports nothing for
  processes it cannot open.
* `handle.exe` is v3.2 on this machine and refuses to enumerate without
  administrator rights, which an agent session does not have.

Four things make Windows refuse to rename a directory, and this script looks
for all four, because each produces the *same* `Access is denied` and picking
one to test is how an investigation gets closed on the wrong answer:

1. **An ordinary open file** inside it.  The commonest case by far, and the one
   the other three checks all miss - an open handle is not an image path, not a
   loaded module and not a cwd.  Verified the hard way: a two-line script
   holding one file open vetoed the rename while a three-cause scan reported
   "no holder visible".
2. **A running executable** whose image is inside it (the image file is mapped
   and cannot be renamed, which blocks every ancestor).
3. **A loaded DLL** inside it - same mechanism, invisible to image-path checks.
   Cargo's proc-macro output and test-harness DLLs live under `target/`.
4. **A process whose current working directory** is inside it.  A cwd is not an
   image path, so no `Get-Process` filter can see it.

Everything here works **without elevation**.  Images, modules and the cwd need
only `PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_VM_READ`; open handles need
`PROCESS_DUP_HANDLE` to put a name to a handle.  All three rights are granted
for processes running as the same user, which is what makes `handle.exe`'s
demand for administrator rights avoidable rather than merely inconvenient.

Processes it cannot open are reported as **unreadable rather than skipped
silently**, and the exit status distinguishes them: "nothing holds it" and "I
could not see what holds it" are different answers, and printing the first when
the second is true is exactly the false negative that left the original issue
unresolved.

Usage:
    python scripts/who-holds-dir.py                   # anything under the repo
    python scripts/who-holds-dir.py <dir> [<dir>...]  # under these directories
    python scripts/who-holds-dir.py --no-handles <d>  # skip the slow scan
    python scripts/who-holds-dir.py --no-modules <d>  # skip the module scan

A full run takes ~15s, nearly all of it naming the ~22k open file handles on
this machine one at a time.  The `--no-*` flags trade that away, and because a
check that did not run cannot have cleared anything, either flag forces the
verdict to "inconclusive" - they buy speed, never certainty.

Exit status:
    0  at least one holder found
    1  no holder, every process was readable, every check ran - really free
    2  inconclusive: no holder found, but something could not be inspected

`0` and `1` are answers; `2` is the absence of one.  Reporting "nothing holds
it" when the truth is "I could not see what holds it" is the specific false
negative that left the original issue unresolved, so the two are never merged.

On POSIX the same four questions are `/proc/<pid>/exe`, `/fd`, `/cwd` and
`/maps` - the same question with an easier answer.
"""

from __future__ import annotations

import os
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent

# A holder record: (pid, image, kind, path).  `kind` is one of "cwd", "image",
# "module" - kept distinct in the output because the remedy differs: a cwd is a
# shell someone left sitting there, a mapped module is a process that must exit.
Holder = tuple[int, str, str, str]


# ---------------------------------------------------------------------------
# POSIX
# ---------------------------------------------------------------------------


def _scan_posix(want_modules: bool):
    for entry in Path("/proc").iterdir():
        if not entry.name.isdigit():
            continue
        pid = int(entry.name)
        try:
            image = os.readlink(entry / "exe")
        except OSError:
            image = "?"
        paths: list[tuple[str, str]] = [("image", image)]
        try:
            paths.append(("cwd", os.readlink(entry / "cwd")))
        except OSError as exc:
            yield pid, image, None, f"cwd unreadable: {exc}"
            continue
        # Open file descriptors: the fourth blocker, and on Windows the one the
        # other three miss.  Here it is simply a directory of symlinks.
        try:
            for fd in (entry / "fd").iterdir():
                try:
                    paths.append(("handle", os.readlink(fd)))
                except OSError:
                    continue  # closed between listing and reading
        except OSError as exc:
            yield pid, image, paths, f"fd unreadable: {exc}"
            continue
        if want_modules:
            try:
                seen = set()
                for line in (entry / "maps").read_text(errors="replace").splitlines():
                    parts = line.split(None, 5)
                    if len(parts) == 6 and parts[5].startswith("/") and parts[5] not in seen:
                        seen.add(parts[5])
                        paths.append(("module", parts[5]))
            except OSError as exc:
                yield pid, image, paths, f"maps unreadable: {exc}"
                continue
        yield pid, image, paths, None


# ---------------------------------------------------------------------------
# Windows
# ---------------------------------------------------------------------------


def _windows_file_handles(timeout_s: float = 30.0):
    """Map pid -> set of file paths that process has **open**.

    This is the fourth blocker, and it is the one the other three miss.  A
    process with an ordinary open file inside a directory blocks renaming every
    ancestor of that file, and such a handle is not the process's image, not a
    loaded module, and not its cwd -- so a scan of those three reports the
    directory as unheld while the rename keeps failing.  That false negative
    was observed directly: a two-line Python script holding one file open
    vetoed the rename, and a three-cause scan printed "no holder visible".

    Getting this without administrator rights turns on one fact:
    `NtQuerySystemInformation(SystemExtendedHandleInformation)` lists every
    handle on the system for any caller, but a handle is only a number until it
    is given a name, and naming it means duplicating it into this process --
    which needs `PROCESS_DUP_HANDLE` on the owner.  That is granted for
    same-user processes.  So the *table* is complete and the *names* are
    partial, and which names are missing is reported rather than assumed empty.

    The file type index is discovered by opening a file here and finding this
    process's own handle in the table, instead of hardcoding an index that
    differs between Windows builds.

    Returns `(paths_by_pid, unnamed_pids, notes)`.
    """
    import ctypes
    import ctypes.wintypes as wt
    import msvcrt
    import threading
    import time

    ntdll = ctypes.WinDLL("ntdll")
    k32 = ctypes.WinDLL("kernel32", use_last_error=True)

    SystemExtendedHandleInformation = 64
    ObjectNameInformation = 1
    STATUS_INFO_LENGTH_MISMATCH = 0xC0000004
    PROCESS_DUP_HANDLE = 0x0040
    DUPLICATE_SAME_ACCESS = 0x0002

    # `GetFileType` distinguishes a file on a volume from a pipe or a console
    # without performing any I/O on the object, which is what makes it safe to
    # call on a handle that would wedge a name query.
    FILE_TYPE_DISK = 0x0001

    # A cheap pre-filter applied before the handle is even duplicated: a
    # synchronous handle with exactly this granted access is nearly always a
    # named pipe with a blocking read outstanding.  It is only an optimisation
    # now -- `GetFileType` below is what actually makes the scan wedge-proof --
    # and skips are counted and reported either way, because a silently
    # dropped handle is the exact failure this module exists to prevent.
    HANG_PRONE_ACCESS = 0x0012019F

    class UNICODE_STRING(ctypes.Structure):
        _fields_ = [
            ("Length", wt.USHORT),
            ("MaximumLength", wt.USHORT),
            ("Buffer", ctypes.c_void_p),
        ]

    class HANDLE_ENTRY(ctypes.Structure):
        _fields_ = [
            ("Object", ctypes.c_void_p),
            ("UniqueProcessId", ctypes.c_size_t),
            ("HandleValue", ctypes.c_size_t),
            ("GrantedAccess", ctypes.c_ulong),
            ("CreatorBackTraceIndex", ctypes.c_ushort),
            ("ObjectTypeIndex", ctypes.c_ushort),
            ("HandleAttributes", ctypes.c_ulong),
            ("Reserved", ctypes.c_ulong),
        ]

    ntdll.NtQuerySystemInformation.argtypes = [
        ctypes.c_int, ctypes.c_void_p, ctypes.c_ulong, ctypes.POINTER(ctypes.c_ulong)
    ]
    ntdll.NtQuerySystemInformation.restype = ctypes.c_long
    ntdll.NtQueryObject.argtypes = [
        wt.HANDLE, ctypes.c_int, ctypes.c_void_p, ctypes.c_ulong,
        ctypes.POINTER(ctypes.c_ulong),
    ]
    ntdll.NtQueryObject.restype = ctypes.c_long
    k32.DuplicateHandle.argtypes = [
        wt.HANDLE, wt.HANDLE, wt.HANDLE, ctypes.POINTER(wt.HANDLE),
        wt.DWORD, wt.BOOL, wt.DWORD,
    ]
    k32.DuplicateHandle.restype = wt.BOOL
    k32.QueryDosDeviceW.argtypes = [wt.LPCWSTR, wt.LPWSTR, wt.DWORD]
    k32.QueryDosDeviceW.restype = wt.DWORD
    k32.GetFileType.argtypes = [wt.HANDLE]
    k32.GetFileType.restype = wt.DWORD
    k32.OpenProcess.argtypes = [wt.DWORD, wt.BOOL, wt.DWORD]
    k32.OpenProcess.restype = wt.HANDLE
    k32.CloseHandle.argtypes = [wt.HANDLE]
    k32.CloseHandle.restype = wt.BOOL
    k32.GetCurrentProcess.restype = wt.HANDLE

    notes: list[str] = []

    # ---- NT device name -> drive letter -------------------------------------
    # Handle names come back as `\Device\HarddiskVolume3\path`, which no path
    # comparison against `D:\...` will ever match.
    devmap: list[tuple[str, str]] = []
    buf = ctypes.create_unicode_buffer(1024)
    for letter in "ABCDEFGHIJKLMNOPQRSTUVWXYZ":
        if k32.QueryDosDeviceW(f"{letter}:", buf, 1024):
            devmap.append((buf.value, f"{letter}:"))
    devmap.sort(key=lambda kv: -len(kv[0]))

    def dos_path(nt_name: str) -> str | None:
        for dev, letter in devmap:
            if nt_name.startswith(dev) and nt_name[len(dev):len(dev) + 1] in ("\\", ""):
                return letter + nt_name[len(dev):]
        return None

    # ---- the whole-system handle table --------------------------------------
    #
    # The probe file is opened *before* the snapshot, not after.  The table is
    # a point-in-time copy, so a handle created afterwards is simply not in it
    # -- which is how the first version of this failed: it searched a snapshot
    # for a handle that had not existed when the snapshot was taken, and
    # concluded the File type could not be identified.
    me = k32.GetCurrentProcessId()
    with open(__file__, "rb") as probe:
        probe_handle = msvcrt.get_osfhandle(probe.fileno())

        size = 1 << 20
        while True:
            table = ctypes.create_string_buffer(size)
            need = ctypes.c_ulong(0)
            status = ntdll.NtQuerySystemInformation(
                SystemExtendedHandleInformation, table, size, ctypes.byref(need)
            )
            if status == 0:
                break
            if status & 0xFFFFFFFF != STATUS_INFO_LENGTH_MISMATCH:
                notes.append(
                    f"handle table unavailable (NTSTATUS 0x{status & 0xFFFFFFFF:08x})"
                )
                return {}, set(), notes
            size = max(size * 2, need.value + (1 << 20))

        base = ctypes.addressof(table)
        count = ctypes.c_size_t.from_address(base).value
        entries = ctypes.cast(
            base + 2 * ctypes.sizeof(ctypes.c_size_t), ctypes.POINTER(HANDLE_ENTRY)
        )

        file_type = None
        for i in range(count):
            e = entries[i]
            if e.UniqueProcessId == me and e.HandleValue == probe_handle:
                file_type = e.ObjectTypeIndex
                break
    if file_type is None:
        notes.append("could not identify the File object type; open handles were NOT checked")
        return {}, set(), notes

    # Note for anyone tempted to dedupe by kernel object address: `Object` is
    # zeroed for an unprivileged caller (Windows redacts kernel pointers), so
    # every entry reads as the same object and the obvious "name each object
    # once, share it among its holders" optimisation is simply unavailable
    # here.  Measured: 333342 handles, 22267 of them files, one distinct
    # `Object` value.  Each handle must therefore be named on its own.
    wanted: dict[int, list[tuple[int, int]]] = {}
    for i in range(count):
        e = entries[i]
        if e.ObjectTypeIndex == file_type and e.UniqueProcessId != me:
            wanted.setdefault(e.UniqueProcessId, []).append((e.HandleValue, e.GrantedAccess))

    paths_by_pid: dict[int, set[str]] = {}
    unnamed: set[int] = set()
    skipped = 0
    lock = threading.Lock()
    todo = list(wanted.items())
    cursor = [0]

    def worker():
        nonlocal skipped
        self_proc = k32.GetCurrentProcess()
        nbuf = ctypes.create_string_buffer(4096)
        while True:
            with lock:
                i = cursor[0]
                if i >= len(todo):
                    return
                cursor[0] = i + 1
            pid, handles = todo[i]
            src = k32.OpenProcess(PROCESS_DUP_HANDLE, False, pid)
            if not src:
                # The table showed this process holds files; their names are
                # out of reach.  Recorded, not dropped.
                with lock:
                    unnamed.add(pid)
                continue
            found: set[str] = set()
            local_skipped = 0
            try:
                for value, access in handles:
                    if access == HANG_PRONE_ACCESS:
                        local_skipped += 1
                        continue
                    dup = wt.HANDLE()
                    if not k32.DuplicateHandle(
                        src, wt.HANDLE(value), self_proc, ctypes.byref(dup),
                        0, False, DUPLICATE_SAME_ACCESS,
                    ):
                        continue
                    try:
                        # The real wedge filter.  `NtQueryObject`'s name query
                        # blocks forever on a synchronous pipe with I/O
                        # outstanding; `GetFileType` answers from the file
                        # object without touching the device, so it cannot.
                        # Only FILE_TYPE_DISK has a path on a volume, so
                        # pipes, consoles and the rest are not merely safe to
                        # skip -- they are incapable of blocking a rename.
                        # This replaced a granted-access heuristic that let
                        # enough wedges through to stall the whole pool.
                        if k32.GetFileType(dup) != FILE_TYPE_DISK:
                            continue
                        need2 = ctypes.c_ulong(0)
                        st = ntdll.NtQueryObject(
                            dup, ObjectNameInformation, nbuf, 4096, ctypes.byref(need2)
                        )
                        if st != 0:
                            continue
                        us = UNICODE_STRING.from_buffer_copy(
                            nbuf.raw[:ctypes.sizeof(UNICODE_STRING)]
                        )
                        if not us.Length or not us.Buffer:
                            continue
                        nt_name = ctypes.string_at(us.Buffer, us.Length).decode(
                            "utf-16-le", errors="replace"
                        )
                        dosified = dos_path(nt_name)
                        if dosified:
                            found.add(dosified)
                    finally:
                        k32.CloseHandle(dup)
            finally:
                k32.CloseHandle(src)
            # Published per process, so a later wedge does not discard the
            # processes already finished.
            with lock:
                skipped += local_skipped
                if found:
                    paths_by_pid.setdefault(pid, set()).update(found)

    # Naming 22k handles one at a time overran a 30s budget on this machine.
    # `DuplicateHandle` and `NtQueryObject` are foreign calls, and ctypes drops
    # the GIL across them, so a pool of threads gets real parallelism rather
    # than the usual Python pretence.
    #
    # They are daemon threads for a second reason: `NtQueryObject` can block
    # forever on a handle the access-mask filter did not catch.  A wedge then
    # costs the work queued behind that one thread rather than the whole run --
    # whatever was published before the deadline is used, and the shortfall is
    # reported rather than passed off as an empty result.
    pool = [threading.Thread(target=worker, daemon=True) for _ in range(16)]
    for t in pool:
        t.start()
    deadline = time.monotonic() + timeout_s
    for t in pool:
        t.join(max(0.0, deadline - time.monotonic()))
    if any(t.is_alive() for t in pool):
        with lock:
            done, queued = cursor[0], len(todo)
        notes.append(
            f"open-handle scan timed out after {timeout_s:.0f}s and is incomplete "
            f"({done}/{queued} processes reached)"
        )
    if skipped:
        notes.append(
            f"{skipped} handle(s) skipped as hang-prone (synchronous pipes; "
            "these cannot block a directory rename)"
        )
    return paths_by_pid, unnamed, notes


def _scan_windows(want_modules: bool, want_handles: bool = True):
    import ctypes
    import ctypes.wintypes as wt

    ntdll = ctypes.WinDLL("ntdll")
    k32 = ctypes.WinDLL("kernel32", use_last_error=True)

    PROCESS_QUERY_LIMITED_INFORMATION = 0x1000
    PROCESS_VM_READ = 0x0010
    STILL_ACTIVE = 259
    LIST_MODULES_ALL = 0x03

    class UNICODE_STRING(ctypes.Structure):
        _fields_ = [
            ("Length", wt.USHORT),
            ("MaximumLength", wt.USHORT),
            ("Buffer", ctypes.c_void_p),
        ]

    class PROCESS_BASIC_INFORMATION(ctypes.Structure):
        _fields_ = [
            ("Reserved1", ctypes.c_void_p),
            ("PebBaseAddress", ctypes.c_void_p),
            ("Reserved2", ctypes.c_void_p * 2),
            ("UniqueProcessId", ctypes.c_void_p),
            ("Reserved3", ctypes.c_void_p),
        ]

    # Offsets into the 64-bit PEB and RTL_USER_PROCESS_PARAMETERS.  Both are
    # documented and stable across every 64-bit Windows release.
    PEB_PROCESS_PARAMETERS = 0x20
    RTL_UPP_CURRENT_DIRECTORY = 0x38

    # Every prototype is declared.  ctypes' default for an undeclared argument
    # is `c_int` and for an undeclared return is `c_int` - both 32-bit - so on
    # win64 a module base address raises `int too long to convert` (which is the
    # loud failure) and a returned HANDLE is silently sign-extended through 32
    # bits (which is the quiet one).  A tool whose whole purpose is to avoid
    # false negatives cannot afford either.
    HMODULE = ctypes.c_void_p
    k32.OpenProcess.argtypes = [wt.DWORD, wt.BOOL, wt.DWORD]
    k32.OpenProcess.restype = wt.HANDLE
    k32.CloseHandle.argtypes = [wt.HANDLE]
    k32.CloseHandle.restype = wt.BOOL
    k32.GetExitCodeProcess.argtypes = [wt.HANDLE, ctypes.POINTER(wt.DWORD)]
    k32.GetExitCodeProcess.restype = wt.BOOL
    k32.ReadProcessMemory.argtypes = [
        wt.HANDLE, ctypes.c_void_p, ctypes.c_void_p, ctypes.c_size_t,
        ctypes.POINTER(ctypes.c_size_t),
    ]
    k32.ReadProcessMemory.restype = wt.BOOL
    k32.K32EnumProcesses.argtypes = [
        ctypes.POINTER(wt.DWORD), wt.DWORD, ctypes.POINTER(wt.DWORD)
    ]
    k32.K32EnumProcesses.restype = wt.BOOL
    k32.QueryFullProcessImageNameW.argtypes = [
        wt.HANDLE, wt.DWORD, wt.LPWSTR, ctypes.POINTER(wt.DWORD)
    ]
    k32.QueryFullProcessImageNameW.restype = wt.BOOL
    k32.K32EnumProcessModulesEx.argtypes = [
        wt.HANDLE, ctypes.POINTER(HMODULE), wt.DWORD, ctypes.POINTER(wt.DWORD),
        wt.DWORD,
    ]
    k32.K32EnumProcessModulesEx.restype = wt.BOOL
    k32.K32GetModuleFileNameExW.argtypes = [wt.HANDLE, HMODULE, wt.LPWSTR, wt.DWORD]
    k32.K32GetModuleFileNameExW.restype = wt.DWORD
    ntdll.NtQueryInformationProcess.argtypes = [
        wt.HANDLE, ctypes.c_int, ctypes.c_void_p, ctypes.c_ulong,
        ctypes.POINTER(ctypes.c_ulong),
    ]
    ntdll.NtQueryInformationProcess.restype = ctypes.c_long

    def read(handle, addr, size):
        buf = ctypes.create_string_buffer(size)
        got = ctypes.c_size_t(0)
        ok = k32.ReadProcessMemory(
            handle, ctypes.c_void_p(addr), buf, ctypes.c_size_t(size), ctypes.byref(got)
        )
        if not ok or got.value != size:
            raise OSError(ctypes.get_last_error(), "ReadProcessMemory")
        return buf.raw

    def enum_pids():
        # Doubling until the buffer is not filled exactly: a completely full
        # buffer means the list was truncated, and a truncated list is the
        # silent false negative this script exists to avoid.
        n = 4096
        while True:
            arr = (wt.DWORD * n)()
            needed = wt.DWORD(0)
            if not k32.K32EnumProcesses(
                arr, ctypes.sizeof(arr), ctypes.byref(needed)
            ):
                raise OSError(ctypes.get_last_error(), "EnumProcesses")
            count = needed.value // ctypes.sizeof(wt.DWORD)
            if count < n:
                return list(arr[:count])
            n *= 2

    def image_name(handle):
        size = wt.DWORD(1024)
        buf = ctypes.create_unicode_buffer(size.value)
        if k32.QueryFullProcessImageNameW(handle, 0, buf, ctypes.byref(size)):
            return buf.value
        return "?"

    def cwd_of(handle):
        pbi = PROCESS_BASIC_INFORMATION()
        status = ntdll.NtQueryInformationProcess(
            handle, 0, ctypes.byref(pbi), ctypes.sizeof(pbi), None
        )
        if status != 0 or not pbi.PebBaseAddress:
            raise OSError(0, f"NtQueryInformationProcess 0x{status & 0xFFFFFFFF:08x}")
        peb = ctypes.cast(pbi.PebBaseAddress, ctypes.c_void_p).value
        params = int.from_bytes(read(handle, peb + PEB_PROCESS_PARAMETERS, 8), "little")
        if not params:
            raise OSError(0, "null ProcessParameters")
        raw = read(handle, params + RTL_UPP_CURRENT_DIRECTORY, ctypes.sizeof(UNICODE_STRING))
        us = UNICODE_STRING.from_buffer_copy(raw)
        if not us.Length or not us.Buffer:
            raise OSError(0, "empty CurrentDirectory")
        return read(handle, us.Buffer, us.Length).decode("utf-16-le", errors="replace")

    def modules_of(handle):
        n = 1024
        while True:
            arr = (ctypes.c_void_p * n)()
            needed = wt.DWORD(0)
            if not k32.K32EnumProcessModulesEx(
                handle,
                arr,
                ctypes.sizeof(arr),
                ctypes.byref(needed),
                LIST_MODULES_ALL,
            ):
                # ERROR_PARTIAL_COPY (299) is normal for a process that is
                # starting or exiting.  It is still raised rather than swallowed:
                # the caller records the process as not-fully-readable instead of
                # counting it as clean.
                raise OSError(ctypes.get_last_error(), "EnumProcessModulesEx")
            count = needed.value // ctypes.sizeof(ctypes.c_void_p)
            if count <= n:
                out = []
                buf = ctypes.create_unicode_buffer(1024)
                for i in range(count):
                    if k32.K32GetModuleFileNameExW(handle, arr[i], buf, 1024):
                        out.append(buf.value)
                return out
            n = count * 2

    # The open-handle table is a single whole-system query, so it is done once
    # up front rather than per process.
    open_files: dict[int, set[str]] = {}
    unnamed_handles: set[int] = set()
    if want_handles:
        open_files, unnamed_handles, notes = _windows_file_handles()
        for note in notes:
            yield None, None, None, note

    for pid in enum_pids():
        if pid == 0:
            continue
        # Open handles are attached first, so that a process whose cwd or module
        # list cannot be read is still reported with the handles that *were*
        # named.  Dropping them on the first failure would hide the holder
        # behind an unrelated permission error.
        paths: list[tuple[str, str]] = [("handle", p) for p in open_files.get(pid, ())]
        held = "; some open handles could not be named" if pid in unnamed_handles else ""

        handle = k32.OpenProcess(
            PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_VM_READ, False, pid
        )
        if not handle:
            yield (pid, "?", paths or None,
                   "OpenProcess denied (different user, or protected)" + held)
            continue
        try:
            code = wt.DWORD(0)
            if k32.GetExitCodeProcess(handle, ctypes.byref(code)) and code.value != STILL_ACTIVE:
                continue  # exited between enumeration and open - not a holder
            image = image_name(handle)
            paths.append(("image", image))
            try:
                paths.append(("cwd", cwd_of(handle)))
            except OSError as exc:
                yield pid, image, paths, f"cwd unreadable: {exc}{held}"
                continue
            if want_modules:
                try:
                    for m in modules_of(handle):
                        paths.append(("module", m))
                except OSError as exc:
                    # Partial information: the cwd was readable but the module
                    # list was not, so this process cannot be cleared.  Its
                    # partial paths are still reported.
                    yield pid, image, paths, f"modules unreadable: {exc}{held}"
                    continue
            yield pid, image, paths, held.lstrip("; ") or None
        finally:
            k32.CloseHandle(handle)


def scan(want_modules: bool, want_handles: bool = True):
    if os.name == "nt":
        return _scan_windows(want_modules, want_handles)
    return _scan_posix(want_modules)


def main(argv: list[str]) -> int:
    flags = {a for a in argv[1:] if a.startswith("--")}
    want_modules = "--no-modules" not in flags
    want_handles = "--no-handles" not in flags
    args = [a for a in argv[1:] if not a.startswith("--")]
    roots = [Path(a).resolve() for a in args] or [REPO]

    print("[who-holds] looking for processes holding anything under:")
    for r in roots:
        print(f"[who-holds]   {r}")
    if not want_modules:
        print("[who-holds] --no-modules: a running .exe or a loaded .dll under "
              "these roots will NOT be found.")
    if not want_handles:
        print("[who-holds] --no-handles: an ordinary OPEN FILE under these roots "
              "will NOT be found, which is the most common blocker of all.")

    def under(p: str) -> bool:
        try:
            rp = Path(p).resolve()
        except OSError:
            return False
        return any(rp == r or r in rp.parents for r in roots)

    holders: list[Holder] = []
    unreadable: list[tuple[int, str, str]] = []
    scan_notes: list[str] = []
    total = 0
    for pid, image, paths, err in scan(want_modules, want_handles):
        if pid is None:
            # A scan-wide note (the handle table itself was incomplete), not a
            # process.  It bears on whether "no holder" can be believed, so it
            # is carried into the verdict rather than merely printed.
            scan_notes.append(err)
            continue
        total += 1
        if err is not None:
            unreadable.append((pid, image, err))
        if paths is None:
            continue
        seen: set[tuple[str, str]] = set()
        for kind, p in paths:
            if p and under(p) and (kind, p) not in seen:
                seen.add((kind, p))
                holders.append((pid, image, kind, str(Path(p).resolve())))

    print(f"[who-holds] {total} process(es) enumerated, "
          f"{len(unreadable)} not fully readable, {len(holders)} reference(s) found")

    by_pid: dict[int, list[Holder]] = {}
    for h in holders:
        by_pid.setdefault(h[0], []).append(h)
    for pid in sorted(by_pid):
        image = by_pid[pid][0][1]
        print(f"[who-holds]   pid {pid:>7}  {image}")
        for _, _, kind, p in sorted(by_pid[pid], key=lambda h: (h[2], h[3])):
            print(f"[who-holds]        {kind:>6}  {p}")

    if unreadable:
        # Deliberately loud: an unreadable process is a process that might be
        # the holder, so a run with unreadable processes and no holders has NOT
        # shown the directory to be free.
        by_reason: dict[str, int] = {}
        for _, _, err in unreadable:
            by_reason[err] = by_reason.get(err, 0) + 1
        print("[who-holds] not fully readable (any of these could be a holder):")
        for reason, count in sorted(by_reason.items(), key=lambda kv: -kv[1]):
            print(f"[who-holds]   {count:>4}x {reason}")
    for note in scan_notes:
        print(f"[who-holds] NOTE: {note}")

    if holders:
        return 0
    # A disabled scan is not a clean scan.  Without it the run cannot even in
    # principle have seen the corresponding kind of holder, so it may not
    # report the directory free -- the flags buy speed, not certainty.
    if unreadable or scan_notes or not want_modules or not want_handles:
        print("[who-holds] INCONCLUSIVE: no holder found, but not everything that "
              "could hold it was inspected.")
        return 2
    print("[who-holds] nothing holds any of these directories.")
    return 1


if __name__ == "__main__":
    sys.exit(main(sys.argv))
