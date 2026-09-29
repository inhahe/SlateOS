#!/usr/bin/env python3
"""Check our `#[repr(C)]` types against musl's headers, using musl as the oracle.

Why
---

On 2026-09-09 `Sigaction` was found to carry the *kernel's* field order under a
comment claiming it was glibc's.  Both layouts are 152 bytes, so the size test
that existed passed; the one field the two orders agree on -- `sa_handler` at
offset 0 -- is the one every test in the tree exercises, so handlers worked and
nothing looked wrong for months.  See `design-decisions.md` 1010.

**No Rust test could have caught it.**  Rust builds a `#[repr(C)]` struct by
field *name*, so it agrees with itself whichever order it declares.  The layout
only matters at a boundary with C, and the crate's own tests have none.  Worse,
the test that existed *certified* the bug, pinning
`offset_of!(Sigaction, sa_flags) == 8` under a comment reading "glibc x86_64".

Fixing three structs by hand was not a response to that.  This is.

How
---

Three programs, and no third copy of anything:

1. `posix::abi_layout` prints C.  The numbers come from `size_of` and
   `offset_of!` and are never typed out by a human.
2. `zig cc --target=x86_64-linux-musl` compiles that C against musl's own
   headers.  A `_Static_assert` that fails is a layout that disagrees.  musl is
   the toolchain every C port in this tree is already built with, so it is the
   right oracle rather than merely an available one.
3. This script also *derives* which types cross the C boundary -- every
   `#[repr(C)] pub struct` reachable as a pointer parameter of an exported
   `extern "C"` function -- and refuses a **new** one that has no entry in
   `abi_layout.rs`.  That is the part that stops the table rotting: coverage
   can only go up.

The third check is a ratchet, not a wall.  `BASELINE_UNCOVERED` below is the
set that was already uncovered when this gate was written; each name removed
from it is one more type checked, and nothing may be added.

The numbers, too
----------------

Since 2026-09-27 the gate checks the other half of what a C caller shares with
the library: its numbers -- every flag, error code and item number
(design-decisions.md section 1119).  An audit that day found 83 of the
library's constants disagreeing with musl's headers.  `nl_langinfo(CODESET)`
answered "Sun": the library's `CODESET` was glibc's item number, a program's
was musl's, and each side was right about its own header.  The audit was a
one-off; this keeps it true.  Three more programs, again with no number typed
by a human:

4. rustdoc's JSON output lists every public constant of `posix` with the
   value the compiler evaluated -- `1 << 4` arrives as `16i32` -- built for
   the SlateOS target, so a `cfg` there is honoured and no Python re-reads a
   Rust expression.
5. `zig cc -dM -E` over musl's headers lists the macros they define; a
   constant whose name is one of them is a number a caller shares with the
   library.  The kernel's headers (`linux/...`) answer, in a unit of their
   own, only for names musl's lack: in one unit `linux/limits.h` would
   redefine musl's `NGROUPS_MAX`.
6. A `_Static_assert` per pair compares the two in the bits both have -- the
   Rust type's width and the C expression's.  So musl's `(1<<31)`, an `int`
   sign-extended on its way to an `unsigned long`, agrees with a Rust `u64` of
   bit 31, and a signed Rust `WEOF` with musl's unsigned one.

The list is derived, never kept: a constant added tomorrow is checked from its
first push.  `KNOWN_DIFFERENT` holds section 1119's deliberate differences and
`NOT_CONSTANT_IN_MUSL` the macros that call a function; both are ratchets that
fail when an entry stops being true.

Usage
-----

    python scripts/check-libc-abi.py            # the gate
    python scripts/check-libc-abi.py --self-test
    python scripts/check-libc-abi.py --print-uncovered   # to shrink the baseline
    python scripts/check-libc-abi.py --list-constants    # every pair checked

Exit codes: 0 pass (or skipped for want of zig), 1 a layout, coverage or
constant failure, 2 the checker could not run.
"""

from __future__ import annotations

import argparse
import json
import os
import re
import shutil
import subprocess
import sys
import tempfile
from dataclasses import dataclass
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
POSIX_SRC = REPO / "posix" / "src"
ABI_LAYOUT = POSIX_SRC / "abi_layout.rs"

BEGIN = "===ABI-C-BEGIN==="
END = "===ABI-C-END==="

MUSL_TARGET = "x86_64-linux-musl"

# C types we have measured, found wrong, and not yet fixed.
#
# **This is the state the baseline below could not express.** `BASELINE_UNCOVERED`
# means "nobody has looked"; this means "somebody looked, it is broken, and here
# is where it is written down". Conflating the two loses the finding: a type
# taken out of `abi_layout.rs` because it fails is indistinguishable from one
# that was never added, and the measurement is thrown away.
#
# It is a **two-way** ratchet, which is the part that matters. A failure naming
# one of these is reported and does not refuse the push. A type in here that
# produces **no** failure is a *hard* error -- it means somebody fixed it and
# did not delete the line, and an exemption for a defect that no longer exists
# is how an exemption list starts covering real ones.
#
# Keyed by the C type name as it appears in the assertion message, because that
# is what the compiler gives back. Every entry must name a known-issues key.
KNOWN_MISMATCH: dict[str, str] = {
    # Empty, and that is a *state*, not a default: every type this table
    # has ever held was fixed within a day of being added to it. Add an
    # entry only with a known-issues key beside it, and delete it the moment
    # the type passes -- the gate refuses a push that leaves a stale one
    # here, which is the half of the ratchet that keeps the table honest.
}


