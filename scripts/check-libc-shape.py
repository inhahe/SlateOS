#!/usr/bin/env python3
"""Assert that `libc.a` has the *object granularity* a libc archive must have.

WHAT THIS GUARDS, AND WHY IT IS NOT A STYLE CHECK
=================================================

Static linking extracts archive members **whole**. A member is pulled in if it
defines any symbol still undefined at that point, and then *every* symbol it
defines becomes a definition in the output. So the way a libc's functions are
distributed across its members is not an implementation detail -- it is part of
what programs can link against it. glibc puts (near enough) one `.c` file in
one member for exactly this reason.

We violated that and it cost us a port. `libc.a` is built by `cargo build`,
whose default `codegen-units = 16` packs the whole `posix` crate into 16
objects, grouping unrelated modules together. The result was:

    fnmatch  shared a member with  fopen, fwrite, stdout, isalpha
    glob     shared a member with  printf, snprintf, vfprintf, uname
    getopt   shared a member with  sem_wait, sched_getaffinity
    error    shared a member with  getenv, environ, setenv, regcomp

Those four left-hand names are precisely the ones **gnulib supplies
replacements for**, which means every GNU package that vendors gnulib --
coreutils, grep, sed, tar, findutils, diffutils, gawk, gcc, binutils, make --
defines them itself. Normally that is harmless: the program's own `getopt`
resolves the reference, libc's member is never extracted, and nothing
collides. Ours could not do that, because the member holding `getopt` also
held `sem_wait`, so it was extracted for *other* reasons and its `getopt`
duplicated the program's.

The collision was therefore **unavoidable from the caller's side**. No link
order, no object subsetting and no `--start-group` can decline half a member.
GNU make 4.4.1 failed to link with 11 duplicate symbols and zero missing ones;
`-C codegen-units=4096` in `toolchain/build-sysroot.ps1` fixed it by restoring
one-module-per-member granularity. See `design-decisions.md` S339.

WHY A DEDICATED CHECK, RATHER THAN RELYING ON THE EXISTING TESTS
================================================================

Nothing in `cargo test` would notice a regression here, and that is structural
rather than an oversight. Every libc test we have links a fixture and calls a
function; a fixture that does not bring its *own* `getopt` links identically
against a coarse archive and a fine one. The defect is invisible until a
third-party program supplies a competing definition -- i.e. it surfaces at the
moment we are trying to port something, which is the worst possible time and
the furthest possible point from the change that caused it.

Hence an assertion on the artifact itself. It is cheap: it reads the archive's
symbol index and nothing else.

GRANULARITY IS NOT THE WHOLE STORY -- SEE CHECK 3
=================================================

`-C codegen-units=4096` is a *ceiling*, not a splitter. rustc partitions at
module granularity and will not divide a single module, so a 156 KB `wchar.rs`
is one codegen unit and hence one member -- exporting 78 symbols -- however
high the ceiling goes. The actual mechanism behind S340's fix was wrapping each
affected function in its own `mod gnu_<name> { ... }` block, which is what
creates a new unit.

The GNU coreutils spike (`scripts/coreutils-spike/`) proved this the expensive
way: five binaries collided on `wmempcpy`, which shares `wchar.rs`'s member
with `mbrtowc`. CHECK 2 could not see it, because it only fires when a member
mixes a replaceable name with one a *hello-world* needs, and `mbrtowc` is not
that. CHECK 3 states the property without that proxy. See its comment block.

NO EXTERNAL TOOLS
=================

This deliberately does not shell out to `nm`. The sysroot is built on Windows
by `toolchain/build-sysroot.ps1`, where `nm` is not on PATH (it exists only
inside WSL here), and a check that silently cannot run where the artifact is
produced is not a check. GNU `ar`'s archive symbol index already stores
exactly what `nm --defined-only -g` would print -- a map from each externally
defined symbol to the member that defines it -- so parsing it directly is both
simpler and dependency-free.

USAGE
=====

    python scripts/check-libc-shape.py [path/to/libc.a] [-v]

Exit codes: 0 clean, 1 violation found, 2 could not run (missing/unparsable
archive). Note that 2 is a failure too -- see `main()`.
"""

