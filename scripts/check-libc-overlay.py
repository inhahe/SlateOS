#!/usr/bin/env python3
"""Check posix/include -- the C header overlay -- against glibc 2.39's headers.

C here is compiled against musl's headers, which declare only what musl
defines, and this C library defines more: glibc's extensions and C23's
additions. posix/include puts a header in front of each musl header that
lacks some of them (`-I posix/include` -- zig's driver searches its own libc
headers before any `-isystem` directory); the overlay header includes musl's
(`#include_next`) and declares the rest, under the feature macros
glibc declares them under. It also has whole headers for the families musl
has none of (<fts.h>, <error.h>, <execinfo.h> ...), and one in place of musl's:
<glob.h>, whose glob_t must name the fields musl's hides (a struct is declared
once, so it cannot be musl's with more after it). A C program written for
glibc compiles against it, then, as it does against glibc -- which is the
claim this gate holds it to.

What it checks
--------------
1. **Every overlay header compiles** -- on its own, and all of them together
   -- with `-Wall -Wextra -Werror`, in each of CONFIGS and in EXTRA_BUILDS
   (older C, C++), so that no feature-macro setting a program might use
   breaks it; and <stdbit.h>'s type-generic macros, expanded on each type
   they take, choose the function for its type (`macro_uses`); and each
   large-file alias the overlay defines as a macro (`#define fts64_open
   fts_open`), expanded where the large-file names are visible, names
   something declared there (`lfs64_uses`). (Not `-Wpedantic`, which objects to `#include_next` itself.) The headers are system headers to
   a program, as musl's are, and their warnings not its business;
   `_SLATEOS_OVERLAY_WARNINGS` makes them ordinary ones here, to be held to
   these warnings themselves.
2. **The overlay declares exactly the reference's names** -- those it adds to
   musl's headers, and those musl's headers declare under narrower feature
   macros than glibc's, which it declares again under glibc's: nothing glibc
   2.39 does not declare (a name no glibc program expects is a name a program
   may itself be using), and nothing the reference lists that the overlay has
   lost.
3. **Each name is declared where glibc declares it** -- after including the
   header glibc declares it in, in exactly the CONFIGS glibc's is (KNOWN
   lists where musl's own headers make the difference) -- **with glibc's
   type**: `__builtin_types_compatible_p` against the type clang read out of
   glibc's headers, typedefs resolved.

4. **The types the overlay defines have glibc's layouts** -- `femode_t`,
   `FTS`, `FTSENT`, `struct mallinfo` and the rest, which musl's headers have
   none of: each one's size, and each field's offset, against glibc's
   (OVERLAY_TYPES, which must name every struct the overlay's text defines).
   `check-libc-abi.py` holds the library's Rust types to these, and so, one
   step removed, to glibc's.

The declarations' calling conventions against the library's definitions are
scripts/check-libc-prototypes.py's to check; this one checks what a program
compiling against the overlay sees.

The reference
-------------
posix/tools/oracle/glibc_declarations.txt: for each name the overlay
declares, the header glibc 2.39 declares it in, the CONFIGS it is declared
in, and its type; and glibc_layouts.txt, each OVERLAY_TYPES type's size and
field offsets -- both read out of glibc's headers with clang by
posix/tools/oracle/glibc_declarations.py, under WSL. After adding to the
overlay, regenerate them:

    python posix/tools/oracle/glibc_declarations.py

Usage
-----
    python scripts/check-libc-overlay.py
    python scripts/check-libc-overlay.py --self-test

Exit: 0 every check holds; 1 a finding; 2 the reference is missing or
malformed; 3 could not run (no zig -- FASTPY_ZIG or PATH -- or no musl
headers in it).
"""
from __future__ import annotations

import argparse
import concurrent.futures
import os
import re
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
OVERLAY = ROOT / "posix" / "include"
REFERENCE = ROOT / "posix" / "tools" / "oracle" / "glibc_declarations.txt"
LAYOUTS = ROOT / "posix" / "tools" / "oracle" / "glibc_layouts.txt"

