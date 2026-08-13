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

# Roots that are allowed to be missing from the binary.
#
# A root with no call sites is not emitted at all, and a function that never
# runs cannot report — so absence is safe, and if a caller is added later the
# symbol reappears and is checked automatically with no edit here. This is
# listed explicitly rather than by relaxing the check for every root, because
# for the rest a missing symbol means a rename and the walk silently losing
# coverage, which is precisely the failure this script exists to prevent.
#
# `mm::poison::poison_alloc` is public tooling API in a module carrying
# `#![allow(dead_code)]`; the slab allocator uses its own `mm::heap::
# poison_alloc` instead, so nothing calls this one today.
POISON_ROOTS_MAY_BE_ABSENT = {
    "2mm6poison12poison_alloc",
}

# Cuts for the third walk.
#
# These run only once a violation has *already* been found — the serial
# formatter, the panic path, the KASAN reporter. They are far too much code to
# keep free of instrumented accesses and they do not need to be: by then the
# detector has done its job, and one report is the intended outcome rather than
# a flood. The walk covers the hot path and stops at the cold one.
POISON_NO_EXPAND_SUBSTRINGS = [
    # `serial::_print`. The doubled underscore is not a typo: v0 escapes an
    # identifier that begins with `_`, so `_print` encodes as `6__print`.
    "6serial6__print",
    "2mm8kasan_rt6report",
    "4core3fmt",
    "9panicking",
]

# The third walk's violation rule is deliberately NOT the first two's.
#
# Walks 1 and 2 flag *every* instrumented function they can reach, and that is
# exactly right for them: before the shadow exists, or underneath the check
# itself, an instrumented access is fatal no matter what memory it touches.
#
# Here it would be wrong, and the first run of this walk proved it — 500-odd
# "violations" that were all `AtomicUsize::fetch_add` on a stats counter,
# `Range::next` on a loop variable, and a `SpinMutexGuard` deref on the serial
# port. None of those touch poisoned memory; they touch ordinary live kernel
# objects, where instrumentation is correct and even desirable (it is how a bug
# in the memory debugger itself would get caught). Flagging them would have
# meant either 500 pointless exemptions or, far more likely, someone
# rubber-stamping the whole walk as noise and losing the one real signal in it.
#
# What actually has to be uninstrumented is much narrower: the accesses to the
# poisoned bytes themselves. Those are reached in exactly two ways, and the walk
# checks both:
#
#   1. The root's own body. Every poison root lives in a module carrying the
#      `#![cfg_attr(kasan_instrumented, sanitize(address = "off"))]` opt-out, so
#      a plain `*ptr` in its body compiles to an uninstrumented access. If a
#      root calls `__asan_*` directly, the opt-out is not in force — the
#      attribute was dropped, or a new module was added without it.
#
#   2. A raw-pointer accessor reached from a root. This is the §118/§119 hazard
#      and the one the attribute provably cannot cover: `core::ptr::
#      read_volatile` and friends are generic, so they monomorphise into *this*
#      crate carrying the default (instrumented) attribute, and the exempt
#      module's attribute never applies. Invisible in the source of the
#      "exempt" module, which is why it is checked mechanically.
#
# The accessor list is `core::ptr` (which covers the `<*const T>::read` /
# `<*mut T>::write` inherent methods too, as they live in `core::ptr::const_ptr`
# / `mut_ptr`) plus `core::intrinsics` (`write_bytes`, `copy_nonoverlapping`).
# `core::slice` is deliberately excluded: slice helpers are used on ordinary
# memory throughout these modules, so including them would reintroduce exactly
# the false positives described above. Poisoned memory is reached through raw
# pointers here, never through a slice.
#
# This can still over-report — a `read_volatile` on an MMIO register reached
# from a root would be flagged though it touches nothing poisoned. That
# direction is the cheap one: clearing it costs a `rawmem` call or a documented
# exception, while a miss costs a ~2.7-hour instrumented boot that dies on its
# own redzone check.
POISON_ACCESSOR_SUBSTRINGS = ["4core3ptr", "4core10intrinsics"]