from __future__ import annotations

import argparse
import struct
import sys
from pathlib import Path

# Never let an un-encodable character eat the diagnostic.
#
# This script is invoked by toolchain/build-sysroot.ps1, i.e. from a Windows
# console, whose code page is frequently cp437 or cp1252 rather than UTF-8.
# Every character this project's prose reaches for by habit -- the em dash, the
# section sign in "design-decisions.md S339", the ellipsis -- is un-encodable in
# cp437, and Python's default `errors="strict"` turns that into a
# UnicodeEncodeError *raised from inside the print that was explaining what is
# wrong*. The build would still fail, but with a traceback about character
# encoding instead of the message naming the dropped compiler flag, which is a
# strictly worse failure than the one being reported.
#
# The message strings below are deliberately plain ASCII so this never fires.
# The guard is here anyway because that is a convention, and conventions decay:
# the next person to edit an error string will type an em dash without thinking,
# and this turns that mistake into a cosmetic `\u2014` rather than a lost
# diagnostic.
for _stream in (sys.stdout, sys.stderr):
    if hasattr(_stream, "reconfigure"):
        try:
            _stream.reconfigure(errors="backslashreplace")
        except (ValueError, OSError):
            # Redirected to something that cannot be reconfigured. Not worth
            # failing over: the strings are ASCII, so this only ever mattered
            # as insurance.
            pass

# --- what we assert -----------------------------------------------------------
#
# Three checks, because they fail for different reasons and generalise
# differently.
#
# CHECK 1 (strict, narrow): for each family below, the member defining it must
# define *nothing else*. This is the exact property glibc has, and it is the
# strongest possible statement: a program bringing its own copy declines the
# member and loses nothing at all. We assert it only for families where it is
# both known to hold and known to matter, so that a failure is unambiguous --
# it means the partitioner started merging again.
#
# Each entry is the set of externally visible names that are ALLOWED to share
# the family's member -- not an assertion that all of them exist. The check is
# "this member defines nothing outside the family", so a name listed here but
# absent from libc costs nothing, while a name missing from this list would be
# reported as an intruder the moment we implement it.
#
# Some listed names are in fact absent today, deliberately: `optreset` is a
# BSD-ism we do not provide, `__getopt_initialized` is glibc-internal, and
# `glob64`/`globfree64` are the large-file aliases (our off_t is already 64-bit,
# so there is nothing for them to alias). Listing them anyway means that if any
# is added later it must land in its family's member, which is the correct
# constraint -- an LFS alias in a different object file from the function it
# aliases would be a duplicate-definition hazard of exactly the kind this
# script exists to catch.
#
# Data symbols are listed alongside the functions on purpose: `optarg` and
# friends are as much a duplicate-definition hazard as `getopt` itself, and
# gnulib defines those too.
#
# NOTE what this therefore does NOT check: that the family is fully present.
# A libc that lost `optarg` would still pass. That is intentional -- a missing
# symbol is a link error at the first use, i.e. loud and immediate, which is
# the opposite of the silent failure mode this script guards.
STRICT_FAMILIES: dict[str, frozenset[str]] = {
    "getopt": frozenset(
        {
            "getopt",
            "getopt_long",
            "getopt_long_only",
            "optarg",
            "opterr",
            "optind",
            "optopt",
            "optreset",
            "__getopt_initialized",
        }
    ),
    "glob": frozenset({"glob", "globfree", "glob64", "globfree64"}),
    "fnmatch": frozenset({"fnmatch"}),
    "error": frozenset(
        {
            "error",
            "error_at_line",
            "error_message_count",
            "error_one_per_line",
            "error_print_progname",
            "verror",
            "verror_at_line",
        }
    ),
}

