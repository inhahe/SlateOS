#!/usr/bin/env python3
"""Build the 70 ring-3 fixture ELFs on demand, and verify what the image ships.

Each fixture is a small program — a C one under `services/ctest-*`, a Python
one under `services/fastpy-*` — compiled against `toolchain/sysroot/lib/libc.a`
and run by the boot test in ring 3. The compiled ELF is a **build output**: it
is gitignored, and this script rebuilds any that is missing or older than its
inputs.

Why the ELFs are not in git (design-decisions.md §355)
------------------------------------------------------
They were, until 2026-08-21, on the argument that the boot test must work on a
machine with no toolchain. That created an invariant git cannot enforce —
*this ELF was built from that source, against that libc* — because `libc.a` is
itself a gitignored build output. Git could not see the dependency, so it never
reported a binary as behind, and five separate incidents came from that.

The measurement that ended it: the stamp machinery described below covered 9 of
the 70 fixtures, and of the 61 unguarded ones **60 were stale in the tree at
once**. The drift was not an occasional accident that a gate could catch, it was
the steady state; the nine stamped fixtures were merely the only ones anybody
could see. Meanwhile the stated cost of building on demand was not real — the
kernel already `include_bytes!`s an untracked build output
(`services/netstack/target/.../netstack`), so a toolchain was already mandatory
— and rebuilding all 70 from cold takes about a minute.

What the guard became
---------------------
The old question was *provenance*: "does this stored binary still match its
inputs?" That question needs a stored binary to be about, and there is not one.
The replacement question is *completeness*, and it exists because of how a
missing fixture fails: `load_test_elf()` returns `None` for a fixture that is
not on the image and the self-test **self-skips**, so a short `/tests` runs
fewer tests and still reports PASS. Under the old arrangement reaching that
state took a deliberate deletion; now it takes a forgotten build. So
`create-ext4-rootfs.sh` counts the tracked `build.py` recipes against the ELFs
present and refuses to pack a short image.

`is_stale` uses mtime, which this script's own history argued at length against
-------------------------------------------------------------------------------
That argument was correct about committed files and does not survive them. It
ran: (1) mtime reads the working tree, so a local rebuild nobody commits
silences it permanently while git stays stale; and (2) a fresh checkout stamps
every file with one time, leaving no ordering to compare, so in a clean clone
the mtime gate is not weak but *silent*.

Both premises were about a **tracked** ELF. Point (1) needs an index the binary
can be stale in; there is no longer one. Point (2) needs the ELF to be present
in a fresh clone in order to be wrongly judged fresh; it is now absent, which
`is_stale` detects perfectly. Git also no longer writes these files, which
removes the other half of the problem — a `checkout` or `merge` used to reorder
a committed ELF against its inputs with nothing having really changed, and that
false STALE is what once blocked an image build across all three lanes.

So the content hashes are gone for the fixtures and kept for the two artifacts
that are still stored: the image manifest and the sysroot stamp. Both are below,
and `sha256_text`'s CRLF-folding rule still applies to them for the reason it was
introduced (2026-08-16): this repo sets `core.autocrlf=input`, which normalises
on commit but does not convert on checkout, so a raw byte hash of a text file is
a property of the worktree rather than of the commit, and two checkouts of the
same commit would disagree.

The image is the third place the same drift hides
-------------------------------------------------
`check` proves the ELF beside `main.c` was built from it. It says nothing
about the ELF the boot test *runs*, which is a copy inside `rootfs.ext4` —
a gitignored image built by `scripts/create-ext4-rootfs.sh` and attached by
`scripts/boot-test.sh` exactly as it finds it.

On 2026-08-16 that gap produced its own false green, within hours of the two
above. Lane B rebuilt all nine fixtures with 38 new `ctest-jobctl` checks,
ran a full boot test, and got PASS — from the *previous* image, because
nothing had rebuilt it. The serial log named the fixture's size, and the
number was the committed ELF's rather than the tree's; had that line not
existed, a merge to `main` would have carried a rung nobody had ever run.
Note that every existing guard was healthy at the time: `check` passed
(source and ELF agreed), and the rootfs script's mtime gates passed (they
compare ELF against `libc.a`, and it had not moved). Nothing anywhere asked
whether the *image* predated the ELFs it stages.

So `image-stamp` records, next to the image, the sha256 of every locally
built ELF that was staged into it, and `image-check` compares that manifest
against the tree. It is content-based for the same reason the fixture stamp
is: mtime cannot survive a checkout, and — worse here — the image is written
by QEMU on every boot, so its own mtime says only when it was last *run*.

The sysroot is the fourth, and it is the one that voids all the others
----------------------------------------------------------------------
The three guards above all compare things to `libc.a`. None of them asks
whether `libc.a` itself is current — and it is a gitignored artifact, so git
will not either. A merge that changes `posix/src` therefore leaves the whole
shelf linking a libc that is not in the tree, *silently*, and the more
diligent you are the quieter it gets: rebuild the fixtures against the stale
libc and every stamp agrees with every ELF again, because they now agree
about a stale input. Three separate requests record this happening
(`a-b-nine-ctest-fixtures-on-main-...`, `a-c-fixture-rebuild-was-correct-on-
lane-c-and-wrong-on-main`, `a-b-ctest-fixture-elfs-and-stamps-are-stale-
against-the-current-libc`), the third of them naming exactly this trap.

So `build` resolves the sysroot *first* and, since §355, **rebuilds it** rather
than advising you to. That closes the sharpest edge of the old design, which
lane A hit on 2026-08-21: the gate held one hash, could not tell which of the
two sides had moved, and therefore printed "rebuild the fixture" in the case
where the correct remedy was "rebuild the sysroot". Following it would have
relinked all nine fixtures against a stale libc and recreated incident #2 by
hand. A build step never faces that question — it rebuilds whatever is behind,
in dependency order, and which side moved stops mattering.

Usage
-----
    python scripts/ctest-fixtures.py build          # build what is missing/stale
    python scripts/ctest-fixtures.py build --force  # rebuild everything
    python scripts/ctest-fixtures.py build --only jobctl
    python scripts/ctest-fixtures.py image-stamp    # after building rootfs.ext4
    python scripts/ctest-fixtures.py image-check    # before booting it
    python scripts/ctest-fixtures.py sysroot-check  # is libc.a behind posix/src?

`build` needs zig and the fastpy `compiler` package importable (each fixture's
own `build.py` documents this), so it must run from Windows rather than from
inside WSL. The `image-*` and `sysroot-*` commands need neither and run anywhere
the image is built.

You do not normally have to arrange the import yourself: `build` looks for a
fastpy checkout beside the repo root (and honours `$FASTPY_DIR`) and puts it on
the child's `PYTHONPATH`. If it cannot find one it says so and names the two
ways to fix it, rather than letting each fixture die with a bare
`ModuleNotFoundError: No module named 'compiler'` -- which is nine identical
tracebacks that do not mention fastpy at all. To override the search:

    FASTPY_DIR="D:/visual studio projects/fastpy" python scripts/ctest-fixtures.py build
    PYTHONPATH="D:/visual studio projects/fastpy" python scripts/ctest-fixtures.py build

**Rebuild through this script, not by running `services/<name>/build.py`
directly.** `build.py` produces a correct ELF and does not touch the `.stamp`,
which still describes the *previous* one — so `check` then reports STALE for a
fixture you just rebuilt, which reads like a failed build rather than an
unwritten stamp. Lane A lost a cycle to exactly this and asked for the line
(`requests/a-b-ctest-fixture-elfs-and-stamps-are-stale-against-the-current-libc.md`).
`build` runs each `build.py` and then stamps, which is the only ordering that
leaves the pair consistent.

`stamp` exists for one narrow case: adopting fixtures that are already known
good (as when this script was introduced, the nine ELFs having just been
rebuilt and boot-verified). It records whatever is on disk, so running it to
silence a `check` failure would defeat the entire point — rebuild instead.
"""

