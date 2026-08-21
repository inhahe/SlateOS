# CPython spike — how much libc does a real interpreter need?

**Answer: CPython 3.12.3 references 478 external symbols, and SlateOS's `libc.a`
provides all 478. It links, and — with its standard library packed as one
`python312.zip` — it starts, imports, and does real work.**

This is the fourth and by a wide margin the largest program pointed at our libc,
after GNU bash 5.2 (design-decisions.md §305, 3 symbols missing at the time),
pkgconf 2.3.0 (zero missing) and GNU make 4.4.1 (§339). It exists because
`roadmap.md`'s "Enough of POSIX libc for: gcc, coreutils, bash, Python
(CPython)" had been open for months with no measurement behind it — and "we
should port CPython eventually" is not a measurement.

The spike ran in two stages, and the second stage is where most of the value
turned out to be:

1. **Linking** (`run.sh` + `slatelink.sh`). Originally measured thirteen missing
   symbols; all thirteen landed in `5531f816c`. That gave a binary.
2. **Running** (`stdlib.sh`). A linked interpreter with no standard library
   cannot get past `init_fs_encoding` — it dies looking for the `encodings`
   package, which is not frozen into the binary. Closing that gap exposed two
   *cross-build configuration* defects that had made the stage-1 measurement an
   undercount, and cost six more libc functions. See "The two defects" below.

## Running it

```bash
scripts/cpython-spike/run.sh        # fetch, configure, build libpython3.12.a
scripts/cpython-spike/slatelink.sh  # link it against SlateOS's own libc.a
scripts/cpython-spike/stdlib.sh     # pack the stdlib, and prove it runs
```

All three derive every path from `scripts/lib/worktree.sh`, so they work in any
of the four checkouts and cannot link one lane's objects against another lane's
libc. Scratch lives in `/tmp/cpython-spike-$SLATE_LANE`; delete it to redo
`configure`.

Two more scripts answer a narrower question — *how much of the 20 MB archive
does a run actually read?* — and are documented under "How much of the archive a
run actually reads" below:

```bash
PYTHONHOME=<prefix> <prefix>/bin/python3 scripts/cpython-spike/pymeasure.py
PYTHONHOME=<prefix> <prefix>/bin/python3 scripts/cpython-spike/pyworkload.py
```

`pymeasure.py` measures what `Py_Initialize` alone reads; `pyworkload.py` adds
the imports the proposed Path-Z rung performs. Both default to whichever `.zip`
is on the running interpreter's `sys.path`, so neither can quietly describe a
different file from the one that produced its module list. They must be run
against a real `<prefix>/bin/python3` + `<prefix>/lib/python312.zip` layout: an
interpreter run out of its build tree keeps `sys.prefix = /usr/local` whatever
`PYTHONHOME` says, and then measures a stdlib *directory* instead of an archive.

Artifacts land on the gitignored shelf in `build/spike/`, and
`scripts/create-ext4-rootfs.sh` stages both of them:

| Artifact | Size | Staged as |
|---|---:|---|
| `python-slateos.elf` | 11,210,656 | `/bin/python3` |
| `python312.zip` | 20,498,464 | `/usr/local/lib/python312.zip` |

They are staged under **one** condition, never separately: an interpreter
without its stdlib is 11 MB on the image that produces a process dying before
`main()`, with an error that reads like a broken libc.

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
| `MODULE_BUILDTYPE=static` | **The single most consequential setting here.** See below. |
| `PKG_CONFIG_LIBDIR=/nonexistent-slateos-cross` | Likewise. See below. |
| `--disable-shared` | SlateOS has no dynamic loader on this path; the target is a static `ET_EXEC`, same as bash and pkgconf. Note this governs *libpython only* — it does **not** imply static stdlib modules. |
| `--without-ensurepip`, `--disable-test-modules` | Megabytes of *Python* source. Cannot affect which libc symbols the interpreter core references. |
| `ac_cv_file__dev_ptmx=no`, `ac_cv_file__dev_ptc=no` | `configure` cannot stat files on the target and hard-errors if left to guess. |
| `ac_cv_buggy_getaddrinfo=no` | `configure` detects the well-known broken-`getaddrinfo` bug by **running** a test program. A cross build cannot, so it assumes the bug is present and errors out. Asserting "not buggy" is the correct cross answer and what distro cross-recipes do. The alternative `configure` suggests, `--disable-ipv6`, would silently compile a *different* interpreter with a smaller socket surface — the opposite of what a spike measuring libc surface wants. If our `getaddrinfo` is genuinely buggy that is a `posix/src` bug to fix, not a reason to build less of CPython. |