# CHECK 2 (broad, generalising): no single member may define both a name that
# third-party code commonly replaces AND a name that no C program can avoid
# needing. That combination is the precise shape of the make failure: the
# member gets extracted for the unavoidable name, and drags the replaceable one
# in behind it.
#
# This is the check that covers the gnulib module we have not thought of yet.
# It is weaker than CHECK 1 (it permits a member to hold two replaceable
# families) but it needs no per-family curation, so it keeps working as the
# libc grows.
#
# Sourced from gnulib's module list: these are functions gnulib ships a
# replacement for and compiles in unconditionally or near-enough on a
# non-glibc target, which is what we look like to a `./configure` run.
#
# HOW TO DECIDE WHETHER A NEW NAME BELONGS HERE. gnulib has two ways of
# replacing a function, and only one of them can collide with us:
#
#   * The function exists on the target but is buggy -> gnulib compiles its
#     copy as `rpl_foo` and adds `#define foo rpl_foo`. Different symbol; it
#     can never collide. (`fflush` is replaced this way, which is why `fflush`
#     is safe to leave in UNAVOIDABLE below rather than here.)
#   * The function is *absent* from the target -> gnulib compiles its copy
#     under the plain name `foo`. That one collides with ours.
#
# `./configure` decides which case applies by probing the reference libc the
# spikes compile against -- zig's bundled musl. So the hazard set is, near
# enough, **the names our libc defines that musl does not**. `wmempcpy`
# qualifies (glibc-only, so gnulib defines it plainly, so it collided with our
# wchar member). `execl` does not (musl has it, so gnulib never defines it),
# which is why the exec family is absent from this list even though gnulib has
# modules named after it.
#
# List whole families, not just the member you happened to trip over. CHECK 3
# asserts that a replaceable name shares its member only with other replaceable
# names, so a family listed half-way reports its own missing half as an
# intruder -- `optarg` without `opterr` makes `getopt`'s member look impure.
REPLACEABLE = frozenset(
    {
        # the four that actually bit us (GNU make, design-decisions.md S339),
        # listed family-complete including the data symbols
        "getopt", "getopt_long", "getopt_long_only",
        "optarg", "opterr", "optind", "optopt", "optreset",
        "__getopt_initialized",
        "glob", "globfree", "glob64", "globfree64", "fnmatch",
        "error", "error_at_line", "verror", "verror_at_line",
        "error_message_count", "error_one_per_line", "error_print_progname",
        # regex: gnulib vendors the whole engine
        "regcomp", "regexec", "regfree", "regerror", "re_compile_pattern",
        # string/memory helpers gnulib routinely replaces
        "strverscmp", "strndup", "strnlen", "memrchr", "rawmemchr",
        "stpcpy", "stpncpy", "strchrnul", "strcasestr", "mempcpy",
        # wide-character: musl lacks `wmempcpy`, so gnulib defines it plainly.
        # This is the one the coreutils spike caught (5 binaries, 1 duplicate)
        # and the reason CHECK 3 exists -- see the note above CHECK 3.
        "wmempcpy",
        # stdio-ish
        "getline", "getdelim", "fseeko", "ftello", "vasprintf", "asprintf",
        # stdio_ext.h: gnulib's freadahead/freadptr/fpending/fpurge/fseterr
        # modules supply these when the target has no glibc-compatible copy.
        # Names not yet implemented are listed anyway, so that implementing one
        # without its own `mod gnu_*` block is caught at once.
        "__fpending", "__fpurge", "__freadahead", "__freadptr",
        "__freadptrinc", "__fseterr", "__freadable", "__fwritable",
        "__freading", "__fwriting", "__flbf", "__fbufsize", "__fsetlocking",
        # misc POSIX-fillers
        "mkstemp", "mkostemp", "mkstemps", "mkostemps", "mkdtemp",
        "canonicalize_file_name", "obstack_free",
        "argp_parse", "getsubopt", "timegm", "strptime",
        "qsort_r", "timespec_get",
    }
)

# Names a program essentially cannot avoid referencing. If one of these shares
# a member with a REPLACEABLE name, that member is guaranteed to be extracted
# and the duplicate is guaranteed to follow.
#
# Kept deliberately short and boring. Every one of these is referenced by a
# hello-world or by the Rust/C startup path, so there is no judgement call
# about whether a real program "might" need it.
UNAVOIDABLE = frozenset(
    {
        "malloc", "free", "calloc", "realloc",
        "memcpy", "memset", "memmove", "memcmp",
        "strlen", "strcmp", "strcpy",
        "printf", "fprintf", "snprintf", "vfprintf", "puts", "putchar",
        "fopen", "fclose", "fread", "fwrite", "fflush",
        "read", "write", "open", "close",
        "getenv", "setenv", "environ",
        "exit", "abort",
    }
)

