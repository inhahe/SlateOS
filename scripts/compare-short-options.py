"""Compare each tool's short->long option mapping against the real program.

A REPORT, NOT A GATE, and it cannot become one: the oracle is the reference
program's own `--help`, which lives outside this repository. On this machine
the references come from WSL. A build host without them would have nothing to
compare against, so wiring this into `scripts/hooks/pre-push` would produce a
check that passes by being unable to look.

WHY IT EXISTS. `check-help-vs-parser.py` compares our help text against our
parser; `check-fields-written-never-read.py` finds fields nothing reads.
Neither can see a short option bound to the WRONG long option, because the
tool is internally consistent -- the help and the parser agree with each
other and both differ from the program being replaced.

`blkid -n` was `--no-encoding` here and is `--match-types` in util-linux, so
`blkid -n vfat,ext3 /dev/sda1` set a no-op flag, consumed `vfat,ext3` as a
DEVICE PATH, and reported an ext2 filesystem the caller had excluded. That is
the shape this looks for: not a missing option, which fails visibly with
"unknown option", but a silently redefined one.

    python scripts/compare-short-options.py            # every crate
    python scripts/compare-short-options.py blkid ss   # named crates
    python scripts/compare-short-options.py --selftest

Exit status is 0 whatever it finds; it is a report.
"""

import re
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent

# `"-x" | "--long"` and `"--long" | "-x"`, the two orders this tree writes.
OURS_A = re.compile(r'"(-[A-Za-z0-9])"\s*\|\s*"(--[a-z0-9][a-z0-9-]*)"')
OURS_B = re.compile(r'"(--[a-z0-9][a-z0-9-]*)"\s*\|\s*"(-[A-Za-z0-9])"')

# `-x, --long` as every util-linux/GNU help formats it. The long name may be
# followed by an argument spec (`<file>`, `[=<dir>]`, `=NAME`), which stops
# the match rather than joining it.
THEIRS = re.compile(r"(?:^|\s)(-[A-Za-z0-9]),\s+(--[a-z0-9][a-z0-9-]*)")

# `progname.ends_with("umount")` and friends: how this tree spells "which
# program am I being run as".
PERSONALITY = re.compile(r'ends_with\("([a-z][a-z0-9_-]*)"\)')


def personalities(crate_name: str, source: str, cargo: str) -> list[str]:
    """Every program this one crate implements.

    A MULTI-PERSONALITY CRATE WAS THIS CHECK'S FIRST FALSE POSITIVE, and the
    correction is worth keeping. `userspace/mount` is both `mount` and
    `umount`; comparing its whole source against `mount --help` reported
    `-f` as `--force` here versus `--fake` upstream, and called it the most
    severe finding of the sweep -- a dry-run flag that performs the action.

    It was wrong. Those arms are in the `umount` branch, and umount(8) really
    does define `-f, --force` and `-l, --lazy`. The binding was correct and
    the checker was comparing it against the wrong program.

    So: collect every name the crate answers to, and treat a short option as
    mis-bound only if it disagrees with EVERY reference that defines it.
    """
    names = {crate_name}
    names.update(PERSONALITY.findall(source))
    names.update(re.findall(r'^name\s*=\s*"([a-z][a-z0-9_-]*)"', cargo, re.M))
    # Only plausible command names: this tree also writes `ends_with(".rs")`
    # and similar, which are suffixes rather than programs.
    return sorted(n for n in names if not n.startswith(".") and len(n) > 1)


def ours(source: str) -> dict[str, str]:
    """Short -> long, as this tree binds them."""
    found: dict[str, str] = {}
    for short, long in OURS_A.findall(source):
        found.setdefault(short, long)
    for long, short in OURS_B.findall(source):
        found.setdefault(short, long)
    return found


def theirs(help_text: str) -> dict[str, str]:
    """Short -> long, as the reference's own `--help` states them."""
    found: dict[str, str] = {}
    for short, long in THEIRS.findall(help_text):
        found.setdefault(short, long)
    return found


def compare(mine: dict[str, str], refs: list[dict[str, str]]) -> list[tuple[str, str, str]]:
    """Shorts bound differently in ALL references that define them.

    Only shorts present in both: a short we do not have is a missing feature
    and a short they do not have is an extension, and neither can silently
    mean the wrong thing, because an unknown option is refused out loud.

    `refs` is a list because one crate may be several programs. A short that
    matches ANY of them is correct for that personality and is not reported.
    """
    out = []
    for short, long in sorted(mine.items()):
        defined = [r[short] for r in refs if short in r]
        if defined and long not in defined:
            out.append((short, long, "/".join(sorted(set(defined)))))
    return out


