#!/usr/bin/env python3
"""Build every program the userland workspace makes, for SlateOS: what the
disk image's /bin is staged from.

The operator's answer to B-Q21 (design-decisions §1053): every program that
builds goes on the image. `scripts/create-ext4-rootfs.sh` stages them all
except what `scripts/rootfs-bin-kept-off.txt` keeps off, and this builds them
-- every binary target of every package under `userspace/`, as
`cargo metadata` lists them, for `x86_64-slateos` (design-decisions §1164).

    python scripts/build-userland.py              # build them all
    python scripts/build-userland.py --list       # NAME<TAB>PACKAGE, one a line
    python scripts/build-userland.py --self-test

ONE LIST, FROM THE WORKSPACE. The programs are what the workspace declares,
never what happens to be in `target/`: the image's contents must not depend
on what a developer last built (the coupling `rootfs-bin-manifest.txt` was
made to end). The rootfs recipe asks `--list` for the same answer this
builds from, so the two cannot disagree.

THREE THINGS BEYOND `cargo build`, each a way the plain build leaves a binary
that is not the one meant:

1. A binary older than the sysroot's libc.a is relinked. Every program links
   `toolchain/sysroot/lib/libc.a`, which cargo cannot see as an input: after a
   libc rebuild it reports `Finished` and relinks nothing, and the binary
   carries the old library (userspace/sysroot-dep's module docs measure it).
   A crate with `sysroot-dep` in its build script is relinked by cargo; for
   the rest, the package is cleaned and built again. `cargo clean` needs
   `--target`: unlike `cargo build` it does not take the target from
   userspace/.cargo/config.toml, and without it cleans the host's directory
   and reports 0 files.
2. A name two packages build ships as the one built by the package named after
   it: `kill` is both a coreutils binary and the `userspace/kill` crate, and
   the second is the one meant (known-issues B-FORTY-TWO-BINARY-NAMES-ARE-
   BUILT-BY-TWO-PACKAGES). Both write `release/kill`, and cargo keeps whichever
   it linked last; building the namesake package again, alone, at the end
   leaves its copy there, since cargo links a requested package's outputs
   into `release/` whether it rebuilt them or found them fresh.
3. `--keep-going`, so one package that does not build costs only its own
   programs -- and this exits 1 naming them, since a program that does not
   build is a defect in the tree, not a smaller image.

Run from anywhere, on Windows or under WSL: under WSL it asks Windows cargo
(`cargo.exe`, through WSL's interop), which is what builds the workspace.
"""

from __future__ import annotations

import json
import os
import subprocess
import sys
from pathlib import Path
from typing import Callable, NamedTuple

ROOT = Path(__file__).resolve().parent.parent
#: The binaries, where `userspace/.cargo/config.toml`'s target puts them.
BIN_DIR = ROOT / "target" / "x86_64-slateos" / "release"
LIBC_A = ROOT / "toolchain" / "sysroot" / "lib" / "libc.a"
#: The target spec, relative to `userspace/`, which `cargo clean` needs named.
TARGET_SPEC = "../toolchain/x86_64-slateos.json"


class Program(NamedTuple):
    """One binary the userland builds: its name, and the package building it."""

    name: str
    package: str


def on_wsl() -> bool:
    """Running under WSL, where cargo is Windows' `cargo.exe`."""
    try:
        return "microsoft" in Path("/proc/version").read_text(encoding="utf-8").lower()
    except OSError:
        return False


def cargo() -> list[str]:
    return ["cargo.exe"] if on_wsl() else ["cargo"]


def host_path(path: Path) -> str:
    """`path` as the cargo that `cargo()` names can read it."""
    if not on_wsl():
        return str(path)
    return subprocess.run(["wslpath", "-w", str(path)], capture_output=True,
                          encoding="utf-8", check=True).stdout.strip()


def metadata() -> dict:
    """`cargo metadata --no-deps` for the workspace: its packages and their
    targets, read from the manifests alone."""
    out = subprocess.run(
        [*cargo(), "metadata", "--no-deps", "--format-version", "1",
         "--manifest-path", host_path(ROOT / "Cargo.toml")],
        capture_output=True, encoding="utf-8", check=True,
    ).stdout
    return json.loads(out)


def programs(meta: dict) -> list[Program]:
    """Every binary target of every package under `userspace/`, sorted.

    A target with `required-features` is left out: the default build does not
    make it, so it is not a program that builds."""
    root = meta["workspace_root"].replace("\\", "/").rstrip("/") + "/"
    found: set[Program] = set()
    for package in meta["packages"]:
        manifest = package["manifest_path"].replace("\\", "/")
        if not manifest.startswith(root + "userspace/"):
            continue
        for target in package["targets"]:
            if "bin" in target["kind"] and not target.get("required-features"):
                found.add(Program(target["name"], package["name"]))
    return sorted(found)


