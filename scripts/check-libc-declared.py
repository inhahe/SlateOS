#!/usr/bin/env python3
"""Refuse a function musl's headers declare that `libc.a` does not define --
and a public name `libc.a` defines that no header declares.

Why
---

C here is compiled against musl's headers (`zig cc --target=x86_64-linux-musl`)
and linked against this tree's `libc.a`. Whatever a header declares, a program
may call -- and if the library does not define it, the program compiles and
then fails to link. On 2026-09-28 an audit found 126 such functions
(known-issues.md -> D-POSIX-LIBC-LACKS-FUNCTIONS-ITS-HEADERS-DECLARE): all of
C11's `<threads.h>`, the `long double` complex functions, and the two helpers
musl's `pthread_cleanup_push`/`pop` macros call, which made every C program
with a cleanup handler unlinkable. Nothing had noticed, because nothing asked:
`check-libc-abi.py` checks the layouts and numbers of what exists, and
`check-libc-shape.py` how it is packed, but neither whether what is declared
exists at all.

How
---

Every header under zig's `generic-musl` include directory, and every one in
`posix/include` -- the overlay in front of them that declares what this
library has beyond musl (design-decisions §1141) -- is preprocessed on its
own (some pairs conflict) with the overlay first (`-I`) and `_GNU_SOURCE`,
`_BSD_SOURCE` and `_LARGEFILE64_SOURCE`, so every declaration a program can
reach is visible.
Each declaration ending in `);` names a function. The names `libc.a` defines
come from its archive index, read by `check-libc-shape.py`'s parser. The
difference, less `NOT_FUNCTIONS` (text the declaration pattern mistakes for a
name), is what a program can call and not link.

It is a ratchet: `BASELINE_MISSING` is what was missing when the gate was
written, less what has been implemented since. The gate fails when

  - a declared function is missing and not in the baseline (a regression, or a
    new header declaration nobody implemented), or
  - a baseline name is now defined (a stale exemption: delete the line).

The other direction
-------------------

Since 2026-09-29 it also asks the converse: is every *public* name `libc.a`
defines -- one a program could collide with or want to call, not reserved
with a leading underscore -- declared by some header a program can include,
musl's or `posix/include`'s, as a function, an object or a macro? Every
header is probed on its own for each name, all feature macros on; a name
declared nowhere is either something C cannot call (clang refuses a call to
an undeclared function, so it wants a declaration in the overlay, as glibc
2.39's headers have it), or a name the library had no business putting in
the program's namespace (so it wants to stop being exported). The few that
are neither, on purpose, are `UNDECLARED_OK`, each with its reason; the
compiler runtime's `_Float16` and `_Float128` functions are the compiler's
business, not the library's. The first run found 81
(known-issues.md -> D-POSIX-LIBC-EXPORTED-NAMES-NO-HEADER-DECLARES).

Exit codes: 0 clean, 1 violation, 2 could not check (no archive, an
unreadable one, too little found in either to judge) -- a failure too, not a
pass -- and 3 could not run: no zig to read the headers with, which
`scripts/run-checker.sh` files as skipped, never as ran.

Usage
-----

    python scripts/check-libc-declared.py [path/to/libc.a]
    python scripts/check-libc-declared.py --self-test
"""

from __future__ import annotations

import argparse
import bisect
import concurrent.futures
import importlib.util
import os
import re
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent

# Text the declaration pattern takes for a function name, and why each is not
# one: keywords inside inline assembly and `tgmath.h`'s macros, a macro's helper
# that no library defines, and MIPS-only cache control, which x86-64 has none
# of.
NOT_FUNCTIONS = frozenset({
    "return",        # tgmath.h's type-generic macros
    "volatile",      # sys/io.h's inline assembly
    "seqbuf_dump",   # sys/soundcard.h: a helper its macros expect the program to define
    "cachectl", "cacheflush", "_flush_cache",  # sys/cachectl.h: MIPS only
})

# What was missing when this gate was written (2026-09-28), less what has been
# implemented since -- which, from the same day's `long double` complex
# functions on, is all of it: every function musl's headers declare is in
# libc.a. Only ever removed from, and so empty for good: a name that turns
# up missing now is a regression, not a baseline entry.
BASELINE_MISSING: frozenset[str] = frozenset()

# Public names libc.a defines that no header declares, on purpose -- name ->
# why. Neither something C must be able to call by name, nor a name that
# should not be exported. Only ever removed from as each gets a declaration.
_SYSCALL = ("a Linux system call glibc 2.39 declares no function for either: C makes it "
            "through syscall(), as on Linux")
