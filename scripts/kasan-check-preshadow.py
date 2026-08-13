#!/usr/bin/env python3
"""kasan-check-preshadow.py — prove the pre-shadow window is uninstrumented.

Between the kernel entry point and the moment `mm::kasan::early_init` finishes
installing the zero shadow, **not one instrumented memory access may execute**.
There is no shadow to read and no IDT to catch the resulting page fault, so a
single stray check is a triple fault: QEMU resets, the kernel emits no serial
output at all, and the boot test reports an indistinguishable "no BOOT_OK".
Diagnosing one costs a `-d int,cpu_reset` run and a symbol lookup.

Marking a module `#![cfg_attr(kasan_instrumented, sanitize(address = "off"))]`
is *not* sufficient to establish this, which is the whole reason this script
exists. The exemption is a per-function LLVM attribute, so it only covers
functions that are ours. Generic `core` functions monomorphise into the kernel
crate, are emitted out-of-line at `-O0`, and carry the default (instrumented)
attribute — and they dereference the pointers we hand them, so the check lands
in `core`'s frame where our exemption has no reach. Two real examples, both of
which triple-faulted a boot before this check existed:

  * `serial::init` takes a spinlock, which calls
    `core::sync::atomic::atomic_compare_exchange_weak::<u8>`, which probes the
    shadow of `serial::SERIAL`.
  * `for i in 0..512` hands `&mut Range<usize>` to
    `core::iter::range::RangeIteratorImpl::spec_next`, which probes the shadow
    of *this* frame's stack slot. (`-asan-stack=0` suppresses instrumentation
    of a function's own allocas — not of a pointer parameter that happens to
    point at a caller's alloca.)

Neither is visible by reading the source of the exempt module. Both are
obvious in the call graph, so that is what this script walks.

The same walk proves a second, structurally identical invariant. The profile
builds with `-asan-instrumentation-with-call-threshold=0`, so every checked
access is a *call* to `mm::kasan_rt::__asan_load8_noabort` & co., which call
`mm::kasan::shadow_allows`. If anything on that path performs an instrumented
access of its own, the check calls back into itself and recurses without bound
— a stack overflow in the one build where you are already debugging something
else. The requirement is again "no reachable `__asan_*` call", so it is the
same walk from different roots, and it is checked here rather than reviewed for
the same reason: the offending access is usually inside a `core` generic that
does not appear in our source at all.

A third walk covers the same invariant from a third root. `mm::heap`'s
free-magic and redzone checks, `mm::poison`'s fills, and `mm::quarantine`'s
parked slots exist to read and write memory KASAN has deliberately poisoned —
the access *is* the detector. Instrumented, they report on every free, spend
the report cap before the window being hunted in, and (through the allocator
traffic in `kasan::self_test_freed_address`) panicked a boot outright. All
three modules already had the module-level `sanitize` opt-out and it protected
none of them, because they touched memory through
`core::ptr::{read_volatile, write_volatile, write_bytes}` — the same generics
this script exists to catch. Cost of learning that the other way: one 2.7-hour
boot.

Usage:
    python scripts/kasan-check-preshadow.py [path/to/kernel]

Exit codes: 0 clean, 1 violation found, 2 the binary is not instrumented at
all (nothing to check — the ordinary build), 3 harness/tooling failure.
"""

from __future__ import annotations

import os
import re
import subprocess
import sys
from collections import deque

PROJECT_ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
DEFAULT_KERNEL = os.path.join(
    PROJECT_ROOT, "target", "x86_64-unknown-none", "debug", "kernel"
)

# The roots of the pre-shadow window.
#
# `kernel_main` itself is deliberately NOT a root: it calls into the entire
# kernel, so its reachable set is everything. Instead the roots are exactly the
# functions that execute before the shadow is live, and `check_entry_order`
# below verifies against the binary that this list is still complete — if
# someone inserts another call ahead of `serial::init`, that check fails rather
# than this list silently going stale.
ROOT_SUBSTRINGS = [
    # The entry trampoline (`#[unsafe(no_mangle)]`, so the symbol is bare):
    # switches to KERNEL_BOOT_STACK and tail-calls kernel_main.
    "kmain",
    # Reads the Limine HHDM offset with raw loads.
    "4boot17hhdm_offset_early",
    # Phase one of `early_init`: everything up to and including the TLB flush
    # that makes the shadow live.
    "2mm5kasan19install_zero_shadow",
]

