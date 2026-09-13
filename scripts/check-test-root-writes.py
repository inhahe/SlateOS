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

WHAT IT DELETES, AND WHAT IT WILL NOT. It removes exactly the entries it has
just watched one crate create, and nothing else. Three lanes share this machine
and therefore share one drive root, so a tool that cleaned the root wholesale
could destroy a directory another lane's run is using at that moment -- that
stays behind `check-drive-root-litter.py --clean`, which is explicit and a
human's decision. An entry that did not exist seconds ago and appeared during
one crate's test run is a different thing: nothing else can be depending on it.

The narrow deletion is not tidiness, it is what makes the gate work at all.
The first version deleted nothing, and so the confirming run below started
with the directory already present, could not "gain" it, and reported every
real finding as UNCONFIRMED -- a gate that looked correct and could never
fire. Found by planting a writer in a crate and watching the gate pass.

CONCURRENCY, stated because it is the failure mode that would make this lie.
If another lane's test run creates a directory while this one is timing a
crate, that crate gets the blame. So a finding is CONFIRMED by re-running the
suspect crate alone: an entry that appears twice, from the same crate, with
the first copy removed in between, is that crate's. One that does not
reproduce is reported as UNCONFIRMED and does not fail the gate -- a gate that
blames the wrong crate once gets switched off.

    python scripts/check-test-root-writes.py --crates udevd authlib
    python scripts/check-test-root-writes.py --all          # attribution sweep
    python scripts/check-test-root-writes.py --selftest
