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

Usage
-----

    python scripts/check-libc-abi.py            # the gate
    python scripts/check-libc-abi.py --self-test
    python scripts/check-libc-abi.py --print-uncovered   # to shrink the baseline

Exit codes: 0 pass (or skipped for want of zig), 1 a layout or coverage
failure, 2 the checker could not run.
"""

from __future__ import annotations

import argparse
import os
import re
import shutil
import subprocess
import sys
import tempfile
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
KNOWN_MISMATCH = {
    "struct aiocb": "B-AIOCB-FIELD-ORDER-IS-NOT-MUSLS -- musl is 136 bytes and "
                    "orders the fields fildes/lio_opcode/reqprio/buf/nbytes/"
                    "sigevent/offset; ours is 168 and starts fildes/offset/buf",
    "struct sysinfo": "B-SYSINFO-IS-368-BYTES-AGAINST-MUSLS-112 -- every counter "
                      "widened to u64 where musl uses long plus a __f pad",
    "struct utmpx": "B-UTMPX-IS-400-BYTES-AGAINST-MUSLS-384 -- ut_tv lands at 344 "
                    "rather than 340, so everything after it is shifted",
    "struct msqid_ds": "B-SYSV-IPC-DS-STRUCTS-FLATTEN-IPC-PERM-AND-COME-UP-SHORT "
                       "-- 80 against musl's 120",
    "struct shmid_ds": "B-SYSV-IPC-DS-STRUCTS-FLATTEN-IPC-PERM-AND-COME-UP-SHORT "
                       "-- 72 against musl's 112",
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
BASELINE_UNCOVERED = {
    "CapEntryInfo",
    "CapUserData",
    "CapUserHeader",
    "CloneArgs",
    "Dbm",
    "FileHandle",
    "Fts",
    "FtsEnt",
    "IoEvent",
    "IoUringParams",
    "Iocb",
    "LandlockRulesetAttr",
    "OpenHow",
    "PerfEventAttr",
    "Statx",
    "SysctlArgs",
    "VaList",
}


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
        c.write_text(src, encoding="utf-8")
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


COVERED_RE = re.compile(r"abi!\(\s*out,\s*hdrs,\s*crate::[\w:]*?(\w+)\s*,")


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
    return sorted(boundary_types() - covered_types() - known)


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
        failures.append("every baseline name is now covered; the ratchet cannot be tested")
    elif check_coverage(BASELINE_UNCOVERED - {victim}) != [victim]:
        failures.append(
            f"removing {victim} from the baseline did not make the ratchet report it"
        )

    # The known-mismatch matcher, including the prefix hazard that makes it
    # "longest match" rather than "first match".
    if known_owner("struct aiocb size") != "struct aiocb":
        failures.append("known_owner did not match a size message")
    if known_owner("struct aiocb.aio_offset") != "struct aiocb":
        failures.append("known_owner did not match a field message")
    if known_owner("struct sigaction size") is not None:
        failures.append("known_owner claimed a type that is not listed")
    probe = dict(KNOWN_MISMATCH)
    try:
        KNOWN_MISMATCH.clear()
        KNOWN_MISMATCH.update({"struct stat": "x", "struct statfs": "y"})
        if known_owner("struct statfs size") != "struct statfs":
            failures.append(
                "known_owner took the shorter prefix: `struct stat` swallowed "
                "`struct statfs`"
            )
    finally:
        KNOWN_MISMATCH.clear()
        KNOWN_MISMATCH.update(probe)

    for f in failures:
        print(f"check-libc-abi --self-test: FAIL: {f}")
    if failures:
        return 1
    print("check-libc-abi --self-test: OK")
    return 0


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--self-test", "--selftest", action="store_true", dest="selftest")
    ap.add_argument("--print-uncovered", action="store_true")
    args = ap.parse_args()

    if args.selftest:
        return self_test()

    if args.print_uncovered:
        for t in sorted(boundary_types() - covered_types()):
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
        return 1 if problems else 0

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

    if problems:
        print(f"\ncheck-libc-abi: {problems} problem(s). See design-decisions.md 1010.")
        return 1

    n = len(covered_types())
    if known_seen:
        # Not "0 mismatches": there are five, they are written down, and a
        # summary line that says zero while the lines above it say otherwise
        # trains the reader to stop reading the lines above it.
        print(
            f"check-libc-abi: OK ({n} types checked against musl; "
            f"{len(known_seen)} known-bad and recorded, 0 new)"
        )
    else:
        print(f"check-libc-abi: OK ({n} types checked against musl, 0 mismatches)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