_STREAMS = ("XSI STREAMS, which POSIX.1-2024 removed and glibc 2.30 stopped declaring "
            "(<stropts.h>): defined for what was built before")
UNDECLARED_OK: dict[str, str] = {
    **{n: _SYSCALL for n in ("arch_prctl", "capget", "capset", "clone3", "delete_module",
                             "faccessat2", "fadvise64", "finit_module", "futex", "init_module",
                             "ioprio_get", "ioprio_set", "kcmp", "openat2", "seccomp",
                             "signalfd4", "userfaultfd")},
    "sysctl": "glibc 2.32 removed it and <sys/sysctl.h>: defined for what was built before",
    **{n: _STREAMS for n in ("fattach", "fdetach", "getmsg", "getpmsg", "putmsg", "putpmsg")},
    "sys_errlist": "glibc 2.32 stopped declaring it (strerror is the interface): defined for "
                   "what was built before",
    "sys_nerr": "as sys_errlist",
    "fpurge": "BSD's name for __fpurge, which <stdio_ext.h> declares; glibc has neither name "
              "declared, and gnulib's fpurge module declares it itself where it finds it",
    "verror": "gnulib's -- its verror.h declares it -- and the va_list form error's trampoline "
              "delegates to; glibc has no such function to declare it as",
    "verror_at_line": "as verror",
    "setkeylayout": "SlateOS's own call, made from Rust (localectl): a C declaration of "
                    "SlateOS's own calls waits on a header set for them",
    "slateos_spawn_caps": "as setkeylayout: posix_spawn with a capability list",
}

# The compiler runtime's (compiler_builtins') _Float16 and _Float128 maths,
# which it exports into libc.a: glibc declares the _Float128 ones only for
# GCC, and this compiler has no _Float128 to declare them with.
FLOAT16_128 = re.compile(r"[a-z_]+f(?:16|128)")

# Every feature-test macro on, so that whatever any header can declare, it does
# -- <regex.h>'s _REGEX_RE_COMP among them, for BSD's re_comp and re_exec,
# which glibc's header declares under nothing else.
ALL_FEATURES = ["-std=gnu17", "-D_GNU_SOURCE", "-D_BSD_SOURCE", "-D_LARGEFILE64_SOURCE",
                "-D__STDC_WANT_IEC_60559_EXT__", "-D_REGEX_RE_COMP"]

DECL = re.compile(r"[^;{}]*\)\s*(?:__attribute__\s*\(\(.*?\)\)\s*)*;", re.S)
# `int (name)(...)`: a parenthesised declarator, which keeps a function-like
# macro of the same name from expanding.
PAREN_NAME = re.compile(r"\s*([A-Za-z_]\w*)\s*\)\s*\(")
LAST_IDENT = re.compile(r"([A-Za-z_]\w*)\s*$")
NOT_NAMES = {"__attribute__", "sizeof", "__typeof__", "_Static_assert", "void", "__asm__",
             "__asm", "__extension__"}


FN_RETURNING_FN_PTR = re.compile(r"\*\s*([A-Za-z_]\w*)\s*\(")

# `int pthread_yield(void) __asm__("sched_yield");`: a declaration whose calls
# link to another symbol -- glibc's __REDIRECT, and the overlay's
# pthread_yield, which is sched_yield as glibc's header makes it. What the
# library must define is the label.
ASM_LABEL = re.compile(r'__asm(?:__)?\s*\(\s*"([A-Za-z_]\w*)"\s*\)')


def linked_name(d: str, name: str) -> str:
    """The symbol a call to the function declaration `d` of `name` links
    to: its asm label if it has one, else its name."""
    m = ASM_LABEL.search(d)
    return m.group(1) if m else name


def decl_name(d: str) -> str | None:
    """The function a declaration declares, or None: the identifier before
    its first `(`, or inside it for `int (name)(...)`. A `(*` opens either a
    pointer to a function -- `void (*name)(int)`, a variable or a type, not a
    function -- or a function *returning* one, `void (*name(int, ...))(int)`,
    whose name is followed by its own parameter list: `sigset`'s shape, and
    `signal`'s. Until 2026-09-29 both were taken for variables, and `sigset`,
    which nothing defined, was never missed."""
    i = d.find("(")
    if i < 0:
        return None
    after = d[i + 1:]
    if after.lstrip().startswith("*"):
        m = FN_RETURNING_FN_PTR.match(after.lstrip())
        return m.group(1) if m else None
    m = PAREN_NAME.match(after)
    if m:
        return m.group(1)
    m = LAST_IDENT.search(d[:i])
    return m.group(1) if m else None