# Types that cross the C boundary and are **not** checked yet.  A ratchet: this
# list may shrink and may never grow.  Removing a name means adding an `abi!`
# line in `posix/src/abi_layout.rs`, which is one line plus its field list.
#
# Three kinds are in here, and they are worth telling apart before picking one
# off:
#
#   * ordinary libc structs nobody has got to yet (`Passwd`, `Group`, `Tm`'s
#     neighbours) -- these are the point of the ratchet;
#   * opaque handles a C program only ever holds a pointer to (`Dir`, `Dbm`,
#     `RegexT`) -- lower stakes, but not zero: `regex_t` is declared *by value*
#     in C, so its size still matters;
#   * types a C program declares by value and whose size decides whether its
#     own stack object is big enough (`PthreadMutexT`, `PthreadAttrT`,
#     `CpuSetT`, `SemT`) -- **the highest stakes in the list**, because being
#     smaller than musl's means our writes land past the caller's object.
# Boundary types with **no external definition to check against**, and why.
#
# The third and last state. `KNOWN_MISMATCH` means "measured and wrong"; the
# baseline below means "nobody has looked"; this means "looked, and there is
# nothing to compare with". Leaving these in the baseline would be a slow lie --
# they would sit there for ever looking like work somebody had not got to.
#
# Two kinds, and the difference decides whether the entry can be checked:
#
#   * a type whose C header this toolchain does not ship. `ndbm.h`, `fts.h` and
#     `linux/sysctl.h` are absent from musl and from the uapi headers zig
#     bundles, so there is no oracle -- but the *claim* that they are absent is
#     itself checkable, and the second tuple element is what checks it. If the
#     header ever appears, the entry fails and the type gets a real check.
#   * a type that is ours and has no C counterpart at all: `None`. This cannot
#     be checked and is the only thing in this file taken on trust, which is why
#     it is kept to types the tree itself defines.
NO_ORACLE: dict[str, tuple[str, tuple[str, str] | None]] = {
    "Dbm": (
        "`DBM` lives in <ndbm.h>, which musl does not ship. Our ndbm is "
        "self-contained and the handle is opaque to callers.",
        ("DBM", "ndbm.h"),
    ),
    "Fts": (
        "`FTS` lives in <fts.h>, which musl does not ship -- a BSD interface "
        "glibc also carries. Opaque to callers.",
        ("FTS", "fts.h"),
    ),
    "FtsEnt": (
        "`FTSENT`, same header and reason as `Fts`. Not opaque -- callers read "
        "it -- so this is the entry here most worth revisiting if a definition "
        "ever becomes available.",
        ("FTSENT", "fts.h"),
    ),
    "SysctlArgs": (
        "`struct __sysctl_args` lived in <linux/sysctl.h>, removed from the "
        "kernel headers along with the syscall it served. Nothing defines it "
        "any more.",
        ("struct __sysctl_args", "linux/sysctl.h"),
    ),
    "CapEntryInfo": (
        "SlateOS's own: the record our capability-query syscall returns, with "
        "no counterpart in any C library or in the Linux uapi.",
        None,
    ),
}


# Empty. Every type crossing the C boundary is either checked against a header
# or listed in NO_ORACLE with the reason there is no header to check it against.
# The ratchet's remaining job is to refuse a *new* boundary type that is neither.
#
# `set()`, not `{}`: an empty brace literal is a **dict** whatever the
# annotation says, and the annotation is not checked at run time. The first
# version of this line was `BASELINE_UNCOVERED: set[str] = {}` and the gate
# died on `set - dict` the moment the baseline reached the state it exists to
# reach.
BASELINE_UNCOVERED: set[str] = set()


def find_zig() -> str | None:
    """The `zig` used for SlateOS C cross-compiles, or ``None``.

    Same discovery order as the fixtures' `build.py`, without importing fastpy:
    an explicit `FASTPY_ZIG`, then `PATH`.  Deliberately *not* the hard-coded
    `D:\\utils` path — a checker that reaches into one machine's layout stops
    being a checker on any other.
    """
    env = os.environ.get("FASTPY_ZIG")
    if env and Path(env).exists():
        return env
    return shutil.which("zig")


def emit_c() -> str:
    """Run the emitter test and return the C between the markers."""
    proc = subprocess.run(
        [
            "cargo", "test", "-q", "-p", "posix",
            "--target", "x86_64-pc-windows-gnu", "--lib", "--",
            "--nocapture", "--exact", "abi_layout::tests::emit_abi_asserts",
        ],
        cwd=REPO,
        capture_output=True,
        text=True,
        timeout=900,
        check=False,
    )
    if BEGIN not in proc.stdout or END not in proc.stdout:
        sys.stderr.write(
            "check-libc-abi: the emitter test did not print its markers.\n"
            f"exit={proc.returncode}\n--- stdout ---\n{proc.stdout[-4000:]}\n"
            f"--- stderr ---\n{proc.stderr[-4000:]}\n"
        )
        sys.exit(2)
    body = proc.stdout.split(BEGIN, 1)[1].split(END, 1)[0]
    return body.replace("\r\n", "\n").lstrip("\n")


# clang prints the message *bare* after the requirement, with no quotes:
#
#   error: static assertion failed due to requirement 'sizeof(x) == 8': x size
#   error: static assertion failed: x size
#
# The first pattern this file carried expected `"..."` and therefore never
# matched anything, so every assertion failure fell through to the
# "(compile error, not an assertion)" branch and arrived as one opaque blob.
# It still *refused*, which is why it looked like it worked -- the classifier
# was wrong and the verdict was right, which is the hardest kind of wrong to
# notice. `--self-test` now pins the parse against a real clang line.
ASSERT_MSG_RE = re.compile(
    r"static assertion failed(?: due to requirement '.*?')?:\s*(.+?)\s*$",
    re.M,
)


def compile_c(zig: str, src: str) -> tuple[list[str], str | None]:
    """Compile `src` against musl; return the failed assertions' messages.

    `-c` to an object in the temp directory, not `-fsyntax-only`.  A
    `_Static_assert` is decided by the front end, so syntax-only ought to be
    enough and is not: `zig cc -fsyntax-only` injects its own `-c`, warns that
    it is unused, and then fails with `error: FileNotFound` looking for an
    output it was told not to produce.  That failure only appears when the
    *clang* stage succeeds, so it masquerades as "the check ran and something
    is wrong" -- the worst shape a gate can fail in.  Writing the object costs
    a few hundred milliseconds and is thrown away with the directory.
    """
    with tempfile.TemporaryDirectory() as tmp:
        c = Path(tmp) / "abi.c"
        # newline="" so the probe is LF on every platform. cc accepts either,
        # but the gate grades the declaration, not the compiler.
        c.write_text(src, encoding="utf-8", newline="")
        proc = subprocess.run(
            [zig, "cc", f"--target={MUSL_TARGET}", "-c",
             str(c), "-o", str(Path(tmp) / "abi.o")],
            capture_output=True,
            text=True,
            timeout=600,
            check=False,
        )
        if proc.returncode == 0:
            return [], None
        msgs = ASSERT_MSG_RE.findall(proc.stderr)
        # Anything that is not an assertion -- a missing header, an unknown
        # field name, a zig that cannot run -- is reported separately and
        # *whole*, because it means the translation unit did not reach a
        # verdict on the assertions after it. Telling the two apart is what
        # lets `main` know whether "this type produced no failure" means
        # "it passes" or "we never got that far".
        n_err = proc.stderr.count("error:")
        other = None
        if n_err > len(msgs):
            other = proc.stderr.strip()
        return msgs, other