# Functions `kernel_main` is allowed to call before `serial::init`.
#
# This differs from ROOT_SUBSTRINGS by `early_init`, which straddles the phase
# boundary: its first half runs with no shadow and its second half runs with
# one. A call-graph walk cannot express "the first half", so the *code* draws
# the line instead — `early_init` is a two-statement wrapper over
# `install_zero_shadow` (walked) and `publish_shadow_roots` (not).
ENTRY_ALLOWED_CALLS = [
    "4boot17hhdm_offset_early",
    "2mm5kasan10early_init",
]

# `kernel_main` is exempt itself, but the order of its calls defines where the
# window ends: the first call to `serial::init` is the first thing that runs
# with the shadow guaranteed live.
ENTRY_SYM = "kernel_main"
WINDOW_END_SUBSTRING = "6serial4init"

# Functions that are checked for instrumentation but whose callees are NOT
# followed.
#
# `kmain` tail-calls `kernel_main`, and `kernel_main` goes on to call the entire
# kernel — expanding it would make the "reachable before the shadow exists" set
# equal to "everything", which is both wrong and useless. Its own body is
# exempt and the part of it that genuinely runs pre-shadow is covered precisely
# by `check_entry_order`, so the walk stops here instead.
NO_EXPAND = {ENTRY_SYM}

# ---------------------------------------------------------------------------
# The second walk: the runtime check path
# ---------------------------------------------------------------------------

# Roots are the outlined check entry points LLVM calls for every instrumented
# access. Matched as a *prefix* of a bare (`no_mangle`) symbol, which covers
# both the `_noabort` and abort-mode variants and every access size.
#
# The `__asan_report_*` entry points are deliberately not roots: they are the
# cold slow path and are allowed to call instrumented code (see NO_EXPAND
# below), so rooting the walk there would report the whole formatting and
# backtrace machinery.
RUNTIME_ROOT_PREFIXES = ("__asan_load", "__asan_store")

# `report` formats a message and prints a backtrace — far too much code to keep
# free of instrumented accesses, and it does not need to be. A report calls
# instrumented code, whose checks call `shadow_allows`, which this walk proves
# is clean, so it returns. That is one level of nesting, not a regress. The
# walk therefore checks `report`'s own body and stops.
RUNTIME_NO_EXPAND_SUBSTRINGS = ["2mm8kasan_rt6report"]

# ---------------------------------------------------------------------------
# The third walk: code that deliberately touches poisoned memory
# ---------------------------------------------------------------------------

# `mm::heap`'s free-magic and redzone checks, `mm::poison`'s fills and verifies,
# and `mm::quarantine`'s parked-slot bookkeeping all exist to read and write
# bytes that `mm::kasan` has marked inaccessible. For them the access *is* the
# detector, so a report on it is noise — and not harmless noise: it fires on
# every free, exhausts the 64-report cap long before the window anyone is
# hunting in, and (through the allocator traffic inside
# `kasan::self_test_freed_address`) once panicked the boot outright. See
# `known-issues.md` → `B-KASAN-INSTRUMENTED-BUILD-PANICS-ON-ITS-OWN-REDZONE-
# CHECKS`.
#
# Every one of those modules already carried the module-level `sanitize`
# opt-out, and it protected none of them, because they did their actual byte
# touching through `core::ptr::{read_volatile, write_volatile, write_bytes}` —
# generic `core` functions, which monomorphise into this crate carrying the
# default (instrumented) attribute. Same hazard as the two walks above, equally
# invisible in the source of the "exempt" module, so it is checked here rather
# than reviewed. The repair funnels all of it through `mm::rawmem`, whose
# accesses are inline `asm!` and therefore cannot be instrumented at all.
POISON_ROOT_SUBSTRINGS = [
    "2mm6rawmem7read_u8",
    "2mm6rawmem8write_u8",
    "2mm6rawmem7fill_u8",
    "2mm4heap11poison_free",
    "2mm4heap12poison_alloc",
    "2mm4heap12check_poison",
    "2mm4heap13check_redzone",
    "2mm6poison11poison_free",
    "2mm6poison12poison_alloc",
    "2mm6poison14poison_redzone",
    "2mm6poison14verify_redzone",
    "2mm6poison12verify_freed",
    "2mm10quarantine11fill_poison",
    "2mm10quarantine15find_corruption",
]

