#!/usr/bin/env python3
"""Report crates whose package name differs from their directory name.

Why this exists
===============

`cargo` selects a crate by **package name**, not by path. Four crates under
`apps/` carry an `-app` suffix, because a crate of the bare name already exists
**in this repository**, under `userspace/`:

    apps/sysinfo   -> package `sysinfo-app`   (userspace/sysinfo owns `sysinfo`)
    apps/tmux      -> package `tmux-app`      (userspace/tmux owns `tmux`)
    apps/backup    -> package `backup-app`    (userspace/backup owns `backup`)
    apps/indexer   -> package `indexer-app`   (userspace/indexer owns `indexer`)

Those are not duplicates: each `userspace/` crate is the command-line tool and
each `apps/` crate is the graphical one for the same subject. They are also in
different lanes -- `userspace/**` is lane B, `apps/**` is lane C -- so a
mis-aimed `-p` crosses a lane boundary as well as a crate one.

So `cargo test -p sysinfo` does **not** test `apps/sysinfo`. On 2026-09-04 it
tested `userspace/sysinfo`, which has no tests, and reported

    running 0 tests
    test result: ok. 0 passed; 0 failed

and `cargo clippy -p sysinfo -- -D warnings` reported no findings — both about
a different crate in a different lane. The intended package had 62 tests. The
zero is the only reason anyone noticed; had `userspace/sysinfo` happened to
carry a plausible-looking test count, the mistake would have passed unremarked.

That is the failure this guards: **a check that runs, passes, and is about the
wrong thing.** It is the same shape as `apps/installer`'s build script, which
made `cargo clippy -p installer -- -D warnings` stop before it analysed the
crate at all — the crate looked clean while carrying 142 findings.

What it does
============

Walks every `Cargo.toml` under the directories it is given (default `apps` and
`userspace`),
compares `package.name` to the directory name, and prints the mismatches. It
**fails only on an unrecorded one**: the four above are expected and listed in
`KNOWN`, each with the reason it cannot simply be renamed. A new mismatch means
either a new collision worth recording here, or a typo worth fixing.

The point of listing rather than forbidding is that the renames are correct. The
cost is not the name; it is that `-p <directory>` silently addresses a different
crate, and nothing said so anywhere until this file.

Usage
=====

    python scripts/check-crate-names.py [--self-test] [DIR ...]

Exit codes: 0 nothing unexpected, 1 an unrecorded mismatch.
"""

from __future__ import annotations

import re
import sys
from pathlib import Path

# Directory name -> (package name, why it cannot just be the directory name).
KNOWN: dict[str, tuple[str, str]] = {
    "sysinfo": ("sysinfo-app", "userspace/sysinfo owns `sysinfo`"),
    "tmux": ("tmux-app", "userspace/tmux owns `tmux`"),
    "backup": ("backup-app", "userspace/backup owns `backup`"),
    "indexer": ("indexer-app", "userspace/indexer owns `indexer`"),
    # Found by this gate on its first run over `userspace/` -- the pattern is
    # not confined to `apps/`.
    "login": ("login-cli", "init/login owns `login`"),
}

_NAME_RE = re.compile(r'^\s*name\s*=\s*"([^"]+)"', re.MULTILINE)


def package_name(manifest: Path) -> str | None:
    """The `[package] name` of a manifest, or `None` if it has no package."""
    try:
        text = manifest.read_text(encoding="utf-8")
    except OSError:
        return None
    # Only the `[package]` section: a `[dependencies]` entry can also carry a
    # `name` key, and matching the first `name =` in the file would find it.
    start = text.find("[package]")
    if start < 0:
        return None
    end = text.find("\n[", start + len("[package]"))
    section = text[start:] if end < 0 else text[start:end]
    m = _NAME_RE.search(section)
    return m.group(1) if m else None


def scan(roots: list[Path]) -> list[tuple[str, str]]:
    """Every (directory, package) pair whose names differ, sorted."""
    found: list[tuple[str, str]] = []
    for root in roots:
        if not root.is_dir():
            continue
        for manifest in sorted(root.glob("*/Cargo.toml")):
            pkg = package_name(manifest)
            if pkg is None:
                continue
            directory = manifest.parent.name
            if pkg != directory:
                found.append((directory, pkg))
    return found


