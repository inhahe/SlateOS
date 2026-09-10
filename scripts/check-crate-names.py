#!/usr/bin/env python3
"""Refuse a crate whose directory name is a *different* crate's package name.

Why
---

The standing command in `CLAUDE.md`, and in every lane's loop, is

    cargo test -p <crate> --target x86_64-pc-windows-gnu

`-p` takes a **package** name. Everybody types the **directory** name, because
for 2944 of this workspace's 2955 crates they are the same string. For five of
them they are not, and the directory name belongs to a different crate -- so
the command succeeds, prints a green result, and tests something else.

Found on 2026-09-10 giving `userspace/login` its exec. That crate's package is
`login-cli`; `login` is `init/login`, an unrelated program. Every
`cargo test -p login` and the `cargo fmt -p login` went to `init/login`. It
surfaced only because the count did not move after three tests were added --
53 `#[test]` in the file, 46 collected -- and the collected names turned out
not to be in the file at all. Nothing else would have said a word: the wrong
crate compiled, its tests passed, and the exit code was 0.

**A wrong `-p` is silent in both directions that matter.** If the directory
name is not any package's name, cargo errors and you find out immediately.
The dangerous case is exactly the one this checks: the name resolves, to
somebody else.

What it does *not* do
---------------------

It does not forbid a package name that differs from its directory. That is
often deliberate -- `gui/toolkit` is `guitk`, `toolchain/stubs` is
`slateos-stubs` -- and harmless, because nothing else claims `toolkit` or
`stubs`, so a mistyped `-p toolkit` fails loudly.

Only the *collisions* matter, and only new ones fail: the five that already
exist are recorded in `KNOWN_COLLISIONS` below, because four of them are
another lane's to rename and a gate that refuses every lane's push over
pre-existing state is a gate that gets bypassed. The list may only shrink --
resolving one and leaving it listed is also a failure, so it cannot rot into
a list of things that used to be true.

Usage
-----

    python scripts/check-crate-names.py
    python scripts/check-crate-names.py --self-test
    python scripts/check-crate-names.py --list

Exit codes: 0 pass, 1 a new collision (or a stale baseline entry), 2 the
checker could not run.
"""

from __future__ import annotations

import argparse
import re
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent

NAME_RE = re.compile(r'^\s*name\s*=\s*"([^"]+)"', re.M)

# Collisions that existed when this gate was written. `dir path -> package`.
#
# Each of these is a crate you cannot reach with `cargo -p <its directory
# name>`: that name belongs to the crate in the second column of the comment,
# and cargo will happily act on that one instead.
#
# This list may only SHRINK. Resolving one without removing it here is a
# failure, so it cannot become a list of things that used to be true.
KNOWN_COLLISIONS: dict[str, str] = {
    # `-p backup` reaches the `backup` crate, not this one.
    "apps/backup": "backup-app",
    # `-p indexer` reaches the `indexer` crate.
    "apps/indexer": "indexer-app",
    # `-p sysinfo` reaches the `sysinfo` crate.
    "apps/sysinfo": "sysinfo-app",
    # `-p tmux` reaches the `tmux` crate.
    "apps/tmux": "tmux-app",
    # `-p login` reaches `init/login`. This is the one that cost a tick.
    "userspace/login": "login-cli",
}


def crates(root: Path) -> dict[str, str]:
    """Every crate under `root`, as `posix/relative/dir -> package name`.

    A `Cargo.toml` with no `src/` beside it is a workspace root, not a crate.
    """
    found: dict[str, str] = {}
    for tom in root.rglob("Cargo.toml"):
        parts = tom.parts
        if "target" in parts or ".git" in parts:
            continue
        if not (tom.parent / "src").is_dir():
            continue
        m = NAME_RE.search(tom.read_text(encoding="utf-8", errors="surrogateescape"))
        if not m:
            continue
        rel = tom.parent.relative_to(root).as_posix()
        found[rel] = m.group(1)
    return found