# Cuts for the third walk.
#
# These run only once a violation has *already* been found — the serial
# formatter, the panic path, the KASAN reporter. They are far too much code to
# keep free of instrumented accesses and they do not need to be: by then the
# detector has done its job, and one report is the intended outcome rather than
# a flood. The walk covers the hot path and stops at the cold one.
POISON_NO_EXPAND_SUBSTRINGS = [
    "6serial6_print",
    "2mm8kasan_rt6report",
    "4core3fmt",
    "9panicking",
]

FUNC_HEADER_RE = re.compile(r"^([0-9a-fA-F]+)\s+<(.+)>:$")
DIRECT_CALL_RE = re.compile(r"\bcallq?\s+0x[0-9a-fA-F]+\s+<([^>]+)>")
INDIRECT_CALL_RE = re.compile(r"\bcallq?\s+\*")


def find_objdump() -> str:
    """Locate llvm-objdump inside whichever rustup toolchain has one."""
    import glob

    candidates = glob.glob(
        os.path.expanduser(
            "~/.rustup/toolchains/*/lib/rustlib/*/bin/llvm-objdump*"
        )
    )
    if candidates:
        return candidates[0]
    for name in ("llvm-objdump", "objdump"):
        try:
            subprocess.run(
                [name, "--version"], capture_output=True, check=True
            )
            return name
        except (OSError, subprocess.CalledProcessError):
            continue
    print("ERROR: no llvm-objdump found", file=sys.stderr)
    sys.exit(3)


def disassemble(objdump: str, kernel: str) -> dict[str, list[str]]:
    """Return {symbol: [disassembly lines]} for every function in the binary."""
    proc = subprocess.run(
        [objdump, "-d", "--no-show-raw-insn", kernel],
        capture_output=True,
        text=True,
        errors="replace",
    )
    if proc.returncode != 0:
        print(f"ERROR: objdump failed: {proc.stderr[:400]}", file=sys.stderr)
        sys.exit(3)

    functions: dict[str, list[str]] = {}
    current: list[str] | None = None
    for line in proc.stdout.splitlines():
        header = FUNC_HEADER_RE.match(line.strip())
        if header:
            current = []
            functions[header.group(2)] = current
        elif current is not None:
            current.append(line)
    return functions


def calls_in(lines: list[str]) -> list[str]:
    """Direct call targets, in program order, with duplicates preserved."""
    out = []
    for line in lines:
        match = DIRECT_CALL_RE.search(line)
        if match:
            out.append(match.group(1))
    return out


def check_entry_order(functions: dict[str, list[str]]) -> list[str]:
    """Verify ROOT_SUBSTRINGS still describes everything `kernel_main` calls
    before the shadow is live.

    Returns a list of problems (empty if fine). This is what keeps the root
    list from going stale: adding a call ahead of `serial::init` without
    listing it here is reported instead of silently escaping the walk.
    """
    if ENTRY_SYM not in functions:
        return [f"entry symbol {ENTRY_SYM!r} not found"]

    problems = []
    seen_end = False
    for callee in calls_in(functions[ENTRY_SYM]):
        if WINDOW_END_SUBSTRING in callee:
            seen_end = True
            break
        if not any(allowed in callee for allowed in ENTRY_ALLOWED_CALLS):
            problems.append(
                f"{ENTRY_SYM}\n    calls {callee}\n    before "
                f"{WINDOW_END_SUBSTRING} — it runs in the pre-shadow window "
                f"but is not in ENTRY_ALLOWED_CALLS"
            )
    if not seen_end:
        problems.append(
            f"no call to {WINDOW_END_SUBSTRING!r} found in {ENTRY_SYM}; "
            "the pre-shadow window's end can no longer be located"
        )
    return problems