# The feature-macro settings a declaration's visibility is compared in: the
# same names, and the same flags, as glibc_declarations.py reads glibc with.
CONFIGS: dict[str, list[str]] = {
    "c17": ["-std=c17"],
    "default": ["-std=gnu17"],
    "gnu": ["-std=gnu17", "-D_GNU_SOURCE"],
    "c23": ["-std=c2x"],
    "gnu23": ["-std=gnu2x"],
    "xopen": ["-std=c17", "-D_XOPEN_SOURCE=700"],
    "posix": ["-std=c17", "-D_POSIX_C_SOURCE=200809L"],
    "bsd": ["-std=c17", "-D_DEFAULT_SOURCE"],
    "bfp": ["-std=c17", "-D__STDC_WANT_IEC_60559_BFP_EXT__"],
    "ext": ["-std=c17", "-D__STDC_WANT_IEC_60559_EXT__"],
    "lfs64": ["-std=c17", "-D_LARGEFILE64_SOURCE"],
}

# Where everything the overlay has is declared: the names it adds are those
# visible here with it and not without it.
WIDEST = ["-std=gnu2x", "-D_GNU_SOURCE", "-D__STDC_WANT_IEC_60559_EXT__"]

# Further settings the headers must compile in, whose visibility is not
# compared: older C, the two large-file settings together, and C++.
EXTRA_BUILDS: dict[str, list[str]] = {
    "c99": ["-std=c99"],
    "gnu89": ["-std=gnu89"],
    "gnu+lfs64": ["-std=gnu17", "-D_GNU_SOURCE", "-D_LARGEFILE64_SOURCE"],
    "c++17": ["-x", "c++", "-std=c++17"],
    "gnu++20": ["-x", "c++", "-std=gnu++20", "-D_GNU_SOURCE"],
}

# (name, config) -> why its visibility there is not glibc's: a difference
# musl's own header makes, which the overlay has no say in.
KNOWN: dict[tuple[str, str], str] = {
    ("getdents64", "lfs64"): "musl's <dirent.h> makes it a macro for getdents under "
                             "_LARGEFILE64_SOURCE, which glibc declares it without",
    **{("pidfd_send_signal", cfg): "musl's <signal.h> has siginfo_t only for a POSIX "
                                   "compilation, and there is none in a strict ISO C one "
                                   "for the declaration to take"
       for cfg in ("c17", "c23", "bfp", "ext", "lfs64")},
}

# The types the overlay defines itself -- musl's headers have none of them, so
# their layouts are the overlay's to get right -- and the header each is in.
# Check 4 holds each to glibc's; `defined_types` sees that none is missing.
OVERLAY_TYPES: dict[str, str] = {
    "femode_t": "fenv.h",
    "FTS": "fts.h",
    "FTSENT": "fts.h",
    "struct mallinfo": "malloc.h",
    "struct mallinfo2": "malloc.h",
    "cookie_io_functions_t": "stdio.h",
    "struct random_data": "stdlib.h",
    "struct drand48_data": "stdlib.h",
    "struct sigstack": "signal.h",
    "Dl_serpath": "dlfcn.h",
    "Dl_serinfo": "dlfcn.h",
    "struct dl_find_object": "dlfcn.h",
    "glob_t": "glob.h",
}

# glibc's name for a field, where the overlay's differs: the overlay's.
FIELD_NAMES: dict[tuple[str, str], str] = {("femode_t", "__glibc_reserved"): "__reserved"}

# Where the layouts are read, both sides: C23, for femode_t; not _GNU_SOURCE,
# so that <stdio.h>'s cookie types are the overlay's own and not musl's.
LAYOUT_FLAGS = ["-std=gnu2x"]
# ... and what a header's types need besides: <dlfcn.h>'s are _GNU_SOURCE's
# alone, in glibc's header and the overlay's alike.
LAYOUT_EXTRA_FLAGS: dict[str, list[str]] = {"dlfcn.h": ["-D_GNU_SOURCE"]}


def layout_flags(header: str) -> list[str]:
    """The flags `header`'s types' layouts are read with, both sides."""
    return LAYOUT_FLAGS + LAYOUT_EXTRA_FLAGS.get(header, [])

# glibc's names for types whose musl names differ, in the reference's types.
# `__sigset_t` is glibc's unnamed struct behind sigset_t; musl's has the tag
# `struct __sigset_t` and no typedef of that name. `__mbstate_t` is the same
# for mbstate_t (<uchar.h>'s mbrtoc8 and c8rtomb).
# `utmp` is `utmpx`: musl's <utmp.h> defines `struct utmp` as `struct utmpx`
# (`#define utmp utmpx`), so glibc's `getutmp (const struct utmpx *, struct
# utmp *)` is the overlay's with `struct utmpx *` twice -- in a unit that has
# not included <utmp.h> there is no `struct utmp` to name.
TYPE_NAMES = {"__sigset_t": "sigset_t", "__mbstate_t": "mbstate_t", "utmp": "utmpx"}