# Both macros. `abi_extensible!` was added after this pattern, and the four
# types that moved to it immediately reported as *uncovered* -- which would
# have had the ratchet demand entries for types it was already checking. A
# parser that knows one spelling of a thing with two is quietly wrong the day
# the second appears.
COVERED_RE = re.compile(
    r"abi(?:_extensible)?!\(\s*out,\s*hdrs,\s*crate::[\w:]*?(\w+)\s*,"
)


def covered_types() -> set[str]:
    """Rust type names that `abi_layout.rs` checks — parsed, not listed."""
    return set(COVERED_RE.findall(ABI_LAYOUT.read_text(encoding="utf-8")))


REPR_C_RE = re.compile(r"#\[repr\(C[^)]*\)\][\s\S]{0,600}?\bpub struct\s+(\w+)")
EXPORT_RE = re.compile(
    r'pub\s+(?:unsafe\s+)?extern\s+"C"\s+fn\s+\w+\s*\((.*?)\)\s*(?:->[^{;]*)?[{;]',
    re.S,
)
PTR_TY_RE = re.compile(r"\*\s*(?:const|mut)\s+([A-Z]\w*)")


def boundary_types() -> set[str]:
    """`#[repr(C)]` structs a C caller can hand to an exported function.

    Derived from the source both ways round — the set of `#[repr(C)] pub
    struct` names, intersected with the set of types appearing as a pointer
    parameter of a `pub extern "C" fn`.  Neither half is written down anywhere,
    so neither can go stale.

    The intersection is what makes this useful: the pointer-parameter set alone
    also catches typedefs like `WcharT` and `TimeT`, which have no layout to
    get wrong.
    """
    structs: set[str] = set()
    params: set[str] = set()
    for f in sorted(POSIX_SRC.rglob("*.rs")):
        s = f.read_text(encoding="utf-8", errors="surrogateescape")
        structs.update(REPR_C_RE.findall(s))
        for m in EXPORT_RE.finditer(s):
            params.update(PTR_TY_RE.findall(m.group(1)))
    return structs & params


def known_owner(msg: str) -> str | None:
    """Which `KNOWN_MISMATCH` entry, if any, owns this failure message.

    The assertion messages `abi_layout` emits open with the C type name --
    `"struct aiocb size"`, `"struct aiocb.aio_offset"` -- so the longest
    matching prefix is the owner. Longest, not first: `"struct stat"` is a
    prefix of `"struct statfs"`, and a shorter entry must not swallow a longer
    type's failures.
    """
    best: str | None = None
    for ty in KNOWN_MISMATCH:
        if msg.startswith(ty) and (best is None or len(ty) > len(best)):
            best = ty
    return best


def check_coverage(baseline: set[str] | None = None) -> list[str]:
    """Ratchet: a boundary type that is neither checked nor in the baseline.

    `baseline` is a parameter rather than a direct read of the constant so that
    `--self-test` can drive the *failure* path. A ratchet whose failure has
    never run is a ratchet nobody knows the sign of.
    """
    known = BASELINE_UNCOVERED if baseline is None else baseline
    return sorted(boundary_types() - covered_types() - known - set(NO_ORACLE))


def self_test() -> int:
    """Fixtures for this checker, per the project's gate rule.

    The one that matters is the second: a checker whose failure path has never
    run is a checker nobody knows the sign of.
    """
    failures: list[str] = []

    zig = find_zig()
    if zig is None:
        print("check-libc-abi --self-test: no zig; compile fixtures skipped")
    else:
        good = (
            "#include <signal.h>\n#include <stddef.h>\n"
            "_Static_assert(offsetof(struct sigaction, sa_mask) == 8, \"good\");\n"
        )
        got, other = compile_c(zig, good)
        if got or other:
            failures.append(f"a TRUE assertion was reported as failing: {got!r} {other!r}")

        bad = (
            "#include <signal.h>\n#include <stddef.h>\n"
            "_Static_assert(offsetof(struct sigaction, sa_flags) == 8, "
            "\"the 1010 bug\");\n"
        )
        got, other = compile_c(zig, bad)
        # Exact, not `in`: the message must arrive *parsed*, on its own. The
        # first version of this file matched on a quoted form clang does not
        # print, so every failure fell through as one opaque blob -- and an
        # `in` test against the whole stderr would have passed anyway. That is
        # the bug this line exists to stop coming back.
        if got != ["the 1010 bug"]:
            failures.append(
                "the 1010 defect was not parsed out as its own message: "
                f"{got!r} (other={other!r})"
            )
        if other is not None:
            failures.append(f"an assertion failure was also reported as a compile error: {other!r}")

        broken = "#include <no_such_header_xyzzy.h>\n"
        got, other = compile_c(zig, broken)
        if other is None:
            failures.append("a compile error was reported as a pass")

        # The NO_ORACLE probe, both ways. A type the toolchain genuinely cannot
        # find must stay quiet; one it can find must be reported, because that
        # is an exemption that has silently stopped being true.
        if stale_no_oracle(zig, {"Absent": ("x", ("struct nope_xyzzy", "no_such_hdr_xyzzy.h"))}):
            failures.append("NO_ORACLE reported a type whose header really is absent")
        if not stale_no_oracle(zig, {"Present": ("x", ("struct sigaction", "signal.h"))}):
            failures.append(
                "NO_ORACLE did NOT report an exemption whose C type is findable"
            )
        if stale_no_oracle(zig):
            failures.append(f"a live NO_ORACLE entry is stale: {stale_no_oracle(zig)}")

    covered = covered_types()
    if "Sigaction" not in covered:
        failures.append(f"the covered-type parser found no Sigaction: {sorted(covered)}")
    boundary = boundary_types()
    for expect in ("Sigaction", "Termios", "Stat"):
        if expect not in boundary:
            failures.append(f"the boundary-type derivation missed {expect}")
    if "WcharT" in boundary:
        failures.append("the derivation kept a typedef (WcharT); the intersection is wrong")

    # The ratchet, both ways. With the real baseline nothing should be new;
    # with a name taken out of it, that name must come back as new -- which is
    # what proves the gate would notice a type someone adds tomorrow.
    if check_coverage():
        failures.append(f"the live baseline is already out of date: {check_coverage()}")
    # The victim must be a name the ratchet would actually report -- one that
    # is in the baseline *and* not covered. Taking the first name in the
    # baseline is not enough: a name can be in both once its `abi!` line is
    # added and the baseline has not been regenerated, and then removing it
    # from the baseline changes nothing and this test fails for a reason that
    # has nothing to do with the ratchet.
    victim = next(iter(sorted(BASELINE_UNCOVERED - covered)), None)
    if victim is None:
        # Not a failure: it means every type that crosses the boundary is
        # checked, which is the goal. Say so rather than reporting it as one.
        print("check-libc-abi --self-test: baseline empty; coverage is complete")
    elif check_coverage(BASELINE_UNCOVERED - {victim}) != [victim]:
        failures.append(
            f"removing {victim} from the baseline did not make the ratchet report it"
        )

    # The known-mismatch matcher, including the prefix hazard that makes it
    # "longest match" rather than "first match".
    # Driven from a *probe* table rather than the live one. These fixtures are
    # about the matcher, not about which types happen to be broken today --
    # tying them to a live entry meant they failed the moment the table was
    # emptied, which is the one state it is trying hardest to reach.
    probe = dict(KNOWN_MISMATCH)
    try:
        KNOWN_MISMATCH.clear()
        KNOWN_MISMATCH.update({"struct probe": "p", "struct stat": "x", "struct statfs": "y"})
        if known_owner("struct probe size") != "struct probe":
            failures.append("known_owner did not match a size message")
        if known_owner("struct probe.some_field") != "struct probe":
            failures.append("known_owner did not match a field message")
        if known_owner("struct sigaction size") is not None:
            failures.append("known_owner claimed a type that is not listed")
        # Longest match, not first: `struct stat` is a prefix of `struct statfs`.
        if known_owner("struct statfs size") != "struct statfs":
            failures.append(
                "known_owner took the shorter prefix: `struct stat` swallowed "
                "`struct statfs`"
            )
    finally:
        KNOWN_MISMATCH.clear()
        KNOWN_MISMATCH.update(probe)

    failures.extend(constants_self_test())

    for f in failures:
        print(f"check-libc-abi --self-test: FAIL: {f}")
    if failures:
        return 1
    print("check-libc-abi --self-test: OK")
    return 0


