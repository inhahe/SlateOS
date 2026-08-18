#!/usr/bin/env python3
"""Build and verify the `services/ctest-*` ring-3 fixtures by *content*.

Each fixture is a small C program checked into git twice: once as `main.c`
and once as the compiled `ctest-<name>.elf` that the boot test actually runs.
Tracking the binary is deliberate — the boot test has to work on a machine
with no zig/WSL toolchain to rebuild it — but it creates an invariant that git
does not enforce: *this ELF was built from that source, against that libc*.

Nothing enforced it, and it broke. On 2026-08-16 commit `6c89903d0` added 236
lines to `services/ctest-jobctl/main.c` (33 new `waitid` checks) without the
rebuilt ELF, and every boot test still passed, because each worktree happened
to hold a locally-rebuilt binary that was never staged. All nine tracked ELFs
were simultaneously ~19,400 bytes behind the current `libc.a`. See
`known-issues.md` →
`B-THE-TRACKED-FIXTURE-BINARIES-DRIFT-FROM-THEIR-SOURCES`.

Why not timestamps
------------------
`scripts/create-ext4-rootfs.sh` already compares each ELF's **mtime** against
`libc.a`'s. That is the right question for a build directory and the wrong one
here, for two independent reasons:

1. It reads the working tree, so rebuilding locally silences it permanently —
   without anything reaching the index. Git stays exactly as stale as it was
   while every local check reports green. That is how this drifted twice
   before (`06d6d1f69`, `94d036ee2` are the same relink, done and undone).
2. **A fresh checkout destroys the information it depends on.** `git clone`
   stamps every file with the checkout time, so `main.c`, `libc.a` and the ELF
   are all the same age and there is no ordering left to compare. In a clean
   clone — CI, a new machine, the operator's next `git clone` — the mtime gate
   is not merely weak, it is silent.

So the stamp is a content hash, which survives a checkout because it does not
depend on the filesystem's opinion of time.

What is hashed
--------------
Inputs are `build.py`, `main.c`, and the `toolchain/sysroot/lib/libc.a` that
was linked in. `build.py` is included because it *is* the compile and link
flags — hashing it means a change to `-O2`, the code model, or the entry
symbol invalidates the fixture exactly as a source edit does, with no separate
list of flags to keep in sync. The output ELF is hashed too, so a corrupted or
hand-substituted binary is caught even when every input matches.

Text inputs are hashed with CRLF folded to LF (stamp format **v2**)
------------------------------------------------------------------
The same argument that rules out mtimes rules out hashing a *text* file's raw
worktree bytes, and for a while this script did exactly that. A raw byte hash
of `build.py` is a property of the worktree that produced it, not of the
commit: this repo sets `core.autocrlf=input`, which normalises on commit but
does not convert on checkout, so any Windows tool that rewrites the file in
text mode leaves CRLF behind and git still reports it clean. Two worktrees of
the *same commit* then hold byte-different sources, and the nine stamps written
in one report all nine fixtures STALE in the other.

That is not hypothetical — it is why v2 exists (2026-08-16): every fixture
failed in the integration worktree immediately after a merge in which nothing
about them had changed. Since `create-ext4-rootfs.sh` exits 1 on a stamp
mismatch, a false STALE blocks the image build on every machine except the one
that wrote the stamp, and its only escape hatch (`ALLOW_STALE_FIXTURES=1`)
switches off the genuine check alongside it. See `sha256_text` for why folding
is sound rather than merely convenient, and why `libc.a` and the ELF are still
hashed byte-for-byte.

A v1 stamp is reported as a **format** mismatch, not as drift, and there is no
compatibility path: v1 and v2 hashes were computed under different rules, so
comparing them could only produce an unfounded accusation.

A missing stamp is a FAILURE, not a skip
----------------------------------------
`check` fails on a fixture that has no `.stamp`. This is the whole reason the
driver globs `services/ctest-*/` instead of naming the nine directories: a
tenth fixture is picked up automatically, and if it somehow is not stamped,
the rootfs build fails loudly on its first run rather than quietly covering
eight of nine. Defaulting an unrecognised artifact to "pass" is the failure
mode `B-PATHZ-PREREQUISITE-SKIPS-ARE-SILENT` is named after.

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

So `check` now runs `sysroot_staleness()` first and fails on it, before any
per-fixture verdict — the per-fixture `ok` lines would otherwise read as a
clean bill of health for a set nobody can vouch for. This is the one guard
here that uses mtime rather than a content hash, because the question is an
ordering ("was this built after that was edited"), which a hash of a file the
stamps do not track cannot answer.

Usage
-----
    python scripts/ctest-fixtures.py check          # verify, exit 1 on drift
    python scripts/ctest-fixtures.py build          # rebuild all, then stamp
    python scripts/ctest-fixtures.py build --only jobctl
    python scripts/ctest-fixtures.py stamp          # re-stamp without building
    python scripts/ctest-fixtures.py image-stamp    # after building rootfs.ext4
    python scripts/ctest-fixtures.py image-check    # before booting it

`build` needs zig and the fastpy `compiler` package importable (each fixture's
own `build.py` documents this); `check` and `stamp` need neither, so the gate
runs anywhere.

`stamp` exists for one narrow case: adopting fixtures that are already known
good (as when this script was introduced, the nine ELFs having just been
rebuilt and boot-verified). It records whatever is on disk, so running it to
silence a `check` failure would defeat the entire point — rebuild instead.
"""