# How clang says a name is not declared -- for a library function it knows
# the type of, "undeclared library function".
# ... and that a word is a type's name, not an object's or a function's.
TYPE_NAME = re.compile(r"unexpected type name '(\w+)'")
UNDECLARED = re.compile(
    r"(?:use of undeclared identifier|call to undeclared (?:library )?function) '(\w+)'")
FAILED_ASSERT = re.compile(r"@(\w+)@")
C_WORDS = frozenset("""auto break case char const continue default defined do double else enum
extern float for goto if inline int long register restrict return short signed sizeof static
struct switch typedef union unsigned void volatile while""".split())


def find_zig() -> str | None:
    z = os.environ.get("FASTPY_ZIG")
    if z and Path(z).exists():
        return z
    return shutil.which("zig")


def musl_include(zig: str) -> Path | None:
    inc = Path(zig).resolve().parent / "lib" / "libc" / "include" / "generic-musl"
    return inc if inc.is_dir() else None


def overlay_headers(overlay: Path) -> list[str]:
    """The overlay's headers a program includes (not its bits/)."""
    return sorted(p.relative_to(overlay).as_posix() for p in overlay.rglob("*.h")
                  if not p.relative_to(overlay).as_posix().startswith("bits/"))


def compile_c(zig: str, source: str, flags: list[str], overlay: Path | None) -> tuple[int, str]:
    """Compile `source` for the target; (exit status, diagnostics). To an
    object, not -fsyntax-only, which zig's driver reports as a missing file."""
    with tempfile.TemporaryDirectory(prefix="libc-overlay-") as t:
        src = Path(t) / "t.c"
        src.write_text(source, encoding="utf-8", newline="\n")
        cmd = [zig, "cc", "--target=x86_64-linux-musl", "-c", "-o", str(Path(t) / "t.o")]
        if overlay is not None:
            cmd += ["-I", str(overlay)]
        r = subprocess.run(cmd + flags + [str(src)], capture_output=True, text=True,
                           encoding="utf-8", errors="replace", timeout=300)
    return r.returncode, r.stderr


def errors_of(diagnostics: str) -> list[str]:
    return [line for line in diagnostics.splitlines() if ": error: " in line]


def visible(zig: str, header: str | list[str], names: list[str], flags: list[str],
            overlay: Path | None) -> set[str]:
    """Which of `names` are declared once `header` (or each of a list) is
    included: a probe that takes each one's address, and clang's word for
    each it does not know. Any other error is the headers' fault and raised."""
    heads = [header] if isinstance(header, str) else header
    src = "".join(f"#include <{h}>\n" for h in heads)
    src += "void slateos_overlay_probe(void) {\n" + "".join(f"(void)&{n};\n" for n in names) + "}\n"
    _, diag = compile_c(zig, src, flags + ["-w", "-ferror-limit=0"], overlay)
    missing = set()
    other = []
    for line in errors_of(diag):
        m = UNDECLARED.search(line) or TYPE_NAME.search(line)
        if m:
            missing.add(m.group(1))
        else:
            other.append(line)
    if other:
        raise RuntimeError(f"probing <{', '.join(heads)}> with {' '.join(flags)}:\n  "
                           + "\n  ".join(other[:10]))
    return set(names) - missing


def c_type(glibc_type: str) -> str:
    """A reference type in musl's names."""
    return re.sub(r"\b\w+\b", lambda m: TYPE_NAMES.get(m.group(0), m.group(0)), glibc_type)


def mistyped(zig: str, header: str, entries: list[tuple[str, str]], flags: list[str],
             overlay: Path) -> set[str]:
    """Which (name, type) pairs' names do not have a type compatible with it."""
    src = f"#include <{header}>\n" + "".join(
        f"_Static_assert(__builtin_types_compatible_p(__typeof__({n}), {c_type(t)}), \"@{n}@\");\n"
        for n, t in entries)
    _, diag = compile_c(zig, src, flags + ["-w", "-ferror-limit=0"], overlay)
    bad = set()
    other = []
    for line in errors_of(diag):
        m = FAILED_ASSERT.search(line)
        if m and "static assertion failed" in line:
            bad.add(m.group(1))
        else:
            other.append(line)
    if other:
        raise RuntimeError(f"checking types in <{header}>:\n  " + "\n  ".join(other[:10]))
    return bad