def collisions(found: dict[str, str]) -> dict[str, str]:
    """Crates whose directory name is a *different* crate's package name."""
    by_package = {pkg: d for d, pkg in found.items()}
    out: dict[str, str] = {}
    for d, pkg in found.items():
        dirname = d.rsplit("/", 1)[-1]
        if dirname == pkg:
            continue
        owner = by_package.get(dirname)
        if owner is not None and owner != d:
            out[d] = pkg
    return out


def report(found: dict[str, str], live: dict[str, str]) -> int:
    by_package = {pkg: d for d, pkg in found.items()}
    new = {d: pkg for d, pkg in live.items() if d not in KNOWN_COLLISIONS}
    stale = sorted(set(KNOWN_COLLISIONS) - set(live))

    if not new and not stale:
        print(
            f"check-crate-names: OK ({len(found)} crates; "
            f"{len(live)} known name collisions, none new)"
        )
        return 0

    if new:
        print("check-crate-names: a crate's directory name belongs to ANOTHER crate.\n")
        for d in sorted(new):
            dirname = d.rsplit("/", 1)[-1]
            print(f"  {d}")
            print(f"      package name:  {new[d]}")
            print(f"      `-p {dirname}` reaches: {by_package.get(dirname)}")
        print(
            "\n  So `cargo test -p <directory name>` on this crate silently tests a\n"
            "  different one, passes, and exits 0. Either rename the package to match\n"
            "  its directory, rename the directory, or -- if the collision is\n"
            "  deliberate -- add it to KNOWN_COLLISIONS in this file with a comment\n"
            "  saying which crate the directory name reaches instead."
        )

    if stale:
        print(
            "\ncheck-crate-names: KNOWN_COLLISIONS lists crates that no longer collide:"
        )
        for d in stale:
            print(f"  {d}")
        print(
            "  Remove them. The list may only shrink, and an entry that is no longer\n"
            "  true makes the rest of it less believable."
        )
    return 1


def self_test() -> int:
    """Fixtures over the pure functions, in both directions."""
    failures: list[str] = []

    # The shape that must be caught: `a/login` is package `login-cli`, and
    # `login` is somebody else's package name.
    caught = collisions(
        {"userspace/login": "login-cli", "init/login": "login", "posix": "posix"}
    )
    if caught != {"userspace/login": "login-cli"}:
        failures.append(f"the collision was not detected: got {caught}")

    # A package name that differs from its directory but collides with nothing
    # is fine -- `-p toolkit` fails loudly, which needs no gate.
    quiet = collisions({"gui/toolkit": "guitk", "posix": "posix"})
    if quiet:
        failures.append(f"a harmless rename was reported as a collision: {quiet}")

    # A crate whose directory matches its own package is never a collision,
    # even though its name is obviously "taken" -- by itself.
    same = collisions({"posix": "posix", "userspace/su": "su"})
    if same:
        failures.append(f"a crate collided with itself: {same}")

    # Two crates in different directories with the same directory *name* but
    # matching packages cannot happen (cargo forbids duplicate packages), so
    # the only asymmetric case is the one above.

    live = collisions(crates(REPO))
    for d in KNOWN_COLLISIONS:
        if d not in live:
            failures.append(
                f"KNOWN_COLLISIONS names `{d}`, which the derivation does not find"
            )

    for f in failures:
        print(f"check-crate-names --self-test: FAIL: {f}")
    if failures:
        return 1
    print(
        f"check-crate-names --self-test: OK "
        f"({len(live)} live collisions, {len(KNOWN_COLLISIONS)} baselined)"
    )
    return 0


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--self-test", "--selftest", action="store_true", dest="selftest")
    ap.add_argument("--list", action="store_true")
    args = ap.parse_args()

    if args.selftest:
        return self_test()

    found = crates(REPO)
    if not found:
        print("check-crate-names: found no crates at all, which cannot be right")
        return 2

    if args.list:
        for d in sorted(found):
            dirname = d.rsplit("/", 1)[-1]
            mark = "" if dirname == found[d] else f"   (dir '{dirname}')"
            print(f"{found[d]}{mark}")
        return 0

    return report(found, collisions(found))


if __name__ == "__main__":
    sys.exit(main())
