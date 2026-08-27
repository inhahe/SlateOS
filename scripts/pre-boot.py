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

One gate here is NOT in boot-test.sh
------------------------------------
`cargo check --workspace --target x86_64-unknown-linux-gnu`, which compiles the
`#[cfg(unix)]` arms this Windows host otherwise never checks, runs here and only
here.  It is a superset rather than a mirror, so the promise above narrows: a
clean run still means the boot test's gate phase will pass, but a *failing* run
may be reporting something the boot test would not have minded.  The line that
says so tells you which.

It is deliberately not in `boot-test.sh`.  That script is the shared blocking
gate -- it also takes the cross-worktree lock that serialises QEMU -- and a
workspace-wide compile check there would let any lane's red tree stop any other
lane's boot test, the exact coupling `boot-test.sh` already refuses for clippy.
Here, a failure outside lane A's own files is reported loudly and does not
block, because lane A is forbidden from writing the fix.  The full argument is
above `_LANE_BY_PREFIX` below.

Usage
-----
    python scripts/pre-boot.py                  # everything
    python scripts/pre-boot.py --no-fmt         # skip the rustfmt pass
    python scripts/pre-boot.py --quick          # skip clippy (the ~113 second pole)
    python scripts/pre-boot.py --no-unix-check  # skip the cfg(unix) compile gate

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


# The unix-target check, and why its failures are triaged by lane
# --------------------------------------------------------------------------
# `#[cfg(unix)]` code is NEVER COMPILED on this Windows dev host: rustc discards
# the tokens rather than checking them, so such an arm can contain plain name
# and syntax errors while `cargo build`, `cargo clippy` and `cargo test` all
# come back green.  SlateOS is a unix -- `toolchain/x86_64-slateos.json` sets
# `"target-family": ["unix"]` -- so the arm that is never checked is the arm
# that ships.  Lane B lost `userspace/backup` to this for three months
# (2026-06-03 to 2026-08-26) and asked all lanes to add one command; lane C
# adopted it the same day.  See
# requests/b-a-a-windows-only-check-never-compiles-your-cfg-unix-arms.md and
# known-issues.md B-DEV-HOST-IS-WINDOWS-SO-CFG-UNIX-CODE-IS-NEVER-COMPILED.
#
# `x86_64-unknown-linux-gnu` rather than `x86_64-slateos`: the latter needs
# `-Zbuild-std` and is far slower, and for `cfg(unix)` coverage the two are
# equivalent.
#
# WHY THIS IS TRIAGED BY LANE INSTEAD OF SIMPLY FAILING.  The command is
# necessarily `--workspace` -- a `cfg(unix)` arm anywhere is the point -- but a
# workspace-wide gate hands every lane a veto over every other lane's ability
# to make progress.  `boot-test.sh` rejects exactly this coupling for clippy
# ("a workspace-wide clippy would let a red crate in lane B's or lane C's tree
# block lane A's boot test, which is the exact coupling the lane split exists
# to prevent").  The asymmetry is that lane A *cannot* fix another lane's crate
# -- writing outside its globs is forbidden -- so a hard failure would convert
# "another lane broke their tree" into "lane A cannot commit", with the only
# remedy being to file a request and wait.
#
# So: a failure in lane A's own files fails this gate, and a failure anywhere
# else is reported loudly, names the owning lane, and does not block.  That
# keeps the detection lane B asked for -- the breakage is found, and found on
# the most frequently run gate in the tree -- without giving it a veto it was
# never meant to carry.
#
# Note that lane A owns *no* `cfg(unix)` code today: zero occurrences in
# `kernel/**` and `bench/**` (checked 2026-08-26; all 515 in the tree are in
# lane B's `userspace/**` or lane C's `apps/**`, `gui/**`, `randrange/**`).
# That makes this gate almost entirely a service to the other two lanes right
# now, which is a reason to keep it non-blocking, not a reason to omit it: the
# kernel is bare-metal today and need not stay that way, and `bench/**` is
# ordinary std code that could grow a unix arm at any time.
UNIX_CHECK_TARGET = "x86_64-unknown-linux-gnu"

# Path prefix -> owning lane.  Mirrors scripts/which-lane.py, which mirrors the
# ownership table in roadmap.md; that table is the authority.  Anything not
# listed (the root leaf crates -- crc32, deflate, sha2, ziparchive, ...) is
# lane A's by default, which is the safe direction: it makes this gate stricter
# on us rather than looser.
_LANE_BY_PREFIX = (
    ("posix/", "B"),
    ("userspace/", "B"),
    ("services/", "B"),
    ("init/", "B"),
    ("gui/", "C"),
    ("apps/", "C"),
    ("pkg/", "C"),
    ("net/", "C"),
    ("netipc/", "C"),
    ("netproto/", "C"),
    ("netring/", "C"),
    ("randrange/", "C"),
)


