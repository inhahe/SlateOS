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
#
# The SHORT name may carry one too, and psmisc writes it that way:
#
#     -N TYPE, --ns-sort=TYPE
#     -H PID, --highlight-pid=PID
#
# Without the optional `[A-Z]+` below, the comma is not adjacent to the letter
# and the whole line was skipped -- so every option written in that style went
# UNCOMPARED. That is a false negative, which costs more than a false positive
# here: a spurious finding is a minute of checking, and a missed one is
# silent. `pstree -N` went unexamined for exactly this reason.
# The trailing `(?!\s*,\s*-)` rejects an ENUMERATION. Ubuntu's resolvconf
# ends its help with a list of options it now ignores:
#
#     -I, -i, -l, -R, -r, -v, -V, --enable-updates, --disable-updates,
#     --updates-are-enabled.
#
# which read as a definition of `-V` as `--enable-updates`. A definition is
# followed by its description; a list is followed by another option.
THEIRS = re.compile(
    r"(?:^|\s)(-[A-Za-z0-9])(?:\s+[A-Z][A-Z_]*)?,\s+(--[a-z0-9][a-z0-9-]*)"
    # `(?![a-z0-9-])` first: without it the capture BACKTRACKS to
    # `--enable-update`, leaving `s` as the next character, and the
    # enumeration guard below never fires.
    r"(?![a-z0-9-])(?!\s*,\s*-)"
)

# `progname.ends_with("umount")` and friends: how this tree spells "which
# program am I being run as".
PERSONALITY = re.compile(r'ends_with\("([a-z][a-z0-9_-]*)"\)')

# The other spelling: `match prog_name.as_str() { "mountpoint" => ... }`.
#
# Keyed on the SCRUTINEE NAME rather than on the shape of the arms, and
# deliberately so. Collecting arms from any match at all would hand the
# checker extra "personalities" whose option tables then EXCUSE real
# collisions -- a false negative, which costs more here than a false positive,
# because a missed collision is silent and a spurious one is a minute of
# checking.
# Bounded by `[^{}]` and stopped at the wildcard arm, NOT by scanning for a
# closing brace. The first version captured to the next line starting with
# `}` -- which is the enclosing FUNCTION's brace, because a match's own
# closing brace is indented. So it swallowed every later match in the same
# function.
#
# `userspace/perf` showed what that costs. Its argv[0] dispatch is fine, but
# the capture ran on into the SUBCOMMAND dispatch and harvested `stat`, `top`,
# `record` and `report` as personalities. `stat` and `top` are real programs,
# so `perf stat -e` was then compared against coreutils `stat` -- an unrelated
# tool's option table, reported as this one's defect.
DISPATCH = re.compile(
    r"match\s+(?:\w*(?:prog|argv0|personality|basename)\w*)"
    r"(?:\.as_str\(\)|\.as_ref\(\))?\s*\{([^{}]*?)_\s*=>",
    re.S | re.I,
)
DISPATCH_ARM = re.compile(r'"([a-z][a-z0-9_-]*)"\s*(?:\||=>)')

# The third spelling: `match name { "lastlog" => Personality::Lastlog, .. }`.
#
# `userspace/last` is last, lastb and lastlog, and its scrutinee is called
# plain `name` -- too generic to key on without collecting arms from every
# unrelated match in the tree. The ARM is the specific part: a string literal
# mapping to a `Personality::` variant says what it is with no ambiguity at
# all, so this pattern can be exact rather than heuristic.
PERSONALITY_ARM = re.compile(r'"([a-z][a-z0-9_-]*)"\s*=>\s*Personality::')