def stale_no_oracle(zig: str, table: dict | None = None) -> list[str]:
    """`NO_ORACLE` entries whose C type the toolchain *can* now find.

    An exemption saying "there is nothing to compare with" stops being true the
    day the toolchain grows the header, and nothing else in this file would
    notice — the type would simply never be checked again, quietly, for ever.

    `table` is a parameter so `--self-test` can drive the failing direction. A
    check whose failure path has never run is a check nobody knows the sign of;
    that is the third time this file has needed the same sentence, which is why
    every table in it now takes one.
    """
    entries = NO_ORACLE if table is None else table
    out: list[str] = []
    for ty, (why, probe) in sorted(entries.items()):
        if probe is None:
            continue
        cty, hdr = probe
        src = (
            f"#define _GNU_SOURCE 1\n#include <{hdr}>\n#include <stddef.h>\n"
            f"size_t probe(void) {{ return sizeof({cty}); }}\n"
        )
        msgs, err = compile_c(zig, src)
        if not msgs and err is None:
            out.append(
                f"check-libc-abi: `{ty}` is listed in NO_ORACLE as having no C\n"
                f"  definition, and `{cty}` from <{hdr}> now compiles. Give it a\n"
                f"  real `abi!` entry and delete the exemption. Recorded as:\n"
                f"  {why}"
            )
    return out


# ===========================================================================
# The numbers: every public constant whose name musl's headers define
# (design-decisions.md section 1119).  See "The numbers, too" above.
# ===========================================================================

POSIX_DIR = REPO / "posix"
LIBC_SPEC = POSIX_DIR / "x86_64-slateos-libc.json"

# The headers whose macros are the oracle: the 2026-09-27 audit's list, which
# is every header a module of the library answers for.  The headers
# `abi_layout.rs` names are added at run time (`constant_headers()`), so a
# header the layout half learns about reaches this half too.
CONSTANT_HEADERS = """
stdio.h stdlib.h stddef.h unistd.h fcntl.h errno.h signal.h termios.h
sys/ioctl.h sys/socket.h netinet/in.h netinet/tcp.h netinet/udp.h arpa/inet.h
netdb.h sys/stat.h sys/mman.h sys/wait.h sys/resource.h sys/time.h time.h
sys/select.h poll.h sys/epoll.h sys/eventfd.h sys/signalfd.h sys/timerfd.h
sys/inotify.h sys/prctl.h sys/ptrace.h sys/reboot.h sys/personality.h
sys/random.h sys/xattr.h sys/statvfs.h sys/vfs.h sys/mount.h sys/swap.h
sys/sysinfo.h sys/utsname.h sys/uio.h sys/un.h sys/sendfile.h sys/file.h
sys/klog.h sys/quota.h sys/sem.h sys/shm.h sys/msg.h sys/ipc.h mqueue.h
semaphore.h pthread.h sched.h spawn.h dlfcn.h locale.h langinfo.h iconv.h
wchar.h wctype.h ctype.h limits.h float.h stdint.h fnmatch.h glob.h regex.h
wordexp.h ftw.h getopt.h syslog.h pwd.h grp.h shadow.h utmpx.h utmp.h
paths.h sysexits.h err.h search.h aio.h ifaddrs.h net/if.h sys/auxv.h
elf.h link.h sys/fsuid.h sys/timex.h sys/times.h utime.h sys/sysmacros.h
stdio_ext.h malloc.h sys/membarrier.h
""".split()

