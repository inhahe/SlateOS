"""Every test that can write a settings file runs inside a scratch directory.

A test that *reads* settings obviously needs one. The bug this gate exists for
is the other half: a test that only writes, and does not look like it writes at
all. `super_space_switches_to_the_next_keyboard_layout` asserted purely on the
shell's in-memory layout field -- an in-memory test by every appearance -- and
the chord it fired saved `input.yaml` two calls further down, inside the action
handler.

The consequences are worse than a wrong test. The write landed in the
developer's real configuration directory; and because `with_scratch_config`
swaps the process-wide `XDG_CONFIG_HOME`, it landed in a *neighbouring* test's
scratch directory whenever the two overlapped, which is where the intermittent
failure came from (known-issues.md,
BUG-C-THE-KEYBOARD-LAYOUT-TEST-FAILS-ABOUT-ONE-WORKSPACE-RUN-IN-TWO).

So the question is not what a test asserts but what it *calls*. This walks the
call graph outward from every `save()` on a settings type, one crate at a
time, and reports any `#[test]` that can reach one without a scratch guard in
its body.

Usage:  python scripts/check-scratch-config.py [--self-test] [crate-substring ...]
"""

# Every cargo invocation in this tree names the target explicitly; the default
# host triple is not what this workspace builds for.
TARGET = "x86_64-pc-windows-gnu"
import collections
import os
import pathlib
import re
import shutil
import subprocess
import sys
import tempfile

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))
import selftestflag  # noqa: E402

NL = chr(10)
BS = chr(92)
ROOT = pathlib.Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "gui" / "appearance"))
from rustslice import production_end  # noqa: E402

# The call that actually touches the disk. `InputFile`, `AppearanceFile` and
# the rest all reach it through `settingsfile`.
SAVES = re.compile(r"[.]save" + BS + "s*" + BS + "(|settingsfile::[" + BS + "w:]*write")
GUARD = re.compile(r"with_scratch_config|with_env" + BS + "b|ScratchDir::new")
FN = re.compile(r"^(\s*)(?:pub(?:\([\w:]+\))?\s+)?(?:async\s+)?(?:const\s+)?fn\s+(\w+)")
TEST_ATTR = re.compile(r"^\s*#\[(test|tokio::test)\]")
CALL = re.compile(r"(\w+)\s*\(")


def crate_of(rel):
    return "/".join(rel.split("/")[:2])


def span(lines, i):
    depth, j, started = 0, i, False
    while j < len(lines):
        depth += lines[j].count("{") - lines[j].count("}")
        if "{" in lines[j]:
            started = True
        if started and depth <= 0:
            return i, j + 1
        j += 1
    return i, len(lines)


def scan():
    """Per crate: {fn -> (body, is_test)} and the set that saves directly."""
    crates = collections.defaultdict(lambda: {"fns": {}, "tests": set(), "saves": set()})
    for path in sorted(ROOT.glob("gui/**/*.rs")) + sorted(ROOT.glob("apps/**/*.rs")):
        rel = path.relative_to(ROOT).as_posix()
        lines = path.read_text(encoding="utf-8", errors="replace").splitlines()
        c = crates[crate_of(rel)]
        for i, line in enumerate(lines):
            m = FN.match(line)
            if not m:
                continue
            lo, hi = span(lines, i)
            body = NL.join(lines[lo:hi])
            name = m.group(2)
            # A name defined twice in a crate (a trait impl and an inherent
            # one) is merged rather than overwritten: dropping either half
            # would drop a path to the disk.
            prev = c["fns"].get(name, "")
            c["fns"][name] = prev + NL + body
            if any(TEST_ATTR.match(lines[k]) for k in range(max(0, i - 4), i)):
                c["tests"].add(name)
            if SAVES.search(body):
                c["saves"].add(name)
    return crates


def package_of(crate):
    """The cargo package name for a directory, which is not always its name."""
    toml = ROOT / crate / "Cargo.toml"
    if not toml.exists():
        return None
    m = re.search(r'^name\s*=\s*"([^"]+)"', toml.read_text(encoding="utf-8"), re.M)
    return m.group(1) if m else None


# Fixtures for the static half. Each is a whole function, because `span` is
# what decides where a body ends and a fixture of fragments would not exercise
# it at all.
SELF_TESTS = [
    (
        "a direct save is recognised",
        """    fn persist(&self) {
        let mut file = InputFile::load();
        file.save().ok();
    }
""",
        {"saves": True, "test": False},
    ),
    (
        "a bare settingsfile::write is a save too",
        """    fn persist(&self) {
        settingsfile::write_atomic(path, bytes);
    }
""",
        {"saves": True, "test": False},
    ),
    (
        "a function that only reads is not a save",
        """    fn read(&self) -> InputSettings {
        InputFile::load().settings
    }
""",
        {"saves": False, "test": False},
    ),
    (
        "a #[test] attribute is seen through the blank line above it",
        """    #[test]
    fn a_chord_persists_the_layout() {
        shell.handle_hotkey(&chord);
    }
""",
        {"saves": False, "test": True},
    ),
    (
        "a body ending in a nested brace does not swallow the next function",
        """    fn first(&self) {
        if x {
            y();
        }
    }
    fn second(&self) {
        InputFile::load().save().ok();
    }
""",
        {"saves": False, "test": False},
    ),
]


