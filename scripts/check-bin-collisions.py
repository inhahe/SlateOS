#!/usr/bin/env python3
"""Refuse two packages that build a binary of the SAME NAME.

Why
---

Every crate in the workspace links into one shared directory,
`target/<triple>/<profile>/`. Two packages whose bin targets have the same
name therefore write the *same file*, and the one that wins is whichever
cargo happened to link last. Cargo says so itself and keeps going:

    warning: output filename collision at target/.../debug/logger.exe
      = note: the bin target `logger` in package `logger` has the same output
              filename as the bin target `logger` in package `coreutils`
      = note: this may become a hard error in the future
      = help: consider changing their names to be unique

A warning in a build that emits thousands of lines is not a gate. This is.

MEASURED, NOT ARGUED (2026-09-16). `target/x86_64-pc-windows-gnu/debug/
logger.exe` was probed twice, half an hour apart, with no source change
between them:

    before a rebuild:  logger -i  ->  logger: invalid option -- 'i'
    after  a rebuild:  logger -i  ->  (accepted)

Two different programs, one path. `userspace/logger` is a syslog client with
thirteen options (`-p -t -i -f -s -u -n -P --json --size --pid`);
`userspace/coreutils/src/bin/logger.rs` is a different program with two
(`-t -p`). `scripts/create-ext4-rootfs.sh` builds `-p coreutils ... -p logger`
and then stages one file called `logger`, so **which `logger` SlateOS ships is
decided by link order**. The same is true of `kill`, where the two programs do
not merely differ in options: `userspace/kill` sends IPC messages, which is
what `design.txt` requires, and the coreutils applet implements the POSIX
signal surface. Shipping either is a position; shipping whichever linked last
is not.

The cost is not only the shipped file. Both copies were *maintained* --
`e12942c8d logger: carry the message as bytes, from argv and from stdin`
(coreutils) and `60468ac46 logger: read argv as bytes, and refuse a message
rather than corrupt it` (standalone) are the same fix, made twice, months
apart, each time to whichever file the author had open. Work spent on the
loser is invisible: it compiles, its tests pass, and it is not what runs.

Why this is a separate gate from `check-crate-names.py`
------------------------------------------------------

That one refuses *a crate whose directory name is a different crate's package
name*, because `cargo test -p <name>` then silently tests something else. It
is a good gate and this is not a duplicate of it: **it answers a question
about what `-p` reaches, and this one answers a question about what a build
writes.** The two are independent -- `kill` and `logger` have distinct
directories AND distinct package names, and collide only in the filename their
bin targets produce, which that gate never looks at.

The distinction is not hypothetical, and the evidence is in that gate's own
docstring. Resolving the `login` package-name collision, it noticed:

    Neither crate declared a `[[bin]]`, so the binary named `login` was the
    Display Manager's, while the console `login(1)` built as `login-cli` --
    and `userspace/getty` execs `/bin/login`. Whoever first populated a
    rootfs would have installed the display manager where getty looks for
    the console login program.

That is exactly this bug, seen in 2026-09-10, fixed *for that one pair*, and
never turned into a check -- so `kill` and `logger` went on colliding for six
more days. This file is the generalisation that should have been written then.

Method, and why it is not a hand-parse
--------------------------------------

The target list comes from `cargo metadata --no-deps`, which is cargo's own
answer, and is filtered to workspace members and `bin` targets. It takes about
0.7s and builds nothing.

Reading `Cargo.toml` directly would be faster and would be *a slightly
different question*: a bin target's name is the package name for `src/main.rs`,
the file stem for `src/bin/*.rs`, the directory name for `src/bin/*/main.rs`,
or an explicit `[[bin]] name`, and `autobins` can switch the implicit ones off.
Hand-rolling that is how a checker comes to report on a set that is nearly the
real one. A gate that exists because a near-miss question was asked should not
ask a near-miss question.

Usage
-----

    python scripts/check-bin-collisions.py
    python scripts/check-bin-collisions.py --list
    python scripts/check-bin-collisions.py --self-test

Exit codes: 0 pass, 1 a new collision (or a stale baseline entry), 2 the
checker could not run.
"""

from __future__ import annotations

import argparse
import collections
import contextlib
import io
import json
import subprocess
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent

# Collisions that existed when this gate was written, `bin name -> packages`.
#
# Baselined rather than fixed on the spot for the reason `check-crate-names.py`
# baselines its own: resolving one of these means DELETING a program, and which
# program is the right one to delete is a decision with evidence on both sides
# (see `known-issues.md` TD-B-TWO-PACKAGES-BUILD-A-BINARY-CALLED-LOGGER and
# TD-B-TWO-PACKAGES-BUILD-A-BINARY-CALLED-KILL). A gate that refuses every
# push until an unrelated design question is settled is a gate that gets
# commented out.
#
# THE LIST MAY ONLY SHRINK. An entry that no longer collides is a failure too,
# so it cannot rot into a record of things that used to be true.
KNOWN_COLLISIONS: dict[str, tuple[str, ...]] = {
    "kill": ("coreutils", "kill"),
    "logger": ("coreutils", "logger"),
}


def load_metadata(root: Path) -> dict:
    """Cargo's own view of the workspace. Raises `RuntimeError` if unavailable."""
    try:
        proc = subprocess.run(
            ["cargo", "metadata", "--no-deps", "--format-version", "1"],
            cwd=root,
            capture_output=True,
            text=True,
            check=False,
        )
    except (OSError, ValueError) as exc:  # cargo missing, or not executable
        raise RuntimeError(f"could not run cargo metadata: {exc}") from exc
    if proc.returncode != 0:
        tail = (proc.stderr or "").strip().splitlines()
        hint = tail[-1] if tail else "no stderr"
        raise RuntimeError(f"cargo metadata exited {proc.returncode}: {hint}")
    try:
        return json.loads(proc.stdout)
    except json.JSONDecodeError as exc:
        raise RuntimeError(f"cargo metadata emitted invalid JSON: {exc}") from exc


def bin_targets(meta: dict) -> dict[str, set[str]]:
    """Map every workspace bin target NAME to the packages that build it.

    The name is what lands in `target/<triple>/<profile>/`, so two packages
    sharing one key is precisely the collision -- there is no separate
    filename to compute, and adding `.exe` would only make the key
    host-dependent for no gain.
    """
    members = set(meta.get("workspace_members", ()))
    out: dict[str, set[str]] = collections.defaultdict(set)
    for pkg in meta.get("packages", ()):
        # Path dependencies outside the workspace appear here too; they build
        # into their own target dir and cannot collide with ours.
        if pkg.get("id") not in members:
            continue
        for tgt in pkg.get("targets", ()):
            if "bin" in tgt.get("kind", ()):
                out[tgt["name"]].add(pkg["name"])
    return dict(out)


def collisions(targets: dict[str, set[str]]) -> dict[str, tuple[str, ...]]:
    """The bin names built by more than one package."""
    return {
        name: tuple(sorted(pkgs)) for name, pkgs in targets.items() if len(pkgs) > 1
    }


def report(live: dict[str, tuple[str, ...]], known: dict[str, tuple[str, ...]]) -> int:
    """Print the verdict; return the exit code."""
    new = {n: p for n, p in live.items() if n not in known}
    stale = sorted(n for n in known if n not in live)
    # A baselined pair that now collides with a DIFFERENT set of packages is
    # not the recorded collision; treat it as new rather than silently
    # accepting it under the old entry.
    changed = {
        n: p for n, p in live.items() if n in known and set(p) != set(known[n])
    }

    for name, pkgs in sorted(new.items()):
        print(f"NEW COLLISION: bin `{name}` is built by {', '.join(pkgs)}")
        print(f"  both write target/<triple>/<profile>/{name}; the last one linked wins")
    for name, pkgs in sorted(changed.items()):
        was = ", ".join(known[name])
        print(f"CHANGED: bin `{name}` is now built by {', '.join(pkgs)} (baselined as {was})")
    for name in stale:
        print(f"STALE BASELINE: bin `{name}` no longer collides -- remove it from")
        print("  KNOWN_COLLISIONS in scripts/check-bin-collisions.py")

    if new or changed or stale:
        return 1

    if known:
        # Printed loudly every run, with what each one costs, because these are
        # DEBT and not decided differences. Silence here would make the
        # baseline a place where a shipped-binary ambiguity goes to be forgotten.
        print(f"{len(known)} known collision(s), still unresolved:")
        for name, pkgs in sorted(known.items()):
            print(f"  {name}: {', '.join(pkgs)} -- /bin/{name} is whichever linked last")
    print("no new binary-name collisions")
    return 0