# CHECK 3 (the generalisation of CHECK 2): a REPLACEABLE name must own its
# member outright, or share it only with other REPLACEABLE names.
#
# WHY CHECK 2 IS NOT ENOUGH. CHECK 2 approximates "the program under port
# references something else in this member" with "this member also defines a
# name no C program can avoid". That approximation has a hole, and the GNU
# coreutils spike fell straight through it:
#
#   `posix::wchar` compiled to one member defining 78 symbols, among them
#   `wmempcpy` (REPLACEABLE -- musl lacks it, so gnulib defines it plainly)
#   and `mbrtowc` (which coreutils calls). Five binaries -- ls, dir, vdir, du,
#   dircolors -- referenced `mbrtowc`, extracted the member, and collided on
#   `wmempcpy`. CHECK 2 stayed silent throughout, because `mbrtowc` is not in
#   UNAVOIDABLE: a hello-world does not call it.
#
# UNAVOIDABLE is "what a hello-world references", which is a proxy for "what
# the program under port references" -- and a weak one, since a real program
# references hundreds of names outside it. Widening UNAVOIDABLE until it covers
# them is not a fix; the honest limit of that process is "every name in libc",
# at which point the rule is just CHECK 3.
#
# So state the property directly instead. If a program brings its own copy of a
# replaceable function, our member holding that function must be one the
# program can afford to decline entirely. That is true exactly when the member
# holds nothing the program might need -- i.e. nothing but other replaceable
# names, which by definition the program is equally happy to supply itself.
#
# This subsumes CHECK 2 (an UNAVOIDABLE name is not REPLACEABLE, so any member
# CHECK 2 flags CHECK 3 flags too), so a member CHECK 2 already reported is
# skipped here to avoid reporting it twice. CHECK 2 is kept because its
# diagnosis is the sharper one when it does apply: "every program needs this"
# is a stronger statement than "some program might".
#
# It is also weaker than CHECK 1, which additionally forbids sharing between
# two replaceable families and demands the family live in exactly one member.

AR_MAGIC = b"!<arch>\n"


def is_collidable(symbol: str) -> bool:
    """Could a third-party program plausibly define this same name?

    Only C-visible names can collide. Rust's mangling namespaces a symbol under
    its crate and path (`_ZN5posix7fnmatch8do_match17h...E`), and LLVM appends a
    `.llvm.<hash>` suffix when it promotes a module-internal function to an
    external symbol so another codegen unit can call it. Neither can be
    duplicated by a C program, so neither is a granularity hazard -- they are
    just the compiler's plumbing showing through the archive index.

    This matters for the strict check: `fnmatch`'s member also exports
    `_ZN5posix7fnmatch8do_match17h...E.llvm....`, which is `fnmatch`'s own helper,
    not an unrelated function riding along. Counting it as a violation would
    make the check cry wolf about the one family it is most meant to protect.
    """
    return not (
        symbol.startswith(("_ZN", "_RN", "_R", "anon."))
        or ".llvm." in symbol
    )


class ArchiveError(Exception):
    """The file is not an archive we can read. Distinct from a shape violation."""