from __future__ import annotations

import argparse
import hashlib
import os
import shutil
import subprocess
import sys
import time
from pathlib import Path

# Never let an un-encodable character eat the output.
#
# This is not defensive programming against a hypothetical: on 2026-08-20
# `python scripts/ctest-fixtures.py --help` crashed on this machine with
# `UnicodeEncodeError: 'charmap' codec can't encode character '\u2192'`. The
# docstring above is passed to argparse as the description, so `--help` prints
# it, and a Windows console is cp1252 here (cp437 on many others) rather than
# UTF-8. One right-arrow in a cross-reference was enough to make the script's
# own help unusable, and on cp437 all 30-odd em dashes would go the same way.
#
# The arrow has been replaced with `->`, but a rule that says "keep this file
# ASCII" is a rule that lasts until the next edit, because the prose style
# everywhere else in this repo reaches for em dashes by default. So the
# encoding is made non-fatal instead: a stray non-ASCII character now degrades
# to a visible `\u2014` rather than replacing the output with a traceback.
#
# The same guard, for the same reason, is at the top of
# scripts/check-libc-shape.py. See design-decisions.md S340.
for _stream in (sys.stdout, sys.stderr):
    if hasattr(_stream, "reconfigure"):
        try:
            _stream.reconfigure(errors="backslashreplace")
        except (ValueError, OSError):
            pass  # redirected to something unreconfigurable; not worth failing over

REPO = Path(__file__).resolve().parent.parent
SERVICES = REPO / "services"
LIBC = REPO / "toolchain" / "sysroot" / "lib" / "libc.a"

# What `libc.a` is built from, and the stamp recording their content at the
# moment it was built. See `sysroot_staleness` for why a stamp exists at all
# rather than the mtime comparison that used to stand alone here.
#
# `build-sysroot.ps1` is in the list because it carries the RUSTFLAGS: the
# soft-float ABI bug (BUG-SYSROOT-SOFT-FLOAT-ABI in known-issues.md) lived in
# *that file*, so a libc.a built before it is wrong even with identical sources.
#
# Kept in step with the `SYSROOT_STALE` block in scripts/create-ext4-rootfs.sh,
# which falls back to these same four roots when python is unavailable.
SYSROOT_ROOTS = (
    "posix/src",
    "posix/Cargo.toml",
    "toolchain/stubs",
    "toolchain/build-sysroot.ps1",
)
SYSROOT_STAMP = REPO / "toolchain" / "sysroot" / ".sysroot.stamp"

# Suffixes hashed with CRLF folded to LF. Anything else is hashed raw. The
# default is *raw* on purpose: folding a file that is genuinely binary could
# hide a real one-byte change, whereas hashing a text file raw only costs a
# false mismatch between worktrees — loud and recoverable rather than silent.
# See `sha256_text`.
TEXT_SUFFIXES = frozenset(
    {".rs", ".toml", ".lock", ".ps1", ".sh", ".py", ".c", ".h", ".md", ".txt", ".json", ".yaml", ".yml"}
)

