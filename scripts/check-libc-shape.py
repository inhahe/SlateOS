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
import contextlib
import io
import struct
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import gittree  # noqa: E402

ROOT = Path(__file__).resolve().parent.parent

# What the archive is built from. `posix` is the crate; build-sysroot.ps1 is
# here because the regression this whole file exists to catch is *a dropped
# compiler flag in that script* (`-C codegen-units=4096`, design-decisions.md
# S339). An edit to it that has not been rebuilt leaves an archive describing
# the old flags, which is exactly the state where a stale OK is most misleading.
_ARCHIVE_INPUTS = ("posix",)
_ARCHIVE_INPUT_FILES = ("toolchain/build-sysroot.ps1",)

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

#: Floors on how much of the archive must have been read before a verdict is
#: worth printing. Not targets: set an order of magnitude below what a real
#: build produces (615 members / 3251 symbols, measured 2026-09-03) and far
#: above what a misparse yields, so ordinary growth or shrinkage of libc never
#: trips them and a symbol index that decoded to nothing always does.
MIN_MEMBERS = 100
MIN_SYMBOLS = 500


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


def stale_against_sources(archive: Path) -> tuple[int, str | None]:
    """Inputs of `archive` that postdate it: how many, and the most recent.

    WHY THIS EXISTS. `libc.a` is a build artifact and is not in git, so every
    worktree holds whatever copy it last happened to build -- and two of the
    three lanes never build `posix` at all. Lane C filed the request below after
    this gate reported seven findings against an archive eleven days and
    fifty-seven `posix/` commits old. Measured 2026-09-02 on a fresh archive:
    all seven were already fixed. Every one of those was a lane being asked to
    act on a fact about a tree nobody had.

        requests/c-b-check-libc-shape-grades-a-build-artifact-without-checking-its-age.md

    That direction is the harmless one. The dangerous one is the same staleness
    with the opposite content: an old archive that happens to be clean prints
    OK, and an OK here is precisely the signal that means "a GNU package will
    link." A gate that can pass on eleven-day-old evidence is not a gate --
    which is the same objection the missing-archive branch in `main` already
    makes, one step earlier and for the same reason.

    WHY MTIME. It is what every build system uses for this question, and it
    needs nothing written down anywhere: no stamp file to fall out of date, no
    recorded commit to be wrong after a checkout. A `git merge` rewrites only
    the files it changes, so a merge that leaves `posix/` alone does not bump
    anything here. The failure mode is a checkout touching a file whose content
    did not change, which reports staleness that is not real -- and that
    direction costs a skipped run, not a false pass.

    The listing goes through `gittree.WorkTree` rather than a walk of its own,
    so the rule for what counts as build output is not spelled a third time.
    See known-issues.md's pre-push-gates entry, step 7: the second spelling of
    that rule had already drifted from the first.
    """
    stamp = archive.stat().st_mtime
    rels: list[str] = list(_ARCHIVE_INPUT_FILES)
    with gittree.WorkTree(str(ROOT)) as tree:
        for prefix in _ARCHIVE_INPUTS:
            rels.extend(tree.files_under(prefix))

    count = 0
    newest_rel: str | None = None
    newest_mtime = stamp
    for rel in rels:
        try:
            mtime = (ROOT / rel).stat().st_mtime
        except OSError:
            # A path we cannot stat says nothing about the archive's age.
            continue
        if mtime > stamp:
            count += 1
            if mtime > newest_mtime:
                newest_mtime, newest_rel = mtime, rel
    return count, newest_rel


def _ar_header(name: str, size: int) -> bytes:
    """A 60-byte GNU `ar` member header. Only name and size are ever read."""
    return (f"{name:<16}{'0':<12}{'0':<6}{'0':<6}{'644':<8}{size:<10}"
            .encode() + b"`\n")