def parse_symbol_index(path: Path) -> dict[int, set[str]]:
    """Map member offset -> set of symbols that member defines externally.

    Reads only the archive's leading symbol index (the `/` or `/SYM64/`
    member), which GNU ar writes as: a count, then that many member offsets,
    then that many NUL-terminated names, in the same order. That is precisely
    the symbol->member map, so no ELF parsing is needed.
    """
    data = path.read_bytes()
    if not data.startswith(AR_MAGIC):
        raise ArchiveError(f"{path} does not start with {AR_MAGIC!r} -- not an ar archive")

    pos = len(AR_MAGIC)
    header = data[pos : pos + 60]
    if len(header) < 60:
        raise ArchiveError(f"{path} is truncated: no member header after the magic")

    name = header[0:16].rstrip()
    try:
        size = int(header[48:58])
    except ValueError as exc:
        raise ArchiveError(f"{path}: unparsable member size {header[48:58]!r}") from exc

    body = data[pos + 60 : pos + 60 + size]
    if len(body) < size:
        raise ArchiveError(f"{path} is truncated inside its first member")

    # `/` is the 32-bit GNU index, `/SYM64/` the 64-bit one used once member
    # offsets exceed 4 GiB. Handle both so a future larger libc.a does not turn
    # this check into a silent skip.
    if name == b"/":
        word, wsize = ">I", 4
    elif name == b"/SYM64/":
        word, wsize = ">Q", 8
    else:
        # An archive with no symbol index cannot be linked against in the
        # normal way, so this is a real problem rather than a reason to skip.
        raise ArchiveError(
            f"{path} has no symbol index (first member is {name!r}). "
            "Was it created without `ranlib`/`ar s`?"
        )

    (count,) = struct.unpack(word, body[:wsize])
    offsets = struct.unpack(f">{count}{'I' if wsize == 4 else 'Q'}", body[wsize : wsize + count * wsize])
    names_blob = body[wsize + count * wsize :]
    names = names_blob.split(b"\0")[:count]
    if len(names) < count:
        raise ArchiveError(f"{path}: symbol index claims {count} names but holds {len(names)}")

    members: dict[int, set[str]] = {}
    for off, raw in zip(offsets, names):
        members.setdefault(off, set()).add(raw.decode("utf-8", "replace"))
    return members


def member_name(path: Path, offset: int) -> str:
    """Best-effort human name for the member at `offset`, for error messages."""
    try:
        data = path.read_bytes()
        header = data[offset : offset + 60]
        return header[0:16].rstrip().rstrip(b"/").decode("utf-8", "replace") or f"@{offset}"
    except (OSError, IndexError):
        return f"@{offset}"