# The rootfs image and the manifest recording what went into it. The manifest
# sits beside the image rather than in `build/` because the two are a pair: an
# image without its manifest is unverifiable, and deleting the image should
# leave nothing behind that claims to describe one.
ROOTFS = REPO / "rootfs.ext4"
ROOTFS_MANIFEST = REPO / "rootfs.ext4.manifest"

# Every locally built artifact the rootfs stages. Globs, not a list, for the
# same reason `fixtures()` globs: a tenth fixture, or a second ported binary, is
# covered the day it lands instead of the day somebody remembers this file.
#
# `build/spike/*.elf` is the ported-binary shelf (bash, pkgconf, make, CPython).
# Those are gitignored build products, so unlike the ctest ELFs there is no
# content stamp behind them at all — the manifest is the *only* thing that can
# catch a relink that never reached the image.
#
# `build/spike/*.zip` is not an executable and is here anyway: it is CPython's
# entire standard library (`python312.zip`, ~20 MB), and an interpreter is not
# meaningfully "the binary" — a `python3` running last week's stdlib is exactly
# as stale as one that *is* last week's binary, and fails in ways that look like
# interpreter bugs. Restricting the manifest to ELFs would have covered the
# 11 MB half of the port and left the 20 MB half unchecked.
STAGED_GLOBS = (
    "services/ctest-*/*.elf",
    "services/fastpy-*/*.elf",
    "build/spike/*.elf",
    "build/spike/*.zip",
)

# There used to be a third stamp here: a per-fixture record of the inputs that
# produced each committed ELF, so that a source change committed without its
# rebuilt binary was detectable by content. design-decisions.md §355 retired it
# along with the committed binary it described — with the ELF built on demand
# there is no artifact whose provenance needs asserting, and the question the
# stamp answered ("does this stored binary match its inputs?") has no subject.
#
# What went with it is worth naming, because it was not dead weight: the stamp
# could see a *content* change that mtime cannot, and the format-version dance
# below exists because that precision was hard-won. The replacement is coarser
# on purpose. `is_stale` asks only "is the output older than its inputs", which
# is a weaker question — but it is asked about a file that git never writes and
# that is absent rather than wrong in a fresh clone, which is exactly the
# situation mtime handles well and the one the stamp was invented to survive.
#
# The two stamps that remain have their own formats and their own lifecycles,
# and must NOT share a version constant. They once did, and bumping it broke
# both at once in a way that is worth recording, because the failure is
# invisible until you look:
#
#   * `sysroot_staleness()` diffs the on-disk sysroot stamp against a fresh
#     `compute_sysroot()` through `_describe_drift`, which compares *every*
#     field including `version`. A bump therefore reports `libc.a` as built from
#     changed inputs - a hard error, before any per-fixture verdict - on every
#     machine whose stamp predates the bump. That is the whole rootfs build,
#     in all three lanes, failing over a constant that has nothing to do with
#     the sysroot.
#   * The image manifest is luckier only by accident: `cmd_image_check` reads
#     `staged` lines and ignores the version, so a bump is silently harmless
#     there. Relying on that is relying on a reader that could grow a version
#     check any day.
#
# One version per format, bumped when *that* format changes.
SYSROOT_STAMP_VERSION = 2
IMAGE_STAMP_VERSION = 2


def sha256(path: Path) -> str:
    """Hash a file in chunks; libc.a is several MB and the ELFs ~2.6 MB each."""
    h = hashlib.sha256()
    with path.open("rb") as fh:
        for chunk in iter(lambda: fh.read(1 << 20), b""):
            h.update(chunk)
    return h.hexdigest()


def sha256_text(path: Path) -> str:
    """Hash a *text* input with CRLF folded to LF, so the stamp is portable.

    A raw byte hash of a tracked text file is not a property of the commit —
    it is a property of the working tree that produced it. This repo sets
    `core.autocrlf=input`, which normalises on commit and does *not* convert on
    checkout, so a clean checkout yields LF. But any tool that rewrites the
    file in Python/text mode on Windows writes CRLF back, and git then reports
    it clean, because `autocrlf=input` normalises again on read. Two worktrees
    of the same commit therefore legitimately hold byte-different `build.py`
    files that git considers identical.

    That is exactly what happened: `services/ctest-*/build.py` carried CRLF in
    lane B's worktree and LF in the integration worktree, so the nine stamps
    written from the former reported all nine fixtures STALE in the latter —
    on the same commit, with nothing wrong. Because `create-ext4-rootfs.sh`
    exits 1 on a stamp mismatch, that is a false failure that blocks the image
    build everywhere except the one machine that wrote the stamp, and whose
    only escape hatch — `ALLOW_STALE_FIXTURES=1` — also disables the real
    check. A stamp that is not reproducible across checkouts fails at the one
    thing the module docstring claims for it: surviving a fresh clone.

    Folding is *sound* rather than merely convenient, because it is applied
    only to inputs whose compiled result does not depend on line endings: C
    source (the preprocessor treats CRLF as a line break) and the Python build
    script (Python's own tokenizer accepts both). `libc.a` and the output ELF
    are hashed raw by `sha256`, where a single byte genuinely matters.

    Not chunked: the largest text input here is a few tens of KB.
    """
    return hashlib.sha256(path.read_bytes().replace(b"\r\n", b"\n")).hexdigest()