def find_zig() -> str | None:
    """`zig`, as `check-libc-abi.py` finds it: `FASTPY_ZIG`, then `PATH`."""
    env = os.environ.get("FASTPY_ZIG")
    if env and Path(env).exists():
        return env
    return shutil.which("zig")


def musl_include(zig: str) -> Path | None:
    """zig's `generic-musl` header directory."""
    r = subprocess.run([zig, "env"], capture_output=True, text=True, timeout=60)
    m = re.search(r'"lib_dir"\s*:\s*"([^"]+)"', r.stdout) or re.search(r"\.lib_dir = \"([^\"]+)\"", r.stdout)
    if not m:
        return None
    inc = Path(m.group(1).replace("\\\\", "\\")) / "libc" / "include" / "generic-musl"
    return inc if inc.is_dir() else None


def names_in(text: str) -> set[str]:
    """Every function a preprocessed header declares."""
    names = set()
    for decl in DECL.findall(text):
        d = " ".join(decl.split())
        if d.startswith("typedef"):
            continue
        n = decl_name(d)
        if n and n not in NOT_NAMES:
            names.add(linked_name(d, n))
    return names


LINE_MARKER = re.compile(r'#\s*\d+\s+"([^"]*)"')


def declarations_by_file(text: str) -> dict[str, str]:
    """Function name -> the file a preprocessor output says declared it.

    The line markers (`# 12 "path"`) are taken out, each remaining line
    remembering the file it came from, and a declaration is charged to the
    file of its closing `;` -- the header that wrote it, not whichever header
    happened to include that one. Removing the markers rather than cutting at
    them keeps a declaration whole when an `#if` inside it made the
    preprocessor emit one."""
    starts: list[int] = []
    files: list[str] = []
    body: list[str] = []
    size = 0
    current = ""
    for line in text.splitlines(keepends=True):
        m = LINE_MARKER.match(line)
        if m:
            current = m.group(1)
            continue
        starts.append(size)
        files.append(current)
        body.append(line)
        size += len(line)
    joined = "".join(body)
    out: dict[str, str] = {}
    for decl in DECL.finditer(joined):
        d = " ".join(decl.group(0).split())
        if d.startswith("typedef"):
            continue
        n = decl_name(d)
        if n and n not in NOT_NAMES:
            at = bisect.bisect_right(starts, decl.end() - 1) - 1
            out.setdefault(linked_name(d, n), files[at] if at >= 0 else "")
    return out


def declared(zig: str, inc: Path, overlay: Path | None = None) -> dict[str, str]:
    """Function name -> the header that declares it (relative to `inc`, or to
    `overlay`), musl's and the overlay's."""
    out: dict[str, str] = {}
    bases = [b for b in (overlay, inc) if b is not None and b.is_dir()]
    headers = sorted({p.relative_to(b).as_posix() for b in bases for p in b.rglob("*.h")
                      if not p.relative_to(b).as_posix().startswith("bits/")})
    first = ["-I", str(overlay)] if overlay is not None and overlay.is_dir() else []
    root = inc.resolve().as_posix().lower()
    with tempfile.TemporaryDirectory() as t:
        src = Path(t) / "h.c"
        for h in headers:
            src.write_text(f"#include <{h}>\n", encoding="utf-8", newline="")
            r = subprocess.run([zig, "cc", "--target=x86_64-linux-musl", "-E", *first,
                                "-D_GNU_SOURCE", "-D_BSD_SOURCE", "-D_LARGEFILE64_SOURCE",
                                "-D_REGEX_RE_COMP", str(src)],
                               capture_output=True, text=True, encoding="utf-8", errors="replace",
                               timeout=120)
            if r.returncode != 0:
                continue
            for n, f in declarations_by_file(r.stdout).items():
                f = f.replace("\\\\", "/").replace("\\", "/")
                rel = f[len(root) + 1:] if f.lower().startswith(root) else f.rsplit("/include/", 1)[-1]
                out.setdefault(n, rel)
    return out