def _lane_of(path: str) -> str:
    """Which lane owns `path`?

    Returns a lane letter, or `"-"` for code that belongs to no lane: a
    third-party dependency, or anything outside this worktree.  That case must
    be distinguished from lane A's, because "default to us" is only the safe
    direction among *our* crates.  A rustc error inside a registry dependency
    is not lane A's to fix any more than lane B's is -- cargo reports those with
    an absolute path into `~/.cargo/registry` or `~/.rustup`, which matches no
    lane prefix and would otherwise fall through to "A" and block a commit over
    a crate nobody here can edit.
    """
    p = path.replace("\\", "/").lstrip("./")

    # Not ours: an absolute path (rustc emits workspace-relative paths for
    # workspace members), or anything under a cargo/rustup cache.
    if (
        ".cargo/registry" in p
        or ".cargo/git" in p
        or ".rustup/" in p
        or p.startswith("/")
        or (len(p) > 2 and p[1] == ":")  # C:/... -- a Windows absolute path
    ):
        return "-"

    for prefix, lane in _LANE_BY_PREFIX:
        if p.startswith(prefix):
            return lane
    return "A"


def _unix_check_paths(out: str):
    """Source paths named by rustc `--> path:line:col` lines in `out`.

    Parsed from the diagnostic locations rather than from `could not compile
    \\`name\\`` because a path attributes to a lane directly, whereas a package
    name would need a `cargo metadata` round-trip to locate.
    """
    seen = []
    for line in out.splitlines():
        s = line.strip()
        if not s.startswith("--> "):
            continue
        path = s[4:].split(":", 1)[0].strip()
        if path and path not in seen:
            seen.append(path)
    return seen


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
    ap.add_argument(
        "--no-unix-check",
        action="store_true",
        help=f"skip the cargo check --workspace --target {UNIX_CHECK_TARGET} gate",
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

    # The one gate here that boot-test.sh does NOT run -- see the long note
    # above _LANE_BY_PREFIX.  Deliberately not added to boot-test.sh: that is
    # the shared blocking gate that also serialises QEMU across all three
    # worktrees, and a workspace-wide compile check there would let one lane's
    # red tree stop another lane's boot test.  Here a non-lane-A failure is
    # advisory, so the detection happens without the veto.
    if not args.no_unix_check:
        t = time.monotonic()
        rc, out = _run(
            [cargo, "check", "--workspace", "--target", UNIX_CHECK_TARGET]
        )
        elapsed = time.monotonic() - t
        label = f"cargo check --workspace --target {UNIX_CHECK_TARGET}  (cfg(unix) arms)"
        if rc == 0:
            print(f"ok    {label}  ({elapsed:.0f}s)")
        else:
            paths = _unix_check_paths(out)
            ours = [p for p in paths if _lane_of(p) == "A"]
            theirs = [p for p in paths if _lane_of(p) != "A"]

            if ours:
                # Lane A's own cfg(unix) code does not compile for a unix
                # target.  That is ours to fix and it blocks.
                failures += 1
                print(f"FAIL  {label}  ({elapsed:.0f}s)")
            else:
                print(f"WARN  {label}  ({elapsed:.0f}s)  -- not lane A's; not blocking")
            print()
            errors = [
                ln
                for ln in out.splitlines()
                if ln.startswith("error") or " error[" in ln
            ]
            for line in errors[:20]:
                print(f"    {line}")
            if len(errors) > 20:
                print(f"    ... and {len(errors) - 20} more")
            print()
            for p in ours[:10]:
                print(f"    lane A  : {p}")
            for p in theirs[:10]:
                lane = _lane_of(p)
                who = "external" if lane == "-" else f"lane {lane} "
                print(f"    {who}: {p}")
            print()
            if ours:
                print("[pre-boot] a cfg(unix) arm in lane A's tree does not compile.")
                print("           rustc discards those tokens on this Windows host, so")
                print("           nothing else in the gate would ever have told you.")
            elif any(_lane_of(p) == "-" for p in theirs):
                print("[pre-boot] the breakage is outside this worktree (a dependency),")
                print("           so it is nobody's lane and does NOT block you.  Most")
                print("           likely a toolchain or dependency version change.")
            else:
                print("[pre-boot] the breakage is in another lane's tree, so this does")
                print("           NOT block you -- lane A must not write there.  File")
                print("           requests/a-<lane>-<slug>.md naming the paths above.")
                print("           (This is the failure mode lane B asked us all to")
                print("           watch for; finding it here is the gate working.)")
            print()

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
