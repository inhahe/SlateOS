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

WHAT THIS IS NOT: the boot test's cfg(unix) check.

They shared the name `cfg-unix` until 2026-09-12 and check different populations, which
caused a false inference: a `cfg-unix` pass in a pre-push `ran:` list was read as meaning
the boot test's check had been pre-run. It had not.

    this script        the crates that CONTAIN a `#[cfg(unix)]` block -- 62, derived by
                       scanning, see `crates_with_unix_code` -- and only their DEFAULT
                       targets. `cargo check --target <unix> -p c1 -p c2 ...`

    boot-test.sh       does not call this script for its main check. It runs `cargo`
                       directly with `--all-targets --exclude kernel`: the whole
                       workspace on a unix target, test targets included. So it also
                       catches ordinary clippy denials in test code that no
                       Windows-target build compiles, which is strictly more than a
                       cfg(unix) check.

A pass here therefore does not predict a pass there. The concrete case: a denial in the
test target of `apps/launcher`, a crate with no `#[cfg(unix)]` code at all, so it is not
in this script's crate list and never compiled here -- and `--all-targets` would not help,
because the crate is absent from the list rather than present with the wrong targets.

This is also the correction to a claim on record. Lane B asked for test-module coverage in
`requests/b-a-the-cfg-unix-gate-skips-every-test-module.md`; lane A answered "taken in
full, both call sites at once". True of boot-test.sh, false here: this script never gained
`--all-targets` and does not accept it.
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
    for name, src in candidate_crates():
        for f in src.rglob("*.rs"):
            if GATED.search(f.read_text(encoding="utf-8", errors="surrogateescape")):
                found.add(name)
                break
    return sorted(found)


def candidate_crates() -> list[tuple[str, pathlib.Path]]:
    """Every crate this gate *could* check: (name, src dir), workspace-wide.

    Exists to supply the DENOMINATOR. The gate checks the subset holding a unix-gated
    block, and for a long time it reported only that subset's size -- "62 crates with
    unix-gated code compile" -- which reads as a complete audit of the workspace and is
    not one. Printing "62 of N" makes the scope visible without anyone having to be
    bitten by it first.

    Suggested by lane B, who hit the same shape from the other side: their
    `getopt-ambiguity-check.py` printed "63 table(s) checked; 0 disagreement(s)" over a
    tree of 85 bins and never named the 22 it skipped, because the coverage note lived
    inside a branch only the push hook reached. Their phrasing is the one to keep -- *the
    run that reads as a complete audit was the one run that said nothing about its own
    gaps.*
"""
    out: list[tuple[str, pathlib.Path]] = []
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
        out.append((m.group(1), src))
    return out


def target_installed() -> bool:
    proc = subprocess.run(
        ["rustup", "target", "list", "--installed"],
        capture_output=True, text=True, timeout=120, check=False,
    )
    return TARGET in proc.stdout


def check(crates: list[str]) -> tuple[int, str]:
    """`cargo check` them all at once. Returns (exit code, output).

    `--all-targets` because without it this gate skipped the population it
    exists for. A `#[cfg(unix)]` block inside a `#[cfg(test)]` module is
    compiled by neither a default `cargo check` (which does not build test
    targets) nor a windows `cargo test` (which does not build the unix block),
    so it was checked by nothing at push time -- `boot-test.sh` ran cargo
    directly with `--all-targets` and caught it hours later, on lane A's
    schedule rather than on the author's.

    The self-test proves the flag is what makes the difference: the same
    fixture compiles clean without it and fails with it.
    """
    args = ["cargo", "check", "--all-targets", "--target", TARGET]
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
            # newline="" so the probe is LF on every platform. rustc accepts
            # either, but the gate grades the declaration, not the compiler.
            f.write_text(src, encoding="utf-8", newline="")
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

            # THE SAME QUESTION FOR A TEST MODULE, which is what `--all-targets`
            # buys. `#[cfg(test)]` code is compiled only under `--test`, so a
            # unix-gated error inside one is invisible to a plain compile --
            # exactly as it was invisible to this gate until 2026-09-12.
            # `rustc --test` is to `rustc` what `cargo check --all-targets` is
            # to `cargo check`.
            tsrc = (
                "pub fn always() -> i32 { 0 }\n"
                "#[cfg(test)]\n"
                "mod tests {\n"
                "    #[cfg(unix)]\n"
                "    #[test]\n"
                "    fn only_on_unix() { let _x: i32 = \"not an integer\"; }\n"
                "}\n"
            )
            tf = Path(tmp) / "probe_test.rs"
            tf.write_text(tsrc, encoding="utf-8", newline="")

            def rustc_test(target: str, as_test: bool) -> int:
                cmd = ["rustc", "--crate-type", "lib", "--target", target,
                       "--out-dir", tmp, str(tf)]
                if as_test:
                    cmd.insert(1, "--test")
                return subprocess.run(
                    cmd, capture_output=True, text=True, timeout=600, check=False,
                ).returncode

            if rustc_test(TARGET, as_test=False) != 0:
                failures.append(
                    "the premise of --all-targets is broken: a cfg(test) module "
                    "was compiled without --test, so the flag buys nothing"
                )
            if rustc_test(TARGET, as_test=True) == 0:
                failures.append(
                    "a unix-only compile error inside a cfg(test) module was NOT "
                    "caught with --test; --all-targets is not reaching test targets"
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
        # 3, not 0. `run_checker` maps 3 to the SKIPPED tally; 0 put this
        # gate in `ran:` beside the gates that really ran, so a host without
        # the target reported a clean cfg(unix) check having compiled nothing.
        # See the "Exit 3" section of scripts/run-checker.sh.
        return 3

    code, output = check(crates)
    if code != 0:
        print(f"check-cfg-unix: {len(crates)} crate(s) checked against {TARGET}; it failed:\n")
        print(output[-8000:])
        return 1
    # Says `--all-targets` out loud. The flag is the difference between
    # checking this population and skipping the half of it that lives in
    # `#[cfg(test)]` modules, and a summary that does not name it reads the
    # same either way -- which is how the gap went unnoticed.
    print(f"check-cfg-unix: OK ({len(crates)} of {len(candidate_crates())} workspace "
          f"crate(s) hold unix-gated code and compile for {TARGET} with "
          f"--all-targets; the other "
          f"{len(candidate_crates()) - len(crates)} are NOT checked here -- boot-test.sh covers the workspace)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
