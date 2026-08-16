# CPython spike — how much libc does a real interpreter need?

**Answer: CPython 3.12.3 references 363 external symbols, and SlateOS's `libc.a`
now provides all 363. It links.** The spike originally measured thirteen
missing; every one turned out to be a small, well-understood function — no
missing subsystem, no architectural surprise — and all thirteen were
implemented in commit `5531f816c`. Re-running the spike after that commit gives
`MISSING_BY_SET_DIFFERENCE=0`, `MISSING_AT_LINK=0`, and a 26 MB statically
linked `python-slateos` ELF.

This is the third and largest program pointed at our libc, after GNU bash 5.2
(design-decisions.md §305, shipped, 3 symbols missing at the time) and pkgconf
2.3.0 (shipped, zero missing). It exists because `roadmap.md`'s "Enough of POSIX
libc for: gcc, coreutils, bash, Python (CPython)" had been open for months with
no measurement behind it — and "we should port CPython eventually" is not a
measurement.

## Running it

```bash
scripts/cpython-spike/run.sh        # fetch, configure, build libpython3.12.a
scripts/cpython-spike/slatelink.sh  # link it against SlateOS's own libc.a
```

Both derive every path from `scripts/lib/worktree.sh`, so they work in any of
the four checkouts and cannot link one lane's objects against another lane's
libc. Scratch lives in `/tmp/cpython-spike-$SLATE_LANE`; delete it to redo
`configure`.

### Prerequisites

- **A host `python3.12` on `PATH`.** CPython 3.11+ cross-builds require
  `--with-build-python` of the *same major.minor* to run `deepfreeze` and
  generate `sysconfig` data. Override with `BUILD_PY=…`. This is why the spike
  pins 3.12 rather than "latest" (roadmap 4.4): WSL here has 3.12.3, which
  removes an entire class of cross-build failure for free, and libc surface
  does not meaningfully move between patch releases. When this graduates from a
  spike to a port, note that a newer CPython needs a newer build interpreter —
  that is a prerequisite, not a preference.
- The pinned zig 0.13.0 cross-toolchain, provisioned automatically by
  `slate_make_zig_wrappers`.
- `curl` and ~1.5 GB of free space in `/tmp`.

### Configure flags that are not obvious

| Flag | Why |
|---|---|
| `--disable-shared` | SlateOS has no dynamic loader on this path; the target is a static `ET_EXEC`, same as bash and pkgconf. |
| `--without-ensurepip`, `--disable-test-modules` | Megabytes of *Python* source. Cannot affect which libc symbols the interpreter core references. |
| `ac_cv_file__dev_ptmx=no`, `ac_cv_file__dev_ptc=no` | `configure` cannot stat files on the target and hard-errors if left to guess. |
| `ac_cv_buggy_getaddrinfo=no` | `configure` detects the well-known broken-`getaddrinfo` bug by **running** a test program. A cross build cannot, so it assumes the bug is present and errors out. Asserting "not buggy" is the correct cross answer and what distro cross-recipes do. The alternative `configure` suggests, `--disable-ipv6`, would silently compile a *different* interpreter with a smaller socket surface — the opposite of what a spike measuring libc surface wants. If our `getaddrinfo` is genuinely buggy that is a `posix/src` bug to fix, not a reason to build less of CPython. |

Optional modules `zlib`, `binascii` and `_ctypes` fail to build for want of
zlib and libffi. That is expected and irrelevant: neither is libc, and neither
is linked into `Programs/python.o` or `libpython3.12.a`.

## The measurement, and the trap in it

`slatelink.sh` computes the answer **twice, by unrelated means**, and the two
agree exactly:

| | before `5531f816c` | after |
|---|---|---|
| `REFERENCED_RAW` | 2225 | 2225 |
| `CPYTHON_SELF_DEFINES` | 2486 | 2486 |
| **`REFERENCED_EXTERNAL`** | **363** | **363** |
| `SYSROOT_DEFINES` | 3011 | 3027 |
| **`MISSING_BY_SET_DIFFERENCE`** | **13** | **0** |
| **`MISSING_AT_LINK`** (ld.lld's own report) | **13**, identical list | **0** |
| `SLATE_LINK_EXIT` | 1 (`NO_SLATE_BINARY`) | 0 |

`SYSROOT_DEFINES` moved by 16, not 13, and the extra three are accounted for
exactly: the four `posix_spawnattr_set*` were implemented together with their
`posix_spawnattr_get*` counterparts (a setter shipped without its getter is
half an API, and the storage backing both is the same struct field), which is
17 new exports minus `syscall`, which already existed under that name. Nothing
unexplained moved.

The two independent methods agreed exactly both before and after, which is the
point of computing the answer twice.

**The trap, recorded because the first run of this script fell into it.**
`nm --undefined-only` on a *static archive* reports every member's undefined
symbols — and a static library is overwhelmingly self-referential. `obmalloc.o`
calls `PyErr_NoMemory`, which is undefined *in obmalloc.o* and defined in
`errors.o` two members later. So the raw 2225 is "external references made by
any member", not "symbols the archive cannot satisfy". Differencing it straight
against `libc.a` produced **1875 "missing" symbols** — a figure that is 99%
CPython's own API (`PyAST_Check`, `PyArg_ParseTuple`, `PyBytes_Type`, …) and
says nothing whatever about our libc. It looked like a measurement and was not
one. Subtracting the archive's own definitions is what makes the number mean
what its name claims.

The two methods are worth keeping side by side because they bound the answer
from opposite directions: the linker only reports symbols on paths it actually
pulled in (a lower bound on what a *fuller* CPython would need), while the set
difference covers every member (an upper bound). Here they coincide, which is
itself the strongest evidence that 13 is right.

## The thirteen — all closed in `5531f816c`

| Symbol | Group | Where CPython uses it | Landed in |
|---|---|---|---|
| `syscall` | raw syscall | `PyThread_get_thread_native_id` (gettid), `os.pidfd_open`, `signal.pidfd_send_signal` | `posix/src/sys_syscall.rs` |
| `pthread_kill` | threads | `signal.pthread_kill` | `posix/src/pthread.rs` |
| `pthread_getcpuclockid` | threads | `time.pthread_getcpuclockid` | `posix/src/pthread.rs` |
| `sigwaitinfo` | signals | `signal.sigwaitinfo` | `posix/src/signal.rs` |
| `ttyname_r` | tty | `os.ttyname` | `posix/src/ioctl.rs` |
| `openpty` | pty | `os.openpty` | `posix/src/pty.rs` (new) |
| `forkpty` | pty | `os.forkpty` | `posix/src/pty.rs` (new) |
| `login_tty` | pty | `os.login_tty` | `posix/src/pty.rs` (new) |
| `posix_spawnattr_setsigmask` | posix_spawn | `os.posix_spawn` | `posix/src/spawn.rs` |
| `posix_spawnattr_setsigdefault` | posix_spawn | `os.posix_spawn` | `posix/src/spawn.rs` |
| `posix_spawnattr_setschedpolicy` | posix_spawn | `os.posix_spawn` | `posix/src/spawn.rs` |
| `posix_spawnattr_setschedparam` | posix_spawn | `os.posix_spawn` | `posix/src/spawn.rs` |
| `__sched_cpucount` | sched | `os.sched_getaffinity` (musl's out-of-line helper behind the `CPU_COUNT` macro) | `posix/src/sched.rs` |

Five groups, thirteen functions, and not one of them required a new kernel
subsystem — SlateOS already had threads, signals, pty groundwork and
`posix_spawn` itself. Two carry documented degradations rather than lies, both
written up in `todo.txt`: `pthread_kill` on a *peer* thread currently delivers
process-directed (the kernel has no task-id-targeted signal syscall yet), and
`pthread_getcpuclockid` returns POSIX's `ENOENT` for a peer. `syscall()` is a
translation table over the dozen numbers CPython actually issues, not a trap
door — the libc-bypass numbers (`read`, `write`, `execve`, …) deliberately
return `ENOSYS`.

**This is the headline result.** The interpreter that "wants threads, dynamic
loading, locales and a far wider syscall surface" than bash or pkgconf turned
out to be 13 functions away from linking — about the same order as bash's 3,
not the order of magnitude more that the roadmap's phrasing implied — and those
13 are now written, tested and shipped.

## What this spike is *not*

`slatelink.sh` now exits 0 and emits `python-slateos`: 26,038,688 bytes,
*ELF 64-bit LSB executable, x86-64, statically linked, with debug_info, not
stripped*, followed by the marker `SLATE_CPYTHON_BUILT`.

**That is a linked interpreter, not a running one.** Linking proves the symbol
surface is complete; it proves nothing about behaviour. A working `python3` on
SlateOS additionally needs the stdlib staged in the rootfs, a functioning
`importlib` bootstrap, and the filesystem and tty semantics those assume — the
very semantics behind the two degradations listed above. The deliverable here
is the measurement and the now-closed list, plus a binary to run once there is
a rootfs to run it in.