def public_names(members: dict[int, set[str]]) -> set[str]:
    """The names libc.a defines for programs: identifiers, not reserved to the
    implementation by a leading underscore (and not the compiler's own
    `anon.*` and the like, which are no identifiers at all)."""
    out: set[str] = set()
    for names in members.values():
        out |= {n for n in names if re.fullmatch(r"[A-Za-z]\w*", n)}
    return out


def overlay_module():
    """`check-libc-overlay.py`, for its compiler driver."""
    spec = importlib.util.spec_from_file_location("check_libc_overlay",
                                                  ROOT / "scripts" / "check-libc-overlay.py")
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    return mod


def undeclared(zig: str, inc: Path, overlay: Path, names: set[str]) -> tuple[set[str], list[str]]:
    """Which of `names` no header declares -- as a function, an object or a
    macro -- each header probed on its own with ALL_FEATURES: (the names, the
    errors of any header that did not compile, which make its answer void)."""
    ov = overlay_module()
    bases = [b for b in (overlay, inc) if b.is_dir()]
    headers = sorted({p.relative_to(b).as_posix() for b in bases for p in b.rglob("*.h")
                      if not p.relative_to(b).as_posix().startswith("bits/")})
    probe_body = "".join(f"#ifndef {n}\n(void)&{n};\n#endif\n" for n in sorted(names))

    def probe(h: str) -> tuple[str, set[str], list[str]]:
        src = f"#include <{h}>\nvoid slateos_probe(void) {{\n{probe_body}}}\n"
        _, diag = ov.compile_c(zig, src, ALL_FEATURES + ["-w", "-ferror-limit=0"], overlay)
        missing, other = set(), []
        for line in ov.errors_of(diag):
            m = ov.UNDECLARED.search(line)
            if m:
                missing.add(m.group(1))
            else:
                other.append(line)
        return h, set(names) - missing, other

    declared_somewhere: set[str] = set()
    broken: list[str] = []
    with concurrent.futures.ThreadPoolExecutor(max_workers=min(16, os.cpu_count() or 4)) as pool:
        for h, seen, other in pool.map(probe, headers):
            if other:
                broken.append(f"<{h}>: {other[0]}")
            else:
                declared_somewhere |= seen
    return set(names) - declared_somewhere, broken


def verdict_undeclared(undecl: set[str], public: set[str], allowed: dict[str, str]) -> list[str]:
    """The violations of the other direction: a public name no header
    declares and UNDECLARED_OK does not excuse, and an UNDECLARED_OK entry
    that is declared now or no longer defined. Empty is clean."""
    out = [f"undeclared: {n} -- libc.a defines it and no header declares it: declare it "
           "(posix/include, as glibc 2.39's headers do), stop exporting it, or say why "
           "neither in UNDECLARED_OK"
           for n in sorted(undecl - set(allowed)) if not FLOAT16_128.fullmatch(n)]
    out += [f"stale UNDECLARED_OK entry: {n} is " + ("declared now" if n in public
                                                      else "no longer defined")
            + " -- delete it" for n in sorted(set(allowed) - undecl)]
    return out


def shape_module():
    """`check-libc-shape.py`, for its archive-index parser."""
    spec = importlib.util.spec_from_file_location("check_libc_shape", ROOT / "scripts" / "check-libc-shape.py")
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    return mod


def verdict(declared_names: dict[str, str], defined: set[str], baseline: frozenset) -> list[str]:
    """The violations: a declared function missing and not in the baseline,
    and a baseline name that is now defined. Empty is clean."""
    missing = {n for n in declared_names if n not in defined and not n.startswith("__")
               and n not in NOT_FUNCTIONS}
    out = [f"missing: {n} (declared in <{declared_names[n]}>) -- a C program calling it "
           f"compiles and does not link" for n in sorted(missing - baseline)]
    out += [f"stale baseline entry: {n} is defined now -- delete it from BASELINE_MISSING"
            for n in sorted(baseline - missing)]
    return out


