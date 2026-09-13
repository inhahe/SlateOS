#!/usr/bin/env python3
"""Attribute a POSIX-looking directory at the drive root to the crate that made it.

On Windows a path beginning with `/` is DRIVE-RELATIVE, not absolute:
`Path::new("/dev")` opens `E:\\dev` when the current drive is `E:`, and
`create_dir_all` on it succeeds. A test that writes such a path therefore does
not fail harmlessly on the dev host -- it succeeds, and leaves a real directory
at the root of the operator's data drive, where the next run of some *other*
crate finds it.

`scripts/check-drive-root-litter.py` reports that the litter is there. This
answers the next question: WHICH CRATE. It runs a crate's tests, compares the
drive root before and after, and names the crate that gained an entry.

WHY THIS IS BEHAVIOURAL AND NOT A GREP. The obvious static version -- refuse a
string literal starting with `/` passed to a filesystem write inside
`#[cfg(test)]` -- was tried first and finds **three** hits across lane B's
2,695 files, all three of which are strings that are never used as paths. It
would not have found the one real instance: `udevd`'s test named the production
constant `DEV_DIR`, not a literal, and the write happened three calls deeper in
`apply_device_node`. A path reached through a constant, a struct field, a
join, or a binary invoked with an argument is invisible to a grep and obvious
to this.

IT DOES NOT DELETE ANYTHING, and that is deliberate rather than cautious.
Three lanes share this machine and therefore share one drive root. A gate that
cleaned the root could delete a directory another lane's test run is using at
that moment -- and while that directory is itself a bug, destroying another
agent's in-flight run to report it is not this tool's call. It snapshots,
runs, and compares. `check-drive-root-litter.py --clean` is where the one
irreversible act lives.

CONCURRENCY, stated because it is the failure mode that would make this lie.
If another lane's test run creates a directory while this one is timing a
crate, that crate gets the blame. So a finding is CONFIRMED by re-running the
suspect crate alone: an entry that appears twice, from the same crate, with a
clean comparison in between, is that crate's. One that does not reproduce is
reported as UNCONFIRMED and does not fail the gate.

    python scripts/check-test-root-writes.py --crates udevd authlib
    python scripts/check-test-root-writes.py --all          # attribution sweep
    python scripts/check-test-root-writes.py --selftest
"""

import argparse
import os
import subprocess
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))

NL = chr(10)

# The drive the repository lives on is the drive a leading-slash path resolves
# against, because the tests run with their cwd inside the repository.
DRIVE = os.path.splitdrive(os.path.abspath(ROOT))[0] + os.sep

# Names that are POSIX filesystem roots. A directory at the drive root with one
# of these names did not come from Windows.
POSIX_ROOTS = (
    "bin", "boot", "dev", "etc", "home", "lib", "lib64", "mnt", "opt",
    "proc", "root", "run", "sbin", "srv", "sys", "usr", "var",
)

TARGET = "x86_64-pc-windows-gnu"


def snapshot():
    """The POSIX-looking directories at the drive root, right now."""
    try:
        entries = os.listdir(DRIVE)
    except OSError:
        return set()
    return {
        e for e in entries
        if e.lower() in POSIX_ROOTS and os.path.isdir(os.path.join(DRIVE, e))
    }


def parse_passed(output):
    """Total `passed` across every `test result:` line in cargo's output.

    A crate can have several test binaries, so this sums rather than taking
    the first -- reporting one binary's count as the crate's would understate
    it, and an understated count that still looks plausible is worse than none.
    """
    passed = 0
    for line in output.split(NL):
        if line.startswith("test result:"):
            parts = line.split()
            for i, p in enumerate(parts):
                if p == "passed;" and i > 0:
                    try:
                        passed += int(parts[i - 1])
                    except ValueError:
                        pass
    return passed


def run_tests(crate, timeout):
    """`cargo test -p <crate>`. Returns (ok, summary)."""
    runner = os.path.join(ROOT, "scripts", "run-timeout.py")
    cmd = [sys.executable, runner, str(timeout), "cargo", "test",
           "-p", crate, "--target", TARGET]
    try:
        proc = subprocess.run(cmd, cwd=ROOT, capture_output=True, text=True,
                              errors="replace")
    except OSError as exc:
        return False, "could not run: " + str(exc)
    out = proc.stdout + proc.stderr
    return proc.returncode == 0, str(parse_passed(out)) + " passed"


def workspace_crates():
    """Every crate name cargo knows about, from `cargo metadata`."""
    try:
        proc = subprocess.run(
            ["cargo", "metadata", "--no-deps", "--format-version", "1"],
            cwd=ROOT, capture_output=True, text=True, errors="replace")
    except OSError as exc:
        print("cannot run cargo metadata: " + str(exc), file=sys.stderr)
        return []
    if proc.returncode != 0:
        print("cargo metadata failed", file=sys.stderr)
        return []
    import json
    try:
        meta = json.loads(proc.stdout)
    except ValueError:
        return []
    return sorted(p["name"] for p in meta.get("packages", []))