from __future__ import annotations

import argparse
import hashlib
import subprocess
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
SERVICES = REPO / "services"
LIBC = REPO / "toolchain" / "sysroot" / "lib" / "libc.a"

# The rootfs image and the manifest recording what went into it. The manifest
# sits beside the image rather than in `build/` because the two are a pair: an
# image without its manifest is unverifiable, and deleting the image should
# leave nothing behind that claims to describe one.
ROOTFS = REPO / "rootfs.ext4"
ROOTFS_MANIFEST = REPO / "rootfs.ext4.manifest"

# Every locally built ELF the rootfs stages. Globs, not a list, for the same
# reason `fixtures()` globs: a tenth fixture, or a second ported binary, is
# covered the day it lands instead of the day somebody remembers this file.
#
# `build/spike/*.elf` is the ported-binary shelf (bash, pkgconf, CPython). Those
# are gitignored build products, so unlike the ctest ELFs there is no content
# stamp behind them at all — the manifest is the *only* thing that can catch a
# relink that never reached the image.
STAGED_GLOBS = (
    "services/ctest-*/*.elf",
    "services/fastpy-*/*.elf",
    "build/spike/*.elf",
)

STAMP_VERSION = 2
_HEADER = (
    "# ctest fixture input stamp - generated by scripts/ctest-fixtures.py\n"
    "# Records the inputs that produced this ELF, so a source change committed\n"
    "# without its rebuilt binary is detectable by content. Timestamps cannot\n"
    "# do this: a fresh checkout gives every file the same mtime.\n"
    "# Text inputs are hashed with CRLF folded to LF (version 2); binary ones\n"
    "# byte-for-byte. See `sha256_text` for why.\n"
    "# Regenerate with: scripts/ctest-fixtures.py build\n"
    "#   (run it with `python` on Windows, `python3` under WSL/Linux)\n"
)


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
    """Every `services/ctest-*/` holding a `build.py`, sorted for stable output.

    Globbed rather than listed so a new fixture is covered the day it lands.
    """
    return sorted(d for d in SERVICES.glob("ctest-*") if (d / "build.py").is_file())


def elf_of(fixture: Path) -> Path:
    return fixture / f"{fixture.name}.elf"


def stamp_of(fixture: Path) -> Path:
    return fixture / f"{fixture.name}.stamp"