def candidates(overlay: Path) -> list[str]:
    """Every identifier the overlay's headers might declare: each written
    before a parenthesis or as the name of an extern object, less keywords,
    reserved names and the overlay's own macros -- a superset, which the
    probe narrows to what is declared."""
    names: set[str] = set()
    macros: set[str] = set()
    for p in overlay.rglob("*.h"):
        text = re.sub(r"/\*.*?\*/", " ", p.read_text(encoding="utf-8"), flags=re.S)
        macros |= set(re.findall(r"#\s*define\s+(\w+)", text))
        text = "\n".join(line for line in text.splitlines() if not line.lstrip().startswith("#"))
        names |= set(re.findall(r"\b([A-Za-z_]\w*)\s*\(", text))
        names |= set(re.findall(r"\(\s*\*\s*([A-Za-z_]\w*)\s*\)\s*\(", text))
        names |= set(re.findall(r"\bextern\b[^;(]*?\b([A-Za-z_]\w*)\s*;", text))
    return sorted(n for n in names - macros - C_WORDS if not n.startswith("_"))


def overlay_names(zig: str, overlay: Path) -> set[str]:
    """The names the overlay's own text declares: those it adds to musl's
    headers, and those it declares under wider feature macros than musl's
    headers do (glibc's, which musl's are narrower than for some)."""
    return visible(zig, overlay_headers(overlay), candidates(overlay), WIDEST, overlay)


def defined_types(overlay: Path) -> list[frozenset[str]]:
    """Each struct the overlay's text defines, as the names it goes by: its
    tag, as `struct tag`, and the typedef naming it."""
    out = []
    for p in sorted(overlay.rglob("*.h")):
        text = re.sub(r"/\*.*?\*/", " ", p.read_text(encoding="utf-8"), flags=re.S)
        for m in re.finditer(r"\b(typedef\s+)?struct(?:\s+(\w+))?\s*\{", text):
            depth, i = 1, m.end()
            while depth and i < len(text):
                depth += {"{": 1, "}": -1}.get(text[i], 0)
                i += 1
            names = {f"struct {m.group(2)}"} if m.group(2) else set()
            if m.group(1):
                after = re.match(r"\s*(\w+)\s*;", text[i:])
                if after:
                    names.add(after.group(1))
            if names:
                out.append(frozenset(names))
    return out


def read_layouts(path: Path) -> dict[str, tuple[str, int, list[tuple[str, int]]]]:
    """C type -> (header, size, [(field, offset)]), glibc's."""
    out = {}
    for n, line in enumerate(path.read_text(encoding="utf-8").splitlines(), 1):
        if not line.strip() or line.startswith("#"):
            continue
        parts = line.split("\t")
        if len(parts) != 4:
            raise ValueError(f"{path.name}:{n}: expected 4 tab-separated fields")
        cty, header, size, fields = parts
        pairs = []
        for f in fields.split(","):
            name, _, off = f.partition("=")
            pairs.append((name, int(off)))
        out[cty] = (header, int(size), pairs)
    return out


def mislaid(zig: str, overlay: Path,
            layouts: dict[str, tuple[str, int, list[tuple[str, int]]]]) -> list[str]:
    """Check 4: each OVERLAY_TYPES type's size and offsets against glibc's."""
    problems = []
    for names in defined_types(overlay):
        if not names & set(OVERLAY_TYPES):
            problems.append(f"the overlay defines {' / '.join(sorted(names))}, which "
                            "OVERLAY_TYPES does not list: its layout is held to nothing")
    by_header: dict[str, list[str]] = {}
    for cty, hdr in sorted(OVERLAY_TYPES.items()):
        if cty not in layouts:
            problems.append(f"{cty} is in OVERLAY_TYPES and not in {LAYOUTS.name}: regenerate "
                            "it (posix/tools/oracle/glibc_declarations.py)")
        else:
            by_header.setdefault(hdr, []).append(cty)
    for hdr, types in sorted(by_header.items()):
        what: list[str] = []
        src = f"#include <{hdr}>\n#include <stddef.h>\n"
        for cty in types:
            _, size, fields = layouts[cty]
            src += f"_Static_assert(sizeof({cty}) == {size}, \"@{len(what)}@\");\n"
            what.append(f"sizeof({cty}) is not glibc's {size}")
            for f, off in fields:
                ours = FIELD_NAMES.get((cty, f), f)
                src += f"_Static_assert(offsetof({cty}, {ours}) == {off}, \"@{len(what)}@\");\n"
                what.append(f"{cty}'s {ours} is not at glibc's offset {off}")
        _, diag = compile_c(zig, src, layout_flags(hdr) + ["-w", "-ferror-limit=0"], overlay)
        for line in errors_of(diag):
            m = FAILED_ASSERT.search(line)
            if m and "static assertion failed" in line:
                problems.append(f"<{hdr}>: {what[int(m.group(1))]}")
            else:
                problems.append(f"<{hdr}>, checking layouts: {line}")
    return problems