def check(path: Path, verbose: bool) -> list[str]:
    """Return a list of human-readable violations; empty means the shape is good."""
    members = parse_symbol_index(path)
    violations: list[str] = []

    if verbose:
        print(f"{path}: {len(members)} members, "
              f"{sum(len(s) for s in members.values())} externally defined symbols")

    # --- CHECK 1: strict families own their member outright -------------------
    for family, expected in STRICT_FAMILIES.items():
        hosts = {off: syms for off, syms in members.items() if syms & expected}
        if not hosts:
            # Not a granularity failure -- the function is simply absent. Say so
            # rather than passing silently, because "no member defines getopt"
            # would otherwise satisfy this loop trivially.
            violations.append(
                f"[missing] no member of {path.name} defines any of the {family} family "
                f"({', '.join(sorted(expected))}). Either libc lost it or the family "
                f"list in this script is stale."
            )
            continue
        if len(hosts) > 1:
            where = ", ".join(member_name(path, o) for o in sorted(hosts))
            violations.append(
                f"[split] the {family} family is spread across {len(hosts)} members ({where}). "
                f"Any program defining its own {family} now collides with whichever piece "
                f"gets extracted."
            )
        for off, syms in hosts.items():
            extra = {s for s in syms - expected if is_collidable(s)}
            if extra:
                shown = ", ".join(sorted(extra)[:12])
                more = f" (+{len(extra) - 12} more)" if len(extra) > 12 else ""
                violations.append(
                    f"[coarse] member {member_name(path, off)} defines the {family} family "
                    f"*and* {len(extra)} unrelated symbol(s): {shown}{more}. "
                    f"A program bringing its own {family} cannot decline this member, so it "
                    f"gets a duplicate definition -- this is the GNU make failure of "
                    f"design-decisions.md S339."
                )
            elif verbose:
                print(f"  ok  {family:<8} -> {member_name(path, off)} "
                      f"({len(syms)} symbol(s), nothing else)")

    # --- CHECK 2: no member mixes replaceable with unavoidable ----------------
    reported_mixed: set[int] = set()
    for off, syms in sorted(members.items()):
        repl = syms & REPLACEABLE
        unav = syms & UNAVOIDABLE
        if repl and unav:
            reported_mixed.add(off)
            violations.append(
                f"[mixed] member {member_name(path, off)} defines replaceable name(s) "
                f"{{{', '.join(sorted(repl))}}} alongside unavoidable name(s) "
                f"{{{', '.join(sorted(unav))}}}. Every program needs the latter, so this "
                f"member is always extracted, so the former is always defined -- and any "
                f"program supplying its own copy fails to link."
            )

    # --- CHECK 3: a replaceable name shares its member only with replaceables -
    for off, syms in sorted(members.items()):
        if off in reported_mixed:
            continue  # CHECK 2 already said this, in sharper words
        collidable = {s for s in syms if is_collidable(s)}
        repl = collidable & REPLACEABLE
        if not repl:
            continue
        riders = collidable - REPLACEABLE
        if not riders:
            if verbose:
                print(f"  ok  member {member_name(path, off)} holds only replaceable "
                      f"name(s): {', '.join(sorted(repl))}")
            continue
        shown = ", ".join(sorted(riders)[:12])
        more = f" (+{len(riders) - 12} more)" if len(riders) > 12 else ""
        violations.append(
            f"[rider] member {member_name(path, off)} defines replaceable name(s) "
            f"{{{', '.join(sorted(repl))}}} alongside {len(riders)} name(s) a program "
            f"may well reference without supplying: {shown}{more}. Referencing any one "
            f"of those extracts the member, and a program that vendors its own copy of "
            f"the replaceable name then gets a duplicate definition. Fix by moving the "
            f"replaceable one into its own `mod gnu_<name>` block (see posix/src/"
            f"string.rs's module header), not by widening this script's lists."
        )

    return violations


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.split("\n", 1)[0])
    parser.add_argument(
        "archive",
        nargs="?",
        default=None,
        help="path to libc.a (default: toolchain/sysroot/lib/libc.a beside this repo)",
    )
    parser.add_argument("-v", "--verbose", action="store_true",
                        help="print the member each strict family resolved to")
    args = parser.parse_args(argv)

    if args.archive:
        path = Path(args.archive)
    else:
        path = Path(__file__).resolve().parent.parent / "toolchain" / "sysroot" / "lib" / "libc.a"

    if not path.is_file():
        # Exit 2, not 0. A sysroot that has not been built is a legitimate state
        # for a fresh checkout, but it is NOT a passing check, and a caller that
        # treats "could not look" as "looked and it was fine" is how a gate ends
        # up permanently green. Callers that genuinely want a skip should test
        # for the file themselves.
        print(f"ERROR: {path} does not exist -- run toolchain/build-sysroot.ps1 first.",
              file=sys.stderr)
        print("       (This is exit 2, 'could not check', not a pass.)", file=sys.stderr)
        return 2

    try:
        violations = check(path, args.verbose)
    except ArchiveError as exc:
        print(f"ERROR: {exc}", file=sys.stderr)
        return 2

    if violations:
        print(f"libc.a SHAPE CHECK FAILED -- {len(violations)} problem(s) in {path}\n",
              file=sys.stderr)
        for v in violations:
            print(f"  - {v}\n", file=sys.stderr)
        if any(v.startswith("[rider]") for v in violations):
            print("A [rider] means one module exports a replaceable name next to names a",
                  file=sys.stderr)
            print("program may reference. Raising codegen-units will NOT split it: rustc",
                  file=sys.stderr)
            print("partitions at module granularity and never divides a single module. Wrap",
                  file=sys.stderr)
            print("the replaceable function in its own `mod gnu_<name> { ... }` block, as",
                  file=sys.stderr)
            print("posix/src/string.rs does. See design-decisions.md S340 and S348.",
                  file=sys.stderr)
        else:
            print("The usual cause is that `-C codegen-units=4096` has been dropped from",
                  file=sys.stderr)
            print("$sysrootFlags in toolchain/build-sysroot.ps1, letting rustc's partitioner",
                  file=sys.stderr)
            print("merge unrelated modules into a handful of objects again. See",
                  file=sys.stderr)
            print("design-decisions.md S339 for why that breaks every gnulib-using port.",
                  file=sys.stderr)
        return 1

    print(f"libc.a shape OK: {path}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