def walk(
    functions: dict[str, list[str]],
    roots: list[str],
    no_expand: set[str],
    check_indirect: bool,
) -> tuple[list[tuple[str, str]], set[str], dict[str, str | None]]:
    """Breadth-first walk of a call graph, reporting instrumented functions.

    Returns `(violations, visited, parent)`. `parent` records how each function
    was reached so a violation can be printed as a path rather than a bare
    symbol name.

    `check_indirect` is on for the pre-shadow walk and off for the runtime one.
    Before the shadow exists an unresolvable call target cannot be proven
    exempt and is a potential triple fault; underneath the instrumentation the
    shadow is live, so the worst an unexpected callee can do is report.
    """
    parent: dict[str, str | None] = {r: None for r in roots}
    queue: deque[str] = deque(roots)
    visited = set(roots)
    violations: list[tuple[str, str]] = []

    while queue:
        name = queue.popleft()
        lines = functions.get(name)
        if lines is None:
            # A call into a symbol we have no body for (PLT-less static link
            # makes this rare). Report rather than assume it is harmless.
            violations.append((name, "no disassembly available for callee"))
            continue

        callees = calls_in(lines)

        # An instrumented access in this function is a violation whether or not
        # we go on to expand it, so this check comes before the cut.
        for callee in callees:
            if callee.startswith("__asan"):
                violations.append((name, f"calls {callee}"))

        if name in no_expand:
            continue

        # Only meaningful for functions we are actually claiming are exempt:
        # an unresolvable target inside the window cannot be proven clean.
        if check_indirect:
            for line in lines:
                if INDIRECT_CALL_RE.search(line):
                    violations.append(
                        (name, f"indirect call, cannot be proven exempt:{line}")
                    )

        for callee in callees:
            if callee not in visited:
                visited.add(callee)
                parent[callee] = name
                queue.append(callee)

    return violations, visited, parent


def format_path(name: str, parent: dict[str, str | None]) -> str:
    chain = []
    node: str | None = name
    while node is not None:
        chain.append(node)
        node = parent.get(node)
    return "\n      <- ".join(chain)


