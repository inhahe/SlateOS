#!/usr/bin/env python3
"""Compile every crate for the target it SHIPS to, when nothing else does.

Eight crates in lane B's trees carry their own `.cargo/config.toml` pinning
`[build] target = "x86_64-unknown-none"`. That pin is what lets them build for
bare metal -- and it is also what puts their real configuration outside every
command anyone runs here.

    posix                toolchain/stubs
    services/hello       services/httpget      services/init
    services/netstack    services/ticker       services/udpget

TWO DIFFERENT GAPS, ONE QUESTION. The six under `services/` are not workspace
members at all: `cargo test --workspace` skips them and `cargo test -p netstack`
answers "did not match any packages". Grepping the push hook and boot-test.sh
for their names returned 0 before this gate existed.

`posix` and `toolchain/stubs` ARE members, which is worse rather than better,
because it looks like coverage. The push hook runs `cargo test -p posix` -- on
the HOST target. Nothing on the push or boot path ever builds posix for
`x86_64-unknown-none`, and the kernel does not link it, so the only thing that
compiles the configuration that actually ships is `toolchain/build-sysroot.ps1`,
run by hand.

Both are the same question: *a verdict whose answer depends on a configuration
the command did not name.*

WHY posix IS THE ONE THAT MATTERS. It is the libc. Every one of ~280 userspace
binaries links `toolchain/sysroot/lib/libc.a`, which is built from this crate
for bare metal. A change that compiles on the host and not on the target makes
`build-sysroot.ps1` fail -- whenever somebody next happens to run it, with the
break already merged and possibly days old.

`services/init` is the sharp one for a different reason: it is the first
userspace process, and a kernel that boots to a broken init has nothing left to
report the breakage with.

COST, measured warm: 0.6 s for the six services crates, 1.0 s for posix, 0.9 s
for toolchain/stubs. Their target directories total 1.2 MB.

WHAT THIS IS NOT. It compiles; it does not test. The services crates have no
host-runnable tests and `scripts/untested-crates-baseline.txt` records why per
line. posix has 20,703 of them and they all run on the host. Compiling for the
shipping target is the floor, and the floor is what was missing.

    python scripts/check-pinned-target-build.py
    python scripts/check-pinned-target-build.py --selftest
"""

import argparse
import os
import re
import subprocess
import sys
import time

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))

NL = chr(10)

# Lane B's trees. A crate anywhere under these is ours to compile if it holds
# BOTH a Cargo.toml and its own `.cargo/config.toml` with a `[build] target` --
# that pin is precisely the marker for "the configuration that ships is not the
# one any workspace command builds".
SEARCH_ROOTS = ("posix", "toolchain", "services", "userspace", "init")

# Zones whose crates inherit a build target from a config ABOVE them rather
# than carrying one of their own. Scanned by `inherited_target_crates`, which
# reports them; they are NOT compiled here. See that function for why the two
# are separate.
INHERIT_ROOTS = ("gui", "apps")

BUILD_TARGET = re.compile(r"^\s*target\s*=\s*\"([^\"]+)\"", re.M)


def pinned_target_crates(roots=SEARCH_ROOTS, base=None):
    """Repo-relative crate directories that pin their own build target.

    Discovered rather than listed, so a ninth is covered the day it is added.
    A hardcoded list is how a gate comes to cover five of six -- and this one
    grew from six to eight the first time the question was asked of a
    different directory.
    """
    out = []
    top = base if base is not None else ROOT
    for root in roots:
        start = os.path.join(top, root)
        if not os.path.isdir(start):
            continue
        for dirpath, dirnames, filenames in os.walk(start):
            dirnames[:] = [d for d in dirnames if d not in ("target", ".git")]
            if "Cargo.toml" not in filenames:
                continue
            cfg = os.path.join(dirpath, ".cargo", "config.toml")
            if not os.path.isfile(cfg):
                continue
            try:
                with open(cfg, encoding="utf-8", errors="replace") as fh:
                    text = fh.read()
            except OSError:
                continue
            # A cargo config with no `[build] target` does not move the crate
            # off whatever the caller asked for, so it is not this gate's.
            if not BUILD_TARGET.search(text):
                continue
            out.append(os.path.relpath(dirpath, top).replace(os.sep, "/"))
    return sorted(out)