# Section 1119's table: where this library's number is deliberately not
# musl's.  Keyed by name; every module's constant of that name is exempt.
# Each entry must keep differing -- `check_constants` refuses one that agrees.
KNOWN_DIFFERENT: dict[str, str] = {
    "FD_SETSIZE":
        "256 here, 1024 in musl: a policy limit, the fd table's size; the "
        "fd_set layout is musl's 1024 bits (FD_SET_BITS, section 1011)",
    "PAGE_SIZE":
        "16384 here, 4096 in musl: this kernel's pages are 16 KiB; a port that "
        "uses the macro instead of sysconf(_SC_PAGESIZE) is wrong here whatever "
        "the library says",
    "SHMLBA":
        "16384 here, 4096 in musl: SysV segments attach on this kernel's 16 KiB "
        "pages, as PAGE_SIZE",
    "ARG_MAX":
        "2 MiB here, 128 KiB in musl: the kernel's real limit (Linux's); musl's "
        "header states its own",
    "HOST_NAME_MAX":
        "64 here, 255 in musl: the kernel's real limit (Linux's)",
    "NGROUPS_MAX":
        "65536 here, 32 in musl: the kernel's real limit (Linux's)",
    "MAXQUOTAS":
        "3 here, 2 in musl: the kernel has project quotas; musl's header "
        "predates them",
    "O_ACCMODE":
        "3 here; musl folds O_SEARCH into the access mode (3|O_PATH): the "
        "library's own masking is glibc's",
}

# musl macros that call a function, which no `_Static_assert` can evaluate.
# The function is this library's, which is what makes the value right: name it
# and say what it answers.  Each entry must stay a call.
NOT_CONSTANT_IN_MUSL: dict[str, str] = {
    "SIGRTMIN":
        "musl: (__libc_current_sigrtmin()), which is this library's and "
        "answers 32 (section 1119)",
    "SIGRTMAX":
        "musl: (__libc_current_sigrtmax()), which is this library's and "
        "answers 64",
    "MB_CUR_MAX":
        "musl: (__ctype_get_mb_cur_max()), which is this library's and "
        "answers 4 (section 1119)",
}

INT_BITS = {
    "i8": 8, "u8": 8, "i16": 16, "u16": 16, "i32": 32, "u32": 32,
    "i64": 64, "u64": 64, "isize": 64, "usize": 64,
}
FLOATS = ("f32", "f64")
VALUE_RE = re.compile(
    r"^(?P<num>-?[0-9_]+(?:\.[0-9_]+)?(?:[eE][+-]?[0-9]+)?)"
    r"(?P<ty>i8|i16|i32|i64|isize|u8|u16|u32|u64|usize|f32|f64)$"
)


@dataclass(frozen=True)
class Const:
    """One public numeric constant of the crate."""

    module: str  # "socket", "stdio::ext", ...
    name: str
    ty: str      # the Rust primitive
    value: int | float

    @property
    def label(self) -> str:
        return f"{self.module}::{self.name}" if self.module else self.name


def parse_value(text: str) -> tuple[int | float, str] | None:
    """rustdoc's rendering of an evaluated constant --
    `18_446_744_073_709_551_615u64`, `-5i32`, `1.5f64` -- as (value, type);
    None if it is not a number."""
    m = VALUE_RE.match(text.strip())
    if m is None:
        return None
    num, ty = m.group("num").replace("_", ""), m.group("ty")
    if ty in FLOATS:
        return float(num), ty
    if "." in num or "e" in num.lower():
        return None
    return int(num), ty


def constants_from_rustdoc(doc: dict) -> tuple[list[Const], list[str]]:
    """Every local, public, numeric constant in a rustdoc JSON document, and
    the labels of those whose value could not be read."""
    paths = doc.get("paths", {})
    out: list[Const] = []
    unread: list[str] = []
    for iid, item in doc.get("index", {}).items():
        if item.get("crate_id", 0) != 0:
            continue
        inner = item.get("inner") or {}
        c = inner.get("constant")
        if not isinstance(c, dict):
            continue
        ty = (c.get("type") or {}).get("primitive")
        if ty not in INT_BITS and ty not in FLOATS:
            continue
        name = item.get("name") or ""
        path = (paths.get(iid) or {}).get("path") or []
        module = "::".join(path[1:-1]) if len(path) > 2 else ""
        raw = (c.get("const") or {}).get("value")
        parsed = parse_value(raw) if isinstance(raw, str) else None
        if parsed is None:
            unread.append(f"{module}::{name}" if module else name)
            continue
        out.append(Const(module, name, ty, parsed[0]))
    out.sort(key=lambda k: (k.name, k.module))
    return out, sorted(unread)


# The bits a C expression has: all 64 for a `long` or wider, else its width.
BITS_MACRO = ("#define SLATE_BITS(e) "
              "(sizeof(e) >= 8 ? ~0ULL : (1ULL << (8 * sizeof(e))) - 1ULL)")


def constant_assert(c: Const) -> str:
    """The `_Static_assert` comparing musl's value of `c.name` with `c`'s, in
    the bits both have: the Rust type's width and the C expression's."""
    label = c.label.replace("\\", "\\\\").replace('"', '\\"')
    n = c.name
    if c.ty in FLOATS:
        return f'_Static_assert((double)({n}) == {float(c.value)!r}, "{label}");'
    rmask = (1 << INT_BITS[c.ty]) - 1
    want = int(c.value) & rmask
    return (f"_Static_assert(((unsigned long long)({n}) & SLATE_BITS({n}) & {rmask:#x}ULL)"
            f' == ({want:#x}ULL & SLATE_BITS({n})), "{label}");')


def macro_names(define_dump: str) -> set[str]:
    """The object-like macros in `cc -dM -E` output (a function-like one is
    not a number a caller passes)."""
    names = set()
    for line in define_dump.splitlines():
        m = re.match(r"#define ([A-Za-z_][A-Za-z0-9_]*)(\(|\s|$)", line)
        if m and m.group(2) != "(":
            names.add(m.group(1))
    return names


def constants_source(includes: list[str], shared: list[Const]) -> tuple[str, dict[int, Const]]:
    """The translation unit, and which line asserts which constant."""
    lines = ["#define _GNU_SOURCE 1"] + [f"#include <{h}>" for h in includes] + [BITS_MACRO]
    at: dict[int, Const] = {}
    for c in shared:
        lines.append(constant_assert(c))
        at[len(lines)] = c
    return "\n".join(lines) + "\n", at


CONST_ERROR_RE = re.compile(r"^consts\.c:(\d+):\d+: (?:fatal )?error: (.*)$", re.M)
# clang follows a failed comparison with the values it compared, musl's first:
#   consts.c:6:86: note: expression evaluates to '10000 == 238328'
CONST_NOTE_RE = re.compile(
    r"^consts\.c:(\d+):\d+: note: expression evaluates to '(.*) == (.*)'$", re.M)
