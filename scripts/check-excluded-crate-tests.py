#!/usr/bin/env python3
"""Run the tests of crates the workspace excludes, which nothing else runs.

`scripts/check-pinned-target-build.py` (push gate 40) closed half of this
question: the six crates under `services/` are not workspace members, so it
compiles them for the target they ship to, because nothing else did. Its
docstring is exact about the half it covers -- "Compiles, does not test: those
crates have no host-runnable tests". Measured 2026-09-15, that is still true:
all six contain zero `#[test]` functions.

THE OTHER HALF. Gate 40 also triggers on `netproto/`, `netipc/` and `netring/`,
because `services/netstack` links them and an edit there can break the daemon.
Those crates are excluded from the workspace too -- and unlike the services,
they are full of tests:

    netproto 79     netipc 41     netring 9     tzrules 59      = 188

Nothing ran them. Grepping every .sh, .py, .ps1, .yml and .toml in the tree for
a `cargo test` naming any of the four returned zero matches on 2026-09-15; the
only hits were prose. They are not workspace members, so `cargo test
--workspace` skips them, and `cargo test -p netproto` answers "did not match
any packages" -- the same two sentences gate 40 was written for, about a
different set of crates and a different verb.

WHY THIS IS WORSE THAN AN UNTESTED CRATE. `scripts/untested-crates-baseline.txt`
excuses `services/netstack` -- 4,329 lines -- on the grounds that "the protocol
logic IS tested, in the shared crates it delegates to -- netproto 79 tests,
netipc 41, netring 9". That sentence is the reason netstack is allowed to have
no tests of its own. It was true about the tests EXISTING and false about them
RUNNING, so the largest crate in lane B's bare-metal tree was resting on a
suite no command executed. All 188 pass today, which is the good case and also
the dangerous one: nothing would have reported it if they had stopped.

THE LIST IS DERIVED, NOT WRITTEN DOWN. The subjects come from `[workspace]
exclude` in the root Cargo.toml, expanded against the disk. Hardcoding the four
names would rebuild the original defect one level up -- the next crate somebody
excludes would be invisible again, and the gate would keep reporting a clean
run while covering less of the tree every month. That is precisely how the four
got here: gate 40's list is hardcoded, correctly, for the question it asks.

Exit codes:
    0    every excluded crate that has tests passed them
    1    a test failed (the failures are printed)
    2    discovery found no crate to run -- the measurement is missing, which
         is not the same as a pass, and must not read like one
"""
import argparse
import re
import subprocess
import sys
import tomllib
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent
MANIFEST = REPO_ROOT / "Cargo.toml"

# The host triple the rest of this project tests on. Named explicitly because
# these crates sit under the repo-root `.cargo/config.toml`, whose `[build]`
# target is the bare-metal one the kernel uses; a bare `cargo test` here builds
# for x86_64-unknown-none and cannot link a test harness.
HOST_TARGET = "x86_64-pc-windows-gnu"

# Excluded paths that are deliberately NOT subjects, each with the reason.
# Same principle as the IGNORE table in scripts/raced-globals.py: a line here
# says *why* and can be argued with, where a silent skip only says "not this
# one". Keyed by the exclude entry as it is spelled in Cargo.toml.
NOT_SUBJECTS: dict[str, str] = {
    "nushell": "self-contained upstream workspace with its own member list and lockfile; built via `cd nushell && cargo build`",
    "userspace/cylance-cli": "empty directory, incomplete crate; the manifest says skip until populated",
    "apps/.cargo": "a cargo config directory caught by the `apps/*` glob, not a crate",
    "gui/.cargo": "a cargo config directory caught by the `gui/*` glob, not a crate",
    "init/.cargo": "a cargo config directory caught by the `init/*` glob, not a crate",
    "net/.cargo": "a cargo config directory caught by the `net/*` glob, not a crate",
    "userspace/.cargo": "a cargo config directory caught by the `userspace/*` glob, not a crate",
}

# A `#[test]` on its own line. Anchored so a commented-out one, a doc-comment
# example and the word inside a string do not count -- this number decides
# whether a crate is a subject at all, so overcounting invents a subject with
# nothing to run and undercounting drops a real one.
_TEST_ATTR = re.compile(r"^\s*#\[test\]\s*$", re.MULTILINE)
_PACKAGE = re.compile(r"^\s*\[package\]", re.MULTILINE)


def parse_exclude(manifest_text: str) -> list[str]:
    """The `[workspace] exclude` list, in the order the manifest gives it."""
    data = tomllib.loads(manifest_text)
    ws = data.get("workspace") or {}
    return [str(e) for e in ws.get("exclude", [])]