def read_reference(path: Path) -> dict[str, tuple[str, frozenset[str], str]]:
    """name -> (header, the CONFIGS it is declared in, its type)."""
    out = {}
    for n, line in enumerate(path.read_text(encoding="utf-8").splitlines(), 1):
        if not line.strip() or line.startswith("#"):
            continue
        parts = line.split("\t")
        if len(parts) != 4:
            raise ValueError(f"{path.name}:{n}: expected 4 tab-separated fields")
        name, header, configs, ctype = parts
        cs = frozenset(configs.split(",")) if configs != "-" else frozenset()
        if not cs <= set(CONFIGS):
            raise ValueError(f"{path.name}:{n}: unknown configuration in {configs!r}")
        out[name] = (header, cs, ctype)
    return out


def macro_uses() -> str:
    """A translation unit using each of the overlay's type-generic macros --
    <stdbit.h>'s -- on each type it takes, and asserting, as a constant
    expression, that each chose by the argument's type: the counts are
    `unsigned int`, `stdc_has_single_bit` a bool, and `stdc_bit_floor` and
    `stdc_bit_ceil` the argument's own type. A header that only declares
    macros is compiled and never expands them; this expands every one."""
    types = ["unsigned char", "unsigned short", "unsigned int", "unsigned long",
             "unsigned long long"]
    counts = ["leading_zeros", "leading_ones", "trailing_zeros", "trailing_ones",
              "first_leading_zero", "first_leading_one", "first_trailing_zero",
              "first_trailing_one", "count_zeros", "count_ones", "bit_width"]
    src = ["#include <stdbit.h>"]
    for i, t in enumerate(types):
        v = f"(({t})1)"
        for f in counts:
            src.append(f"_Static_assert(_Generic(stdc_{f}({v}), unsigned int: 1, default: 0), "
                       f"\"stdc_{f} on {t}\");")
        src.append(f"_Static_assert(_Generic(stdc_has_single_bit({v}), bool: 1, default: 0), "
                   f"\"stdc_has_single_bit on {t}\");")
        for f in ("bit_floor", "bit_ceil"):
            src.append(f"_Static_assert(_Generic(stdc_{f}({v}), {t}: 1, default: 0), "
                       f"\"stdc_{f} on {t}\");")
        # and a call, so the functions it names are declared as used
        src.append(f"unsigned int use{i}({t} x) {{ return stdc_leading_zeros(x) + "
                   f"(unsigned int)stdc_bit_ceil(x); }}")
    return "\n".join(src) + "\n"


def lfs64_aliases(overlay: Path) -> list[tuple[str, str]]:
    """The large-file aliases the overlay's headers define as macros, as
    musl's headers do (`#define glob64 glob`, `#define FTS64 FTS`): each
    `#define` of a name with 64 in it to another name, sorted."""
    found: set[tuple[str, str]] = set()
    for p in overlay.rglob("*.h"):
        text = re.sub(r"/\*.*?\*/", " ", p.read_text(encoding="utf-8"), flags=re.S)
        for alias, name in re.findall(
                r"^[ \t]*#[ \t]*define[ \t]+([A-Za-z]\w*)[ \t]+([A-Za-z_]\w*)[ \t]*$",
                text, flags=re.M):
            if "64" in alias:
                found.add((alias, name))
    return sorted(found)


