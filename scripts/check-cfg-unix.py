#!/usr/bin/env python3
"""Compile the `#[cfg(unix)]` code that `cargo test` never looks at.

Why
---

The standing test command in `CLAUDE.md`, and in every lane's loop, is

    cargo test -p <crate> --target x86_64-pc-windows-gnu

On that target `cfg(unix)` is **false**, so the compiler does not read a single
line inside a `#[cfg(unix)]` block. The real target, `x86_64-slateos`, *is*
unix. Those blocks are therefore exactly the code that runs on the machine and
is never compiled by the rig that tests it: it can be not merely wrong but
**uncompilable**, and every test still passes.

Found on 2026-09-10 while giving `su -` its login-shell `argv[0]`. The fix is
one `cmd.arg0(...)` under `#[cfg(unix)]`; it built and tested clean on the host
without the compiler having read it once. `userspace/sshd` has had the same
exposure since it was written -- its own comment shows the author knew the host
build could not see the call and reasoned about the `dead_code` allow instead,
which was the best available without a way to compile it.

There is a lot of this code. When this was written, **57 crates** contained
unix-gated blocks and `userspace/coreutils` alone had **533** of them.

How
---

`cargo check` every crate that contains such a block, against
`x86_64-unknown-linux-gnu` -- a target already installed here, and unix. One
invocation with every `-p`, because 57 separate ones pay the dependency graph
57 times: the whole set is about **7 seconds** warm.

The crate list is **derived**, never enumerated: a file that grows its first
`#[cfg(unix)]` block joins the gate by existing. That is the property that
makes this worth having rather than a list that rots -- the same argument as
`scripts/check-libc-abi.py`'s derived boundary set.

`x86_64-unknown-linux-gnu` and not `x86_64-slateos`, deliberately. The point is
to compile the unix branch, not to reproduce the target: slateos needs
`-Zbuild-std` and a built sysroot, which is minutes and a nightly, and would
make the gate too expensive to run on every push. Any unix target reads the
same lines.

Usage
-----

    python scripts/check-cfg-unix.py
    python scripts/check-cfg-unix.py --self-test
    python scripts/check-cfg-unix.py --list     # the derived crate list

Exit codes: 0 pass (or skipped for want of the target), 1 a compile failure,
2 the checker could not run.
"""

from __future__ import annotations

import argparse
import re
import subprocess
import sys
import tempfile
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
TARGET = "x86_64-unknown-linux-gnu"

# What counts as unix-gated. `not(windows)` is in here because it is the same
# thing said the other way round, and this tree uses both.
GATED = re.compile(
    r'#\[cfg\(unix\)\]'
    r'|#\[cfg_attr\(unix'
    r'|cfg\(target_family\s*=\s*"unix"\)'
    r'|#\[cfg\(not\(windows\)\)\]'
)
CRATE_NAME = re.compile(r'^\s*name\s*=\s*"([^"]+)"', re.M)


def crates_with_unix_code() -> list[str]:
    """Every crate in the workspace holding a unix-gated block.

    Derived by reading, so a crate joins by growing its first one. Skips
    `target/` and `.git/`, and skips a `Cargo.toml` with no `src/` -- a
    workspace root is not a crate to check.
    """
    found: set[str] = set()
    for tom in REPO.rglob("Cargo.toml"):
        parts = tom.parts
        if "target" in parts or ".git" in parts:
            continue
        src = tom.parent / "src"
        if not src.is_dir():
            continue
        m = CRATE_NAME.search(tom.read_text(encoding="utf-8", errors="surrogateescape"))
        if not m:
            continue
        for f in src.rglob("*.rs"):
            if GATED.search(f.read_text(encoding="utf-8", errors="surrogateescape")):
                found.add(m.group(1))
                break
    return sorted(found)


def target_installed() -> bool:
    proc = subprocess.run(
        ["rustup", "target", "list", "--installed"],
        capture_output=True, text=True, timeout=120, check=False,
    )
    return TARGET in proc.stdout


def check(crates: list[str]) -> tuple[int, str]:
    """`cargo check` them all at once. Returns (exit code, output)."""
    args = ["cargo", "check", "--target", TARGET]
    for c in crates:
        args += ["-p", c]
    proc = subprocess.run(
        args, cwd=REPO, capture_output=True, text=True, timeout=3600, check=False
    )
    return proc.returncode, proc.stdout + proc.stderr


def self_test() -> int:
    """Fixtures, in both directions.

    The second is the one that matters and is the whole premise of the gate: a
    `#[cfg(unix)]` block containing a plain compile error must **pass** a
    windows-target compile and **fail** a unix one. If that ever stops being
    true this gate is buying nothing.
    """
    failures: list[str] = []

    derived = crates_with_unix_code()
    for expect in ("su", "sshd", "coreutils"):
        if expect not in derived:
            failures.append(f"the derivation missed `{expect}`, which has unix-gated code")
    if len(derived) < 20:
        failures.append(f"the derivation found only {len(derived)} crates; it is probably broken")

    if not target_installed():
        print(f"check-cfg-unix --self-test: {TARGET} not installed; compile fixtures skipped")
    else:
        src = (
            "#[cfg(unix)]\n"
            "pub fn only_on_unix() -> i32 { let x: i32 = \"not an integer\"; x }\n"
            "pub fn always() -> i32 { 0 }\n"
        )
        with tempfile.TemporaryDirectory() as tmp:
            f = Path(tmp) / "probe.rs"
            f.write_text(src, encoding="utf-8")
            def rustc(target: str) -> int:
                return subprocess.run(
                    ["rustc", "--crate-type", "lib", "--target", target,
                     "--out-dir", tmp, str(f)],
                    capture_output=True, text=True, timeout=600, check=False,
                ).returncode
            if rustc("x86_64-pc-windows-gnu") != 0:
                failures.append(
                    "the premise is broken: a unix-only compile error was caught "
                    "by the windows target, so this gate is redundant"
                )
            if rustc(TARGET) == 0:
                failures.append(
                    "a unix-only compile error was NOT caught by the unix target; "
                    "this gate detects nothing"
                )

    for f in failures:
        print(f"check-cfg-unix --self-test: FAIL: {f}")
    if failures:
        return 1
    print(f"check-cfg-unix --self-test: OK ({len(derived)} crates derived)")
    return 0


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--self-test", "--selftest", action="store_true", dest="selftest")
    ap.add_argument("--list", action="store_true")
    args = ap.parse_args()

    if args.selftest:
        return self_test()

    crates = crates_with_unix_code()
    if args.list:
        for c in crates:
            print(c)
        return 0

    if not crates:
        print("check-cfg-unix: no unix-gated code found, which is itself suspicious")
        return 1

    if not target_installed():
        print(
            f"check-cfg-unix: SKIPPED -- {TARGET} is not installed.\n"
            f"  Install it with `rustup target add {TARGET}`. Until then every\n"
            "  `#[cfg(unix)]` block in this tree is compiled by nothing."
        )
        return 0

    code, output = check(crates)
    if code != 0:
        print(f"check-cfg-unix: {len(crates)} crate(s) checked against {TARGET}; it failed:\n")
        print(output[-8000:])
        return 1
    print(f"check-cfg-unix: OK ({len(crates)} crates with unix-gated code compile for {TARGET})")
    return 0


if __name__ == "__main__":
    sys.exit(main())