def is_crate(rel: str, read) -> bool:
    """Does `rel/Cargo.toml` declare a `[package]`?

    `read` is a callable returning the file's text or None, so the self-test
    can drive this without a filesystem.
    """
    text = read(f"{rel}/Cargo.toml")
    return text is not None and _PACKAGE.search(text) is not None


def expand(entry: str, read, listdir) -> list[str]:
    """One exclude entry -> the crate paths under it.

    An entry is usually a crate. It may also be a DIRECTORY of crates --
    `"services"` excludes six of them at once -- which is the case that made
    this worth writing as a function: an expansion that stopped at the entry
    would find no `[package]` in `services/`, report nothing, and look exactly
    like an entry with nothing to run.
    """
    if is_crate(entry, read):
        return [entry]
    return [f"{entry}/{c}" for c in sorted(listdir(entry) or [])
            if is_crate(f"{entry}/{c}", read)]


def count_tests(sources: list[str]) -> int:
    return sum(len(_TEST_ATTR.findall(s)) for s in sources)


def _read(rel: str):
    try:
        return (REPO_ROOT / rel).read_text(encoding="utf-8", errors="replace")
    except OSError:
        return None


def _listdir(rel: str) -> list[str]:
    p = REPO_ROOT / rel
    return [c.name for c in p.iterdir() if c.is_dir()] if p.is_dir() else []


def crate_sources(rel: str) -> list[str]:
    p = REPO_ROOT / rel / "src"
    if not p.is_dir():
        return []
    return [f.read_text(encoding="utf-8", errors="replace")
            for f in sorted(p.rglob("*.rs"))]


def run_tests(rel: str, verbose: bool) -> tuple[bool, str]:
    """Run one crate's tests on the host target. Returns (passed, summary)."""
    cmd = [
        "cargo", "test",
        "--manifest-path", str(REPO_ROOT / rel / "Cargo.toml"),
        "--target", HOST_TARGET,
        # Into the canonical cache, never a new one: an excluded crate builds
        # into `<crate>/target/` by default, and four of those are four leaked
        # build directories nobody goes back for.
        "--target-dir", str(REPO_ROOT / "target"),
    ]
    # No pipeline: the status read here has to be cargo's own. A `| tail` would
    # report the tail's success for a failing run, which is the trap
    # scripts/workspace-test.py is written around.
    proc = subprocess.run(cmd, cwd=REPO_ROOT, capture_output=True,
                          text=True, errors="replace", check=False)
    out = proc.stdout + proc.stderr
    if verbose:
        print(out)
    summary = next((ln for ln in out.splitlines()
                    if ln.startswith("test result:")), "<no result line>")
    # Both halves: cargo's status AND a result line that says ok. A crate that
    # fails to compile exits non-zero with no result line at all, and a harness
    # that never ran exits zero with none either.
    ok = proc.returncode == 0 and summary.startswith("test result: ok.")
    return ok, (summary if ok else f"{summary}   (cargo exit {proc.returncode})")