NOT_ICE = "not an integral constant expression"


@dataclass
class ConstVerdict:
    mismatched: dict[str, list[tuple[Const, str]]]  # name -> [(constant, musl's value)]
    not_constant: dict[str, list[Const]]
    other: list[str]  # errors that are neither, whole


def classify_constants(stderr: str, at: dict[int, Const]) -> ConstVerdict:
    """Sort the compiler's errors by what they say about each constant."""
    v = ConstVerdict({}, {}, [])
    compared = {int(n.group(1)): n.group(2) for n in CONST_NOTE_RE.finditer(stderr)}
    for m in CONST_ERROR_RE.finditer(stderr):
        line, msg = int(m.group(1)), m.group(2)
        c = at.get(line)
        if c is None:
            v.other.append(m.group(0))
        elif msg.startswith("static assertion failed"):
            musl = compared.get(line, "(clang did not say)")
            v.mismatched.setdefault(c.name, []).append((c, musl))
        elif NOT_ICE in msg:
            v.not_constant.setdefault(c.name, []).append(c)
        else:
            v.other.append(m.group(0))
    # An error this did not read -- one inside a header has another file's
    # name -- still means the unit reached no verdict after it.
    unread = stderr.count("error:") - len(CONST_ERROR_RE.findall(stderr))
    if unread > 0:
        v.other.append(f"{unread} error(s) outside consts.c:\n{stderr[-3000:]}")
    return v


def constant_headers() -> tuple[list[str], list[str]]:
    """musl's own headers -- the audit's, and the ones `abi_layout.rs` names --
    and, apart, the kernel's (`linux/`, `asm/`) that `abi_layout.rs` names.

    Apart because a kernel header can redefine a musl name: `linux/limits.h`
    says `NGROUPS_MAX` is 65536 where musl's `limits.h` says 32, and in one
    translation unit whichever comes last wins.  A name musl's own headers
    define is judged by them; a kernel header answers only for the names
    musl's do not have (`PERF_EVENT_IOC_*`, `LANDLOCK_*`, ...)."""
    libc, kernel = list(CONSTANT_HEADERS), []
    if ABI_LAYOUT.exists():
        for line in ABI_LAYOUT.read_text(encoding="utf-8").splitlines():
            # Code only: the module's docs show the macro's shape with a
            # placeholder "header.h".
            if line.lstrip().startswith("//"):
                continue
            for h in re.findall(r'"([a-z0-9_/]+\.h)"', line):
                side = kernel if h.startswith(("linux/", "asm/", "asm-generic/")) else libc
                if h not in side:
                    side.append(h)
    return libc, sorted(kernel)


def target_directory() -> Path | None:
    """The target directory cargo builds `posix` into."""
    env = os.environ.get("CARGO_TARGET_DIR")
    if env:
        return Path(env) if Path(env).is_absolute() else POSIX_DIR / env
    try:
        proc = subprocess.run(["cargo", "metadata", "--format-version", "1", "--no-deps"],
                              cwd=POSIX_DIR, capture_output=True, text=True, timeout=300,
                              check=False)
    except (OSError, subprocess.TimeoutExpired):
        return None
    if proc.returncode != 0:
        return None
    try:
        return Path(json.loads(proc.stdout)["target_directory"])
    except (ValueError, KeyError, TypeError):
        return None


def run_rustdoc() -> tuple[dict | None, str]:
    """rustdoc's JSON for `posix`, built for the SlateOS target."""
    env = dict(os.environ, CARGO_UNSTABLE_JSON_TARGET_SPEC="true")
    cmd = ["cargo", "+nightly", "rustdoc", "--lib", "--release",
           "--target", str(LIBC_SPEC), "-Zbuild-std=core,compiler_builtins",
           "--", "-Z", "unstable-options", "--output-format", "json"]
    try:
        proc = subprocess.run(cmd, cwd=POSIX_DIR, env=env, capture_output=True,
                              text=True, timeout=1800, check=False)
    except (OSError, subprocess.TimeoutExpired) as e:
        return None, f"rustdoc did not run: {e}"
    if proc.returncode != 0:
        return None, f"rustdoc failed (exit {proc.returncode}):\n{proc.stderr[-4000:]}"
    # Cargo names no output file for `--output-format json` (it prints a
    # "Generated" line for HTML only), so the path is built the way cargo
    # builds it: the target directory, the target's name, `doc`.
    tdir = target_directory()
    if tdir is None:
        return None, "cargo metadata did not name a target directory"
    out = tdir / LIBC_SPEC.stem / "doc" / "posix.json"
    if not out.exists():
        return None, f"rustdoc succeeded and {out} is not there:\n{proc.stderr[-2000:]}"
    return json.loads(out.read_text(encoding="utf-8")), ""


def run_zig_unit(zig: str, args: list[str], src: str) -> subprocess.CompletedProcess:
    """`zig cc` over `src`, from a temporary directory."""
    with tempfile.TemporaryDirectory() as tmp:
        # newline="" keeps the unit LF on every platform, and the relative name
        # keeps a drive letter's colon out of the diagnostics read above.
        (Path(tmp) / "consts.c").write_text(src, encoding="utf-8", newline="")
        return subprocess.run([zig, "cc", f"--target={MUSL_TARGET}", *args, "consts.c"],
                              cwd=tmp, capture_output=True, text=True, timeout=600,
                              check=False)


@dataclass
class Numbers:
    """What `check_constants` found."""

    problems: int
    summary: str
    skipped: bool = False
    broken: bool = False  # the tooling failed, so no verdict