def main() -> int:
    kernel = sys.argv[1] if len(sys.argv) > 1 else DEFAULT_KERNEL
    if not os.path.isfile(kernel):
        print(f"ERROR: no such kernel binary: {kernel}", file=sys.stderr)
        return 3

    objdump = find_objdump()
    functions = disassemble(objdump, kernel)

    if not any(name.startswith("__asan") for name in functions):
        print(
            "kasan-check-preshadow: binary has no __asan symbols — not an "
            "instrumented build, nothing to check."
        )
        return 2

    problems = check_entry_order(functions)

    # -----------------------------------------------------------------------
    # Walk 1: the pre-shadow window.
    # -----------------------------------------------------------------------
    # An entry that names a symbol exactly (the `no_mangle` ones) is matched
    # exactly; anything else is a fragment of a mangled name and is matched as
    # a substring. Without the exact case, a bare name like "kmain" would also
    # sweep in unrelated symbols that happen to contain it.
    pre_roots: list[str] = []
    for root in ROOT_SUBSTRINGS:
        if root in functions:
            pre_roots.append(root)
            continue
        matched = [name for name in functions if root in name]
        if not matched:
            problems.append(f"root {root!r} not found in the binary (renamed?)")
        pre_roots.extend(matched)

    pre_violations, pre_visited, pre_parent = walk(
        functions, pre_roots, NO_EXPAND, check_indirect=True
    )

    # -----------------------------------------------------------------------
    # Walk 2: the runtime check path.
    # -----------------------------------------------------------------------
    rt_roots = [
        name
        for name in functions
        if name.startswith(RUNTIME_ROOT_PREFIXES)
    ]
    if not rt_roots:
        problems.append(
            "no __asan_load*/__asan_store* check entry points found; either "
            "the build is no longer using outlined instrumentation "
            "(-asan-instrumentation-with-call-threshold=0) or mm::kasan_rt "
            "stopped defining them"
        )

    rt_no_expand = {
        name
        for name in functions
        if any(frag in name for frag in RUNTIME_NO_EXPAND_SUBSTRINGS)
    }
    for frag in RUNTIME_NO_EXPAND_SUBSTRINGS:
        if not any(frag in name for name in functions):
            problems.append(
                f"runtime cut {frag!r} not found in the binary (renamed?)"
            )

    rt_violations, rt_visited, rt_parent = walk(
        functions, rt_roots, rt_no_expand, check_indirect=False
    )

    # -----------------------------------------------------------------------
    # Walk 3: code that deliberately touches poisoned memory.
    # -----------------------------------------------------------------------
    poison_roots: list[str] = []
    for root in POISON_ROOT_SUBSTRINGS:
        matched = [name for name in functions if root in name]
        if not matched:
            problems.append(
                f"poisoned-memory root {root!r} not found in the binary "
                "(renamed, or inlined away — if it was inlined, root the walk "
                "at its caller instead of deleting the entry)"
            )
        poison_roots.extend(matched)

    poison_no_expand = {
        name
        for name in functions
        if any(frag in name for frag in POISON_NO_EXPAND_SUBSTRINGS)
    }

    poison_violations, poison_visited, poison_parent = walk(
        functions, poison_roots, poison_no_expand, check_indirect=False
    )

    if pre_violations or rt_violations or poison_violations or problems:
        print("=== KASAN uninstrumented-path invariants are NOT satisfied ===\n")
        for problem in problems:
            print(f"  STRUCTURE: {problem}\n")
        for name, why in pre_violations:
            print(
                f"  PRE-SHADOW: {name}\n    {why}\n    reached via:\n"
                f"      {format_path(name, pre_parent)}\n"
            )
        for name, why in rt_violations:
            print(
                f"  CHECK PATH: {name}\n    {why}\n    reached via:\n"
                f"      {format_path(name, rt_parent)}\n"
            )
        if pre_violations:
            print(
                "PRE-SHADOW: every function above executes before the KASAN\n"
                "shadow exists, so an instrumented access in it is a triple\n"
                "fault at boot with no serial output.\n"
            )
        if rt_violations:
            print(
                "CHECK PATH: every function above runs *underneath* the\n"
                "instrumentation, so an instrumented access in it calls the\n"
                "check again — unbounded recursion, i.e. a stack overflow with\n"
                "no explanation.\n"
            )
        if poison_violations:
            print(
                "POISONED MEMORY: every function above reads or writes bytes\n"
                "KASAN has deliberately marked inaccessible — that access is\n"
                "the detector, not a bug. Instrumented, it reports on every\n"
                "free, spends the report cap before the window you are hunting\n"
                "in, and can panic the boot. Route the access through\n"
                "`mm::rawmem` (asm!-based, uninstrumentable); note that a\n"
                "module-level `sanitize` opt-out will NOT save a\n"
                "`core::ptr::{read_volatile,write_volatile,write_bytes}` call,\n"
                "which is exactly how this was introduced the first time.\n"
            )
        print(
            "Fix either by keeping the offending operation inside an exempt\n"
            "function: emit the load/store with `asm!` (see\n"
            "`mm::kasan::raw_load_u64`), or replace the `core` generic that\n"
            "owns it (`for`-over-`Range` -> `while` with a plain counter,\n"
            "atomics -> raw loads, `/` and `%` -> shifts and masks, checked\n"
            "arithmetic -> `wrapping_*`)."
        )
        return 1

    print(
        f"kasan-check-preshadow: OK — {len(pre_visited)} function(s) reachable "
        f"before the shadow is installed, {len(rt_visited)} on the check path, "
        f"and {len(poison_visited)} reachable from the deliberate "
        "poisoned-memory accessors, none instrumented."
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
