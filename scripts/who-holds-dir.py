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

Three things make Windows refuse to rename a directory, and this script looks
for all three, because each produces the *same* `Access is denied` and picking
one to test is how an investigation gets closed on the wrong answer:

1. **A running executable** whose image is inside it (the image file is mapped
   and cannot be renamed, which blocks every ancestor).
2. **A loaded DLL** inside it - same mechanism, invisible to image-path checks.
   Cargo's proc-macro output and test-harness DLLs live under `target/`.
3. **A process whose current working directory** is inside it.  A cwd is not an
   image path, so no `Get-Process` filter can see it.

It needs only `PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_VM_READ`, granted for
processes running as the same user, so **no elevation is required**.

Processes it cannot open are reported as **unreadable rather than skipped
silently**, and the exit status distinguishes them: "nothing holds it" and "I
could not see what holds it" are different answers, and printing the first when
the second is true is exactly the false negative that left the original issue
unresolved.

Usage:
    python scripts/who-holds-dir.py                 # anything under the repo
    python scripts/who-holds-dir.py <dir> [<dir>...]  # under these directories
    python scripts/who-holds-dir.py --no-modules .. # cwd only (much faster)

Exit status:
    0  at least one holder found
    1  no holder, and every process was readable - the directory really is free
    2  inconclusive: no holder found, but some processes could not be inspected

Windows only for the module scan; on POSIX it reads `/proc/<pid>/cwd` and
`/proc/<pid>/maps`, which is the same question with an easier answer.
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


def _scan_windows(want_modules: bool):
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

    for pid in enum_pids():
        if pid == 0:
            continue
        handle = k32.OpenProcess(
            PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_VM_READ, False, pid
        )
        if not handle:
            yield pid, "?", None, "OpenProcess denied (different user, or protected)"
            continue
        try:
            code = wt.DWORD(0)
            if k32.GetExitCodeProcess(handle, ctypes.byref(code)) and code.value != STILL_ACTIVE:
                continue  # exited between enumeration and open - not a holder
            image = image_name(handle)
            paths: list[tuple[str, str]] = [("image", image)]
            try:
                paths.append(("cwd", cwd_of(handle)))
            except OSError as exc:
                yield pid, image, None, f"cwd unreadable: {exc}"
                continue
            if want_modules:
                try:
                    for m in modules_of(handle):
                        paths.append(("module", m))
                except OSError as exc:
                    # Partial information: the cwd was readable but the module
                    # list was not, so this process cannot be cleared.  Its
                    # partial paths are still reported.
                    yield pid, image, paths, f"modules unreadable: {exc}"
                    continue
            yield pid, image, paths, None
        finally:
            k32.CloseHandle(handle)


def scan(want_modules: bool):
    if os.name == "nt":
        return _scan_windows(want_modules)
    return _scan_posix(want_modules)


def main(argv: list[str]) -> int:
    want_modules = "--no-modules" not in argv[1:]
    args = [a for a in argv[1:] if a != "--no-modules"]
    roots = [Path(a).resolve() for a in args] or [REPO]

    print("[who-holds] looking for processes holding anything under:")
    for r in roots:
        print(f"[who-holds]   {r}")
    if not want_modules:
        print("[who-holds] --no-modules: only current directories are checked, so a "
              "running .exe or a loaded .dll under these roots will NOT be found.")

    def under(p: str) -> bool:
        try:
            rp = Path(p).resolve()
        except OSError:
            return False
        return any(rp == r or r in rp.parents for r in roots)

    holders: list[Holder] = []
    unreadable: list[tuple[int, str, str]] = []
    total = 0
    for pid, image, paths, err in scan(want_modules):
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

    if holders:
        return 0
    if unreadable:
        print("[who-holds] INCONCLUSIVE: no holder found, but not every process could "
              "be inspected.")
        return 2
    print("[who-holds] nothing holds any of these directories.")
    return 1


if __name__ == "__main__":
    sys.exit(main(sys.argv))