# The fourth spelling: `stem.contains("id")`. `userspace/mktemp` is also
# `id`, and decides that way -- neither a match arm nor an `ends_with`.
#
# Keyed on the RECEIVER, for the same reason `DISPATCH` is keyed on its
# scrutinee: `.contains()` is far too common a call to harvest string
# literals from indiscriminately.
#
# The obvious alternative -- harvesting the `Personality::` VARIANT names --
# was tried and reverted in the same sitting. `userspace/perf` has
# `Personality::Stat` and `Personality::Top` as shorthands for the multi-call
# names `perf-stat` and `perf-top`; lowercased they are `stat` and `top`,
# which are real and unrelated programs, so `perf stat -e` went straight back
# to being compared against coreutils `stat`. A variant name is a label for a
# personality, not the name the binary answers to.
PERSONALITY_CONTAINS = re.compile(
    r"(?:stem|base|basename|prog|progname|argv0|name)\.contains\("
    r'"([a-z][a-z0-9_-]*)"\)'
)


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

    `userspace/findmnt` is the same story told a second way -- it is findmnt
    and `mountpoint`, dispatching on `match prog_name.as_str()` rather than on
    `ends_with`, so the first version of this function could not see it and
    the checker reported `-d`/`--fs-devno` as wrong. mountpoint(1) defines
    exactly that.

    And a third: `userspace/last` is last, lastb and lastlog, matching on a
    scrutinee called plain `name`. `-t`/`--time` was reported wrong; lastlog
    defines `-t, --time DAYS` exactly. Three spellings of one idea, and each
    was found by checking a finding rather than acting on it.

    So: collect every name the crate answers to, and treat a short option as
    mis-bound only if it disagrees with EVERY reference that defines it.
    """
    names = {crate_name}
    names.update(PERSONALITY.findall(source))
    names.update(re.findall(r'^name\s*=\s*"([a-z][a-z0-9_-]*)"', cargo, re.M))
    for block in DISPATCH.findall(source):
        names.update(DISPATCH_ARM.findall(block))
    names.update(PERSONALITY_ARM.findall(source))
    names.update(PERSONALITY_CONTAINS.findall(source))
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


FN_START = re.compile(r"^fn\s+([a-z_][a-z0-9_]*)", re.M)


def ours(source: str) -> dict[str, str]:
    """Short -> long, as this tree binds them."""
    return {short: long for short, long, _ in ours_located(source)}


def ours_located(source: str) -> list[tuple[str, str, str]]:
    """Every binding, with the name of the function it sits in.

    THE ENCLOSING FUNCTION IS WHICH PROGRAM THE BINDING BELONGS TO, and
    without it a multi-personality crate is judged against whichever of its
    references happens to be installed. `userspace/selinux` answers to twelve
    names; only `chcon` is present on this machine, so `-r`/`--range` -- which
    lives in `semanage_login`, and is real semanage syntax -- was reported
    against chcon's `-r, --role`. Likewise `userspace/xdg`: `-n`/`--no-open`
    is in `run_xdg_open`, and was judged against `mimeopen`.

    Both were wrong, and neither was visible from the binding alone.
    """
    source = strip_comments(source)
    bounds = [(m.start(), m.group(1)) for m in FN_START.finditer(source)]

    def enclosing(at: int) -> str:
        name = ""
        for start, fn in bounds:
            if start > at:
                break
            name = fn
        return name

    out = []
    for pat, swapped in ((OURS_A, False), (OURS_B, True)):
        for m in pat.finditer(source):
            short, long = (m.group(2), m.group(1)) if swapped else (m.group(1), m.group(2))
            out.append((short, long, enclosing(m.start())))
    return out


def owning_personality(fn_name: str, names: list[str]) -> str | None:
    """The personality a function belongs to, if its name says so.

    `run_xdg_open` -> `xdg-open`, `semanage_login` -> `semanage`. Longest
    match wins, so `xdg-mime` is not mistaken for `xdg`.
    """
    spelled = fn_name.replace("_", "-")
    matches = [n for n in names if n in spelled]
    return max(matches, key=len) if matches else None


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
    # psmisc's form: an argument on the SHORT name, before the comma.
    assert theirs(" -N TYPE, --ns-sort=TYPE") == {"-N": {"--ns-sort"}}
    assert theirs(" -H PID, --highlight-pid=PID") == {"-H": {"--highlight-pid"}}
    cases += 2

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

    # An enumeration of ignored options is not a definition.
    assert theirs(" -I, -i, -v, -V, --enable-updates, --disable-updates,") == {}
    assert theirs(" -V, --version display version") == {"-V": {"--version"}}
    # `stem.contains("id")` names a program; a `Personality::` variant does
    # not -- perf's `Stat`/`Top` are shorthands for `perf-stat`/`perf-top`.
    assert "id" in personalities("mktemp", 'if stem.contains("id") {', "")
    assert "stat" not in personalities("perf", "Personality::Stat => 1,", "")
    cases += 4

    # Attribution: the enclosing function says which program a binding is.
    located = ours_located(
        'fn run_xdg_open(a: &[String]) {\n    "-n" | "--no-open" => {}\n}\n'
        'fn run_mimeopen(a: &[String]) {\n    "-v" | "--verbose" => {}\n}\n'
    )
    assert ("-n", "--no-open", "run_xdg_open") in located
    assert ("-v", "--verbose", "run_mimeopen") in located
    assert owning_personality("run_xdg_open", ["xdg", "xdg-open", "mimeopen"]) == "xdg-open"
    assert owning_personality("semanage_login", ["chcon", "semanage"]) == "semanage"
    assert owning_personality("do_thing", ["chcon", "semanage"]) is None
    cases += 7

    # Personality extraction, all three spellings this tree uses.
    assert "umount" in personalities("mount", 'progname.ends_with("umount")', "")
    assert personalities("ss", "", 'name = "sockstat"') == ["sockstat", "ss"]
    dispatch = (
        "    match prog_name.as_str() {\n"
        '        "mountpoint" => cmd_mountpoint(&rest),\n'
        "        _ => cmd_findmnt(&rest),\n"
        "    }\n}"
    )
    assert "mountpoint" in personalities("findmnt", dispatch, "")
    # The `Personality::` arm form, whose scrutinee is too generic to key on.
    enum_arms = '    match name {\n        "lastlog" => Personality::Lastlog,\n    }'
    assert "lastlog" in personalities("last", enum_arms, "")
    # A match on something that is NOT the program name must contribute
    # nothing: extra personalities would excuse real collisions.
    other = (
        "    match direction.as_str() {\n"
        '        "backward" => go_back(),\n'
        "        _ => go_forward(),\n"
        "    }\n}"
    )
    assert personalities("findmnt", other, "") == ["findmnt"]

    # A SECOND match in the same function must not be swallowed by the first.
    two = (
        "fn main() {\n"
        "    match prog_name.as_str() {\n"
        '        "mountpoint" => a(),\n'
        "        _ => b(),\n"
        "    }\n"
        "    match sub.as_str() {\n"
        '        "stat" => c(),\n'
        '        "top" => d(),\n'
        "        _ => e(),\n"
        "    }\n"
        "}"
    )
    got = personalities("perf", two, "")
    assert "mountpoint" in got, got
    assert "stat" not in got and "top" not in got, (
        f"subcommands harvested as personalities: {got}"
    )
    cases += 5

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

    checked = cleared = unjudged = 0
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
        names = personalities(crate.name, source, cargo)
        tables: dict[str, dict[str, set[str]]] = {}
        for name in names:
            help_text = reference_help(name)
            if help_text is not None:
                tables[name] = theirs(help_text)
        if not tables:
            no_reference.append(crate.name)
            continue
        checked += 1

        # Group bindings by the reference set that may judge them: a binding
        # inside `semanage_login` is answerable only to semanage, and is
        # SKIPPED when semanage is not installed rather than measured against
        # whichever sibling is.
        bad = []
        general: dict[str, str] = {}
        for short, long, fn_name in ours_located(source):
            owner = owning_personality(fn_name, names)
            if owner is None:
                general.setdefault(short, long)
            elif owner in tables:
                bad.extend(compare({short: long}, [tables[owner]]))
            else:
                unjudged += 1
        bad.extend(compare(general, list(tables.values())))
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
          f"{len(no_reference)} with no reference available"
          + (f", {unjudged} binding(s) left unjudged." if unjudged else "."))
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