def fixtures() -> list[Path]:
    """Every fixture directory holding a `build.py`, sorted for stable output.

    Both families: the nine `services/ctest-*` C fixtures and the 61
    `services/fastpy-*` ones. They were separate populations only because the
    C nine were the ones carrying stamps; with the ELFs no longer stored in git
    (design-decisions.md §355) there is one job — build what is missing — and it
    is the same job for both. Keeping them separate is what let 60 of the 61
    fastpy fixtures sit stale with nothing able to notice.

    Globbed rather than listed so a new fixture is covered the day it lands.
    """
    found = [d for d in SERVICES.glob("ctest-*") if (d / "build.py").is_file()]
    found += [d for d in SERVICES.glob("fastpy-*") if (d / "build.py").is_file()]
    return sorted(found)


def elf_of(fixture: Path) -> Path:
    return fixture / f"{fixture.name}.elf"


def _inputs(fixture: Path) -> list[tuple[str, Path, bool]]:
    """The files whose content determines the ELF.

    `build.py` stands in for the compile/link flags, which live nowhere else.
    `main.c` exists only for the C fixtures — a fastpy fixture's source is
    embedded in its `build.py` — so it is included only when present rather
    than reported as a missing input.

    The third element is "this is text": true for tracked sources, whose line
    endings differ between worktrees without differing between commits, and
    false for `libc.a`, which is a build product where every byte counts.
    Getting this flag *wrong in the false direction* would make the comparison
    unreproducible; wrong in the true direction would let a real single-byte
    change hide, but only if that byte were a `\\r` immediately before a `\\n`
    in a binary — see `sha256_text`.
    """
    got: list[tuple[str, Path, bool]] = [("build.py", fixture / "build.py", True)]
    main_c = fixture / "main.c"
    if main_c.is_file():
        got.append(("main.c", main_c, True))
    got.append(("toolchain/sysroot/lib/libc.a", LIBC, False))
    return got


def is_stale(fixture: Path) -> str | None:
    """Why `fixture` needs rebuilding, or `None` if its ELF is current.

    An ordering test on mtime, which is the right instrument again now that the
    ELFs are build outputs rather than committed files. The long-standing
    objection to mtime here — recorded at length on `sysroot_staleness` — was
    that git writes files it has not edited, so a `checkout` or `merge` could
    make a *committed* ELF look stale or fresh at random. That objection dies
    with the committed ELF: git no longer writes these files at all, and the
    case mtime was genuinely blind to (a fresh clone, where every file shares
    one timestamp) is now the case it detects best, because in a fresh clone
    the ELF is simply absent.
    """
    elf = elf_of(fixture)
    if not elf.is_file():
        return "no ELF"
    try:
        built = elf.stat().st_mtime
    except OSError as exc:  # pragma: no cover - racing filesystem
        return f"cannot stat ELF ({exc})"
    behind = []
    for label, path, _is_text in _inputs(fixture):
        try:
            if path.stat().st_mtime > built:
                behind.append(label)
        except OSError:
            # A missing input is the build's problem to report, not ours; it
            # is not evidence about the ELF's freshness either way.
            continue
    return f"older than {', '.join(behind)}" if behind else None


def _body(text: str) -> list[str]:
    """Comment-stripped, blank-stripped lines — what actually gets compared."""
    return [ln for ln in text.splitlines() if ln.strip() and not ln.startswith("#")]


def _describe_drift(recorded: str, actual: str) -> list[str]:
    """Name the specific inputs that moved, not just 'they differ'.

    Which one moved is the diagnosis: `main.c` means a source edit was
    committed without its rebuild, `libc.a` means the fixture needs a relink,
    and the ELF alone means the binary was replaced behind the build's back.
    """
    def index(text: str) -> dict[str, str]:
        out: dict[str, str] = {}
        for ln in _body(text):
            parts = ln.split()
            if len(parts) >= 4 and parts[0] in ("input", "output"):
                out[f"{parts[0]} {parts[1]}"] = parts[3]
            elif len(parts) >= 4 and parts[0] == "builder":
                # `builder <name> <kind> <value...>`. Keyed by name, not by the
                # first token, or the three builder lines would collapse into
                # one entry and only the last would ever be reported. The value
                # keeps every remaining token because a version banner is
                # several words ("LLD 20.1.1 (compatible with GNU linkers)"),
                # unlike a hash.
                out[f"builder {parts[1]}"] = " ".join(parts[3:])
            elif parts:
                out[parts[0]] = " ".join(parts[1:])
        return out

    was, now = index(recorded), index(actual)
    drift: list[str] = []
    for key in sorted(set(was) | set(now)):
        old, new = was.get(key), now.get(key)
        if old == new:
            continue
        if old is None:
            drift.append(f"{key}: not in the recorded stamp (new input)")
        elif new is None:
            drift.append(f"{key}: recorded but no longer present")
        else:
            drift.append(f"{key}: recorded {_elide(old)} but on disk {_elide(new)}")
    return drift