def selftest() -> int:
    failures: list[str] = []
    rules: list[str] = []
    current = ""

    def rule(name: str) -> None:
        nonlocal current
        current = name
        rules.append(name)

    def expect(label: str, got, want) -> None:
        if got != want:
            failures.append(f"{current}| {label}: want {want!r}, got {got!r}")

    # 1. The list comes out of the manifest, in order, and ignores `members`.
    rule("exclude is parsed from the manifest")
    got = parse_exclude(
        '[workspace]\n'
        'members = ["kernel", "apps/*"]\n'
        'exclude = ["services", "netproto", "nushell"]\n'
    )
    expect("entries", got, ["services", "netproto", "nushell"])
    expect("a manifest with no exclude", parse_exclude("[workspace]\n"), [])

    # 2. Expansion. A directory of crates yields the crates, not the directory
    # -- the case that makes `services` six subjects rather than a silent zero.
    rule("a directory entry expands to the crates inside it")
    files = {
        "netproto/Cargo.toml": '[package]\nname = "netproto"\n',
        "services/hello/Cargo.toml": '[package]\nname = "hello"\n',
        "services/init/Cargo.toml": '[package]\nname = "init"\n',
        "services/notes/README.md": "not a crate",
    }
    read = files.get
    dirs = {"services": ["hello", "init", "notes"]}
    expect("a plain crate is itself", expand("netproto", read, dirs.get), ["netproto"])
    expect("a directory expands", expand("services", read, dirs.get),
           ["services/hello", "services/init"])
    # The control: a subdirectory with no [package] is not invented as a crate.
    expect("a non-crate subdirectory is dropped",
           "services/notes" in expand("services", read, dirs.get), False)
    # ...and an entry that is neither is empty rather than an error.
    expect("an entry that is not a crate or a dir", expand("gone", read, dirs.get), [])

    # 3. Counting decides whether a crate is a subject, so both directions of a
    # miscount matter. The commented-out and doc-comment forms are the ones
    # that actually appear in this tree.
    rule("only a real #[test] attribute counts")
    expect("two real ones", count_tests(["#[test]\nfn a() {}\n    #[test]\nfn b() {}\n"]), 2)
    expect("commented out", count_tests(["// #[test]\nfn a() {}\n"]), 0)
    expect("doc example", count_tests(["/// #[test]\nfn a() {}\n"]), 0)
    expect("inside a string", count_tests(['let s = "#[test]";\n']), 0)
    expect("across files", count_tests(["#[test]\n", "#[test]\n", ""]), 2)
    # The control for the anchor: a real attribute indented inside a module
    # still counts, or `services/*` would read as "no tests" for the wrong
    # reason and this gate would agree with itself by accident.
    expect("indented in a mod",
           count_tests(["mod t {\n        #[test]\n        fn a() {}\n}\n"]), 1)

    for f in failures:
        print(f"FAIL {f}", file=sys.stderr)
    bad = {f.split("|", 1)[0] for f in failures}
    print(f"selftest: {len(rules) - len(bad)}/{len(rules)} rules ok")
    return 1 if failures else 0


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("--selftest", "--self-test", action="store_true", dest="selftest")
    ap.add_argument("--list", action="store_true",
                    help="print the discovered subjects and exit 0")
    ap.add_argument("-v", "--verbose", action="store_true",
                    help="print each cargo invocation's full output")
    args = ap.parse_args()

    if args.selftest:
        return selftest()

    manifest = _read("Cargo.toml")
    if manifest is None:
        print(f"[excluded-tests] cannot read {MANIFEST}", file=sys.stderr)
        return 2

    entries = parse_exclude(manifest)
    subjects: list[str] = []
    skipped: list[tuple[str, str]] = []
    for e in entries:
        if e in NOT_SUBJECTS:
            skipped.append((e, NOT_SUBJECTS[e]))
            continue
        found = expand(e, _read, _listdir)
        if not found:
            skipped.append((e, "no [package] found under this entry"))
        subjects.extend(found)

    counts = {c: count_tests(crate_sources(c)) for c in subjects}
    with_tests = [(c, n) for c, n in counts.items() if n]
    build_only = [c for c, n in counts.items() if not n]

    print(f"[excluded-tests] exclude entries: {len(entries)}"
          f"   crates: {len(subjects)}"
          f"   with tests: {len(with_tests)}")
    for e, why in skipped:
        print(f"    skip {e}: {why}")
    if build_only:
        print("    build-only (0 tests; push gate 40 compiles these): "
              f"{', '.join(build_only)}")

    if args.list:
        for c, n in with_tests:
            print(f"    {c}: {n} test(s)")
        return 0

    # The same trap scripts/workspace-test.py names fourth: a run with nothing
    # to report is indistinguishable from a run with nothing wrong. If the
    # exclude list stops yielding testable crates -- a renamed directory, a
    # manifest reshuffle, a `[package]` that moved -- this gate would print a
    # clean verdict having executed nothing.
    if not with_tests:
        print("[excluded-tests] NO EXCLUDED CRATE HAS TESTS - this is not a pass.")
        print("    Four did on 2026-09-15 (netproto, netipc, netring, tzrules,")
        print("    188 between them). Check the exclude list and the paths.")
        return 2

    failed: list[str] = []
    total = 0
    for crate, n in with_tests:
        ok, summary = run_tests(crate, args.verbose)
        total += n
        print(f"    {'ok  ' if ok else 'FAIL'} {crate:<12} {summary}")
        if not ok:
            failed.append(crate)

    if failed:
        print(f"[excluded-tests] {len(failed)} crate(s) FAILED: {', '.join(failed)}")
        print("    Re-run one with:")
        print(f"      cargo test --manifest-path {failed[0]}/Cargo.toml"
              f" --target {HOST_TARGET} --target-dir target")
        return 1

    print(f"[excluded-tests] PASS - {total} test(s) across "
          f"{len(with_tests)} excluded crate(s)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