def reference_help(tool: str) -> str | None:
    """`<tool> --help` from WSL, or None if the tool is not there."""
    try:
        proc = subprocess.run(
            ["wsl", "-d", "Ubuntu", "--", "bash", "-lc",
             f"command -v {tool} >/dev/null 2>&1 && {tool} --help 2>&1 || true"],
            capture_output=True, text=True, timeout=30,
        )
    except (OSError, subprocess.SubprocessError):
        return None
    text = proc.stdout
    return text if text.strip() else None


def selftest() -> int:
    """The comparator must be able to say NO, or its silence means nothing."""
    cases = 0

    # Extraction, both orders this tree writes.
    assert ours('"-n" | "--match-types" =>') == {"-n": "--match-types"}
    assert ours('"--match-types" | "-n" =>') == {"-n": "--match-types"}
    cases += 2

    # Reference extraction, with and without an argument spec.
    assert theirs(" -d, --no-encoding   don't encode") == {"-d": "--no-encoding"}
    assert theirs(" -n, --match-types <list>  filter") == {"-n": "--match-types"}
    assert theirs(" -m, --mount[=<file>] unshare mounts") == {"-m": "--mount"}
    cases += 3

    # THE ONE THAT MATTERS: the real blkid regression must be reported.
    mine = {"-n": "--no-encoding", "-c": "--cache-file"}
    ref = {"-n": "--match-types", "-d": "--no-encoding", "-c": "--cache-file"}
    assert compare(mine, [ref]) == [("-n", "--no-encoding", "--match-types")], (
        "the comparator failed to flag the blkid -n collision it exists for"
    )
    cases += 1

    # And it must NOT cry wolf on the cases that are fine.
    assert compare({"-c": "--cache-file"}, [ref]) == [], "identical binding flagged"
    assert compare({"-z": "--zeta"}, [ref]) == [], "a short they lack is not a collision"
    assert compare({}, [ref]) == [], "no options, no findings"
    cases += 3

    # THE FALSE POSITIVE THIS CHECK ACTUALLY PRODUCED. `userspace/mount` is
    # both mount and umount; `-f` is `--fake` in one and `--force` in the
    # other, and ours is umount's. Matching EITHER personality clears it.
    mount_ref = {"-f": "--fake", "-l": "--show-labels"}
    umount_ref = {"-f": "--force", "-l": "--lazy"}
    assert compare({"-f": "--force"}, [mount_ref, umount_ref]) == [], (
        "a binding correct for one personality must not be reported"
    )
    # ...but a binding wrong for BOTH still is.
    assert compare({"-f": "--frobnicate"}, [mount_ref, umount_ref]) == [
        ("-f", "--frobnicate", "--fake/--force")
    ], "a binding wrong for every personality must still be reported"
    cases += 2

    # Personality extraction.
    assert "umount" in personalities("mount", 'progname.ends_with("umount")', "")
    assert personalities("ss", "", 'name = "sockstat"') == ["sockstat", "ss"]
    cases += 2

    print(f"selftest: {cases}/{cases} cases pass")
    return 0


def main(argv: list[str]) -> int:
    if "--selftest" in argv:
        return selftest()

    names = [a for a in argv if not a.startswith("-")]
    crates = (
        [ROOT / "userspace" / n for n in names]
        if names
        else sorted((ROOT / "userspace").iterdir())
    )

    checked = cleared = 0
    findings: list[tuple[str, str, str, str]] = []
    no_reference: list[str] = []

    for crate in crates:
        main_rs = crate / "src" / "main.rs"
        if not main_rs.is_file():
            continue
        source = main_rs.read_text(encoding="utf-8", errors="replace")
        mine = ours(source)
        if not mine:
            continue
        cargo_path = crate / "Cargo.toml"
        cargo = cargo_path.read_text(encoding="utf-8", errors="replace") if cargo_path.is_file() else ""
        refs = []
        for name in personalities(crate.name, source, cargo):
            help_text = reference_help(name)
            if help_text is not None:
                refs.append(theirs(help_text))
        if not refs:
            no_reference.append(crate.name)
            continue
        checked += 1
        bad = compare(mine, refs)
        if bad:
            for short, long, other in bad:
                findings.append((crate.name, short, long, other))
        else:
            cleared += 1

    if findings:
        print(f"=== short options bound differently from the reference ({len(findings)}) ===")
        for tool, short, long, other in findings:
            print(f"  {tool}: {short} is {long} here, {other} upstream")
    else:
        print("no short option is bound differently from its reference")

    print()
    print(f"-- {checked} crate(s) compared, {cleared} clear, "
          f"{len(no_reference)} with no reference available.")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
