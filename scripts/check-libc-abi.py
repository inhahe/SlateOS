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


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--self-test", "--selftest", action="store_true", dest="selftest")
    ap.add_argument("--print-uncovered", action="store_true")
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