def _inputs(fixture: Path) -> list[tuple[str, Path, bool]]:
    """The files whose content determines the ELF, in stamp order.

    `build.py` stands in for the compile/link flags, which live nowhere else.

    The third element is "this is text": true for tracked sources, whose line
    endings differ between worktrees without differing between commits, and
    false for `libc.a`, which is a build product where every byte counts.
    Getting this flag *wrong in the false direction* would make the stamp
    unreproducible again; wrong in the true direction would let a real
    single-byte change hide, but only if that byte were a `\\r` immediately
    before a `\\n` in a binary — see `sha256_text`.
    """
    return [
        ("build.py", fixture / "build.py", True),
        ("main.c", fixture / "main.c", True),
        ("toolchain/sysroot/lib/libc.a", LIBC, False),
    ]


def compute(fixture: Path) -> tuple[str, list[str]]:
    """Return (stamp text, missing-file descriptions) for `fixture`.

    Missing files are reported rather than raising, so `check` can say *which*
    piece is absent instead of dying on the first one.
    """
    missing: list[str] = []
    lines = [_HEADER, f"version {STAMP_VERSION}\n"]
    for label, path, is_text in _inputs(fixture):
        if not path.is_file():
            missing.append(f"missing input {label} ({path})")
            continue
        digest = sha256_text(path) if is_text else sha256(path)
        lines.append(f"input  {label} sha256 {digest}\n")
    elf = elf_of(fixture)
    if not elf.is_file():
        missing.append(f"missing output {elf.name} ({elf})")
    else:
        lines.append(f"output {elf.name} sha256 {sha256(elf)} size {elf.stat().st_size}\n")
    return "".join(lines), missing


def _body(text: str) -> list[str]:
    """Comment-stripped, blank-stripped lines — what actually gets compared."""
    return [ln for ln in text.splitlines() if ln.strip() and not ln.startswith("#")]


def _stamp_version(text: str) -> int:
    """The `version` field of a stamp, or 0 if it has none.

    0 rather than an exception: a stamp too malformed to state its own format
    still has to produce a message a reader can act on, and "format v0" routes
    to the same repair as any other unreadable format.
    """
    for ln in _body(text):
        parts = ln.split()
        if len(parts) == 2 and parts[0] == "version" and parts[1].isdigit():
            return int(parts[1])
    return 0


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
            drift.append(f"{key}: recorded {old[:16]}... but on disk {new[:16]}...")
    return drift


def _self_cmd() -> str:
    """How to re-invoke this script *from the shell that is reading the message*.

    The rootfs build runs under WSL Ubuntu, which has `python3` and no `python`,
    so a hard-coded "python scripts/..." hint is a command that fails when
    pasted in the very place it is printed. `sys.executable` is the interpreter
    actually running, so it is correct on both sides of the WSL boundary.
    """
    return f"{sys.executable} scripts/ctest-fixtures.py"


def sysroot_staleness() -> list[str]:
    """Repo-relative sources that are newer than `libc.a`, newest question first.

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
    were freshly rebuilt. mtime, not a hash, because the question is "was this
    built after that was edited" — an ordering, which a content hash of a file
    the stamps do not track cannot answer.

    Mirrors the `SYSROOT_STALE` block in `scripts/create-ext4-rootfs.sh`; keep
    the two input lists in step.
    """
    if not LIBC.is_file():
        return []
    libc_mtime = LIBC.stat().st_mtime
    roots = [
        REPO / "posix" / "src",
        REPO / "posix" / "Cargo.toml",
        REPO / "toolchain" / "stubs",
        REPO / "toolchain" / "build-sysroot.ps1",
    ]
    newer: list[str] = []
    for root in roots:
        if not root.exists():
            continue
        candidates = [root] if root.is_file() else sorted(root.rglob("*"))
        for path in candidates:
            if not path.is_file():
                continue
            try:
                if path.stat().st_mtime > libc_mtime:
                    newer.append(path.relative_to(REPO).as_posix())
            except OSError:
                # A file that vanished mid-scan is not evidence either way.
                continue
    return newer