def check_constants(zig: str, list_pairs: bool = False) -> Numbers:
    """The numbers half: every constant a C caller shares, against musl."""
    doc, err = run_rustdoc()
    if doc is None:
        if "toolchain 'nightly" in err or "is not installed" in err:
            print(f"check-libc-abi: SKIPPED the constants -- no nightly rustdoc:\n{err}")
            return Numbers(0, "constants skipped", skipped=True)
        print(f"check-libc-abi: the constants could not be read: {err}")
        return Numbers(1, "", broken=True)
    consts, unread = constants_from_rustdoc(doc)

    libc_h, kernel_h = constant_headers()
    tables: dict[str, set[str]] = {}
    for side, includes in (("musl", libc_h), ("kernel", kernel_h)):
        if not includes:
            tables[side] = set()
            continue
        dump = run_zig_unit(zig, ["-dM", "-E"], "#define _GNU_SOURCE 1\n"
                            + "".join(f"#include <{h}>\n" for h in includes))
        if dump.returncode != 0:
            print(f"check-libc-abi: the {side} headers did not preprocess:\n"
                  f"{dump.stderr[-4000:]}")
            return Numbers(1, "", broken=True)
        tables[side] = macro_names(dump.stdout)
    in_libc = [c for c in consts if c.name in tables["musl"]]
    in_kernel = [c for c in consts
                 if c.name in tables["kernel"] and c.name not in tables["musl"]]
    shared = in_libc + in_kernel
    if list_pairs:
        for c in shared:
            print(f"{c.label} = {c.value} ({c.ty})")

    verdict = ConstVerdict({}, {}, [])
    for includes, group in ((libc_h, in_libc), (kernel_h, in_kernel)):
        if not group:
            continue
        src, at = constants_source(includes, group)
        proc = run_zig_unit(zig, ["-ferror-limit=0", "-c", "-o", "consts.o"], src)
        if proc.returncode == 0:
            continue
        v = classify_constants(proc.stderr, at)
        if not (v.mismatched or v.not_constant or v.other):
            print("check-libc-abi: the constants' C did not compile, and said nothing "
                  f"parseable:\n{proc.stderr[-4000:]}")
            return Numbers(1, "", broken=True)
        for name, hits in v.mismatched.items():
            verdict.mismatched.setdefault(name, []).extend(hits)
        for name, hits in v.not_constant.items():
            verdict.not_constant.setdefault(name, []).extend(hits)
        verdict.other.extend(v.other)

    problems = 0
    for label in unread:
        print(f"check-libc-abi: rustdoc gave the constant `{label}` no value it could read")
        problems += 1
    for name, hits in sorted(verdict.mismatched.items()):
        if name in KNOWN_DIFFERENT:
            continue
        for c, musl in hits:
            print(f"check-libc-abi: CONSTANT MISMATCH {c.label}: here {c.value} ({c.ty}), "
                  f"musl's header {musl} (compared in the bits both have)")
            problems += 1
    for name, hits in sorted(verdict.not_constant.items()):
        if name not in NOT_CONSTANT_IN_MUSL:
            labels = ", ".join(c.label for c in hits)
            print(f"check-libc-abi: musl's `{name}` is not a constant, so {labels} cannot "
                  "be checked. Say why its value is right in NOT_CONSTANT_IN_MUSL.")
            problems += 1
    shared_names = {c.name for c in shared}
    for name, why in sorted(KNOWN_DIFFERENT.items()):
        if name not in shared_names:
            print(f"check-libc-abi: KNOWN_DIFFERENT names `{name}`, which no constant "
                  "shares with musl any more. Delete the entry.")
            problems += 1
        elif name not in verdict.mismatched:
            print(f"check-libc-abi: `{name}` is KNOWN_DIFFERENT and now agrees with musl. "
                  f"Delete the entry; it read:\n  {why}")
            problems += 1
    for name, why in sorted(NOT_CONSTANT_IN_MUSL.items()):
        if name not in shared_names:
            print(f"check-libc-abi: NOT_CONSTANT_IN_MUSL names `{name}`, which no "
                  "constant shares with musl any more. Delete the entry.")
            problems += 1
        elif name not in verdict.not_constant:
            print(f"check-libc-abi: musl's `{name}` is a constant now, so it is checked. "
                  f"Delete its NOT_CONSTANT_IN_MUSL entry; it read:\n  {why}")
            problems += 1
    for line in verdict.other:
        print(f"check-libc-abi: the constants' C did not compile: {line}")
    if verdict.other:
        # A unit that failed for another reason reached no verdict on the
        # assertions after the failure, so a missing mismatch means nothing.
        return Numbers(problems + len(verdict.other), "", broken=True)
    return Numbers(
        problems,
        f"{len(shared)} constants checked against musl, {len(KNOWN_DIFFERENT)} known "
        f"different, {len(NOT_CONSTANT_IN_MUSL)} not constants in musl",
    )