def synth_archive(members: list[tuple[str, list[str]]]) -> bytes:
    """Build a GNU `ar` archive whose symbol index says exactly this.

    Written out here rather than shelled out to `ar` on purpose: this gate's
    subject *is* the archive format, so a fixture produced by the same family
    of tool that produced the real file would leave the parser's own reading of
    the layout untested. Two passes, because the index has to name member
    offsets that only exist once the index's own length is known.
    """
    all_syms = [s for _n, syms in members for s in syms]
    index_size = 4 + 4 * len(all_syms) + sum(len(s) + 1 for s in all_syms)
    pos = len(AR_MAGIC) + 60 + index_size + (index_size % 2)

    offsets, blobs = [], []
    for name, _syms in members:
        offsets.append(pos)
        body = b""  # nothing reads a member's contents, only its header
        blobs.append(_ar_header(name + "/", len(body)) + body)
        pos += 60 + len(body)

    sym_offsets = [off for off, (_n, syms) in zip(offsets, members)
                   for _ in syms]
    index = (struct.pack(">I", len(all_syms))
             + b"".join(struct.pack(">I", o) for o in sym_offsets)
             + b"".join(s.encode() + b"\0" for s in all_syms))
    pad = b"\n" if index_size % 2 else b""
    return (AR_MAGIC + _ar_header("/", index_size) + index + pad
            + b"".join(blobs))