def _report_sysroot_staleness(newer: list[str]) -> None:
    """Print the diagnosis for a stale `libc.a`. Caller decides the exit code."""
    print(f"[ctest] ERROR: {LIBC.relative_to(REPO).as_posix()} is OLDER than {len(newer)} tracked source(s).")
    for name in newer[:8]:
        print(f"[ctest]          newer: {name}")
    if len(newer) > 8:
        print(f"[ctest]          ... and {len(newer) - 8} more")
    print("[ctest]        libc.a is a gitignored build artifact, so git cannot tell you it is behind.")
    print("[ctest]        Every fixture below links it, so a per-fixture 'ok' only means the ELF")
    print("[ctest]        matches a libc that is not the one in the tree. Rebuild in this order:")
    print("[ctest]          powershell -File toolchain/build-sysroot.ps1")
    print(f"[ctest]          {_self_cmd()} build")
    print("[ctest]          wsl -d Ubuntu -- bash scripts/bash-spike/slatelink.sh   # if present")
    print("[ctest]          wsl -d Ubuntu -- bash scripts/create-ext4-rootfs.sh")


def cmd_stamp(only: str | None) -> int:
    rc = 0
    for fixture in fixtures():
        if only and only not in fixture.name:
            continue
        text, missing = compute(fixture)
        if missing:
            for m in missing:
                print(f"[ctest] ERROR {fixture.name}: {m}")
            rc = 1
            continue
        stamp_of(fixture).write_text(text, encoding="utf-8", newline="\n")
        print(f"[ctest] stamped {fixture.name}")
    return rc


def cmd_check(only: str | None) -> int:
    found = fixtures()
    if not found:
        print(f"[ctest] ERROR: no ctest fixtures found under {SERVICES}")
        return 1
    rc = 0
    # Before any per-fixture verdict, because a stale libc.a invalidates all of
    # them at once and the per-fixture lines would otherwise read as a clean
    # bill of health. Fatal, on the same grounds as a missing stamp above:
    # "cannot vouch for this" must not exit 0.
    stale = sysroot_staleness()
    if stale:
        _report_sysroot_staleness(stale)
        rc = 1
    for fixture in found:
        if only and only not in fixture.name:
            continue
        stamp = stamp_of(fixture)
        if not stamp.is_file():
            # Deliberately fatal. An unstamped fixture is one we cannot make
            # any statement about, and "cannot verify" must not read as "fine".
            print(f"[ctest] ERROR {fixture.name}: no {stamp.name} - cannot verify this fixture.")
            print("[ctest]        Build it so it gets stamped:")
            print(f"[ctest]          {_self_cmd()} build --only {fixture.name}")
            rc = 1
            continue
        actual, missing = compute(fixture)
        if missing:
            for m in missing:
                print(f"[ctest] ERROR {fixture.name}: {m}")
            rc = 1
            continue
        recorded = stamp.read_text(encoding="utf-8")
        was = _stamp_version(recorded)
        if was != STAMP_VERSION:
            # Distinguished from STALE on purpose. A version mismatch means the
            # hashes were computed under different rules, so "the ELF does not
            # match its inputs" would be an unfounded accusation - the two sides
            # were never comparable. Rebuilding is the sound repair either way,
            # because it is the only step that re-establishes the claim rather
            # than restating it.
            print(f"[ctest] ERROR {fixture.name}: stamp is format v{was}, this script writes v{STAMP_VERSION}.")
            print("[ctest]        The hashes are not comparable across formats; this is not evidence of drift.")
            print("[ctest]        Rebuild to re-establish the stamp under the current rules:")
            print(f"[ctest]          {_self_cmd()} build --only {fixture.name}")
            rc = 1
            continue
        if _body(recorded) != _body(actual):
            print(f"[ctest] ERROR {fixture.name}: STALE - the ELF does not match its inputs.")
            for line in _describe_drift(recorded, actual):
                print(f"[ctest]          {line}")
            print("[ctest]        Rebuild it (do NOT re-stamp - that only records the drift):")
            print(f"[ctest]          {_self_cmd()} build --only {fixture.name}")
            rc = 1
            continue
        print(f"[ctest] ok {fixture.name}")
    return rc