def _elide(value: str) -> str:
    """Shorten a hash to its first 16 hex digits; leave anything else intact.

    A 64-character hash is unreadable in full and unambiguous in its prefix. A
    version banner is neither: truncating "LLD 20.1.1 (compatible with GNU
    linkers)" at 16 characters cuts off the digits that are the entire point of
    printing it. So the test is on the *shape* of the value, not on the field.
    """
    if len(value) == 64 and all(c in "0123456789abcdef" for c in value):
        return f"{value[:16]}..."
    return f"'{value}'"


def _self_cmd() -> str:
    """How to re-invoke this script *from the shell that is reading the message*.

    The rootfs build runs under WSL Ubuntu, which has `python3` and no `python`,
    so a hard-coded "python scripts/..." hint is a command that fails when
    pasted in the very place it is printed. `sys.executable` is the interpreter
    actually running, so it is correct on both sides of the WSL boundary.
    """
    return f"{sys.executable} scripts/ctest-fixtures.py"


def _sysroot_inputs() -> list[tuple[str, Path, bool]]:
    """Every file `libc.a` is built from, as (repo-relative label, path, is_text).

    Sorted by label so the stamp is byte-identical for the same tree regardless
    of directory iteration order.

    `target/` is excluded: it holds the *output* of building these sources, and
    a build artifact that appears in the input list would make the stamp
    self-referential — building the sysroot would change its own inputs, so no
    two consecutive runs could ever agree. (The mtime scan this replaces had the
    same exposure and got away with it only because nobody had run cargo inside
    `toolchain/stubs`.) Dot-directories go too, for `.git` and friends.
    """
    out: list[tuple[str, Path, bool]] = []
    for rel in SYSROOT_ROOTS:
        root = REPO / rel
        if not root.exists():
            continue
        candidates = [root] if root.is_file() else sorted(root.rglob("*"))
        for path in candidates:
            if not path.is_file():
                continue
            try:
                label = path.relative_to(REPO).as_posix()
            except ValueError:
                continue
            parts = label.split("/")
            if any(p == "target" or p.startswith(".") for p in parts):
                continue
            out.append((label, path, path.suffix.lower() in TEXT_SUFFIXES))
    out.sort(key=lambda item: item[0])
    return out


def compute_sysroot() -> str:
    """The stamp text describing the sysroot's inputs as they are on disk now."""
    lines = [
        "# sysroot input stamp - generated by scripts/ctest-fixtures.py sysroot-stamp\n"
        "# Records the content of everything toolchain/sysroot/lib/libc.a was built\n"
        "# from, so a source change that arrived without a sysroot rebuild is\n"
        "# detectable by content. Written by toolchain/build-sysroot.ps1.\n"
        "# Text inputs are hashed with CRLF folded to LF; binary ones raw.\n",
        f"version {SYSROOT_STAMP_VERSION}\n",
    ]
    for label, path, is_text in _sysroot_inputs():
        try:
            digest = sha256_text(path) if is_text else sha256(path)
        except OSError:
            # A file that vanished mid-scan is not evidence either way, and a
            # stamp that dies here would leave the sysroot with none at all.
            continue
        lines.append(f"input  {label} sha256 {digest}\n")
    return "".join(lines)


def sysroot_staleness() -> tuple[str, list[str]]:
    """Is `libc.a` behind the sources it is built from? Returns (mode, findings).

    Every fixture stamp records `libc.a`'s hash, so `check` can prove a fixture
    matches the libc *on disk* — and that is the narrower question than the one
    a reader believes it is asking. `libc.a` is a gitignored build artifact, so
    a merge or a checkout that changes `posix/src` leaves it behind without
    saying anything. From that moment the whole shelf links a libc that is not
    in the tree, and every stamp agrees with every ELF, because they agree about
    a stale input. That is a green check over a fixture set nobody can vouch
    for, which is the one outcome this script exists to prevent.

    It has now happened three times (`requests/a-b-nine-ctest-fixtures-on-main-...`,
    `requests/a-c-fixture-rebuild-was-correct-on-lane-c-and-wrong-on-main.md`,
    `requests/a-b-ctest-fixture-elfs-and-stamps-are-stale-against-the-current-libc.md`),
    and on each the fixtures' own checks stayed quiet precisely because they
    were freshly rebuilt.

    **Why this is a content stamp and no longer an mtime comparison.** The
    original reasoning here was: *"mtime, not a hash, because the question is
    'was this built after that was edited' — an ordering, which a content hash
    of a file the stamps do not track cannot answer."* The ordering argument is
    sound; the premise was not. mtime does not record when a file was *edited*,
    it records when it was *written*, and git writes files it has not edited —
    `checkout`, `merge` and `stash` all do. Since `CLAUDE.md` requires a merge
    from `origin/main` at the start of every task, that is routine, not an edge
    case.

    Worse, the gate it produced was **unsatisfiable by its own instructions**:
    it told the reader to re-run `build-sysroot.ps1`, but PowerShell's
    `Copy-Item` preserves the source timestamp, so `libc.a`'s mtime is whenever
    cargo last *linked* `libposix.a`. If posix has not changed, cargo does not
    relink, the mtime does not move, and re-running changes nothing — leaving
    `ALLOW_STALE_FIXTURES=1` as the only exit, which also disables the real
    content check. See known-issues.md
    `A-SYSROOT-STALENESS-GATE-IS-WEDGED-BY-GIT-TOUCHING-A-FILE-IT-WATCHES`.

    A stamp answers the objection directly: the inputs stop being "files the
    stamps do not track". It is also satisfiable, because `build-sysroot.ps1`
    rewrites the stamp every run whether or not cargo relinked anything.

    Returns `("stamp", drift)` when the stamp exists — `drift` naming the inputs
    whose content moved — and `("mtime", newer)` when it does not, falling back
    to the old ordering test exactly as the fixture gate falls back when python
    is absent. `("", [])` means nothing to report.
    """
    if not LIBC.is_file():
        return "", []

    if SYSROOT_STAMP.is_file():
        try:
            recorded = SYSROOT_STAMP.read_text(encoding="utf-8")
        except OSError:
            recorded = ""
        if recorded.strip():
            return "stamp", _describe_drift(recorded, compute_sysroot())

    libc_mtime = LIBC.stat().st_mtime
    newer: list[str] = []
    for label, path, _is_text in _sysroot_inputs():
        try:
            if path.stat().st_mtime > libc_mtime:
                newer.append(label)
        except OSError:
            # A file that vanished mid-scan is not evidence either way.
            continue
    return "mtime", newer


