#!/usr/bin/env python3
"""Compile the `services/*` crates that nothing else compiles.

Six crates under `services/` are NOT workspace members: `hello`, `httpget`,
`init`, `netstack`, `ticker` and `udpget`. Each carries its own
`.cargo/config.toml` pinning `x86_64-unknown-none`, which is what lets them
build at all -- and what keeps them out of every check that runs here.

    grep -c 'services/(hello|httpget|init|netstack|ticker|udpget)'
        scripts/hooks/pre-push   -> 0
        scripts/boot-test.sh     -> 0

`cargo test --workspace` does not build them. `cargo check --workspace` does
not build them. `cargo test -p netstack` answers "package ID specification did
not match any packages". Nothing in the push hook or the boot test names them.

WHY THAT MATTERS RATHER THAN BEING TIDY. They depend on crates that ARE in the
workspace and do change: `services/netstack` pulls in `netproto`, `netipc` and
`netring`, and its own Cargo.toml explains that netproto "inherits this crate's
x86_64-unknown-none target when built as a dependency". So an edit to any of
those three can break the daemon, every workspace check stays green, and the
breakage waits until somebody rebuilds the rootfs -- or until a boot.

`services/init` is the sharper case: it is the first userspace process, and a
kernel that boots to a broken init has nothing to report the breakage with.

COST: 0.6 s for all six fully warm, 1.9 s on the first run after a dependency
changes. Both measured. They are `no_std` programs of a few thousand lines
and their target directories total 1.2 MB between them.

WHAT THIS IS NOT. It compiles; it does not test. Those crates have no tests and
mostly cannot have host-runnable ones -- they are raw syscall wrappers and the
boot sequences that call them, which is recorded per line in
`scripts/untested-crates-baseline.txt`. Compiling is the floor, and the floor
is what was missing.

    python scripts/check-services-build.py
    python scripts/check-services-build.py --selftest
"""

import argparse
import os
import subprocess
import sys
import time

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))

NL = chr(10)

# Where the self-building crates live. A directory here is one of ours if it
# holds BOTH a Cargo.toml and its own `.cargo/config.toml` -- the second is
# what makes it buildable outside the workspace, and is therefore the exact
# marker for "nothing else compiles this".
SERVICES = os.path.join(ROOT, "services")


def self_building_crates(base=SERVICES):
    """Crate directories under `services/` that carry their own cargo config.

    Discovered rather than listed, so a seventh one is covered the day it is
    added. A hardcoded list is how a gate comes to cover five of six.
    """
    out = []
    try:
        names = sorted(os.listdir(base))
    except OSError:
        return out
    for name in names:
        d = os.path.join(base, name)
        if not os.path.isdir(d):
            continue
        if not os.path.isfile(os.path.join(d, "Cargo.toml")):
            continue
        if not os.path.isfile(os.path.join(d, ".cargo", "config.toml")):
            continue
        out.append(name)
    return out


def check_one(name, timeout):
    """`cargo check` in one crate. Returns (ok, seconds, first_error_line)."""
    d = os.path.join(SERVICES, name)
    started = time.time()
    try:
        proc = subprocess.run(
            ["cargo", "check", "--quiet"],
            cwd=d, capture_output=True, text=True, errors="replace",
            timeout=timeout,
        )
    except subprocess.TimeoutExpired:
        return False, time.time() - started, "timed out after " + str(timeout) + "s"
    except OSError as exc:
        return False, time.time() - started, "could not run cargo: " + str(exc)
    elapsed = time.time() - started
    if proc.returncode == 0:
        return True, elapsed, ""
    first = ""
    for line in (proc.stderr + proc.stdout).split(NL):
        if line.startswith("error"):
            first = line.strip()
            break
    return False, elapsed, first or ("cargo exited " + str(proc.returncode))


def selftest():
    bad = 0
    checks = 0

    def ck(ok, msg):
        nonlocal bad, checks
        checks += 1
        if not ok:
            print("selftest FAIL: " + msg, file=sys.stderr)
            bad += 1

    found = self_building_crates()
    # The corpus must be real. A discovery that returned nothing would report a
    # clean tree however broken it was, which is the one outcome a gate must
    # never produce.
    ck(len(found) >= 6,
       "expected at least six self-building services crates, found "
       + repr(found))
    for expected in ("init", "netstack"):
        ck(expected in found,
           expected + " must be discovered -- it is the reason this gate exists")

    # The marker must be BOTH files. A directory with a Cargo.toml alone is a
    # workspace member and is already covered; one with only a cargo config is
    # not a crate.
    import tempfile
    with tempfile.TemporaryDirectory() as td:
        os.makedirs(os.path.join(td, "only-manifest"))
        open(os.path.join(td, "only-manifest", "Cargo.toml"), "w").close()
        os.makedirs(os.path.join(td, "only-config", ".cargo"))
        open(os.path.join(td, "only-config", ".cargo", "config.toml"), "w").close()
        os.makedirs(os.path.join(td, "both", ".cargo"))
        open(os.path.join(td, "both", "Cargo.toml"), "w").close()
        open(os.path.join(td, "both", ".cargo", "config.toml"), "w").close()
        got = self_building_crates(td)
        ck(got == ["both"],
           "only a directory with BOTH markers is self-building, got " + repr(got))

    # Deliberately a path that does not exist -- the temp directory above has
    # been removed by now, which is the point rather than an accident.
    ck(self_building_crates(os.path.join(ROOT, "no-such-directory-zzq")) == [],
       "a missing directory is an empty list, not a crash")

    print("selftest: " + str(checks - bad) + "/" + str(checks) + " cases pass")
    return 1 if bad else 0


def main():
    ap = argparse.ArgumentParser(
        description=__doc__,
        formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--timeout", type=int, default=300,
                    help="seconds per crate (default 300)")
    ap.add_argument("--selftest", "--self-test", dest="selftest",
                    action="store_true")
    args = ap.parse_args()

    if args.selftest:
        return selftest()

    crates = self_building_crates()
    if not crates:
        print("check-services-build: no self-building crate found under "
              "services/ -- nothing to judge.", file=sys.stderr)
        return 2

    failed = []
    total = 0.0
    for name in crates:
        ok, secs, err = check_one(name, args.timeout)
        total += secs
        mark = "ok " if ok else "FAIL"
        print("  " + mark + "  " + name.ljust(12)
              + format(secs, ".1f") + "s"
              + (("   " + err) if err else ""))
        if not ok:
            failed.append(name)

    print(str(len(crates)) + " crate(s) checked in "
          + format(total, ".1f") + "s; " + str(len(failed)) + " failed.")

    if failed:
        print("", file=sys.stderr)
        print("A crate above does not compile, and nothing else in this "
              "repository would have told you: these are not workspace "
              "members, so `cargo test --workspace` and every check in the "
              "push hook skip them.", file=sys.stderr)
        print("They depend on crates that ARE in the workspace -- netstack "
              "pulls in netproto, netipc and netring -- so an edit over there "
              "can break them while everything here stays green.",
              file=sys.stderr)
        print("services/init is the one to fix first if it is in the list: a "
              "kernel that boots to a broken init has nothing to report the "
              "breakage with.", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
