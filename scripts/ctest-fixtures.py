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

STAMP_VERSION = 1
_HEADER = (
    "# ctest fixture input stamp - generated by scripts/ctest-fixtures.py\n"
    "# Records the inputs that produced this ELF, so a source change committed\n"
    "# without its rebuilt binary is detectable by content. Timestamps cannot\n"
    "# do this: a fresh checkout gives every file the same mtime.\n"
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


def fixtures() -> list[Path]:
    """Every `services/ctest-*/` holding a `build.py`, sorted for stable output.

    Globbed rather than listed so a new fixture is covered the day it lands.
    """
    return sorted(d for d in SERVICES.glob("ctest-*") if (d / "build.py").is_file())


def elf_of(fixture: Path) -> Path:
    return fixture / f"{fixture.name}.elf"


def stamp_of(fixture: Path) -> Path:
    return fixture / f"{fixture.name}.stamp"


def _inputs(fixture: Path) -> list[tuple[str, Path]]:
    """The files whose content determines the ELF, in stamp order.

    `build.py` stands in for the compile/link flags, which live nowhere else.
    """
    return [
        ("build.py", fixture / "build.py"),
        ("main.c", fixture / "main.c"),
        ("toolchain/sysroot/lib/libc.a", LIBC),
    ]


def compute(fixture: Path) -> tuple[str, list[str]]:
    """Return (stamp text, missing-file descriptions) for `fixture`.

    Missing files are reported rather than raising, so `check` can say *which*
    piece is absent instead of dying on the first one.
    """
    missing: list[str] = []
    lines = [_HEADER, f"version {STAMP_VERSION}\n"]
    for label, path in _inputs(fixture):
        if not path.is_file():
            missing.append(f"missing input {label} ({path})")
            continue
        lines.append(f"input  {label} sha256 {sha256(path)}\n")
    elf = elf_of(fixture)
    if not elf.is_file():
        missing.append(f"missing output {elf.name} ({elf})")
    else:
        lines.append(f"output {elf.name} sha256 {sha256(elf)} size {elf.stat().st_size}\n")
    return "".join(lines), missing


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