def _report_sysroot_staleness(mode: str, findings: list[str]) -> None:
    """Print the diagnosis for a stale `libc.a`. Caller decides the exit code."""
    rel = LIBC.relative_to(REPO).as_posix()
    if mode == "stamp":
        print(f"[ctest] ERROR: {rel} was built from {len(findings)} input(s) that have since changed.")
    else:
        print(f"[ctest] ERROR: {rel} is OLDER than {len(findings)} tracked source(s).")
    for name in findings[:8]:
        print(f"[ctest]          {'changed' if mode == 'stamp' else 'newer'}: {name}")
    if len(findings) > 8:
        print(f"[ctest]          ... and {len(findings) - 8} more")
    print("[ctest]        libc.a is a gitignored build artifact, so git cannot tell you it is behind.")
    print("[ctest]        Every fixture below links it, so a per-fixture 'ok' only means the ELF")
    print("[ctest]        matches a libc that is not the one in the tree. Rebuild in this order:")
    print("[ctest]          powershell -File toolchain/build-sysroot.ps1")
    print(f"[ctest]          {_self_cmd()} build")
    print("[ctest]          wsl -d Ubuntu -- bash scripts/bash-spike/slatelink.sh   # if present")
    print("[ctest]          wsl -d Ubuntu -- bash scripts/create-ext4-rootfs.sh")
    if mode == "mtime":
        print(f"[ctest]        (no {SYSROOT_STAMP.relative_to(REPO).as_posix()}, so this is the mtime")
        print("[ctest]         fallback, which git can trip by merely writing a file. Re-running")
        print("[ctest]         build-sysroot.ps1 writes the stamp and makes this check exact.)")


def _fastpy_dir() -> Path | None:
    """The fastpy checkout whose `compiler` package each build.py imports.

    Searched, in order: $FASTPY_DIR, anything already on $PYTHONPATH, then a
    sibling of the repo root named exactly `fastpy`. The sibling lookup is what
    makes the common case need no configuration at all -- every worktree here
    (`os`, `os-lane-a/b/c`) sits next to `fastpy` in the same parent directory.

    The sibling must be named `fastpy`, not merely *contain* a `compiler`
    package. An earlier draft accepted the first sibling that looked importable
    and so selected `_fastpy_before` -- an old snapshot that sorts ahead of
    `fastpy` -- silently cross-compiling nine fixtures against a stale
    compiler. Picking a build input by "first directory that seems plausible"
    is not a search, it is a guess; when the guess is wrong it produces working
    binaries built from the wrong source, which no later check catches. Anyone
    whose checkout lives elsewhere sets FASTPY_DIR.

    A candidate only counts if it actually contains `compiler/__init__.py`, so
    that a wrong path is reported here rather than nine times over as a bare
    import error in the children.
    """

    def usable(p: Path) -> bool:
        return (p / "compiler" / "__init__.py").is_file()

    env_dir = os.environ.get("FASTPY_DIR")
    if env_dir:
        cand = Path(env_dir)
        # An explicit FASTPY_DIR that is wrong is a mistake worth reporting,
        # not something to silently fall through from into a sibling that
        # happens to work -- that would build against a checkout the caller
        # did not name.
        if usable(cand):
            return cand
        print(f"[ctest] ERROR: FASTPY_DIR={env_dir} has no compiler/__init__.py")
        return None

    for entry in os.environ.get("PYTHONPATH", "").split(os.pathsep):
        if entry and usable(Path(entry)):
            return Path(entry)

    sibling = REPO.parent / "fastpy"
    return sibling if usable(sibling) else None


def _build_sysroot() -> bool:
    """Rebuild `libc.a` from `posix/src`. True if it now exists and is current.

    §355 promotes the sysroot check from advice into an action. The old gate
    printed "rebuild in this order: build-sysroot.ps1, then build" and left the
    reader to do it, which is where lane A's wrong-remedy incident came from:
    the gate held one hash, could not tell which side had moved, and so had to
    guess which of the two rebuilds to recommend. A build step never has to
    guess — it rebuilds whatever is behind, in dependency order, and the
    question of "which side moved" stops being one anybody has to answer.
    """
    script = REPO / "toolchain" / "build-sysroot.ps1"
    if not script.is_file():
        print(f"[ctest] ERROR: no {script.relative_to(REPO).as_posix()} to rebuild the sysroot with")
        return False
    for exe in ("powershell.exe", "powershell", "pwsh"):
        found = shutil.which(exe)
        if found:
            break
    else:
        print("[ctest] ERROR: libc.a needs rebuilding but no PowerShell was found to run")
        print(f"[ctest]        {script.relative_to(REPO).as_posix()}. Run it from Windows, then re-run this.")
        return False
    print(f"[ctest] sysroot: rebuilding libc.a via {script.relative_to(REPO).as_posix()} ...")
    result = subprocess.run(
        [found, "-NoProfile", "-File", str(script)],
        cwd=str(REPO),
        timeout=3600,
        check=False,
    )
    if result.returncode != 0:
        print(f"[ctest] ERROR: build-sysroot.ps1 exited {result.returncode}")
        return False
    return LIBC.is_file()