"""

import argparse
import os
import re
import shutil
import subprocess
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))

NL = chr(10)

# The drive the repository lives on is the drive a leading-slash path resolves
# against, because the tests run with their cwd inside the repository.
DRIVE = os.path.splitdrive(os.path.abspath(ROOT))[0] + os.sep

# The SOURCE trees this gate is responsible for. Named `SOURCE_ROOTS` rather
# than `ROOTS` because `POSIX_ROOTS` below means something completely
# different -- directory names at the DRIVE root -- and one file holding two
# unrelated things called "roots" is how the wrong one gets used.
SOURCE_ROOTS = ("userspace", "services", "init", "posix")

# Names that are POSIX filesystem roots. A directory at the drive root with one
# of these names did not come from Windows.
POSIX_ROOTS = (
    "bin", "boot", "dev", "etc", "home", "lib", "lib64", "mnt", "opt",
    "proc", "root", "run", "sbin", "srv", "sys", "usr", "var",
)

TARGET = "x86_64-pc-windows-gnu"


def snapshot():
    """The POSIX-looking directories at the drive root, right now.

    Matched CASE-SENSITIVELY, which is the whole reason `Boot` is not reported.
    Windows keeps its boot configuration in `E:\\Boot` with a capital B; a Rust
    `create_dir("/boot")` creates `boot`, because NTFS is case-insensitive but
    case-PRESERVING. Lowercasing before the comparison listed Windows' own
    directory as POSIX litter on every run -- harmless here, since a
    pre-existing entry can never appear in the before/after difference, but it
    is exactly the confusion this tool's own self-test forbids.
    """
    try:
        entries = os.listdir(DRIVE)
    except OSError:
        return set()
    return {
        e for e in entries
        if is_posix_root_name(e) and os.path.isdir(os.path.join(DRIVE, e))
    }


def is_posix_root_name(name):
    """Is `name` a POSIX filesystem root, spelled as a POSIX write would?

    Extracted so the self-test can assert BOTH directions -- that Windows'
    `Boot` is not matched AND that a lowercase `boot` still is. Lane C's
    version of this check asserts both and mine asserted only the first, which
    would pass just as happily if the scan were reverted to matching nothing.
    """
    return name in POSIX_ROOTS


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


def crate_dir_of(rel):
    """The crate directory a repo-relative path belongs to, or None.

    Two segments, the same rule `check-scratch-config.py` uses, so the two
    gates agree about what "a touched crate" means.
    """
    parts = rel.replace(os.sep, "/").split("/")
    if len(parts) < 2 or parts[0] not in SOURCE_ROOTS:
        return None
    return parts[0] + "/" + parts[1]


def package_of(crate_dir):
    """The cargo package name for a directory, which is not always its name."""
    toml = os.path.join(ROOT, crate_dir, "Cargo.toml")
    try:
        with open(toml, encoding="utf-8") as fh:
            text = fh.read()
    except OSError:
        return None
    m = re.search(r'^name\s*=\s*"([^"]+)"', text, re.M)
    return m.group(1) if m else None


# Run these when the checker itself is edited. They are the two crates that
# actually had the defect, so a change that breaks the detection is caught by
# the same push that makes it, rather than by the next unlucky crate.
CANARIES = ("udevd", "logind")


def crates_touching(paths):
    """Package names for the crates these paths belong to, in a stable order."""
    dirs = []
    for p in paths:
        d = crate_dir_of(p)
        if d and d not in dirs:
            dirs.append(d)
    out = []
    for d in dirs:
        name = package_of(d)
        if name and name not in out:
            out.append(name)
    return sorted(out)


def read_path_list(path):
    try:
        if path == "-":
            return [ln.strip() for ln in sys.stdin if ln.strip()]
        with open(path, encoding="utf-8") as fh:
            return [ln.strip() for ln in fh if ln.strip()]
    except OSError as exc:
        print("check-test-root-writes: cannot read " + str(path) + ": "
              + str(exc), file=sys.stderr)
        return None


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

        # Remove exactly what this crate was just watched creating, so the
        # second run has the same starting conditions as the first. Without
        # this the confirm step CANNOT succeed -- the directory already
        # exists, so nothing is "gained" and every real finding reports as
        # unconfirmed, which is a gate that looks like it works and never
        # fires. Found by planting a writer and watching the gate pass.
        #
        # This is a much narrower claim than `--clean`: these entries did not
        # exist seconds ago, this process watched them appear during one
        # crate's test run, so nothing else can depend on them. Anything that
        # was already there is untouched.
        for g in gained:
            victim = os.path.join(DRIVE, g)
            try:
                shutil.rmtree(victim)
            except OSError as exc:
                print("       could not remove " + victim + ": " + str(exc)
                      + " -- confirming without it", file=sys.stderr)

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
    ck(all(s in POSIX_ROOTS for s in snap),
       "snapshot returned something that is not a POSIX root: " + repr(snap))
    # Windows' own `Boot` must not be reported. It is spelled with a capital
    # B; a `/boot` created from Rust is lowercase, because NTFS preserves the
    # case it was given. This is the only thing separating the two.
    ck("Boot" not in POSIX_ROOTS,
       "POSIX_ROOTS must not contain Windows' capitalised Boot")
    ck("boot" in POSIX_ROOTS,
       "a genuine lowercase /boot write must still be caught")
    ck("Boot" not in snap,
       "Windows' E:/Boot is being reported as POSIX litter: " + repr(snap))
    # Both directions, which is lane C's shape and better than mine was. The
    # first assertion alone passes just as happily if the scan is reverted to
    # matching nothing at all; the second is what fails in that case.
    ck(not is_posix_root_name("Boot"),
       "Windows' capitalised Boot must not match")
    ck(is_posix_root_name("boot"),
       "a lowercase /boot -- what a POSIX write actually creates -- MUST match")
    ck(is_posix_root_name("dev") and is_posix_root_name("var"),
       "the two roots actually observed here must match")
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

    # The narrowing. A gate that tested the wrong crates would be worse than
    # no gate, because it would report a clean tree having looked elsewhere.
    ck(crate_dir_of("userspace/udevd/src/main.rs") == "userspace/udevd",
       "a source path must resolve to its crate directory")
    ck(crate_dir_of("init/loginmgr/src/main.rs") == "init/loginmgr",
       "init/ is one of this gate's trees")
    ck(crate_dir_of("gui/compositor/src/lib.rs") is None,
       "another lane's tree must not be narrowed INTO -- this gate cannot fix it")
    ck(crate_dir_of("README.md") is None,
       "a top-level file belongs to no crate")
    ck(crate_dir_of("userspace") is None,
       "a bare root directory is not a crate")
    ck(crate_dir_of("userspace\\udevd\\src\\main.rs") == "userspace/udevd",
       "a Windows-separated path must resolve the same way")
    # The real repository answers, so a rename that breaks the mapping fails
    # here rather than silently narrowing to nothing.
    ck(crates_touching(["userspace/udevd/src/main.rs"]) == ["udevd"],
       "the udevd package must be found from one of its files")
    ck(crates_touching(["README.md"]) == [],
       "a path in no crate selects no crate, which is a pass and not a refusal")
    ck(CANARIES and all(c in set(workspace_crates() or CANARIES) for c in CANARIES),
       "the canary crates must exist: " + repr(CANARIES))

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
    ap.add_argument("--paths", nargs="*", metavar="PATH", default=None,
                    help="repo-relative paths a push touched; the crates they "
                         "belong to are the ones tested")
    ap.add_argument("--paths-from", metavar="FILE", default=None,
                    help="read the touched paths from FILE, or '-' for stdin")
    ap.add_argument("--timeout", type=int, default=300,
                    help="seconds per crate (default 300)")
    ap.add_argument("--no-confirm", action="store_true",
                    help="report the first observation without re-running")
    ap.add_argument("--selftest", "--self-test", dest="selftest",
                    action="store_true")
    args = ap.parse_args()

    if args.selftest:
        return selftest()

    paths = list(args.paths or [])
    if args.paths_from is not None:
        extra = read_path_list(args.paths_from)
        if extra is None:
            return 2
        paths.extend(extra)

    if args.all:
        crates = workspace_crates()
        if not crates:
            # Nothing in the whole workspace: the scan has rotted, or this is
            # the wrong tree. Not a pass -- exit 2, no verdict.
            print("check-test-root-writes: cargo metadata named no package "
                  "at all -- nothing to judge.", file=sys.stderr)
            return 2
    elif args.crates:
        crates = args.crates
        known = set(workspace_crates())
        unknown = [c for c in crates if known and c not in known]
        if unknown:
            # A name that matches no package is a typo, and a typo that
            # silently tested nothing would report a clean tree.
            print("check-test-root-writes: no such package: "
                  + ", ".join(unknown), file=sys.stderr)
            return 2
    elif paths:
        # The narrowing. A push that touched no crate under these roots cannot
        # have changed what its tests write, so there is nothing to run -- and
        # that IS a pass, not a refusal. The distinction is lane C's: their
        # check-scratch-config refused its own first push because a narrowing
        # that had done its job looked the same as a scan that had found
        # nothing to look at.
        touched_checker = any(
            p.replace(os.sep, "/").endswith("scripts/check-test-root-writes.py")
            for p in paths
        )
        crates = crates_touching(paths)
        if touched_checker:
            for c in CANARIES:
                if c not in crates:
                    crates.append(c)
            print("check-test-root-writes: the checker itself changed; adding "
                  + ", ".join(CANARIES) + " as canaries.")
        if not crates:
            print("check-test-root-writes: none of the " + str(len(paths))
                  + " path(s) is in a crate under "
                  + "/, ".join(SOURCE_ROOTS) + "/ -- nothing this gate can judge.")
            return 0
    else:
        ap.error("give --crates NAME..., --paths PATH..., or --all")
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