def lfs64_uses(overlay: Path) -> str:
    """A translation unit that includes every overlay header and names each
    large-file alias through `__typeof__`, which takes a type or an
    expression alike: an alias whose standard name is not declared where
    the alias is visible -- a misspelling, a feature macro missed -- does not
    compile. A header that only defines a macro never expands it; this
    expands every one."""
    src = [f"#include <{h}>" for h in overlay_headers(overlay)]
    for i, (alias, _) in enumerate(lfs64_aliases(overlay)):
        src.append(f"typedef __typeof__({alias}) *lfs64_use{i};")
    return "\n".join(src) + "\n"


def check_builds(zig: str, overlay: Path) -> list[str]:
    """Check 1: the headers compile, alone and together, everywhere."""
    heads = overlay_headers(overlay)
    together = "".join(f"#include <{h}>\n" for h in heads)
    jobs = [(f"<{h}> alone ({cfg})", f"#include <{h}>\n", flags)
            for h in heads for cfg, flags in (("gnu", CONFIGS["gnu"]), ("c17", CONFIGS["c17"]))]
    jobs += [(f"every header together ({cfg})", together, flags)
             for cfg, flags in {**CONFIGS, **EXTRA_BUILDS}.items()]
    jobs += [(f"<stdbit.h>'s type-generic macros ({cfg})", macro_uses(), flags)
             for cfg, flags in (("c11", ["-std=c11"]), ("c23", CONFIGS["c23"]),
                                ("gnu", CONFIGS["gnu"]))]
    jobs.append(("the large-file aliases (gnu+lfs64)", lfs64_uses(overlay),
                 EXTRA_BUILDS["gnu+lfs64"]))
    strict = ["-D_SLATEOS_OVERLAY_WARNINGS", "-Wall", "-Wextra", "-Werror"]

    def run(job):
        what, src, flags = job
        rc, diag = compile_c(zig, src, flags + strict, overlay)
        return what, rc, diag

    problems = []
    with concurrent.futures.ThreadPoolExecutor(max_workers=min(16, os.cpu_count() or 4)) as pool:
        for what, rc, diag in pool.map(run, jobs):
            if rc != 0:
                first = (errors_of(diag) or diag.splitlines() or ["(no diagnostics)"])[:3]
                problems.append(f"{what} does not compile cleanly: " + " | ".join(first))
    return problems


def check_declarations(zig: str, overlay: Path,
                       ref: dict[str, tuple[str, frozenset[str], str]]) -> tuple[list[str], int]:
    """Checks 2 and 3; (problems, the number of names compared)."""
    problems = []
    names = overlay_names(zig, overlay)
    for n in sorted(names - set(ref)):
        problems.append(f"the overlay declares {n}, which the reference does not have: glibc 2.39 "
                        "declares no such name, or the reference predates it -- regenerate it "
                        "(posix/tools/oracle/glibc_declarations.py)")
    for n in sorted(set(ref) - names):
        problems.append(f"{n} is in the reference but the overlay no longer declares it")
    by_header: dict[str, list[str]] = {}
    for n in sorted(names & set(ref)):
        by_header.setdefault(ref[n][0], []).append(n)
    jobs = [(h, cfg) for h in sorted(by_header) for cfg in CONFIGS]

    def probe(job):
        h, cfg = job
        return h, cfg, visible(zig, h, by_header[h], CONFIGS[cfg], overlay)

    with concurrent.futures.ThreadPoolExecutor(max_workers=min(16, os.cpu_count() or 4)) as pool:
        seen = list(pool.map(probe, jobs))
        typed = dict(zip(sorted(by_header), pool.map(
            lambda h: mistyped(zig, h, [(n, ref[n][2]) for n in by_header[h]], WIDEST, overlay),
            sorted(by_header))))
    for h, cfg, vis in seen:
        for n in by_header[h]:
            want = cfg in ref[n][1]
            if (n in vis) != want and (n, cfg) not in KNOWN:
                how = "declares" if n in vis else "does not declare"
                problems.append(f"<{h}> {how} {n} in the {cfg} configuration "
                                f"({' '.join(CONFIGS[cfg])}), where glibc's "
                                f"{'does' if want else 'does not'}")
    for (n, cfg), _why in KNOWN.items():
        if n in ref and any(h == ref[n][0] and c == cfg and ((n in vis) == (cfg in ref[n][1]))
                            for h, c, vis in seen):
            problems.append(f"KNOWN[({n!r}, {cfg!r})] no longer differs from glibc: delete it")
    for h, bad in typed.items():
        for n in sorted(bad):
            problems.append(f"<{h}> declares {n} with a type incompatible with glibc's "
                            f"`{ref[n][2]}`")
    return problems, len(names & set(ref))