def cmd_build(only: str | None, force: bool = False) -> int:
    # A stale libc.a is not a warning any more. Every fixture links it, so
    # building against one that is behind produces 70 binaries that test a
    # system this tree cannot build -- which is the whole of B-Q5. Fix it
    # first, then build on top of a sysroot that matches the source.
    if not LIBC.is_file() or sysroot_staleness()[1]:
        if LIBC.is_file():
            mode, stale = sysroot_staleness()
            print(f"[ctest] sysroot: libc.a is behind {len(stale)} input(s) ({mode}); rebuilding before the fixtures.")
        if not _build_sysroot():
            return 1
        mode, stale = sysroot_staleness()
        if stale:
            _report_sysroot_staleness(mode, stale)
            print("[ctest] ERROR: libc.a is still behind after a rebuild; not building fixtures against it.")
            return 1
    # Resolve fastpy before building anything. Without it every fixture dies
    # with `ModuleNotFoundError: No module named 'compiler'`, which names
    # neither fastpy nor PYTHONPATH -- nine identical tracebacks that read like
    # a broken toolchain rather than an unset variable.
    fastpy = _fastpy_dir()
    if fastpy is None:
        print("[ctest] ERROR: cannot find a fastpy checkout (needs compiler/__init__.py).")
        print("[ctest]        Each services/ctest-*/build.py imports fastpy's `compiler`")
        print("[ctest]        package to drive the zig cross-compile. Point at it with:")
        print("[ctest]          FASTPY_DIR=<path-to-fastpy> " f"{_self_cmd()} build")
        print("[ctest]        or place the fastpy checkout beside this repo:")
        print(f"[ctest]          {REPO.parent / 'fastpy'}")
        return 1
    child_env = dict(os.environ)
    existing = child_env.get("PYTHONPATH", "")
    child_env["PYTHONPATH"] = (
        f"{fastpy}{os.pathsep}{existing}" if existing else str(fastpy)
    )
    print(f"[ctest] fastpy: {fastpy}")
    selected = [f for f in fixtures() if not only or only in f.name]
    if not selected:
        print(f"[ctest] ERROR: no fixture matches --only {only!r}" if only
              else f"[ctest] ERROR: no fixtures found under {SERVICES}")
        return 1
    rc, built, skipped = 0, 0, 0
    started = time.monotonic()
    for fixture in selected:
        why = "forced" if force else is_stale(fixture)
        if why is None:
            skipped += 1
            continue
        print(f"[ctest] building {fixture.name} ({why}) ...")
        result = subprocess.run(
            [sys.executable, str(fixture / "build.py")],
            capture_output=True,
            text=True,
            timeout=900,
            env=child_env,
        )
        if result.returncode != 0:
            print(f"[ctest] ERROR {fixture.name}: build.py exited {result.returncode}")
            print(result.stdout)
            print(result.stderr)
            rc = 1
            continue
        built += 1
        print(f"[ctest]   {result.stdout.strip().splitlines()[-1] if result.stdout.strip() else 'built'}")
    print(
        f"[ctest] {built} built, {skipped} already current, "
        f"{len(selected)} total in {time.monotonic() - started:.0f}s"
    )
    if rc:
        print("[ctest] ERROR: at least one fixture failed to build; the image would be short.")
    return rc


def _staged_artifacts() -> list[Path]:
    """Every locally built file the rootfs stages, sorted, repo-relative order."""
    found: list[Path] = []
    for pattern in STAGED_GLOBS:
        found.extend(p for p in REPO.glob(pattern) if p.is_file())
    return sorted(set(found))


_IMAGE_HEADER = (
    "# rootfs.ext4 content manifest - generated by scripts/ctest-fixtures.py\n"
    "# The sha256 of every locally built artifact that was staged into the image\n"
    "# (the ctest/fastpy ELFs, the ported binaries, and CPython's stdlib zip).\n"
    "# `image-check` compares this against the tree, so a fixture rebuilt after\n"
    "# the image was packed is caught BEFORE a boot test reports PASS about the\n"
    "# previous binary. Regenerate by rebuilding the image:\n"
    "#   wsl -d Ubuntu -- bash scripts/create-ext4-rootfs.sh\n"
)


def cmd_image_stamp() -> int:
    if not ROOTFS.is_file():
        print(f"[ctest] ERROR: no {ROOTFS.name} to stamp ({ROOTFS})")
        return 1
    staged = _staged_artifacts()
    lines = [_IMAGE_HEADER, f"version {IMAGE_STAMP_VERSION}\n"]
    for art in staged:
        lines.append(f"staged {art.relative_to(REPO).as_posix()} sha256 {sha256(art)}\n")
    ROOTFS_MANIFEST.write_text("".join(lines), encoding="utf-8", newline="\n")
    print(f"[ctest] stamped {ROOTFS_MANIFEST.name} ({len(staged)} staged artifacts)")
    return 0