def self_test() -> int:
    """Fixtures: the pattern, and the ratchet in both directions."""
    failures = []
    got = names_in("int foo(int);\nvoid bar(void) __attribute__((noreturn));\n"
                   "typedef int (*fp)(int);\nint (*table)(void);\nstatic int x;\n"
                   "int (qux)(int);\nvoid (*sigset(int, void (*)(int)))(int);\n"
                   'int old(void) __asm__("new_one") __attribute__((__deprecated__("x")));\n')
    if got != {"foo", "bar", "qux", "sigset", "new_one"}:
        failures.append(f"declaration pattern: {sorted(got)} -- a function returning a "
                        "function pointer is a function, a pointer to one is not; an asm "
                        "label is the symbol a call links to")
    decl = {"foo": "a.h", "bar": "b.h", "baz": "c.h", "return": "tgmath.h", "__internal": "d.h"}
    if verdict(decl, {"foo", "bar", "baz"}, frozenset()) != []:
        failures.append("a complete library was not clean")
    v = verdict(decl, {"foo"}, frozenset({"bar"}))
    if len(v) != 1 or "missing: baz" not in v[0]:
        failures.append(f"a new gap was not refused: {v}")
    v = verdict(decl, {"foo", "bar", "baz"}, frozenset({"bar"}))
    if len(v) != 1 or "stale baseline entry: bar" not in v[0]:
        failures.append(f"a stale exemption was not refused: {v}")
    pub = public_names({0: {"open", "_start", "__errno_location", "anon.1a2b.0.llvm.9",
                            "sqrtf128"}})
    if pub != {"open", "sqrtf128"}:
        failures.append(f"public names: {sorted(pub)} -- reserved and compiler names are not")
    v = verdict_undeclared({"ok", "new", "sqrtf128"}, {"ok", "new", "sqrtf128", "gone2"},
                           {"ok": "why", "gone": "why", "gone2": "why"})
    if len(v) != 3 or "undeclared: new" not in v[0] \
            or not any("entry: gone is no longer defined" in x for x in v) \
            or not any("entry: gone2 is declared now" in x for x in v):
        failures.append(f"the other direction's verdict: {v}")
    for f in failures:
        print(f"check-libc-declared --self-test: {f}", file=sys.stderr)
    if failures:
        return 1
    print("check-libc-declared --self-test: all 6 fixtures pass")
    return 0


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.split("\n", 1)[0])
    parser.add_argument("archive", nargs="?", default=None,
                        help="path to libc.a (default: toolchain/sysroot/lib/libc.a)")
    parser.add_argument("--self-test", "--selftest", action="store_true",
                        help="run this script's own fixtures")
    args = parser.parse_args(argv)
    if args.self_test:
        return self_test()

    path = Path(args.archive) if args.archive else ROOT / "toolchain" / "sysroot" / "lib" / "libc.a"
    if not path.is_file():
        print(f"ERROR: {path} does not exist -- run toolchain/build-sysroot.ps1 first.\n"
              "       (Exit 2, 'could not check', not a pass.)", file=sys.stderr)
        return 2
    zig = find_zig()
    inc = musl_include(zig) if zig else None
    if inc is None:
        print("SKIPPED: no zig (FASTPY_ZIG or PATH), or no musl headers in it -- the "
              "declarations cannot be read. (Exit 3: could not run.)", file=sys.stderr)
        return 3
    shape = shape_module()
    try:
        members = shape.parse_symbol_index(path)
    except shape.ArchiveError as exc:
        print(f"ERROR: {exc}", file=sys.stderr)
        return 2
    defined = set().union(*members.values()) if members else set()
    decl = declared(zig, inc, ROOT / "posix" / "include")
    # A floor on discovery: a broken preprocessor run must not read as "no
    # function is declared, so none is missing".
    if len(decl) < 1000 or len(defined) < 1000:
        print(f"ERROR: found {len(decl)} declarations and {len(defined)} definitions -- too "
              "few to judge; the headers or the archive were not read. (Exit 2.)", file=sys.stderr)
        return 2
    problems = verdict(decl, defined, BASELINE_MISSING)
    # The other direction.
    public = public_names(members)
    undecl, broken = undeclared(zig, inc, ROOT / "posix" / "include", public)
    if broken:
        problems += [f"a header did not compile, so what it declares is unknown: {b}"
                     for b in broken]
    problems += verdict_undeclared(undecl, public, UNDECLARED_OK)
    for p in problems:
        print(f"check-libc-declared: {p}", file=sys.stderr)
    if problems:
        return 1
    compiler = sum(1 for n in undecl if FLOAT16_128.fullmatch(n))
    print(f"check-libc-declared: {len(decl)} functions declared by musl's headers and "
          f"posix/include; {len(BASELINE_MISSING)} known missing (baseline), no new ones. "
          f"Of the {len(public)} public names libc.a defines, every one is declared but "
          f"{len(UNDECLARED_OK)} (UNDECLARED_OK) and {compiler} of the compiler's")
    return 0


if __name__ == "__main__":
    sys.exit(main())