def self_test() -> int:
    failures = 0

    def check(what: str, ok: bool) -> None:
        nonlocal failures
        if not ok:
            failures += 1
            print(f"FAIL: {what}")

    check("a glibc type name maps to musl's",
          c_type("int (int, const __sigset_t *)") == "int (int, const sigset_t *)")
    check("an unrelated name is left alone", c_type("__sigset_tx *") == "__sigset_tx *")
    with tempfile.TemporaryDirectory() as t:
        d = Path(t)
        (d / "a.h").write_text(
            "/* comment(x) */\n#define M(x) x\nint f(int);\nextern int v;\n"
            "extern void (*hook)(void);\nstruct s { int (*g)(void); };\nint _r(void);\n",
            encoding="utf-8", newline="")
        c = candidates(d)
        check("a function is a candidate", "f" in c)
        check("an extern object is a candidate", "v" in c)
        check("a function pointer object is a candidate", "hook" in c)
        check("a comment's words are not", "comment" not in c)
        check("a macro is not", "M" not in c)
        check("a reserved name is not", "_r" not in c)
        (d / "r.txt").write_text("# x\nf\ta.h\tgnu,c17\tint (int)\n", encoding="utf-8", newline="")
        ref = read_reference(d / "r.txt")
        check("the reference is read", ref == {"f": ("a.h", frozenset({"gnu", "c17"}), "int (int)")})
        (d / "bad.txt").write_text("f\ta.h\tnonesuch\tint (int)\n", encoding="utf-8", newline="")
        try:
            read_reference(d / "bad.txt")
            check("an unknown configuration is refused", False)
        except ValueError:
            pass
        (d / "defs").mkdir()
        (d / "defs" / "b.h").write_text(
            "typedef struct { int a; } plain_t;\nstruct tagged { int b; };\n"
            "typedef struct both_tag { int c; } both_t;\nstruct tagged *use(void);\n",
            encoding="utf-8", newline="")
        found = set(defined_types(d / "defs"))
        check("an unnamed struct's typedef, a tag and a tagged typedef are found",
              found == {frozenset({"plain_t"}), frozenset({"struct tagged"}),
                        frozenset({"struct both_tag", "both_t"})})
        (d / "l.txt").write_text("# x\nfemode_t\tfenv.h\t8\t__control_word=0,__mxcsr=4\n",
                                 encoding="utf-8", newline="")
        check("the layouts are read",
              read_layouts(d / "l.txt") == {"femode_t": ("fenv.h", 8, [("__control_word", 0),
                                                                     ("__mxcsr", 4)])})
        (d / "lfs").mkdir()
        (d / "lfs" / "c.h").write_text(
            "#define f64 f\n  #  define T64 T\n#define __NEED_off64_t\n#define N64 1\n"
            "/* #define gone64 gone */\n#define g64(x) g(x)\n#define h h64\n",
            encoding="utf-8", newline="")
        check("a large-file alias is found, and nothing else is",
              lfs64_aliases(d / "lfs") == [("T64", "T"), ("f64", "f")])
    zig = find_zig()
    if zig and musl_include(zig):
        with tempfile.TemporaryDirectory() as t:
            d = Path(t)
            (d / "stdio.h").write_text(
                "#include_next <stdio.h>\n#ifdef _GNU_SOURCE\nint fcloseall(void);\n#endif\n",
                encoding="utf-8", newline="")
            v = visible(zig, "stdio.h", ["fcloseall", "printf"], CONFIGS["gnu"], d)
            check("the probe sees an overlay declaration", v == {"fcloseall", "printf"})
            v = visible(zig, "stdio.h", ["fcloseall", "printf"], CONFIGS["c17"], d)
            check("and not where its feature macro is off", v == {"printf"})
            bad = mistyped(zig, "stdio.h", [("fcloseall", "int (void)"), ("printf", "int (int)")],
                           CONFIGS["gnu"], d)
            check("a wrong type is caught, a right one passes", bad == {"printf"})
            check("the overlay's names are what it adds to musl's",
                  overlay_names(zig, d) == {"fcloseall"})
            lfs = d / "lfs"
            (lfs / "sys").mkdir(parents=True)
            (lfs / "sys" / "al.h").write_text(
                "#include <sys/stat.h>\n#include <glob.h>\n#define fstatx64 fstat\n"
                "#define globx64_t glob_t\n", encoding="utf-8", newline="")
            ok, _ = compile_c(zig, lfs64_uses(lfs), EXTRA_BUILDS["gnu+lfs64"] + ["-Werror"], lfs)
            check("an alias for a declared function or type compiles", ok == 0)
            (lfs / "sys" / "al.h").write_text(
                "#include <sys/stat.h>\n#define fstatx64 fstatt\n", encoding="utf-8", newline="")
            bad, _ = compile_c(zig, lfs64_uses(lfs), EXTRA_BUILDS["gnu+lfs64"] + ["-Werror"], lfs)
            check("an alias for an undeclared name does not", bad != 0)
            (d / "sys").mkdir()
            (d / "sys" / "lay.h").write_text("typedef struct { int a; long b; } lay_t;\n",
                                             encoding="utf-8", newline="")
            saved = dict(OVERLAY_TYPES)
            try:
                OVERLAY_TYPES.clear()
                OVERLAY_TYPES["lay_t"] = "sys/lay.h"
                good = {"lay_t": ("sys/lay.h", 16, [("a", 0), ("b", 8)])}
                bad = {"lay_t": ("sys/lay.h", 16, [("a", 0), ("b", 4)])}
                check("a layout that is glibc's passes", mislaid(zig, d, good) == [])
                check("an offset that is not is caught",
                      any("b is not at glibc's offset 4" in p for p in mislaid(zig, d, bad)))
                OVERLAY_TYPES.clear()
                check("a struct OVERLAY_TYPES does not list is caught",
                      any("lay_t" in p and "does not list" in p for p in mislaid(zig, d, good)))
            finally:
                OVERLAY_TYPES.clear()
                OVERLAY_TYPES.update(saved)
    else:
        print("check-libc-overlay self-test: no zig; the compiler cases were not run")
    print(f"check-libc-overlay self-test: {failures} failure(s)")
    return 1 if failures else 0


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.split("\n", 1)[0])
    parser.add_argument("--self-test", "--selftest", action="store_true")
    parser.add_argument("--overlay", default=str(OVERLAY), help="the overlay (default: this tree's)")
    parser.add_argument("--reference", default=str(REFERENCE), help="the reference to compare with")
    parser.add_argument("--layouts", default=str(LAYOUTS), help="glibc's layouts to compare with")
    args = parser.parse_args(argv)
    if args.self_test:
        return self_test()
    zig = find_zig()
    if zig is None or musl_include(zig) is None:
        print("SKIPPED: no zig (FASTPY_ZIG or PATH), or no musl headers in it. (Exit 3: could not "
              "run.)", file=sys.stderr)
        return 3
    overlay = Path(args.overlay)
    try:
        ref = read_reference(Path(args.reference))
        layouts = read_layouts(Path(args.layouts))
    except (OSError, ValueError) as e:
        print(f"ERROR: the reference: {e} (Exit 2.)", file=sys.stderr)
        return 2
    if len(ref) < 100:
        print(f"ERROR: {len(ref)} names in the reference -- too few to be it. (Exit 2.)",
              file=sys.stderr)
        return 2
    try:
        problems = check_builds(zig, overlay)
        found, compared = check_declarations(zig, overlay, ref)
        problems += mislaid(zig, overlay, layouts)
    except RuntimeError as e:
        print(f"check-libc-overlay: {e}", file=sys.stderr)
        return 1
    problems += found
    for p in problems:
        print(f"check-libc-overlay: {p}", file=sys.stderr)
    if problems:
        return 1
    print(f"check-libc-overlay: {len(overlay_headers(overlay))} headers compile in "
          f"{len(CONFIGS) + len(EXTRA_BUILDS)} settings; {compared} declarations appear where "
          f"glibc 2.39's do ({len(CONFIGS)} settings each), with glibc's types; "
          f"{len(OVERLAY_TYPES)} types it defines have glibc's layouts; "
          f"{len(lfs64_aliases(overlay))} large-file aliases name what they alias")
    return 0


if __name__ == "__main__":
    sys.exit(main())