def zone_configs(roots, base=None):
    """`{directory: target}` for every `.cargo/config.toml` that pins a target.

    Keyed by the directory that CONTAINS `.cargo`, which is the directory
    cargo's upward search would find it from.
    """
    top = base if base is not None else ROOT
    out = {}
    for root in roots:
        start = os.path.join(top, root)
        if not os.path.isdir(start):
            continue
        for dirpath, dirnames, filenames in os.walk(start):
            dirnames[:] = [d for d in dirnames if d not in ("target", ".git")]
            if os.path.basename(dirpath) != ".cargo" or "config.toml" not in filenames:
                continue
            try:
                with open(os.path.join(dirpath, "config.toml"),
                          encoding="utf-8", errors="replace") as fh:
                    text = fh.read()
            except OSError:
                continue
            found = BUILD_TARGET.search(text)
            if found:
                out[os.path.dirname(dirpath)] = found.group(1)
    return out


def inherited_target_crates(roots=INHERIT_ROOTS, base=None):
    """Crates whose build target comes from a config ABOVE their directory.

    **This exists because the per-crate predicate reported a confident zero.**
    [`pinned_target_crates`] keys on a directory holding both a `Cargo.toml`
    and its own `.cargo/config.toml`, which is how `posix/` and `services/*`
    are laid out. `gui/` and `apps/` put ONE config at the zone root and let
    cargo's upward search give it to all 161 crates beneath. So the predicate
    matched nothing there, and widening `--roots` alone would still have
    matched nothing -- the roots were not the narrowing, the predicate was.
    Reported by lane C on 2026-09-14; the zero was reproduced here before
    this was written.

    The docstring on [`pinned_target_crates`] already named the class -- "a
    hardcoded list is how a gate comes to cover five of six" -- and this is the
    same failure one level down: the list was fine and the *rule* encoded one
    layout.

    **Why these are reported and not compiled.** `check_one` runs `cargo
    check` per crate, and the two zone configs also set
    `build-std = ["core", "alloc", "std", "panic_abort"]`, so a cold run
    compiles the standard library from source before it compiles anything
    else. Adding 161 of those to a gate that runs on every push is a cost
    nobody has measured yet -- so this function makes the coverage gap
    VISIBLE without paying for it, and the measurement is the next step
    rather than a guess baked into the push path.
    """
    top = base if base is not None else ROOT
    configs = zone_configs(roots, base=top)
    out = []
    for root in roots:
        start = os.path.join(top, root)
        if not os.path.isdir(start):
            continue
        for dirpath, dirnames, filenames in os.walk(start):
            dirnames[:] = [d for d in dirnames if d not in ("target", ".git")]
            if "Cargo.toml" not in filenames:
                continue
            if os.path.isfile(os.path.join(dirpath, ".cargo", "config.toml")):
                # Its own config; `pinned_target_crates` already has it.
                continue
            # Cargo searches upward. The NEAREST config wins, so walk up from
            # the crate rather than assuming the zone root.
            probe = dirpath
            while True:
                if probe in configs:
                    out.append((
                        os.path.relpath(dirpath, top).replace(os.sep, "/"),
                        os.path.relpath(probe, top).replace(os.sep, "/"),
                    ))
                    break
                parent = os.path.dirname(probe)
                if parent == probe or len(probe) <= len(top):
                    break
                probe = parent
    return out