def _selftest() -> int:
    """Drive the parser and the three shape checks against synthetic archives.

    Needs no `libc.a`, which is the whole point: this gate is wired
    `--may-skip` and declines on any tree whose sysroot has not been built, so
    a self-test that needed the artifact would be absent from exactly the runs
    where the gate was absent too -- covering nothing, on every machine where
    the coverage was the only thing left.

    The fixtures are graded against the module's own REAL symbol tables rather
    than injected ones. That is deliberate and buys a second thing: a clean
    fixture can only be constructed if STRICT_FAMILIES, REPLACEABLE and
    UNAVOIDABLE are mutually consistent, so a name added to two of them at once
    turns this red without anyone having to think of it as a case.
    """
    import tempfile

    checks = bad = 0

    def check_(label, ok):
        nonlocal checks, bad
        checks += 1
        if ok:
            print(f"ok   {label}")
        else:
            print(f"selftest FAIL: {label}", file=sys.stderr)
            bad += 1

    tmp = Path(tempfile.mkdtemp(prefix="libcshape-"))

    def graded(members):
        """Violations for a synthetic archive with these members."""
        p = tmp / "t.a"
        p.write_bytes(synth_archive(members))
        return check(p, False)

    def tags(vs):
        return sorted(v.split("]")[0] + "]" for v in vs)

    # A clean archive: every strict family alone in its own member, one member
    # of unavoidable names, and nothing sharing.
    #
    # Members are named after the family they hold rather than numbered. The
    # numbered version coupled the fixtures to `STRICT_FAMILIES`' *insertion*
    # order while the cases below pick their target by *sorted* order, so
    # "fam0" silently meant getopt where the case said error -- and the two
    # disagreed about which member they were talking about.
    clean = [(family, sorted(syms)) for family, syms in STRICT_FAMILIES.items()]
    clean.append(("core", sorted(UNAVOIDABLE)))

    try:
        # --- the format reader ------------------------------------------
        p = tmp / "t.a"
        p.write_bytes(synth_archive(clean))
        idx = parse_symbol_index(p)
        check_("a synthetic archive round-trips through the index reader",
               len(idx) == len(clean))
        check_("...with each member's symbols attributed to it",
               sorted(sorted(s) for s in idx.values())
               == sorted(sorted(s) for _n, s in clean))
        check_("member names come back for the error messages",
               {member_name(p, o) for o in idx} == {n for n, _s in clean})

        p.write_bytes(b"not an archive at all")
        try:
            parse_symbol_index(p)
        except ArchiveError as exc:
            check_("a non-archive is an ArchiveError, not a violation",
                   "not an ar archive" in str(exc))
        else:
            check_("a non-archive is an ArchiveError, not a violation", False)

        # An archive whose first member is a real member rather than the index.
        # This is the shape that must NOT be read as "no symbols, all clean":
        # it is how an archive built without `ranlib` looks.
        p.write_bytes(AR_MAGIC + _ar_header("foo.o/", 0))
        try:
            parse_symbol_index(p)
        except ArchiveError as exc:
            check_("an archive with no symbol index is refused, not read as "
                   "empty", "no symbol index" in str(exc))
        else:
            check_("an archive with no symbol index is refused", False)

        p.write_bytes(AR_MAGIC + _ar_header("/", 4000) + b"\0" * 4)
        try:
            parse_symbol_index(p)
        except ArchiveError as exc:
            check_("an index whose body is short is refused", "truncated" in str(exc))
        else:
            check_("an index whose body is short is refused", False)

        # ...and the OTHER truncation, which the case above does not reach: a
        # body of exactly the declared length whose name table runs out before
        # the symbol count does. The case above trips the length guard and
        # returns, so the name-table guard went untested -- mutation testing
        # deleted `if len(names) < count` and the suite stayed green.
        #
        # count is 4 rather than 3 on purpose. `split(b"\0")` on a table of N
        # NUL-*terminated* names yields N+1 fields (the last empty), so two
        # names satisfy a count of three by accident and prove nothing.
        body = struct.pack(">I", 4) + struct.pack(">4I", 100, 100, 100, 100) + b"a\0b\0"
        p.write_bytes(AR_MAGIC + _ar_header("/", len(body)) + body)
        try:
            parse_symbol_index(p)
        except ArchiveError as exc:
            check_("...as is one whose name table is shorter than its count",
                   "claims 4 names" in str(exc))
        else:
            check_("...as is one whose name table is shorter than its count", False)

        # --- the three checks, each seen to pass and to fire -------------
        check_("a well-shaped archive yields no violations", graded(clean) == [])

        first = sorted(STRICT_FAMILIES)[0]
        fam = sorted(STRICT_FAMILIES[first])

        # CHECK 1 [coarse]: the family's member also exports something a C
        # program could define. This is the GNU make failure of S339.
        #
        # The expected tag set is DERIVED from the tables, not written down: a
        # plain-C rider always earns [coarse], and additionally earns [rider]
        # exactly when the family's own names are replaceable, because then the
        # member holds a replaceable name beside an ordinary one. Every strict
        # family is wholly replaceable today, so today this is always both --
        # but hardcoding ["[coarse]", "[rider]"] would encode that coincidence
        # as a requirement and turn red on the first family that isn't.
        rider_too = ["[rider]"] if STRICT_FAMILIES[first] & REPLACEABLE else []
        vs = graded([(n, s + ["some_unrelated_c_function"] if n == first else s)
                     for n, s in clean])
        check_(f"[coarse] a rider on the {first} member is caught",
               tags(vs) == sorted(["[coarse]"] + rider_too))

        # ...and that a Rust-mangled or .llvm. rider is NOT caught, which is
        # what is_collidable is for -- a check that cried wolf on the
        # compiler's own plumbing would be turned off.
        vs = graded([(n, s + ["_ZN5posix6getopt4findE",
                              "helper.llvm.12345"] if n == first else s)
                     for n, s in clean])
        check_("...but a Rust-mangled or .llvm. rider is not a violation",
               vs == [])

        # CHECK 1 [split]: the family lives in two members.
        vs = graded([(n, s) for n, s in clean if n != first]
                    + [("split_a", fam[:1]), ("split_b", fam[1:])]
                    if len(fam) > 1 else clean)
        check_(f"[split] the {first} family in two members is caught",
               tags(vs) == ["[split]"] if len(fam) > 1 else True)

        # CHECK 1 [missing]: the family is absent entirely. "No member defines
        # getopt" would otherwise satisfy the loop trivially.
        vs = graded([(n, s) for n, s in clean if n != first])
        check_(f"[missing] the {first} family absent is caught, not passed",
               tags(vs) == ["[missing]"])

        # CHECK 2 [mixed]: a replaceable name in the same member as one every
        # program needs, so the member is always extracted.
        repl = sorted(REPLACEABLE - {s for f in STRICT_FAMILIES.values()
                                     for s in f})[0]
        vs = graded([(n, s + [repl] if n == "core" else s) for n, s in clean])
        check_(f"[mixed] {repl} beside an unavoidable name is caught",
               "[mixed]" in tags(vs))

        # CHECK 3 [rider]: a replaceable name beside an ordinary one. Weaker
        # than [mixed], and reported only where [mixed] did not already speak.
        vs = graded(clean + [("mix", [repl, "some_other_c_function"])])
        check_(f"[rider] {repl} beside a plain C name is caught",
               tags(vs) == ["[rider]"])
        check_("...and a member of only replaceable names is not",
               graded(clean + [("ok", [repl])]) == [])

        # --- is_collidable, which the two above depend on ----------------
        check_("C names are collidable", is_collidable("getopt"))
        check_("Rust-mangled names are not",
               not is_collidable("_ZN5posix7fnmatch8do_matchE"))
        check_("...nor are LLVM-promoted internals",
               not is_collidable("do_match.llvm.9876"))

        # --- the floor, driven through main() rather than asserted -------
        # The first two are about the constants: a fixture-sized archive must
        # sit below both halves, or the floor could not fire on the misparse it
        # exists to catch. `MIN_SYMBOLS > 0` used to stand in for the second and
        # was worth nothing -- gutting MIN_SYMBOLS to 1 satisfied it, and
        # mutation testing duly walked out with that survivor.
        clean_symbols = sum(len(s) for _n, s in clean)
        check_("a fixture-sized archive is below the member floor",
               len(clean) < MIN_MEMBERS)
        check_("...and below the symbol floor",
               clean_symbols < MIN_SYMBOLS)

        # ...and these two are about whether anything CONSULTS them. The pair
        # above pins the constants; neither shows that main() ever reads one. A
        # main() with the floor block deleted passes both with the numbers
        # perfectly intact -- the same shape of hole as an escape alphabet that
        # is verified against its source and then never looked at. So drive the
        # real entry point, in both directions: it must refuse the small archive
        # and accept a large one, or "refuses everything" would pass as well.
        #
        # main() age-checks only its DEFAULT archive, so an explicit path here
        # needs no --ignore-age and cannot be turned red by a stale sysroot.
        def run_main(members) -> tuple[int, str]:
            p = tmp / "t.a"
            p.write_bytes(synth_archive(members))
            out, err = io.StringIO(), io.StringIO()
            with contextlib.redirect_stdout(out), contextlib.redirect_stderr(err):
                rc = main([str(p)])
            return rc, out.getvalue() + err.getvalue()

        # Filler members. The names are Rust-mangled on purpose: is_collidable
        # rejects them, so they add bulk without adding violations, and these
        # cases stay about the floor.
        def pad(n_members: int, per_member: int):
            return [(f"pad{i}", [f"_ZN5posix3pad{i}_{j}E" for j in range(per_member)])
                    for i in range(n_members)]

        rc, text = run_main(clean)
        check_("main() refuses a fixture-sized archive rather than passing it",
               rc == 2 and "below the floor" in text)

        rc, text = run_main(clean + pad(MIN_MEMBERS + 1, 6))
        check_("...and grades one that clears the floor",
               rc == 0 and "shape OK" in text)

        # The two halves are also driven APART. Both cases above sit below both
        # floors or above both, so an `and` where the code says `or` -- a floor
        # that checks only one half -- passes them unchanged. Mutation testing
        # found exactly that: dropping either half left the suite green. These
        # two fixtures each breach one half only, so each is refused by one
        # half and would be waved through by the other.
        few_members = MIN_MEMBERS - 1 - len(clean)
        rc, text = run_main(clean + pad(few_members, MIN_SYMBOLS // few_members + 2))
        check_("...refuses too-few members even when the symbols are plentiful",
               rc == 2 and "below the floor" in text)

        rc, text = run_main(clean + pad(MIN_MEMBERS + 1, 1))
        check_("...and too-few symbols even when the members are plentiful",
               rc == 2 and "below the floor" in text)
    finally:
        for f in tmp.iterdir():
            f.unlink()
        tmp.rmdir()

    if bad:
        print(f"selftest: {bad} of {checks} cases FAILED", file=sys.stderr)
        return 1
    print(f"selftest: {checks}/{checks} cases pass")
    return 0


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
    parser.add_argument("--ignore-age", action="store_true",
                        help="grade the archive even if posix/ is newer than it")
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

    # Only the default archive is age-checked. An explicitly named one is
    # somebody deliberately grading a different file -- a saved copy, another
    # lane's, one under bisection -- and comparing *that* to this checkout's
    # posix/ would be answering a question nobody asked.
    #
    # The concrete case, and the reason this exemption is load-bearing rather
    # than tidy: toolchain/build-sysroot.ps1:148 invokes this script with the
    # path it has *just built*. That caller must never see a staleness skip --
    # it is the one context where the archive is guaranteed current, and where
    # a skip would mean the sysroot build stopped checking its own output.
    if not args.archive and not args.ignore_age:
        stale, newest = stale_against_sources(path)
        if stale:
            print(f"SKIP: {path} predates {stale} of its own input(s) -- the "
                  f"most recent is {newest}.", file=sys.stderr)
            print("      Run toolchain/build-sysroot.ps1 to grade the archive "
                  "this tree would actually produce.", file=sys.stderr)
            print("      (This is exit 2, 'could not check', not a pass. Use "
                  "--ignore-age to grade it anyway.)", file=sys.stderr)
            return 2

    try:
        members = parse_symbol_index(path)
    except ArchiveError as exc:
        print(f"ERROR: {exc}", file=sys.stderr)
        return 2

    # A floor on discovery, asked BEFORE the shape checks rather than after,
    # because it is a question about whether the archive could be judged at all
    # and not about whether it passed. `check()` returning no violations is
    # spelled identically whether it read 615 members or three, and an index
    # that yielded almost nothing satisfies most of it trivially: CHECK 2 and
    # CHECK 3 iterate over what was found, so an empty `members` passes both.
    # Only CHECK 1 objects, via [missing] -- and only while STRICT_FAMILIES
    # stays non-empty, which is not a property this check should rely on
    # another list to supply.
    #
    # It exits 2, not 1: a misparse means the question was not answered, not
    # that the answer was no. That also keeps it out of the skip channel --
    # `--may-skip` accepts a decline, and this one is a decline, so the call
    # site in boot-test.sh will report it as a skip with this text as the
    # reason. That is the correct outcome and the reason the text says which
    # numbers it saw.
    n_symbols = sum(len(s) for s in members.values())
    if len(members) < MIN_MEMBERS or n_symbols < MIN_SYMBOLS:
        print(f"ERROR: the symbol index of {path} yielded only {len(members)} "
              f"member(s) and {n_symbols} symbol(s), below the floor of "
              f"{MIN_MEMBERS}/{MIN_SYMBOLS}.", file=sys.stderr)
        print("       A real libc.a has hundreds of both (615 and 3251 when "
              "this floor was set), so this is a", file=sys.stderr)
        print("       misparse or a truncated archive rather than a clean bill "
              "of health.", file=sys.stderr)
        print("       (This is exit 2, 'could not check', not a pass.)",
              file=sys.stderr)
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

    print(f"libc.a shape OK: {path} ({len(members)} members, "
          f"{n_symbols} symbols)")
    return 0


if __name__ == "__main__":
    if "--self-test" in sys.argv[1:] or "--selftest" in sys.argv[1:]:
        sys.exit(_selftest())
    sys.exit(main())