def chosen_producers(progs: list[Program]) -> dict[str, str | None]:
    """For each name more than one package builds, the package whose copy
    ships: the one named after the program, or None when none is."""
    by_name: dict[str, set[str]] = {}
    for p in progs:
        by_name.setdefault(p.name, set()).add(p.package)
    return {name: (name if name in pkgs else None)
            for name, pkgs in sorted(by_name.items()) if len(pkgs) > 1}


def stale_packages(progs: list[Program], mtime: Callable[[Path], float | None],
                   libc_mtime: float | None) -> list[str]:
    """The packages with a built binary older than libc.a. `mtime` is the
    file's, or None when it does not exist."""
    if libc_mtime is None:
        return []
    stale = set()
    for p in progs:
        when = mtime(BIN_DIR / p.name)
        if when is not None and when < libc_mtime:
            stale.add(p.package)
    return sorted(stale)


def missing(progs: list[Program], exists: Callable[[Path], bool]) -> list[Program]:
    """The programs with no binary."""
    return [p for p in progs if not exists(BIN_DIR / p.name)]


def flags(packages: list[str]) -> list[str]:
    return [arg for package in packages for arg in ("-p", package)]


def file_mtime(path: Path) -> float | None:
    try:
        return path.stat().st_mtime
    except OSError:
        return None


def build(progs: list[Program], run: Callable[[list[str]], int],
          mtime: Callable[[Path], float | None] = file_mtime,
          exists: Callable[[Path], bool] = Path.is_file, libc: Path = LIBC_A) -> int:
    """Build `progs`, relink what links an old libc, and leave each shared
    name's chosen copy in `release/`. 0 when every program is built and
    current; 1, saying which, when not. `run` runs one cargo command in
    `userspace/` and returns its status."""
    packages = sorted({p.package for p in progs})
    nightly = [*cargo(), "+nightly"]
    print(f"build-userland: {len(progs)} programs from {len(packages)} packages")
    status = run([*nightly, "build", "--release", "--keep-going", *flags(packages)])

    # 1. What links a libc older than the sysroot's, built again.
    stale = stale_packages(progs, mtime, file_mtime(libc))
    if stale:
        print(f"build-userland: {len(stale)} package(s) link an older libc.a; "
              f"cleaning and building them again: {' '.join(stale)}")
        run([*nightly, "clean", "--release", "--target", TARGET_SPEC, *flags(stale)])
        status = run([*nightly, "build", "--release", "--keep-going", *flags(stale)]) or status

    # 2. Each shared name's namesake copy, linked last.
    shared = chosen_producers(progs)
    chosen = sorted({pkg for pkg in shared.values() if pkg})
    if chosen:
        status = run([*nightly, "build", "--release", *flags(chosen)]) or status

    failed = False
    unchosen = sorted(name for name, pkg in shared.items() if pkg is None)
    if unchosen:
        print("build-userland: no package is named after these, and more than one "
              f"builds each, so which copy ships is undecided: {' '.join(unchosen)}")
        failed = True
    absent = missing(progs, exists)
    if absent:
        print(f"build-userland: {len(absent)} program(s) did not build: "
              + " ".join(f"{p.name} ({p.package})" for p in absent))
        failed = True
    still = stale_packages(progs, mtime, file_mtime(libc))
    if still:
        print(f"build-userland: still older than libc.a after a rebuild: {' '.join(still)}")
        failed = True
    if status and not failed:
        print(f"build-userland: cargo exited {status}, though every program is built")
        failed = True
    if failed:
        return 1
    print(f"build-userland: {len(progs)} programs built and current")
    return 0


def run_in_userspace(cmd: list[str]) -> int:
    env = dict(os.environ, CARGO_UNSTABLE_JSON_TARGET_SPEC="true")
    print("build-userland: $ " + " ".join(cmd[:4]) + (" ..." if len(cmd) > 4 else ""),
          flush=True)
    return subprocess.run(cmd, cwd=ROOT / "userspace", env=env).returncode


# ---------------------------------------------------------------------------
# Self-test
# ---------------------------------------------------------------------------