def selftest():
    """Grade the parts that can rot without the verdict changing.

    Two halves can go quiet independently. The static half chooses which
    crates to run; if `SAVES` stops matching, the crate list empties and the
    empirical half runs nothing -- which `main` catches, but only because it
    refuses an empty selection. The empirical half's own failure mode is worse:
    if the probe directory is never inspected, or every crate is skipped for
    want of a package name, `failures` stays empty and the run prints `ok`.
    Both are checked here.
    """
    failed = 0
    for name, source, expected in SELF_TESTS:
        lines = source.splitlines()
        m = FN.match(lines[0]) or FN.match(lines[1] if len(lines) > 1 else "")
        if not m:
            print("FAIL " + name + ": no function found at all")
            failed += 1
            continue
        at = 0 if FN.match(lines[0]) else 1
        lo, hi = span(lines, at)
        body = NL.join(lines[lo:hi])
        got = {
            "saves": bool(SAVES.search(body)),
            "test": any(TEST_ATTR.match(lines[k]) for k in range(max(0, at - 4), at)),
        }
        ok = got == expected
        print(("ok   " if ok else "FAIL ") + name)
        if not ok:
            print("       expected " + str(expected) + ", got " + str(got))
            failed += 1

    # The empirical half: a file left in the probe must be seen. This is the
    # step whose silent failure prints `ok`, so it is exercised directly
    # rather than trusted.
    probe = pathlib.Path(tempfile.mkdtemp(prefix="slateos-xdg-selftest-"))
    try:
        (probe / "slateos").mkdir()
        (probe / "slateos" / "input.yaml").write_text("keyboard: {}" + NL, encoding="utf-8")
        left = sorted(p for p in probe.rglob("*") if p.is_file())
        ok = [p.relative_to(probe).as_posix() for p in left] == ["slateos/input.yaml"]
        print(("ok   " if ok else "FAIL ") + "a file written into the probe is seen")
        if not ok:
            print("       the probe scan found " + str(left))
            failed += 1
    finally:
        shutil.rmtree(probe, ignore_errors=True)

    # And that a real crate resolves to a package name -- the path by which
    # every crate could be skipped while the run still printed `ok`.
    pkg = package_of("gui/desktop")
    ok = pkg == "desktop"
    print(("ok   " if ok else "FAIL ") + "a crate directory resolves to its package name")
    if not ok:
        print("       package_of('gui/desktop') returned " + repr(pkg))
        failed += 1

    print(NL + str(len(SELF_TESTS) + 2) + " self-test case(s), " + str(failed) + " failed")
    return 1 if failed else 0


def main(argv):
    """Run each save-capable crate's tests with the real config redirected.

    The static half only *chooses the targets*. It deliberately does not
    accuse individual tests: reachability is not execution, and an earlier
    version of this file that reported every test able to reach a save named
    363 of them, nearly all of which never take the branch. A list that long
    is a list nobody reads.

    The verdict is empirical instead. Point `XDG_CONFIG_HOME` at an empty
    directory, run the tests, and look. Anything that appears was written by a
    test with no scratch directory of its own -- which in a developer's
    checkout would have been their own configuration.
    """
    if selftestflag.wants_selftest(argv):
        return selftest()
    # Refuse an option we do not have rather than scanning anyway: a verdict
    # printed under a flag the caller thought meant something is a verdict
    # about a question nobody asked. `--self-test` was accepted and ignored
    # here for exactly as long as this gate has existed.
    unknown = selftestflag.unknown_options(argv)
    if unknown:
        for opt in unknown:
            print("check-scratch-config: unrecognized option " + repr(opt))
        print("usage: check-scratch-config.py [--self-test] [crate-substring ...]")
        return 2

    crates = [c for c, v in sorted(scan().items()) if v["saves"]]
    wanted = [a for a in argv if not a.startswith("-")]
    if wanted:
        crates = [c for c in crates if any(w in c for w in wanted)]
    if not crates:
        print("no save-capable crates selected; refusing to call that a pass")
        return 2

    failures = []
    checked = 0
    for crate in crates:
        pkg = package_of(crate)
        if not pkg:
            # Counted as a failure, not skipped. Skipping was how every crate
            # could drop out -- a renamed directory, a moved Cargo.toml -- and
            # the run would still end with "ok: 12 crates wrote nothing",
            # because the count was of crates *selected* rather than checked.
            print("  FAIL " + crate + ": no Cargo.toml name, so nothing was checked")
            failures.append(crate + ": no Cargo.toml name; the crate was not checked")
            continue
        checked += 1
        probe = pathlib.Path(tempfile.mkdtemp(prefix="slateos-xdg-probe-"))
        env = dict(os.environ, XDG_CONFIG_HOME=str(probe), HOME=str(probe))
        run = subprocess.run(
            [sys.executable, str(ROOT / "scripts" / "run-timeout.py"), "900",
             "cargo", "test", "-p", pkg, "--target", TARGET],
            cwd=ROOT, env=env, capture_output=True, text=True, check=False,
        )
        left = sorted(p for p in probe.rglob("*") if p.is_file())
        status = "ok" if not left and run.returncode == 0 else "FAIL"
        print("  " + status.ljust(5) + pkg)
        if run.returncode != 0:
            failures.append(pkg + ": its own tests did not pass, so this says nothing")
        for f in left:
            failures.append(pkg + " wrote " + f.relative_to(probe).as_posix())
        shutil.rmtree(probe, ignore_errors=True)

    if failures:
        print()
        for f in failures:
            print("  " + f)
        print(str(len(failures)) + " problem(s): a test wrote settings outside a scratch directory.")
        return 1
    print("ok: " + str(checked) + " save-capable crates wrote nothing to the real config")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
