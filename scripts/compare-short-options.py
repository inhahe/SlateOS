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
    python scripts/compare-short-options.py --selftest  # or --self-test

Exit status is 0 whatever it finds; it is a report.
"""

import re
import subprocess
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import selftestflag  # noqa: E402  (needs the path above)

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


def strip_comments(source: str) -> str:
    """Remove `//` comments, so prose about options is not read as code.

    THIS CHECK REPORTED ITS OWN DOCUMENTATION. A comment added to `dmesg`
    explaining that the comparator reads `"-x" | "--long"` pairs was matched
    as an `-x` binding, and the next run duly reported `-x` as bound to
    `--long` here and `--decode` upstream. The finding was a sentence about
    findings.

    The `//` is only a comment when it is outside a string, which the parity
    of unescaped quotes before it decides. Getting that wrong can only DROP a
    binding, never invent one, so the failure direction is a missed
    comparison rather than a false alarm.
    """
    out = []
    for line in source.splitlines():
        quotes = 0
        cut = None
        i = 0
        while i < len(line):
            ch = line[i]
            if ch == "\\":
                i += 2
                continue
            if ch == '"':
                quotes += 1
            elif ch == "/" and line[i : i + 2] == "//" and quotes % 2 == 0:
                cut = i
                break
            i += 1
        out.append(line if cut is None else line[:cut])
    return "\n".join(out)


def ours(source: str) -> dict[str, str]:
    """Short -> long, as this tree binds them."""
    source = strip_comments(source)
    found: dict[str, str] = {}
    for short, long in OURS_A.findall(source):
        found.setdefault(short, long)
    for long, short in OURS_B.findall(source):
        found.setdefault(short, long)
    return found


def theirs(help_text: str) -> dict[str, set[str]]:
    """Short -> every long the reference gives it.

    A SET, because a reference may legitimately bind one letter twice.
    util-linux `mkfs` documents both

        -V, --verbose   explain what is being done
        -V, --version   display version information and exit

    and resolves them by context -- `-V` alone is `--version`, `-V` with other
    options is `--verbose`. Keeping only the first match reported our
    `-V`/`--version` as a collision when it is one of the two right answers.
    """
    found: dict[str, set[str]] = {}
    for short, long in THEIRS.findall(help_text):
        found.setdefault(short, set()).add(long)
    return found


def compare(
    mine: dict[str, str], refs: list[dict[str, set[str]]]
) -> list[tuple[str, str, str]]:
    """Shorts bound differently in EVERY reading the references allow.

    Only shorts present in both: a short we do not have is a missing feature
    and a short they do not have is an extension, and neither can silently
    mean the wrong thing, because an unknown option is refused out loud.

    Two separate reasons a short may have several right answers, and matching
    any one of them clears it:

      * the crate is several programs (`userspace/mount` is mount and
        umount), so `refs` is a list;
      * one program binds the letter twice (`mkfs -V` is both `--verbose`
        and `--version`), so each value is a set.
    """
    out = []
    for short, long in sorted(mine.items()):
        allowed: set[str] = set()
        for ref in refs:
            allowed |= ref.get(short, set())
        if allowed and long not in allowed:
            out.append((short, long, "/".join(sorted(allowed))))
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

    # A COMMENT IS NOT A BINDING. This check reported its own documentation
    # once: a comment in `dmesg` explaining that it reads `"-x" | "--long"`
    # pairs was matched as an `-x` binding.
    assert ours('// reads `"-x" | "--long"` pairs') == {}
    assert ours('    /// like `"-q" | "--quiet"` above') == {}
    # ...but a real binding on a line that also has a trailing comment stays.
    assert ours('"-q" | "--quiet" => o.q = true, // quiet') == {"-q": "--quiet"}
    # ...and a `//` inside a string does not truncate the line.
    assert ours('let u = "https://x/"; "-q" | "--quiet" =>') == {"-q": "--quiet"}
    cases += 4

    # Reference extraction, with and without an argument spec.
    assert theirs(" -d, --no-encoding   don't encode") == {"-d": {"--no-encoding"}}
    assert theirs(" -n, --match-types <list>  filter") == {"-n": {"--match-types"}}
    assert theirs(" -m, --mount[=<file>] unshare mounts") == {"-m": {"--mount"}}
    # One letter documented twice keeps BOTH.
    assert theirs(" -V, --verbose explain\n -V, --version display") == {
        "-V": {"--verbose", "--version"}
    }
    cases += 4

    # THE ONE THAT MATTERS: the real blkid regression must be reported.
    mine = {"-n": "--no-encoding", "-c": "--cache-file"}
    ref = {"-n": {"--match-types"}, "-d": {"--no-encoding"}, "-c": {"--cache-file"}}
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
    mount_ref = {"-f": {"--fake"}, "-l": {"--show-labels"}}
    umount_ref = {"-f": {"--force"}, "-l": {"--lazy"}}
    assert compare({"-f": "--force"}, [mount_ref, umount_ref]) == [], (
        "a binding correct for one personality must not be reported"
    )
    # ...but a binding wrong for BOTH still is.
    assert compare({"-f": "--frobnicate"}, [mount_ref, umount_ref]) == [
        ("-f", "--frobnicate", "--fake/--force")
    ], "a binding wrong for every personality must still be reported"
    cases += 2

    # THE mkfs FALSE POSITIVE: one program, one letter, two documented
    # meanings. Ours is the second, and must not be reported.
    mkfs_ref = {"-V": {"--verbose", "--version"}}
    assert compare({"-V": "--version"}, [mkfs_ref]) == []
    assert compare({"-V": "--verbose"}, [mkfs_ref]) == []
    assert compare({"-V": "--vorpal"}, [mkfs_ref]) == [
        ("-V", "--vorpal", "--verbose/--version")
    ]
    cases += 3

    # Personality extraction.
    assert "umount" in personalities("mount", 'progname.ends_with("umount")', "")
    assert personalities("ss", "", 'name = "sockstat"') == ["sockstat", "ss"]
    cases += 2

    print(f"selftest: {cases}/{cases} cases pass")
    return 0


def main(argv: list[str]) -> int:
    # `selftestflag`, not `"--selftest" in argv`: both spellings must reach
    # the self-test. When only one does, the other falls through to the real
    # scan and exits 0, so a mistyped invocation reports success without
    # having tested anything -- indistinguishable from a genuine pass. The
    # pre-push gate `check-selftest-flag-spellings.py` refused this script
    # until it used the helper, which is the gate working.
    if selftestflag.wants_selftest(argv):
        return selftest()

    unknown = selftestflag.unknown_options(argv)
    if unknown:
        print(f"unrecognised option(s): {', '.join(unknown)}", file=sys.stderr)
        return 2

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