def self_test() -> int:
    failures: list[str] = []
    checked = 0

    def check(label: str, ok: bool) -> None:
        nonlocal checked
        checked += 1
        print(("ok   " if ok else "FAIL ") + label)
        if not ok:
            failures.append(label)

    def package(name: str, where: str, *targets: tuple) -> dict:
        return {"name": name, "manifest_path": f"C:\\ws\\{where}\\Cargo.toml",
                "targets": [{"name": t, "kind": kind, **extra} for t, kind, extra in targets]}

    meta = {"workspace_root": "C:\\ws", "packages": [
        package("coreutils", "userspace\\coreutils",
                ("coreutils", ["lib"], {}), ("cat", ["bin"], {}), ("kill", ["bin"], {})),
        package("kill", "userspace\\kill", ("kill", ["bin"], {})),
        package("libcall", "userspace\\libcall", ("libcall", ["lib"], {})),
        package("gated", "userspace\\gated", ("gated", ["bin"], {"required-features": ["x"]})),
        package("viewer", "apps\\viewer", ("viewer", ["bin"], {})),
        # A directory that only starts like userspace/ is not under it.
        package("lookalike", "userspace-tools\\lookalike", ("lookalike", ["bin"], {})),
    ]}
    progs = programs(meta)
    check("programs are every userspace bin target, sorted",
          progs == [Program("cat", "coreutils"), Program("kill", "coreutils"),
                    Program("kill", "kill")])
    check("...not a library, nor one needing a feature, nor another tree's",
          not {p.name for p in progs} & {"coreutils", "libcall", "gated", "viewer", "lookalike"})

    check("a name two packages build ships as its namesake package's",
          chosen_producers(progs) == {"kill": "kill"})
    check("...and is undecided when neither is named after it",
          chosen_producers([Program("x", "a"), Program("x", "b")]) == {"x": None})

    times = {BIN_DIR / "cat": 5.0, BIN_DIR / "kill": 20.0}
    check("a package with a binary older than libc.a is stale",
          stale_packages(progs, times.get, 10.0) == ["coreutils"])
    check("...and nothing is, with no libc.a to be older than",
          stale_packages(progs, times.get, None) == [])
    check("an unbuilt binary is missing, not stale",
          stale_packages([Program("new", "n")], times.get, 10.0) == []
          and missing([Program("new", "n")], lambda p: p in times) == [Program("new", "n")])

    # The orchestration: build all, rebuild the stale, re-link the chosen. A
    # crate without sysroot-dep is not relinked by a build -- cargo finds it
    # fresh -- until it has been cleaned, which is the point of the clean.
    calls: list[list[str]] = []
    after = dict(times)
    cleaned: set[str] = set()

    def run(cmd: list[str]) -> int:
        calls.append(cmd)
        named = {cmd[i + 1] for i, a in enumerate(cmd) if a == "-p"}
        if "clean" in cmd:
            cleaned.update(named)
        elif "coreutils" in named & cleaned:
            after[BIN_DIR / "cat"] = 30.0      # relinked, against today's libc
        return 0

    import tempfile
    with tempfile.TemporaryDirectory() as td:
        libc = Path(td) / "libc.a"
        libc.write_bytes(b"")
        os.utime(libc, (10.0, 10.0))
        rc = build(progs, run, mtime=after.get, exists=lambda p: p in after, libc=libc)
    verbs = [next(a for a in c if a in ("build", "clean")) for c in calls]
    check("the build is: everything, the stale cleaned and rebuilt, the namesakes last",
          verbs == ["build", "clean", "build", "build"])
    check("...everything with --keep-going",
          "--keep-going" in calls[0] and calls[0].count("-p") == 2)
    check("...the clean names the target, as cargo clean needs",
          "--target" in calls[1] and calls[1][-2:] == ["-p", "coreutils"])
    check("...and the last build is the namesake package alone",
          calls[-1][-2:] == ["-p", "kill"] and calls[-1].count("-p") == 1)
    check("...and it all comes to 0", rc == 0)

    # A program that does not build fails the run, by name.
    calls.clear()
    with tempfile.TemporaryDirectory() as td:
        rc = build(progs + [Program("broken", "broken")], lambda c: calls.append(c) or 101,
                   mtime=after.get, exists=lambda p: p in after, libc=Path(td) / "absent")
    check("a program that did not build is a failure", rc == 1)

    print(f"build-userland self-test: {checked - len(failures)}/{checked} cases pass")
    return 1 if failures else 0


def main(argv: list[str]) -> int:
    if any(a in ("--self-test", "--selftest") for a in argv):
        return self_test()
    if argv not in ([], ["--list"]):
        print(__doc__.split("\n\n", 1)[0], file=sys.stderr)
        print("usage: build-userland.py [--list | --self-test]", file=sys.stderr)
        return 2
    try:
        progs = programs(metadata())
    except (OSError, subprocess.CalledProcessError, json.JSONDecodeError) as exc:
        print(f"build-userland: cannot read the workspace's programs: {exc}", file=sys.stderr)
        return 2
    if argv == ["--list"]:
        for p in progs:
            print(f"{p.name}\t{p.package}")
        return 0
    return build(progs, run_in_userspace)


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