def report(found: list[tuple[str, str]]) -> int:
    """Print the findings and return the exit code."""
    unexpected = [(d, p) for d, p in found if KNOWN.get(d, (None, None))[0] != p]
    for directory, pkg in found:
        if (directory, pkg) in unexpected:
            continue
        why = KNOWN[directory][1]
        print(f"  ok      {directory:<12} -> {pkg:<14} ({why})")
    for directory, pkg in unexpected:
        expected = KNOWN.get(directory)
        if expected is None:
            print(
                f"  UNKNOWN {directory:<12} -> {pkg:<14} "
                f"(`cargo ... -p {directory}` will not address this crate)"
            )
        else:
            print(
                f"  CHANGED {directory:<12} -> {pkg:<14} "
                f"(recorded as `{expected[0]}`)"
            )
    if unexpected:
        print()
        print(
            f"{len(unexpected)} crate(s) whose package name is not what this file "
            "records."
        )
        print(
            "Either add it to KNOWN with the reason it cannot be renamed, or "
            "rename it to match its directory."
        )
        return 1
    print(f"{len(found)} recorded mismatch(es); no unrecorded ones.")
    return 0


def _self_test() -> int:
    """Check the scanner against manifests written for the purpose.

    A scanner that has stopped scanning reports zero findings in exactly the
    way a clean tree does, which is the failure this guards against elsewhere;
    it would be a poor joke to ship one here without a test.
    """
    import tempfile

    cases = 0
    failures = 0
    with tempfile.TemporaryDirectory() as tmp:
        root = Path(tmp) / "apps"
        def crate(directory: str, body: str) -> None:
            d = root / directory
            d.mkdir(parents=True)
            (d / "Cargo.toml").write_text(body, encoding="utf-8")

        crate("matching", '[package]\nname = "matching"\nversion = "0.1.0"\n')
        crate("renamed", '[package]\nname = "renamed-app"\nversion = "0.1.0"\n')
        # A `name` key outside `[package]` must not be mistaken for the
        # package's own: this is what a dependency table looks like.
        crate(
            "decoy",
            '[package]\nname = "decoy"\n\n[dependencies.foo]\nname = "not-the-package"\n',
        )
        # A manifest with no `[package]` at all (a virtual manifest) is skipped
        # rather than reported.
        crate("virtual", '[workspace]\nmembers = []\n')

        found = dict(scan([root]))
        cases += 1
        if "matching" in found:
            print("FAIL: a matching name was reported as a mismatch")
            failures += 1
        cases += 1
        if found.get("renamed") != "renamed-app":
            print(f"FAIL: renamed crate not detected, got {found.get('renamed')!r}")
            failures += 1
        cases += 1
        if "decoy" in found:
            print("FAIL: a `name` in a dependency table was read as the package name")
            failures += 1
        cases += 1
        if "virtual" in found:
            print("FAIL: a manifest with no [package] was reported")
            failures += 1

    # And that an unrecorded mismatch is actually refused. `report` prints, and
    # its output here would read as real findings, so it is captured.
    import contextlib
    import io

    def quiet_report(pairs: list[tuple[str, str]]) -> int:
        with contextlib.redirect_stdout(io.StringIO()):
            return report(pairs)

    cases += 1
    if quiet_report([("brand-new", "brand-new-app")]) == 0:
        print("FAIL: an unrecorded mismatch was accepted")
        failures += 1
    cases += 1
    if quiet_report([("sysinfo", KNOWN["sysinfo"][0])]) != 0:
        print("FAIL: a recorded mismatch was refused")
        failures += 1
    cases += 1
    # A directory that is recorded but now carries a *different* package name
    # is a change worth refusing, not a silent pass.
    if quiet_report([("sysinfo", "sysinfo-gui")]) == 0:
        print("FAIL: a changed package name was accepted")
        failures += 1

    print(f"selftest: {cases - failures}/{cases} cases pass")
    return 1 if failures else 0


def main(argv: list[str]) -> int:
    args = [a for a in argv[1:] if not a.startswith("--")]
    if "--self-test" in argv[1:] or "--selftest" in argv[1:]:
        return _self_test()
    roots = [Path(a) for a in args] or [Path("apps"), Path("userspace")]
    return report(scan(roots))


if __name__ == "__main__":
    sys.exit(main(sys.argv))