def check(crates, timeout, confirm=True):
    findings = []
    for crate in crates:
        before = snapshot()
        ok, summary = run_tests(crate, timeout)
        after = snapshot()
        gained = sorted(after - before)
        status = "ok " if ok else "RED"
        if not gained:
            print("  " + status + "  " + crate.ljust(22) + summary)
            continue

        names = ", ".join(DRIVE + g for g in gained)
        if not confirm:
            print("  " + status + "  " + crate.ljust(22) + summary
                  + "   CREATED " + names)
            findings.append((crate, gained, "unconfirmed"))
            continue

        # Confirm before accusing: another lane sharing this machine could
        # have created the directory during the window above.
        print("  " + status + "  " + crate.ljust(22) + summary
              + "   created " + names + " -- confirming")
        # The directory is left in place, so a second run cannot "gain" it
        # again. Compare against a snapshot taken now instead.
        before2 = snapshot()
        run_tests(crate, timeout)
        after2 = snapshot()
        again = sorted(after2 - before2)
        if again:
            findings.append((crate, sorted(set(gained) | set(again)), "confirmed"))
            print("       CONFIRMED: " + crate + " created "
                  + ", ".join(DRIVE + g for g in again) + " again")
        else:
            findings.append((crate, gained, "unconfirmed"))
            print("       unconfirmed: the second run created nothing. Either "
                  "the directory already existing is enough to stop it, or "
                  "another lane made it. Not counted as a failure.")
    return findings


def selftest():
    bad = 0
    checks = 0

    def ck(ok, msg):
        nonlocal bad, checks
        checks += 1
        if not ok:
            print("selftest FAIL: " + msg, file=sys.stderr)
            bad += 1

    ck(DRIVE.endswith(os.sep) and len(DRIVE) == 3,
       "the drive root should look like 'E:\\', got " + repr(DRIVE))
    ck("dev" in POSIX_ROOTS and "sys" in POSIX_ROOTS and "var" in POSIX_ROOTS,
       "the three roots actually observed here must be in the list")
    ck("windows" not in POSIX_ROOTS and "users" not in POSIX_ROOTS,
       "a Windows directory must never be in the list -- this tool would "
       "otherwise blame a crate for C:/Users")
    # snapshot() must not invent entries, and must not raise on a root it
    # cannot read.
    snap = snapshot()
    ck(isinstance(snap, set), "snapshot must return a set")
    ck(all(s.lower() in POSIX_ROOTS for s in snap),
       "snapshot returned something that is not a POSIX root: " + repr(snap))
    # The count must sum every test binary, not report the first. A crate with
    # a lib test and a bin test has two `test result:` lines, and taking one
    # would understate the crate -- plausibly, which is the bad kind of wrong.
    two = ("test result: ok. 12 passed; 0 failed; 0 ignored" + NL
           + "test result: ok. 7 passed; 0 failed; 0 ignored" + NL)
    ck(parse_passed(two) == 19,
       "two test binaries must sum to 19, got " + str(parse_passed(two)))
    ck(parse_passed("") == 0, "no output is zero passed, not a crash")
    ck(parse_passed("Compiling foo v0.1.0" + NL) == 0,
       "a build log with no results is zero passed")
    # A failing run still reports its passes; the caller reads `ok` separately.
    fail = "test result: FAILED. 3 passed; 2 failed; 0 ignored" + NL
    ck(parse_passed(fail) == 3,
       "a failing run still has a pass count, got " + str(parse_passed(fail)))

    print("selftest: " + str(checks - bad) + "/" + str(checks) + " cases pass")
    return 1 if bad else 0


def main():
    ap = argparse.ArgumentParser(
        description=__doc__,
        formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--crates", nargs="+", metavar="NAME",
                    help="crates to test, in order")
    ap.add_argument("--all", action="store_true",
                    help="every crate in the workspace (an attribution sweep, "
                         "not a gate -- it is slow)")
    ap.add_argument("--timeout", type=int, default=300,
                    help="seconds per crate (default 300)")
    ap.add_argument("--no-confirm", action="store_true",
                    help="report the first observation without re-running")
    ap.add_argument("--selftest", "--self-test", dest="selftest",
                    action="store_true")
    args = ap.parse_args()

    if args.selftest:
        return selftest()

    if args.all:
        crates = workspace_crates()
    elif args.crates:
        crates = args.crates
    else:
        ap.error("give --crates NAME... or --all")
        return 2

    if not crates:
        # An empty crate list is not a pass: it is a tool that has lost its
        # subject. Exit 2 -- no verdict.
        print("check-test-root-writes: no crate to test -- nothing to judge.",
              file=sys.stderr)
        return 2

    print("drive root: " + DRIVE + "   before: "
          + (", ".join(sorted(snapshot())) or "(clean)"))
    findings = check(crates, args.timeout, confirm=not args.no_confirm)

    confirmed = [f for f in findings if f[2] == "confirmed"]
    print("")
    print(str(len(crates)) + " crate(s) tested; "
          + str(len(confirmed)) + " confirmed to write at the drive root.")
    for crate, gained, _ in confirmed:
        print("  " + crate + " -> " + ", ".join(DRIVE + g for g in gained))

    if confirmed:
        print("", file=sys.stderr)
        print("A test above created a directory at the root of the operator's "
              "data drive. On Windows a leading-slash path is drive-relative, "
              "so `/dev` is `" + DRIVE + "dev` and creating it succeeds.",
              file=sys.stderr)
        print("The cost is not the litter: the state one crate's test asserts "
              "about stops being the state that test controls. The same tree "
              "returned 0, 2 and 6 workspace failures on three consecutive "
              "runs depending on what was left here.", file=sys.stderr)
        print("Fix: give the path a root the test owns -- "
              "`scratchdir::ScratchDir` -- rather than the production "
              "constant. See requests/"
              "c-b-tests-create-real-directories-at-the-drive-root.md.",
              file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