def check_one(rel, timeout):
    """`cargo check` in one crate. Returns (ok, seconds, first_error_line)."""
    d = os.path.join(ROOT, rel.replace("/", os.sep))
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

    found = pinned_target_crates()
    # The corpus must be real. A discovery that returned nothing would report a
    # clean tree however broken it was, which is the one outcome a gate must
    # never produce.
    ck(len(found) >= 8,
       "expected at least eight pinned-target crates, found " + repr(found))
    for expected in ("posix", "toolchain/stubs", "services/init",
                     "services/netstack"):
        ck(expected in found,
           expected + " must be discovered -- it is one of the reasons this "
           "gate exists")
    # posix is the one that looks covered and is not: the push hook runs
    # `cargo test -p posix` on the HOST. If it ever drops out of this list,
    # the libc every userspace binary links stops being compiled for the
    # target it ships to and nothing says so.
    ck("posix" in found, "posix must never silently leave this gate")

    # The inherited scan must find the zones the per-crate predicate cannot.
    # A floor, not an equality: the number grows as lane C adds crates, and a
    # gate that fails on growth teaches people to edit the gate.
    inherited = inherited_target_crates()
    ck(len(inherited) >= 100,
       "expected the gui/apps zones to contribute a large inherited set, "
       "found " + str(len(inherited)))
    # ...and it must not double-count anything the per-crate predicate has.
    own = set(pinned_target_crates())
    ck(not (own & {c for c, _ in inherited}),
       "a crate with its own config must not also appear as inheriting one")

    # The marker must be BOTH files. A directory with a Cargo.toml alone is a
    # workspace member and is already covered; one with only a cargo config is
    # not a crate.
    import tempfile
    with tempfile.TemporaryDirectory() as td:
        os.makedirs(os.path.join(td, "only-manifest"))
        open(os.path.join(td, "only-manifest", "Cargo.toml"), "w",
             newline="").close()
        os.makedirs(os.path.join(td, "only-config", ".cargo"))
        with open(os.path.join(td, "only-config", ".cargo",
                               "config.toml"), "w", newline="") as fh:
            fh.write("[build]" + NL + 'target = "x86_64-unknown-none"' + NL)
        os.makedirs(os.path.join(td, "both", ".cargo"))
        open(os.path.join(td, "both", "Cargo.toml"), "w", newline="").close()
        with open(os.path.join(td, "both", ".cargo", "config.toml"), "w",
                  newline="") as fh:
            fh.write("[build]" + NL + 'target = "x86_64-unknown-none"' + NL)
        # ...and one with a config that pins NOTHING, which does not move the
        # crate off whatever the caller asked for and is therefore not ours.
        os.makedirs(os.path.join(td, "config-no-target", ".cargo"))
        open(os.path.join(td, "config-no-target", "Cargo.toml"), "w",
             newline="").close()
        with open(os.path.join(td, "config-no-target", ".cargo",
                               "config.toml"), "w", newline="") as fh:
            fh.write("[net]" + NL + "offline = true" + NL)
        got = pinned_target_crates(roots=("",), base=td)
        ck(got == ["both"],
           "only a directory with BOTH markers and a [build] target counts, "
           "got " + repr(got))

    # Deliberately a path that does not exist -- the temp directory above has
    # been removed by now, which is the point rather than an accident.
    ck(pinned_target_crates(roots=("no-such-directory-zzq",)) == [],
       "a missing directory is an empty list, not a crash")

    # Mirror of the assertion in check-untested-crates.py. The two gates
    # disagreed about lane B's scope once -- this one included `toolchain` and
    # that one did not -- and toolchain/stubs hid in the gap.
    ck(sorted(SEARCH_ROOTS) == ["init", "posix", "services", "toolchain",
                                "userspace"],
       "roots are " + repr(SEARCH_ROOTS) + " -- they must match "
       "check-untested-crates.py's ROOTS")

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

    crates = pinned_target_crates()
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
        print("  " + mark + "  " + name.ljust(20)
              + format(secs, ".1f") + "s"
              + (("   " + err) if err else ""))
        if not ok:
            failed.append(name)

    print(str(len(crates)) + " crate(s) checked in "
          + format(total, ".1f") + "s; " + str(len(failed)) + " failed.")

    # DISCOVERED BUT NOT COMPILED, and the distinction is deliberate.
    #
    # These crates inherit a pinned target from a config above them rather than
    # carrying one of their own, so the per-crate predicate above never saw
    # them and this gate reported a confident zero about 161 crates. They are
    # counted here so the gap is visible; they are not compiled here because
    # those zone configs also set `build-std`, which means a cold run builds
    # the standard library from source before anything else. Paying that on
    # every push is a decision that needs a measurement, not a default.
    inherited = inherited_target_crates()
    if inherited:
        zones = sorted({cfg for _, cfg in inherited})
        print("")
        print(str(len(inherited)) + " further crate(s) pin a target by "
              "INHERITING one from " + ", ".join(zones)
              + " -- discovered, not compiled here. See "
              "`inherited_target_crates` for why, and "
              "requests/ for the open question about what should compile them.")

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
