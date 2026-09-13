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

BASELINE, 2026-09-13: **empty**. Lane B fixed both writers (`logind`'s SYSTEM
faillock at `/var/run/authlib/tally`, and `udevd`), and a full `cargo test
--workspace` against a cleared root now leaves it clear -- 581 targets, pass,
nothing written. So a non-empty report from this script is a NEW writer, not a
known list to triage past. That is a much stronger thing for it to say than it
could say when it was written.

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
#
# `boot` is in the list only because the scan below is case-SENSITIVE. Windows
# spells its own boot store `E:\Boot`, and NTFS is case-insensitive but
# case-preserving, so `Path("E:/boot").is_dir()` answers True for it -- which
# is how this tool would have reported the machine's boot configuration as test
# litter. Lane B hit exactly that in their own version and said so
# (`.git/coordination/notice-c-b-20260913T194019Z.md`); this one had no
# instance yet because `boot` was not in the list, which is luck rather than
# design. A Rust `create_dir("/boot")` produces lowercase `boot`, so comparing
# the real on-disk spelling separates the two exactly.
POSIX_ROOTS = (
    "etc", "sys", "var", "run", "dev", "usr", "proc", "opt", "srv", "boot",
)


def litter(drive: str) -> list[tuple[pathlib.Path, list[str]]]:
    r"""Every POSIX root present on `drive`, with a sample of what is inside.

    Reads the directory and compares the names it reports, rather than asking
    whether a constructed path exists. On Windows those are different
    questions: the filesystem folds case when *resolving* a path, so
    `Path("E:/boot").is_dir()` is True on a machine whose boot store is
    `E:\Boot` -- and the tool would then report Windows' own directory as
    litter left by a test. Listing gives the real spelling, and a test that
    writes `/boot` writes `boot`.
    """
    try:
        present = {entry.name: entry for entry in pathlib.Path(drive + "/").iterdir()}
    except OSError:
        # No such drive, or not readable. Absent is not an offence.
        return []
    found = []
    for name in POSIX_ROOTS:
        entry = present.get(name)
        if entry is None or not entry.is_dir():
            continue
        try:
            inside = sorted(p.name for p in entry.iterdir())
        except OSError:
            # Unreadable is not the same as absent, and a permission error here
            # is itself worth seeing rather than swallowing.
            inside = ["<unreadable>"]
        found.append((entry, inside[:6]))
    return found


def _self_test() -> int:
    """The parts that can be checked without a drive full of litter."""
    cases = []
    cases.append(("the root list is not empty", len(POSIX_ROOTS) > 0))
    cases.append(("tmp is excluded on purpose", "tmp" not in POSIX_ROOTS))
    cases.append(("etc is included", "etc" in POSIX_ROOTS))
    # Lane B's finding, as a fixture. `E:/Boot` is Windows' boot store on this
    # machine; `E:/boot` is what a test writing `/boot` would leave. The scan
    # must tell them apart, and `is_dir` on a constructed path cannot.
    boot = pathlib.Path("E:/Boot")
    if boot.is_dir():
        cases.append(
            (
                "Windows' own Boot store is not reported as litter",
                not any(e.name == "boot" for e, _ in litter("E:")),
            )
        )
        cases.append(
            (
                "and a constructed lowercase path would have matched it",
                pathlib.Path("E:/boot").is_dir(),
            )
        )
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
