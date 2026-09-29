#!/usr/bin/env python3
"""Refuse a function musl's headers declare that `libc.a` does not define.

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

Every header under zig's `generic-musl` include directory is preprocessed on
its own (some pairs conflict) with `_GNU_SOURCE`, `_BSD_SOURCE` and
`_LARGEFILE64_SOURCE`, so every declaration a program can reach is visible.
Each declaration ending in `);` names a function. The names `libc.a` defines
come from its archive index, read by `check-libc-shape.py`'s parser. The
difference, less `NOT_FUNCTIONS` (text the declaration pattern mistakes for a
name), is what a program can call and not link.

It is a ratchet: `BASELINE_MISSING` is what was missing when the gate was
written, less what has been implemented since. The gate fails when

  - a declared function is missing and not in the baseline (a regression, or a
    new header declaration nobody implemented), or
  - a baseline name is now defined (a stale exemption: delete the line).

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

DECL = re.compile(r"[^;{}]*\)\s*(?:__attribute__\s*\(\(.*?\)\)\s*)*;", re.S)
# `int (name)(...)`: a parenthesised declarator, which keeps a function-like
# macro of the same name from expanding.
PAREN_NAME = re.compile(r"\s*([A-Za-z_]\w*)\s*\)\s*\(")
LAST_IDENT = re.compile(r"([A-Za-z_]\w*)\s*$")
NOT_NAMES = {"__attribute__", "sizeof", "__typeof__", "_Static_assert", "void", "__asm__",
             "__asm", "__extension__"}


FN_RETURNING_FN_PTR = re.compile(r"\*\s*([A-Za-z_]\w*)\s*\(")


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
            names.add(n)
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
            out.setdefault(n, files[at] if at >= 0 else "")
    return out


def declared(zig: str, inc: Path) -> dict[str, str]:
    """Function name -> the header that declares it (relative to `inc`)."""
    out: dict[str, str] = {}
    headers = sorted(p.relative_to(inc).as_posix() for p in inc.rglob("*.h")
                     if not p.relative_to(inc).as_posix().startswith("bits/"))
    root = inc.resolve().as_posix().lower()
    with tempfile.TemporaryDirectory() as t:
        src = Path(t) / "h.c"
        for h in headers:
            src.write_text(f"#include <{h}>\n", encoding="utf-8", newline="")
            r = subprocess.run([zig, "cc", "--target=x86_64-linux-musl", "-E",
                                "-D_GNU_SOURCE", "-D_BSD_SOURCE", "-D_LARGEFILE64_SOURCE", str(src)],
                               capture_output=True, text=True, encoding="utf-8", errors="replace",
                               timeout=120)
            if r.returncode != 0:
                continue
            for n, f in declarations_by_file(r.stdout).items():
                f = f.replace("\\\\", "/").replace("\\", "/")
                rel = f[len(root) + 1:] if f.lower().startswith(root) else f.rsplit("/include/", 1)[-1]
                out.setdefault(n, rel)
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
                   "int (qux)(int);\nvoid (*sigset(int, void (*)(int)))(int);\n")
    if got != {"foo", "bar", "qux", "sigset"}:
        failures.append(f"declaration pattern: {sorted(got)} -- a function returning a "
                        "function pointer is a function, a pointer to one is not")
    decl = {"foo": "a.h", "bar": "b.h", "baz": "c.h", "return": "tgmath.h", "__internal": "d.h"}
    if verdict(decl, {"foo", "bar", "baz"}, frozenset()) != []:
        failures.append("a complete library was not clean")
    v = verdict(decl, {"foo"}, frozenset({"bar"}))
    if len(v) != 1 or "missing: baz" not in v[0]:
        failures.append(f"a new gap was not refused: {v}")
    v = verdict(decl, {"foo", "bar", "baz"}, frozenset({"bar"}))
    if len(v) != 1 or "stale baseline entry: bar" not in v[0]:
        failures.append(f"a stale exemption was not refused: {v}")
    for f in failures:
        print(f"check-libc-declared --self-test: {f}", file=sys.stderr)
    if failures:
        return 1
    print("check-libc-declared --self-test: all 4 fixtures pass")
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
    decl = declared(zig, inc)
    # A floor on discovery: a broken preprocessor run must not read as "no
    # function is declared, so none is missing".
    if len(decl) < 1000 or len(defined) < 1000:
        print(f"ERROR: found {len(decl)} declarations and {len(defined)} definitions -- too "
              "few to judge; the headers or the archive were not read. (Exit 2.)", file=sys.stderr)
        return 2
    problems = verdict(decl, defined, BASELINE_MISSING)
    for p in problems:
        print(f"check-libc-declared: {p}", file=sys.stderr)
    if problems:
        return 1
    print(f"check-libc-declared: {len(decl)} functions declared by musl's headers; "
          f"{len(BASELINE_MISSING)} known missing (baseline), no new ones")
    return 0


if __name__ == "__main__":
    sys.exit(main())
