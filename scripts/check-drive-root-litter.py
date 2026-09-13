"""Report POSIX-looking directories at the drive root, which make runs lie.

On Windows a path beginning with `/` is **drive-relative**, not absolute:
`Path::new("/etc/passwd")` opens `E:\\etc\\passwd` when the current drive is
`E:`. A test that writes such a path therefore does not fail harmlessly on the
dev host -- it succeeds, and leaves a real directory at the root of the
operator's data drive, where the next run finds it.

WHAT THIS COSTS, measured on 2026-09-13. Three `cargo test --workspace` runs of
the same tree, hours apart, returned:

    25 484 passed,  2 failed   -- `userspace/systemctl`, after something
                                  created `E:/sys/fs/cgroup`
    60 638 passed,  0 failed   -- with `E:/sys` deleted
    60 533 passed,  6 failed   -- `init/loginmgr`, after something created
                                  `E:/etc/passwd`, `shadow` and `users.yaml`

Neither crate had a bug. `cargo test -p systemctl` alone is 154 passed and
`cargo test -p loginmgr` alone is 46 passed, and neither recreates the
directory that broke it. The failures belong to whichever *other* crate wrote
the path first, and which that is depends on scheduling.

So the result of a workspace run on this machine is not a property of the tree
alone. This tells you, in one command, whether a red run is real.

DELIBERATELY DOES NOT DELETE ANYTHING BY DEFAULT. `E:/etc` is test debris and
`E:/etc` is also a plausible thing for a human to have made on purpose, and
this cannot tell them apart. `--clean` removes them; without it this only
reports, and exits 1 so a script can branch on it. The one irreversible act is
kept behind an explicit flag.

THE REAL FIX IS NOT HERE. It is for the tests that write these paths to take
their root as a parameter and point it at a scratch directory. That is
`userspace/**` and `init/**`, which lane C must not write -- see
`requests/c-b-tests-create-real-directories-at-the-drive-root.md` and
`TD-C-A-TEST-THAT-WRITES-TO-AN-ABSOLUTE-POSIX-PATH-WRITES-TO-THE-DEV-DRIVE-ROOT`.
"""

import pathlib
import shutil
import sys

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))
import selftestflag  # noqa: E402

NL = chr(10)

# The roots a POSIX program reaches for. `/tmp` is deliberately absent: this
# tree's own scratch helper uses the real temp directory, and `E:/tmp` is a
# normal thing for a developer to keep, so flagging it would be noise in the
# one report that has to stay worth reading.
POSIX_ROOTS = ("etc", "sys", "var", "run", "dev", "usr", "proc", "opt", "srv")


def litter(drive: str) -> list[tuple[pathlib.Path, list[str]]]:
    """Every POSIX root present on `drive`, with a sample of what is inside."""
    found = []
    for name in POSIX_ROOTS:
        path = pathlib.Path(f"{drive}/{name}")
        try:
            if not path.is_dir():
                continue
            inside = sorted(p.name for p in path.iterdir())
        except OSError:
            # Unreadable is not the same as absent, and a permission error here
            # is itself worth seeing rather than swallowing.
            inside = ["<unreadable>"]
        found.append((path, inside[:6]))
    return found


def _self_test() -> int:
    """The parts that can be checked without a drive full of litter."""
    cases = []
    cases.append(("the root list is not empty", len(POSIX_ROOTS) > 0))
    cases.append(("tmp is excluded on purpose", "tmp" not in POSIX_ROOTS))
    cases.append(("etc is included", "etc" in POSIX_ROOTS))
    # A drive that cannot exist has no litter, which also proves the scan
    # tolerates a missing drive rather than raising.
    cases.append(("a nonexistent drive yields nothing", litter("Q:") == []))
    bad = 0
    for why, ok in cases:
        if not ok:
            print(f"SELF-TEST FAIL: {why}", file=sys.stderr)
            bad += 1
    if bad:
        return 1
    print(f"self-test ok -- {len(cases)} case(s)")
    return 0


def main(argv) -> int:
    if selftestflag.wants_selftest(argv):
        return _self_test()
    unknown = selftestflag.unknown_options(argv, known=("--clean",))
    if unknown:
        print("unrecognised option(s): " + " ".join(unknown), file=sys.stderr)
        return 2
    clean = "--clean" in argv

    drive = pathlib.Path.cwd().drive or "E:"
    found = litter(drive)
    if not found:
        print(f"ok -- no POSIX-looking directories at the root of {drive}")
        return 0

    print(f"POSIX-looking directories at the root of {drive}:", file=sys.stderr)
    for path, inside in found:
        listing = ", ".join(inside) if inside else "(empty)"
        print(f"  {path}  --  {listing}", file=sys.stderr)

    if not clean:
        print(
            NL + "A workspace test run on this machine is not repeatable while "
            "these exist: the" + NL + "same tree returned 0, 2 and 6 failures "
            "on three runs today depending on what was" + NL + "left here. If a "
            "run just went red, re-check it after:" + NL
            + NL + "    python scripts/check-drive-root-litter.py --clean" + NL
            + NL + "They are written by tests that open a path beginning with "
            "'/', which on Windows" + NL + "is drive-relative rather than "
            "absolute. The real fix is in those tests; see" + NL
            + "requests/c-b-tests-create-real-directories-at-the-drive-root.md.",
            file=sys.stderr,
        )
        return 1

    removed = 0
    for path, _inside in found:
        try:
            shutil.rmtree(path)
            print(f"removed {path}")
            removed += 1
        except OSError as exc:
            print(f"could not remove {path}: {exc}", file=sys.stderr)
    if removed != len(found):
        return 1
    print(f"{removed} director(ies) removed; the next run starts from the tree alone.")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