def cmd_image_check() -> int:
    if not ROOTFS.is_file():
        # No image is a legitimate state: it is gitignored, the boot test omits
        # it, and the Path-Z rungs self-skip. Silence would be wrong (that is
        # `B-PATHZ-PREREQUISITE-SKIPS-ARE-SILENT`), so say so and pass.
        print(f"[ctest] no {ROOTFS.name} - nothing to verify (Path-Z rungs will self-skip)")
        return 0
    if not ROOTFS_MANIFEST.is_file():
        print(f"[ctest] ERROR: {ROOTFS.name} exists but {ROOTFS_MANIFEST.name} does not.")
        print("[ctest]        The image predates this check, so what is inside it")
        print("[ctest]        cannot be established. Rebuild it:")
        print("[ctest]          wsl -d Ubuntu -- bash scripts/create-ext4-rootfs.sh")
        return 1

    recorded: dict[str, str] = {}
    for ln in _body(ROOTFS_MANIFEST.read_text(encoding="utf-8")):
        parts = ln.split()
        if len(parts) == 4 and parts[0] == "staged":
            recorded[parts[1]] = parts[3]

    actual = {p.relative_to(REPO).as_posix(): sha256(p) for p in _staged_artifacts()}
    drift: list[str] = []
    for key in sorted(set(recorded) | set(actual)):
        was, now = recorded.get(key), actual.get(key)
        if was == now:
            continue
        if was is None:
            drift.append(f"{key}: built after the image was packed (not in it)")
        elif now is None:
            drift.append(f"{key}: staged into the image but no longer in the tree")
        else:
            drift.append(f"{key}: image has {was[:16]}..., tree has {now[:16]}...")

    if drift:
        print(f"[ctest] ERROR: {ROOTFS.name} is STALE - it does not hold what this tree built.")
        for line in drift:
            print(f"[ctest]          {line}")
        print("[ctest]        A boot test against this image reports PASS about")
        print("[ctest]        binaries that are no longer the ones you built. Repack it:")
        print("[ctest]          wsl -d Ubuntu -- bash scripts/create-ext4-rootfs.sh")
        return 1
    print(f"[ctest] ok {ROOTFS.name} ({len(actual)} staged artifacts match the tree)")
    return 0


def cmd_sysroot_stamp() -> int:
    """Record what the sysroot was just built from. Called by build-sysroot.ps1.

    Deliberately *not* implemented in PowerShell alongside the copy it follows.
    The checker is here, and a stamp written by one implementation and verified
    by another is a second place for the CRLF-folding rule to be got wrong —
    which is the exact bug that took the fixture stamps to `version 2`.

    **Running this by hand asserts something you have to have earned.** It
    records the sources as they are *now* and claims libc.a was built from
    them; it does not verify that, because it cannot. Invoked out of order it
    is a silencer for exactly the failure the stamp exists to catch — the same
    hazard `stamp` carries for the fixtures, and the reason both are separate
    commands rather than something `check` does on your behalf. If the check is
    complaining, run `toolchain/build-sysroot.ps1`, which rebuilds first and
    stamps second.
    """
    if not LIBC.is_file():
        print(f"[ctest] ERROR: no {LIBC.relative_to(REPO).as_posix()} to stamp")
        return 1
    SYSROOT_STAMP.parent.mkdir(parents=True, exist_ok=True)
    text = compute_sysroot()
    SYSROOT_STAMP.write_text(text, encoding="utf-8", newline="\n")
    print(f"[ctest] stamped {SYSROOT_STAMP.relative_to(REPO).as_posix()} ({len(_body(text)) - 1} inputs)")
    return 0


def cmd_sysroot_check() -> int:
    """Standalone verdict on the sysroot, for callers that cannot import this.

    `scripts/create-ext4-rootfs.sh` runs under WSL bash and had its own `find`
    reimplementation of the mtime test. It now shells out to this instead, so
    the input list and the folding rule exist once.
    """
    if not LIBC.is_file():
        print(f"[ctest] ERROR: missing {LIBC.relative_to(REPO).as_posix()}; run toolchain/build-sysroot.ps1")
        return 1
    mode, stale = sysroot_staleness()
    if stale:
        _report_sysroot_staleness(mode, stale)
        return 1
    where = "content stamp" if mode == "stamp" else "mtime fallback"
    print(f"[ctest] ok sysroot ({where}: libc.a matches the sources it is built from)")
    return 0


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument(
        "command",
        choices=("build", "image-stamp", "image-check", "sysroot-stamp", "sysroot-check"),
    )
    parser.add_argument("--only", help="substring of a fixture directory name, e.g. jobctl")
    parser.add_argument(
        "--force",
        action="store_true",
        help="rebuild every selected fixture, not just the ones whose inputs moved",
    )
    args = parser.parse_args()
    if args.command == "build":
        return cmd_build(args.only, args.force)
    if args.command == "image-stamp":
        return cmd_image_stamp()
    if args.command == "image-check":
        return cmd_image_check()
    if args.command == "sysroot-stamp":
        return cmd_sysroot_stamp()
    return cmd_sysroot_check()


if __name__ == "__main__":
    sys.exit(main())