def self_test() -> int:
    """Prove this gate can FAIL, and prove it can PASS.

    Both arms are required. A checker shown only to reject is a checker whose
    accepting path has never been observed -- and the accepting path is the one
    that runs on every clean push, so it is the one whose silence means
    nothing. This gate's whole subject is a program that looked fine because
    nobody had compared it to anything, so it does not get to make that mistake
    about itself.
    """
    failures = []

    def check(label: str, got, want) -> None:
        if got != want:
            failures.append(f"{label}: got {got!r}, want {want!r}")

    def verdict(live, known) -> int:
        """`report` for its exit code only, with its printing swallowed.

        The self-test drives `report` through failing cases on purpose, and
        letting those print would interleave real-looking `NEW COLLISION`
        lines with the self-test's own result -- so a reader scanning the
        output could not tell a rehearsal from a finding.
        """
        with contextlib.redirect_stdout(io.StringIO()):
            return report(live, known)

    # A minimal cargo-metadata shape. Two packages, one shared bin name.
    dirty = {
        "workspace_members": ["a 0.1.0 (path+file:///a)", "b 0.1.0 (path+file:///b)"],
        "packages": [
            {
                "id": "a 0.1.0 (path+file:///a)",
                "name": "alpha",
                "targets": [{"name": "dup", "kind": ["bin"]}],
            },
            {
                "id": "b 0.1.0 (path+file:///b)",
                "name": "beta",
                "targets": [
                    {"name": "dup", "kind": ["bin"]},
                    {"name": "solo", "kind": ["bin"]},
                ],
            },
        ],
    }
    check("detects a collision", collisions(bin_targets(dirty)), {"dup": ("alpha", "beta")})
    check("reports a new collision as failure", verdict({"dup": ("alpha", "beta")}, {}), 1)

    # The accepting arm: the same shape with the duplicate renamed.
    clean = json.loads(json.dumps(dirty))
    clean["packages"][1]["targets"][0]["name"] = "notdup"
    check("passes a clean workspace", collisions(bin_targets(clean)), {})
    check("reports clean as success", verdict({}, {}), 0)

    # A baselined collision is tolerated, and only that one.
    check(
        "tolerates a baselined pair",
        verdict({"dup": ("alpha", "beta")}, {"dup": ("alpha", "beta")}),
        0,
    )
    check(
        "refuses a baselined pair whose packages changed",
        verdict({"dup": ("alpha", "gamma")}, {"dup": ("alpha", "beta")}),
        1,
    )
    check(
        "refuses a stale baseline entry",
        verdict({}, {"dup": ("alpha", "beta")}),
        1,
    )

    # Non-bin targets must not count: a lib and a bin may share a name, and
    # nearly every crate here does exactly that.
    libs = {
        "workspace_members": ["a 0.1.0 (path+file:///a)", "b 0.1.0 (path+file:///b)"],
        "packages": [
            {
                "id": "a 0.1.0 (path+file:///a)",
                "name": "alpha",
                "targets": [{"name": "same", "kind": ["lib"]}],
            },
            {
                "id": "b 0.1.0 (path+file:///b)",
                "name": "beta",
                "targets": [{"name": "same", "kind": ["bin"]}],
            },
        ],
    }
    check("ignores non-bin targets", collisions(bin_targets(libs)), {})

    # A package outside `workspace_members` builds elsewhere and cannot collide.
    outside = json.loads(json.dumps(dirty))
    outside["workspace_members"] = ["a 0.1.0 (path+file:///a)"]
    check("ignores non-members", collisions(bin_targets(outside)), {})

    if failures:
        for line in failures:
            print(f"SELF-TEST FAILED: {line}")
        return 1
    print("self-test: 8/8 cases pass")
    return 0


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument(
        "--self-test",
        "--selftest",
        dest="self_test",
        action="store_true",
        help="prove the checker can both fail and pass, then exit",
    )
    ap.add_argument(
        "--list",
        action="store_true",
        help="print every workspace bin target and the package that builds it",
    )
    args = ap.parse_args()

    if args.self_test:
        return self_test()

    try:
        meta = load_metadata(REPO)
    except RuntimeError as exc:
        # Exit 2, never 0: a checker that could not run must not be
        # indistinguishable from one that found nothing.
        print(f"cannot check: {exc}", file=sys.stderr)
        return 2

    targets = bin_targets(meta)
    if args.list:
        for name, pkgs in sorted(targets.items()):
            print(f"{name}\t{', '.join(sorted(pkgs))}")
        print(f"{len(targets)} bin target name(s) across the workspace")
        return 0

    return report(collisions(targets), KNOWN_COLLISIONS)


if __name__ == "__main__":
    sys.exit(main())