## The two defects — why stage 1 measured the wrong interpreter

Both were silent. Both produced a build that linked, exited 0, and was wrong.

### 1. `MODULE_BUILDTYPE` defaults to `shared`, and `--disable-shared` does not change it

CPython 3.12's `configure` contains `MODULE_BUILDTYPE=${MODULE_BUILDTYPE:-shared}`
and only overrides it for wasm hosts. `--disable-shared` governs *libpython*;
the stdlib's C extensions are a separate axis. Left at the default, every one
of them — `_struct`, `_json`, `math`, `binascii`, `select`, `_socket`, … — is
built as a `.so` in `lib-dynload`, which on a system with no dynamic loader is
a file that can never be opened.

The symptom was not a build failure. The interpreter linked and started, and
then `import struct` raised `ModuleNotFoundError: No module named '_struct'`.
`sys.builtin_module_names` held **31** entries — the frozen bootstrap set and
nothing else.

With `MODULE_BUILDTYPE=static` it holds **83**, `libpython3.12.a` goes from
46,400,194 to 61,462,452 bytes, and the stdlib stops being decorative.
`stdlib.sh` prints that count on every run, first thing, because it is the
number most likely to regress silently.

### 2. `pkg-config` answers for the build machine

`configure` locates zlib, OpenSSL, libffi, sqlite3, liblzma, bzip2, ncurses,
readline and libuuid through `pkg-config`, which in a cross build cheerfully
describes the *host's* library set. So CPython compiled `zlib`, `binascii` and
`_ctypes` against Ubuntu's headers and then failed to link them — and the
previous version of this README recorded that as "expected and irrelevant".

It was neither. `MAKE_EXIT` was **2**, i.e. the build was failing, and the
failure was being read as normal. Setting `PKG_CONFIG_LIBDIR` to a directory
that does not exist makes every probe answer "no", which is *the truth for our
sysroot*. `MAKE_EXIT` is now 0 with zero error lines.

The general lesson, which is not specific to CPython: **in a cross build, a
probe that consults the host is not a failed probe, it is a wrong answer.**

### Three archives that live outside `libpython`

Once modules went static, `_decimal`, `pyexpat`, `_elementtree` and the SHA-2
implementation stopped carrying their own copies in `lib-dynload` and started
needing the vendored archives CPython's own Makefile appends via `MODLIBS`:

```
Modules/_decimal/libmpdec/libmpdec.a
Modules/expat/libexpat.a
Modules/_hacl/libHacl_Hash_SHA2.a
```

A hand-written link naming only `libpython3.12.a` gets `undefined symbol:
PyExpat_XML_SetEntityDeclHandler` and friends. `slatelink.sh` globs them rather
than typing them out, so a CPython that renames one fails loudly at the link
instead of silently dropping a module.

## The measurement, and the trap in it

`slatelink.sh` computes the answer **twice, by unrelated means**, and the two
agree exactly:

| | stage 1 (shared modules) | stage 2 (static modules) |
|---|---:|---:|
| `REFERENCED_RAW` | 2225 | 2756 |
| `CPYTHON_SELF_DEFINES` | 2486 | 3041 |
| **`REFERENCED_EXTERNAL`** | **363** | **478** |
| `SYSROOT_DEFINES` | 3027 | 3060 |
| **`MISSING_BY_SET_DIFFERENCE`** | **0** | **0** |
| **`MISSING_AT_LINK`** (ld.lld's own report) | **0** | **0** |
| `SLATE_LINK_EXIT` | 0 | 0 |

The jump from 363 to 478 is the 52 extra C extension modules arriving, not a
regression: those 115 symbols were always going to be needed by an interpreter
that can `import struct`. Stage 1's 363 was an honest measurement of the wrong
binary.

**The trap, recorded because the first run of this script fell into it.**
`nm --undefined-only` on a *static archive* reports every member's undefined
symbols — and a static library is overwhelmingly self-referential. `obmalloc.o`
calls `PyErr_NoMemory`, which is undefined *in obmalloc.o* and defined in
`errors.o` two members later. So the raw 2756 is "external references made by
any member", not "symbols the archive cannot satisfy". Differencing it straight
against `libc.a` produced **1875 "missing" symbols** — a figure that is 99%
CPython's own API (`PyAST_Check`, `PyArg_ParseTuple`, `PyBytes_Type`, …) and
says nothing whatever about our libc. It looked like a measurement and was not
one. Subtracting the archive's own definitions is what makes the number mean
what its name claims.

The two methods are worth keeping side by side because they bound the answer
from opposite directions: the linker only reports symbols on paths it actually
pulled in (a lower bound on what a *fuller* CPython would need), while the set
difference covers every member (an upper bound). Here they coincide.

## How much of the archive a run actually reads

`requests/b-a-cpython-path-z-self-test.md` originally worried, in prose, that
CPython "opens a 20 MB zip and reads its central directory". `pymeasure.py` and
`pyworkload.py` replace that with a count of bytes, taken under the musl control
interpreter against the identical archive:

| | bytes | note |
|---|---:|---|
| Central directory | 66,083 | read once, 1,034 entries, at the *end* of the file |
| `Py_Initialize` members | 20,372 | 3 members, all `encodings` — the part that must work before `main()` |
| The Path-Z rung's imports | 409,311 | 19 more members: `json`, `base64`, `struct` and their closure (`re`, `enum`, `collections`, `functools`, …) |
| **Total** | **495,766** | **2.42% of the 20,498,464-byte archive** |

Under half a megabyte across ~22 members, plus one 66 KB read near EOF. If the
rung ever hangs, bulk throughput is not the suspect: a seek, a short read near
the end of the file, or an `mmap` of a large file that is mostly never touched
are. That is a much narrower thing to look at than "it reads 20 MB".

**Two traps, both of which I fell into first.**

1. **Snapshot `sys.modules` before the measuring script imports anything of its
   own.** The first version did `import zipfile` at the top and *then* reported
   the startup set, counting zipfile's dependency closure — pathlib, urllib,
   ipaddress, shutil, threading — as startup work, inflating the answer ~2x.
   `sys` is the only safe thing to touch above the snapshot: it is a builtin and
   is already imported before any user code runs.

2. **`__file__` is not the authority on "came from the zip".**
   `importlib._bootstrap` and `importlib._bootstrap_external` are frozen into
   the interpreter — they have to be, they *are* the import system — yet CPython
   still points their `__file__` at where the source would have lived, inside
   the archive. Believing it credits them with **117,451 bytes of reads that
   never happen**, on the two modules guaranteed never to be read that way. The
   loader is the authority: a member is read from the archive only when its
   `__spec__.loader` is zipimport's. `pymeasure.py` prints that overcount
   underneath the real figure, so the error is visible rather than asserted.

The check that the filter is right is that the two scripts arrive at the startup
figure independently and agree: 3 members, 20,372 bytes.

## The nineteen symbols CPython cost us

Thirteen from stage 1 (`5531f816c`), six more from stage 2.

### Stage 1 — the interpreter core

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

Two carry documented degradations rather than lies, both written up in
`todo.txt`: `pthread_kill` on a *peer* thread currently delivers
process-directed (the kernel has no task-id-targeted signal syscall yet), and
`pthread_getcpuclockid` returns POSIX's `ENOENT` for a peer. `syscall()` is a
translation table over the dozen numbers CPython actually issues, not a trap
door — the libc-bypass numbers (`read`, `write`, `execve`, …) deliberately
return `ENOSYS`.

### Stage 2 — the static stdlib modules

| Symbol | Group | Where CPython uses it | Landed in |
|---|---|---|---|
| `getspnam` | shadow | `spwd.getspnam` | `posix/src/shadow.rs` (new) |
| `getspent` | shadow | `spwd.getspall` | `posix/src/shadow.rs` |
| `setspent` | shadow | `spwd.getspall` | `posix/src/shadow.rs` |
| `endspent` | shadow | `spwd.getspall` | `posix/src/shadow.rs` |
| `gethostbyname_r` | resolver | `socket.gethostbyname_ex` | `posix/src/socket.rs` |
| `gethostbyaddr_r` | resolver | `socket.gethostbyaddr` | `posix/src/socket.rs` |

`shadow.rs` is the other half of a marker `pwd.rs` had already planted: our
single `root` entry reports `pw_passwd = "x"`, which is the Unix convention for
"the hash is in the shadow database". Until now there was no shadow database
for it to point at. The entry it returns has `sp_pwdp = "!"` — a string `crypt`
cannot produce, so the database can never grant access, chosen over an empty
hash (which authenticates anybody) and over "no such user" (which would
contradict `getpwnam`).

## Why the standard library is one file

`<prefix>/lib/python312.zip` is the **first** entry of CPython's default
`sys.path`, and `zipimport` is frozen into the binary. This is not a trick
bolted on afterwards — it is the layout CPython already looks for before
anything else. The alternative is 569 files and ~12 MB of small reads our ext4
driver walks at every boot to deliver exactly the same modules.

Three decisions inside `stdlib.sh` that are measured rather than assumed:

- **`ZIP_STORED`, not `ZIP_DEFLATED`.** `zlib` has no target build, so
  `zipimport` cannot inflate a compressed member: the deflated variant of this
  archive fails at startup with `No module named 'zlib'` raised from inside
  `<frozen zipimport>`. Deflate would take the same content from 10.3 MB to
  2.6 MB — revisit if zlib is ever ported. `create-ext4-rootfs.sh` asserts
  zero deflated members at stage time, because a build host whose `zipfile`
  defaults changed would reintroduce this invisibly.
- **`--invalidation-mode unchecked-hash`.** A normal `.pyc` records the
  source's mtime and size and the loader re-validates against them; inside a
  zip that check is answered from the zip's directory entry, which is a
  different clock from the one that compiled the file. One skewed timestamp and
  every module silently falls back to re-parsing source on every import.
  `unchecked-hash` removes the question rather than trying to answer it, which
  is right for a stdlib shipped as one immutable file.
- **Both `.py` and `.pyc` go in.** `zipimport` tries `.pyc` first, so imports
  never parse source; the `.py` rides along so that tracebacks and
  `inspect.getsource` show real lines. On a system where a Python traceback may
  be the only debugging tool that works, that is worth its megabytes — and it
  is verified, not asserted: `stdlib.sh` provokes a `json` error and checks the
  traceback contains a source line.

`compileall -d /usr/local/lib/python312.zip` rewrites the path baked into each
code object. Without it every traceback on SlateOS would name
`/tmp/cpython-spike-<lane>/zipsrc/json/…`, a build-machine scratch directory
that does not exist on the target and never will.

Dropped from the tree, with reasons: `test/` (~30 MB, and `--disable-test-modules`
already removed its C half), `idlelib/` + `tkinter/` + `turtledemo/` (need Tcl/Tk),
`lib2to3/` (removed upstream in 3.13), `ensurepip/` (we build `--without-ensurepip`),
`venv/` (needs a package installer to be useful).

Deliberately **kept** even though their C extension is absent: `ssl`, `sqlite3`,
`bz2`, `lzma`. `import ssl` then fails with `No module named '_ssl'`, which
names the actual missing piece. Deleting the pure-Python half instead would
report `No module named 'ssl'` and send whoever hits it looking for the wrong
thing.

## What `stdlib.sh` proves, and where

The verification runs against the musl-linked control interpreter that `make`
builds from the *identical objects* with the identical `MODLIBS`. `run.sh`
proves CPython compiles; `slatelink.sh` proves it links against our `libc.a`;
this closes the remaining gap on the host, so that the only thing left untested
is SlateOS itself.

It runs from an isolated `$ISO` directory, **not** from the build tree: CPython's
`getpath` finds a *build tree* by the landmark `Lib/os.py` next to the
executable, so a probe run from the build directory answers a question nobody is
asking. That exact mistake produced a false pass the first time it was measured.

```
BUILTIN_MODULES= 83
ZIP_SOURCES=517 ZIP_BYTECODE=517
ZIP_BYTES=20498464
SLATE_PYTHON_OK 3.12.3
json   {'a': [1, 2, 3]}          re     ['slate', 'os']
b64    c2xhdGVvcw==             struct (7, 42)
ctr    [('i', 4), ('s', 4)]     sha256 8dc798e3a54a1d25
math   5.0                      dec    0.1428571428571428571428571429
date   2026-08-21               uni    LATIN SMALL LETTER E
xml    1                        path   /usr/local/lib/python312.zip
rand   17
SLATE_PYTHON_STDLIB_OK
File "/usr/local/lib/python312.zip/json/decoder.py", line 353, in raw_decode
HAS_SOURCE_LINE True
FILE_BACKED_AT_STARTUP_NOSITE= 8
```

That last line is the minimum SlateOS must serve to get an interpreter running:
under `-S`, exactly eight file-backed modules are imported before the prompt —
`_frozen_importlib_external`, `abc`, `codecs`, `encodings`,
`encodings.aliases`, `encodings.utf_8`, `io`, `zipimport`. All eight come out
of the zip.

(`-E` is *not* used for these probes and would be counterproductive: it discards
`PYTHONHOME`, which is the only thing telling the interpreter where the zip is.)

## What this spike is still *not*

`python-slateos.elf` has never executed on SlateOS. Everything above was
measured on the host, either by the linker or by a control interpreter built
from the same objects. The remaining unknowns are ours, not CPython's: whether
our ext4 driver, `mmap`, tty and `getrandom` behave the way `zipimport` and
`init_fs_encoding` assume.

Closing that needs a Path-Z self-test in `kernel/src/proc/spawn.rs`, which is
lane A's tree — filed as `requests/b-a-cpython-path-z-self-test.md`.