# The bulk accessors, which are *not* reachable under either name above.
#
# `core::intrinsics` turns out to emit only `rotate_left`/`rotate_right` in this
# binary — pure register operations that touch no memory — because
# `write_bytes` and `copy_nonoverlapping` are lowered by LLVM straight to
# `memset`/`memcpy` calls rather than being emitted as callable monomorphisations.
# So a `write_bytes` aimed at poisoned memory appears in the call graph as a
# plain `call memset`, and the substring list above would sail straight past it —
# the single most likely way to write this bug, missed.
#
# Matched exactly rather than as substrings: these are short, unmangled names
# (`memset`, not `_RNv...6memset`), and a substring test would sweep in every
# unrelated symbol that merely contains them.
POISON_ACCESSOR_EXACT = {"memset", "memcpy", "memmove"}

# What this branch is actually doing today: nothing, and that is the point.
#
# After the `rawmem` conversion, *zero* accessors of any kind are reachable from
# the poison roots — the byte touching is all inline `asm!`, which emits no call
# at all. So the accessor branch currently judges an empty set, and passes
# because there is nothing to judge rather than because what it judged was
# clean. That is the correct end state, but it is worth stating plainly, because
# a check that is vacuous by accident and one that is vacuous by success look
# identical from the exit code. The OK line reports the accessor count for
# exactly this reason.
#
# The branch earns its keep prospectively: it fires the moment someone
# reintroduces a `core::ptr` call or a `memset` onto a poison path, which is the
# specific regression that cost a 2.7-hour boot. It has been positive-controlled
# — before the `6__print` cut was fixed, it is what caught the instrumented
# `core::ptr::read_volatile` reached through the printer.
#
# The root-body branch, by contrast, is doing real work on every run: it
# verifies that all thirteen roots are genuinely uninstrumented, i.e. that the
# module-level opt-out is in force.


def is_poison_accessor(name: str) -> bool:
    """Whether `name` is a raw byte accessor that walk 3 judges."""
    return name in POISON_ACCESSOR_EXACT or any(
        frag in name for frag in POISON_ACCESSOR_SUBSTRINGS
    )

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