def cmd_build(only: str | None) -> int:
    if not LIBC.is_file():
        print(f"[ctest] ERROR: missing {LIBC}; run toolchain/build-sysroot.ps1 first")
        return 1
    # Warning, not fatal, unlike `check`. Building against a stale libc.a is a
    # real defect, but refusing to build would leave no way to rebuild the
    # fixtures at all on a tree whose sysroot is behind - and the rebuild is
    # step two of the very repair this message asks for. So say it loudly and
    # proceed; `check` is where it stops the line.
    stale = sysroot_staleness()
    if stale:
        _report_sysroot_staleness(stale)
        print("[ctest] WARNING: building anyway - the ELFs below will link the stale libc.a.")
    rc = 0
    for fixture in fixtures():
        if only and only not in fixture.name:
            continue
        print(f"[ctest] building {fixture.name} ...")
        result = subprocess.run(
            [sys.executable, str(fixture / "build.py")],
            capture_output=True,
            text=True,
            timeout=900,
        )
        if result.returncode != 0:
            print(f"[ctest] ERROR {fixture.name}: build.py exited {result.returncode}")
            print(result.stdout)
            print(result.stderr)
            rc = 1
            continue
        print(f"[ctest]   {result.stdout.strip().splitlines()[-1] if result.stdout.strip() else 'built'}")
    if rc:
        # Stamping after a partial build would record a mix of fresh and stale
        # binaries as if all were fresh, which is worse than not stamping.
        print("[ctest] not stamping: at least one build failed")
        return rc
    return cmd_stamp(only)


def _staged_elfs() -> list[Path]:
    """Every locally built ELF the rootfs stages, sorted, repo-relative order."""
    found: list[Path] = []
    for pattern in STAGED_GLOBS:
        found.extend(p for p in REPO.glob(pattern) if p.is_file())
    return sorted(set(found))


_IMAGE_HEADER = (
    "# rootfs.ext4 content manifest - generated by scripts/ctest-fixtures.py\n"
    "# The sha256 of every locally built ELF that was staged into the image.\n"
    "# `image-check` compares this against the tree, so a fixture rebuilt after\n"
    "# the image was packed is caught BEFORE a boot test reports PASS about the\n"
    "# previous binary. Regenerate by rebuilding the image:\n"
    "#   wsl -d Ubuntu -- bash scripts/create-ext4-rootfs.sh\n"
)


def cmd_image_stamp() -> int:
    if not ROOTFS.is_file():
        print(f"[ctest] ERROR: no {ROOTFS.name} to stamp ({ROOTFS})")
        return 1
    lines = [_IMAGE_HEADER, f"version {STAMP_VERSION}\n"]
    for elf in _staged_elfs():
        lines.append(f"staged {elf.relative_to(REPO).as_posix()} sha256 {sha256(elf)}\n")
    ROOTFS_MANIFEST.write_text("".join(lines), encoding="utf-8", newline="\n")
    print(f"[ctest] stamped {ROOTFS_MANIFEST.name} ({len(_staged_elfs())} staged ELFs)")
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

    actual = {p.relative_to(REPO).as_posix(): sha256(p) for p in _staged_elfs()}
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
        print(f"[ctest] ERROR: {ROOTFS.name} is STALE - it does not contain the ELFs in this tree.")
        for line in drift:
            print(f"[ctest]          {line}")
        print("[ctest]        A boot test against this image reports PASS about")
        print("[ctest]        binaries that are no longer the ones you built. Repack it:")
        print("[ctest]          wsl -d Ubuntu -- bash scripts/create-ext4-rootfs.sh")
        return 1
    print(f"[ctest] ok {ROOTFS.name} ({len(actual)} staged ELFs match the tree)")
    return 0


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("command", choices=("check", "build", "stamp", "image-stamp", "image-check"))
    parser.add_argument("--only", help="substring of a fixture directory name, e.g. jobctl")
    args = parser.parse_args()
    if args.command == "check":
        return cmd_check(args.only)
    if args.command == "build":
        return cmd_build(args.only)
    if args.command == "image-stamp":
        return cmd_image_stamp()
    if args.command == "image-check":
        return cmd_image_check()
    return cmd_stamp(args.only)


if __name__ == "__main__":
    sys.exit(main())
