#!/usr/bin/env python3
"""Run everything `boot-test.sh` checks before it builds, without building anything.

Why this exists
---------------
`boot-test.sh` runs a long gate phase before it compiles anything: the whole
`scripts/check-*.py` suite, the unwrap/expect scan, and `cargo clippy -p kernel`
under a deny-level `clippy::all`.  Any one of them refuses the build.

The failure this removes is a specific and repeatable one.  A pre-commit routine
of "cargo check, then the check-*.py suite" passes clean, the boot test starts,
and ~340 seconds later it dies in the clippy gate having never reached QEMU --
because `cargo check` does not run clippy, and clippy is the one gate the .py
suite does not cover.  The lint is then a one-line fix, and the whole boot test
has to be started again from the top.  That happened on 2026-08-26 for a
`hits >= 1 + k.hits` that clippy wanted spelled `hits > k.hits`.

Running this first costs the same ~6 minutes the gate phase would have cost
inside the boot test, and buys back the ~13-minute run that would have been
thrown away.  It is not a substitute for the boot test -- it never builds the
kernel or starts QEMU, so it cannot tell you the kernel still boots.  It only
tells you the boot test will get that far.

Do NOT run this while a boot test is running
--------------------------------------------
`cargo clippy -p kernel` drives the same `target/` directory the boot test
builds into, so running the two at once produces confusing rebuilds and can
invalidate the other's cache mid-run.  This refuses to start if the
cross-worktree boot lock is held, but that lock is taken for the QEMU phase --
it does not cover the gate and build phases, so the check is a courtesy, not a
guarantee.  If you started a boot test, wait for it.

Order, and why it is not "stop at the first failure"
----------------------------------------------------
The cheap gates -- rustfmt, the `check-*.py` suite, the unwrap scan -- all run
even when an earlier one fails, and every failure is printed.  Fixing four
things in one pass beats four rounds of "run, fix one, run again" when the
whole set costs under a minute.

Clippy is the exception and runs last, alone, and only if everything above it
was clean.  It is the ~113-second pole, and there is nothing to learn from
linting a tree that a formatter or a checker has already condemned.  So a run
with a broken checker exits in seconds rather than two minutes -- which is the
right trade in the case that actually happens, where the cheap gate caught the
same edit clippy would have.

Usage
-----
    python scripts/pre-boot.py            # everything
    python scripts/pre-boot.py --no-fmt   # skip the rustfmt pass
    python scripts/pre-boot.py --quick    # skip clippy (the ~113 second pole)

Exit codes: 0 all clear; 1 a gate failed (its output is printed); 2 could not
run (bad cwd, missing cargo, or a boot test appears to be in progress).
"""

import argparse
import pathlib
import shutil
import subprocess
import sys
import time

ROOT = pathlib.Path(__file__).resolve().parent.parent
SCRIPTS = ROOT / "scripts"


def _run(cmd, cwd=ROOT):
    """Run a command, capturing both streams together. Returns (rc, output)."""
    proc = subprocess.run(
        cmd,
        cwd=str(cwd),
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        text=True,
        errors="replace",
    )
    return proc.returncode, proc.stdout


def _boot_lock_held():
    """True if the cross-worktree boot lock directory exists.

    Mirrors boot-test.sh: the lock lives in the shared git common dir so that
    all three lane worktrees contend for one lock, falling back to a per-tree
    path when that cannot be resolved.
    """
    rc, out = _run(["git", "rev-parse", "--git-common-dir"])
    if rc == 0 and out.strip():
        common = (ROOT / out.strip()).resolve()
        if (common / "slateos-boot-lock").is_dir():
            return True
    return (ROOT / "build" / ".boot-lock").is_dir()