def walk_poison(
    functions: dict[str, list[str]],
    roots: list[str],
    no_expand: set[str],
) -> tuple[list[tuple[str, str]], set[str], dict[str, str | None]]:
    """Walk the call graph from the deliberate-poisoned-memory roots.

    Same traversal as `walk`, different — much narrower — violation rule: only
    the root bodies themselves and the raw-pointer accessors they reach are
    checked, not every function on the path. See the commentary on
    `POISON_ACCESSOR_SUBSTRINGS` for why the transitive rule the other two walks
    use would be actively harmful here.
    """
    parent: dict[str, str | None] = {r: None for r in roots}
    queue: deque[str] = deque(roots)
    visited = set(roots)
    root_set = set(roots)
    violations: list[tuple[str, str]] = []

    while queue:
        name = queue.popleft()
        is_root = name in root_set
        is_accessor = is_poison_accessor(name)

        lines = functions.get(name)
        if lines is None:
            # Only worth reporting for the functions this walk actually judges;
            # a missing body anywhere else on the path is irrelevant here.
            if is_root or is_accessor:
                violations.append((name, "no disassembly available for callee"))
            continue

        callees = calls_in(lines)

        if is_root or is_accessor:
            for callee in callees:
                if not callee.startswith("__asan"):
                    continue
                if is_root:
                    why = (
                        f"calls {callee} in its own body — this function is in a "
                        "module carrying the `sanitize(address = \"off\")` "
                        "opt-out, so the opt-out is no longer in force"
                    )
                else:
                    why = (
                        f"calls {callee} — a generic `core` accessor reachable "
                        "from a poisoned-memory root, which no module-level "
                        "`sanitize` attribute can exempt (§118/§119)"
                    )
                violations.append((name, why))

        if name in no_expand:
            continue

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
        if not matched and root not in POISON_ROOTS_MAY_BE_ABSENT:
            problems.append(
                f"poisoned-memory root {root!r} not found in the binary. "
                "Either it was renamed (fix the entry), or it lost its last "
                "call site and was not emitted — if that is intended, add it "
                "to POISON_ROOTS_MAY_BE_ABSENT with the reason rather than "
                "deleting it, so it is re-checked the moment a caller returns."
            )
        poison_roots.extend(matched)

    poison_no_expand = {
        name
        for name in functions
        if any(frag in name for frag in POISON_NO_EXPAND_SUBSTRINGS)
    }
    # A cut that matches nothing fails silently and in the *unsafe* direction:
    # the walk runs on past where it was meant to stop and buries the real
    # signal in noise from code this check never intended to cover. That is not
    # hypothetical — `'6serial6_print'` was one underscore short of the actual
    # symbol (v0 escapes the leading `_` of `_print`, giving `6__print`), so the
    # walk expanded through the printer into the APIC MMIO accessors and
    # reported those. Same validation as the roots get.
    for frag in POISON_NO_EXPAND_SUBSTRINGS:
        if not any(frag in name for name in functions):
            problems.append(
                f"poisoned-memory cut {frag!r} not found in the binary "
                "(renamed, or mistyped?). A cut that matches nothing does not "
                "stop the walk, so this must be fixed rather than dropped."
            )

    # The accessor list is the *only* thing that turns a reached function into a
    # violation, so an entry that matches nothing does not narrow the walk — it
    # switches that part of the check off, silently and permanently. Validated
    # against the whole binary rather than against the reachable set: an
    # accessor that exists but is not reachable from a poison root is the
    # expected, passing state, whereas one that exists nowhere at all is a typo
    # or a rename. Both lists are known to match today (7937 symbols for
    # `4core3ptr`, 4 for `4core10intrinsics`, and the three `mem*` builtins).
    for frag in POISON_ACCESSOR_SUBSTRINGS:
        if not any(frag in name for name in functions):
            problems.append(
                f"poisoned-memory accessor pattern {frag!r} matches no symbol "
                "in the binary. It is not narrowing the walk, it is disabling "
                "part of it — fix the pattern rather than dropping it."
            )
    for exact in sorted(POISON_ACCESSOR_EXACT):
        if exact not in functions:
            problems.append(
                f"poisoned-memory accessor {exact!r} is not defined in the "
                "binary. `write_bytes`/`copy_nonoverlapping` lower to these, so "
                "losing the name means losing the check on bulk poison fills."
            )

    poison_violations, poison_visited, poison_parent = walk_poison(
        functions, poison_roots, poison_no_expand
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
        for name, why in poison_violations:
            print(
                f"  POISONED MEMORY: {name}\n    {why}\n    reached via:\n"
                f"      {format_path(name, poison_parent)}\n"
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

    # The accessor count is reported separately from the reachable count
    # because the two answer different questions, and only the first one is
    # about the §118/§119 hazard. A reachable count of 46 with 0 accessors means
    # the poison paths reach no raw byte accessor at all — the intended state
    # after the `rawmem` conversion — whereas a non-zero accessor count means
    # the walk found accessors and cleared them. Both exit 0; conflating them in
    # the output would hide a check that had quietly stopped judging anything.
    poison_accessors = sum(1 for name in poison_visited if is_poison_accessor(name))
    print(
        f"kasan-check-preshadow: OK — {len(pre_visited)} function(s) reachable "
        f"before the shadow is installed, {len(rt_visited)} on the check path, "
        f"and {len(poison_visited)} reachable from the deliberate "
        f"poisoned-memory roots ({poison_accessors} of them raw byte "
        f"accessors), none instrumented."
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