def constants_self_test() -> list[str]:
    """The numbers half's parsing and verdicts, against fixtures: no cargo,
    no zig."""
    failures: list[str] = []

    def check(what: str, ok: bool) -> None:
        if not ok:
            failures.append(what)

    check("a u64 with separators",
          parse_value("18_446_744_073_709_551_615u64") == (2**64 - 1, "u64"))
    check("a negative i32", parse_value("-5i32") == (-5, "i32"))
    check("an f64", parse_value("1.5f64") == (1.5, "f64"))
    check("a string is not a number", parse_value('"x"') is None)
    check("a float on an integer type is refused", parse_value("1.5i32") is None)

    doc = {
        "index": {
            "1": {"crate_id": 0, "name": "AF_INET",
                  "inner": {"constant": {"type": {"primitive": "i32"},
                                         "const": {"value": "2i32"}}}},
            "2": {"crate_id": 0, "name": "WEOF",
                  "inner": {"constant": {"type": {"primitive": "i32"},
                                         "const": {"value": "-1i32"}}}},
            "3": {"crate_id": 0, "name": "PATH",
                  "inner": {"constant": {"type": {"borrowed_ref": {}},
                                         "const": {"value": None}}}},
            "4": {"crate_id": 1, "name": "FOREIGN",
                  "inner": {"constant": {"type": {"primitive": "i32"},
                                         "const": {"value": "1i32"}}}},
            "5": {"crate_id": 0, "name": "ODD",
                  "inner": {"constant": {"type": {"primitive": "u8"},
                                         "const": {"value": "?"}}}},
        },
        "paths": {"1": {"path": ["posix", "socket", "AF_INET"]},
                  "2": {"path": ["posix", "wchar", "WEOF"]}},
    }
    consts, unread = constants_from_rustdoc(doc)
    check("numeric local constants are read, others are not",
          [c.label for c in consts] == ["socket::AF_INET", "wchar::WEOF"])
    check("a numeric constant with no readable value is reported", unread == ["ODD"])

    weof = Const("wchar", "WEOF", "i32", -1)
    check("a signed constant is compared as its bits, in the width both have",
          "& SLATE_BITS(WEOF) & 0xffffffffULL) == (0xffffffffULL & SLATE_BITS(WEOF))"
          in constant_assert(weof))
    check("a float is compared as a double",
          constant_assert(Const("m", "M_E", "f64", 2.5))
          .startswith("_Static_assert((double)(M_E) == 2.5,"))

    dump = "#define A 1\n#define F(x) x\n#define B\n#define _C (2)\n"
    check("object-like macros only", macro_names(dump) == {"A", "B", "_C"})

    src, at = constants_source(["fcntl.h"], [Const("fcntl", "O_CREAT", "i32", 65),
                                             Const("signal", "SIGRTMIN", "i32", 32)])
    check("the width macro precedes the assertions", BITS_MACRO in src.splitlines()[2])
    check("each assertion's line is known", at.get(4) is not None and at[4].name == "O_CREAT")
    stderr = (
        "consts.c:4:16: error: static assertion failed due to requirement "
        "'((unsigned long long)(0100) & ...) == (0x41ULL & ...)': fcntl::O_CREAT\n"
        "consts.c:4:86: note: expression evaluates to '64 == 65'\n"
        "consts.c:5:38: error: static assertion expression is not an integral "
        "constant expression\n"
        "consts.c:1:10: fatal error: 'nosuch.h' file not found\n"
    )
    v = classify_constants(stderr, at)
    check("a failed assertion is a mismatch, with musl's value from clang's note",
          [(c.label, m) for c, m in v.mismatched.get("O_CREAT", [])]
          == [("fcntl::O_CREAT", "64")])
    check("a call is not a constant",
          [c.label for c in v.not_constant.get("SIGRTMIN", [])] == ["signal::SIGRTMIN"])
    check("anything else is reported whole", len(v.other) == 1 and "nosuch.h" in v.other[0])
    v = classify_constants("/x/include/bits/foo.h:9:1: error: unknown type name 'bar'\n", at)
    check("an error in a header is reported too",
          len(v.other) == 1 and "outside" in v.other[0])
    return [f"constants: {f}" for f in failures]


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--self-test", "--selftest", action="store_true", dest="selftest")
    ap.add_argument("--print-uncovered", action="store_true")
    ap.add_argument("--list-constants", action="store_true",
                    help="print every constant the numbers half checks, with its value")
    args = ap.parse_args()

    if args.selftest:
        return self_test()

    if args.print_uncovered:
        # NO_ORACLE is subtracted here too. It was not, and regenerating
        # the baseline from this put the five accounted-for types straight
        # back into it -- reintroducing the very conflation between
        # "unexamined" and "accounted for" that NO_ORACLE exists to end.
        for t in sorted(boundary_types() - covered_types() - set(NO_ORACLE)):
            print(f'    "{t}",')
        return 0

    problems = 0

    new = check_coverage()
    if new:
        print("check-libc-abi: these types cross the C boundary and are unchecked:")
        for t in new:
            print(f"  {t}")
        print(
            "\nAdd an `abi!` line for each in posix/src/abi_layout.rs. If one is a\n"
            "*kernel* wire format rather than a C library type, say so where it is\n"
            "defined and name the function that translates it -- do not add it to\n"
            "BASELINE_UNCOVERED, which is a ratchet and may only shrink."
        )
        problems += len(new)

    zig = find_zig()
    if zig is None:
        print(
            "check-libc-abi: SKIPPED the layout check -- no `zig` on PATH and no\n"
            "FASTPY_ZIG. This is the same compiler the ctest fixtures need, so a\n"
            "machine that can build the image can run this."
        )
        # 3, not 0, when there is nothing else to report: `run_checker` maps 3
        # to the SKIPPED tally. Returning 0 meant every push touching posix/src
        # since this gate was written reported it as having RUN -- the gate that
        # found five real ABI bugs in its first hour, including the transposed
        # `struct addrinfo` that handed `connect()` a hostname string. A finding
        # still outranks a skip, so `problems` keeps its 1.
        # See the "Exit 3" section of scripts/run-checker.sh.
        return 1 if problems else 3

    for msg in stale_no_oracle(zig):
        print(msg)
        problems += 1

    failed, other = compile_c(zig, emit_c())
    known_seen: set[str] = set()
    for msg in failed:
        owner = known_owner(msg)
        if owner is None:
            print(f"check-libc-abi: LAYOUT MISMATCH: {msg}")
            problems += 1
        else:
            known_seen.add(owner)
            print(f"check-libc-abi: known mismatch, not refusing: {msg}")

    if other is not None:
        print(f"check-libc-abi: the C did not compile:\n{other}")
        problems += 1
        print(
            "\ncheck-libc-abi: skipping the known-mismatch sweep -- a translation\n"
            "unit that did not compile reached no verdict on the assertions after\n"
            "the error, so 'this type produced no failure' would mean nothing."
        )
    else:
        # The other direction, and the reason this is a ratchet rather than a
        # list of excuses: an exemption for a defect that no longer exists is
        # how an exemption list starts covering real ones.
        for ty, why in sorted(KNOWN_MISMATCH.items()):
            if ty not in known_seen:
                print(
                    f"check-libc-abi: `{ty}` is listed as a KNOWN_MISMATCH and "
                    f"now passes.\n  Delete its entry from KNOWN_MISMATCH in "
                    f"this file. It was recorded as:\n  {why}"
                )
                problems += 1

    if known_seen:
        print(
            f"\ncheck-libc-abi: {len(known_seen)} type(s) are known-bad and "
            "recorded in known-issues.md; they do not refuse the push."
        )

    numbers = check_constants(zig, list_pairs=args.list_constants)
    problems += numbers.problems

    if numbers.broken:
        print("\ncheck-libc-abi: the constants half reached no verdict. "
              "See design-decisions.md 1119.")
        return 2

    if problems:
        print(f"\ncheck-libc-abi: {problems} problem(s). See design-decisions.md 1010 "
              "(layouts) and 1119 (constants).")
        return 1

    n = len(covered_types())
    if known_seen:
        # Not "0 mismatches": there are five, they are written down, and a
        # summary line that says zero while the lines above it say otherwise
        # trains the reader to stop reading the lines above it.
        print(
            f"check-libc-abi: OK ({n} types checked against musl; "
            f"{len(known_seen)} known-bad and recorded, 0 new; {numbers.summary})"
        )
    else:
        print(f"check-libc-abi: OK ({n} types checked against musl, 0 mismatches; "
              f"{numbers.summary})")
    return 3 if numbers.skipped else 0


if __name__ == "__main__":
    sys.exit(main())