def _report(label, rc, out, elapsed):
    """Print one result line; on failure print the captured output too."""
    if rc == 0:
        # ASCII only: this prints to a console whose code page is not UTF-8.
        print(f"ok    {label}  ({elapsed:.0f}s)")
        return True
    print(f"FAIL  {label}  ({elapsed:.0f}s)")
    print()
    for line in out.splitlines():
        print(f"    {line}")
    print()
    return False


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--no-fmt", action="store_true", help="skip the rustfmt pass")
    ap.add_argument(
        "--quick", action="store_true", help="skip clippy (the slowest gate)"
    )
    ap.add_argument(
        "--force",
        action="store_true",
        help="run even if the boot lock looks held",
    )
    args = ap.parse_args()

    cargo = shutil.which("cargo")
    if cargo is None:
        print("[pre-boot] cargo not found on PATH", file=sys.stderr)
        return 2
    if not (SCRIPTS / "boot-test.sh").is_file():
        print(f"[pre-boot] not a SlateOS worktree: {ROOT}", file=sys.stderr)
        return 2

    if _boot_lock_held() and not args.force:
        print(
            "[pre-boot] the boot lock is held -- a boot test is running, and\n"
            "           clippy would fight it for the shared target dir.\n"
            "           Wait for it, or pass --force if you know better.",
            file=sys.stderr,
        )
        return 2

    failures = 0

    # rustfmt first: it rewrites files, so anything after it sees the final text.
    if not args.no_fmt:
        t = time.monotonic()
        rc, out = _run([cargo, "fmt", "-p", "kernel"])
        if not _report("cargo fmt -p kernel", rc, out, time.monotonic() - t):
            failures += 1

    # The .py gate suite, in the same order boot-test.sh globs it.
    for path in sorted(SCRIPTS.glob("check-*.py")):
        t = time.monotonic()
        rc, out = _run([sys.executable, str(path)])
        if not _report(path.name, rc, out, time.monotonic() - t):
            failures += 1

    # The scan-*.py gates are not check-*.py, so the glob above misses them --
    # which is exactly the kind of gap this script exists to close.  Each also
    # takes a bespoke flag, which is the reason they cannot simply be renamed
    # into the glob: the glob runs a script bare, and both of these do something
    # else when run bare (a full report rather than a verdict).
    for name, flag, why in (
        ("scan-unwrap.py", "--summary", "unwrap/expect in kernel production paths"),
        ("scan-orphan-modules.py", "--check", "newly unreachable library modules"),
    ):
        scan = SCRIPTS / name
        if not scan.is_file():
            continue
        t = time.monotonic()
        rc, out = _run([sys.executable, str(scan), flag])
        if not _report(f"{name} {flag}  ({why})", rc, out, time.monotonic() - t):
            failures += 1

    # Clippy last: it is the long pole (~113s after a source edit, ~5s warm --
    # boot-test.sh's own measured table), and there is no point paying it while
    # something cheaper is already broken.
    if failures:
        print()
        print(f"[pre-boot] {failures} gate(s) failed -- fix them before the boot test.")
        return 1

    if args.quick:
        print("[pre-boot] --quick: skipped clippy.  The boot test still runs it.")
        return 0

    t = time.monotonic()
    # Matches boot-test.sh's invocation: debug profile (its default), short
    # format.  A different profile would lint a different cfg and could pass
    # here while failing there.
    #
    # `-p kernel` covers the root leaf crates too (`crc32`, `deflate`, `sha2`,
    # `ziparchive`, ...), even though every line of the log below is a
    # `kernel\...` path.  Cargo does not print warnings for non-primary
    # packages; it does surface their errors, which is what this gate checks.
    # Verified by planting a deny-level lint in `ziparchive` and watching this
    # command exit 101 naming it -- see boot-test.sh's check_kernel_clippy for
    # the full note.  Do not "fix" the apparent gap by adding `-p` flags: the
    # crates are dependencies of `kernel` in the same invocation and are built
    # in that role regardless of being named.
    rc, out = _run([cargo, "clippy", "-p", "kernel", "--message-format=short"])

    # Kept, not discarded.  The ~18,000 pedantic-level lines are the known
    # backlog someone will eventually work through, and a clippy run that
    # touched a source file costs ~113s -- throwing the text away means paying
    # that twice to read it.  Deliberately NOT boot-test.sh's
    # build/clippy-kernel.log: same command, but writing to that path would
    # leave a file whose provenance is ambiguous between the two runners.
    log = ROOT / "build" / "clippy-preboot.log"
    try:
        log.parent.mkdir(parents=True, exist_ok=True)
        log.write_text(out, encoding="utf-8", errors="replace")
    except OSError as exc:
        # Not fatal: the gate's verdict is in `rc`, and losing the transcript
        # costs a re-run, not a wrong answer.
        print(f"[pre-boot] could not write {log}: {exc}", file=sys.stderr)

    if rc != 0:
        # Only the deny-level errors, the same way boot-test.sh does -- the
        # pedantic backlog is warn-level by workspace policy and is NOT what
        # fails the gate.  Printing all of it would bury the two lines that did.
        errors = [
            ln for ln in out.splitlines() if " error: " in ln or ln.startswith("error")
        ]
        print(f"FAIL  cargo clippy -p kernel  ({time.monotonic() - t:.0f}s)")
        print()
        for line in errors[:40]:
            print(f"    {line}")
        if len(errors) > 40:
            print(f"    ... and {len(errors) - 40} more")
        print()
        print(f"    full output: {log}")
        print()
        print("[pre-boot] clippy::all is deny-level here.  Fix these rather than")
        print("           allowing them; if a lint genuinely does not apply, the")
        print("           allow goes at the narrowest scope with a comment saying why.")
        return 1
    warns = sum(1 for ln in out.splitlines() if " warning: " in ln)
    _report(
        f"cargo clippy -p kernel (0 errors, {warns} pedantic warnings)",
        0,
        out,
        time.monotonic() - t,
    )

    print()
    print("[pre-boot] all clear -- the boot test's gate phase will pass.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
