#!/usr/bin/env python3
"""Find command names that a program answers to but that nothing can invoke.

## Why this exists

A *multi-call* program inspects the name it was started under and behaves as a
different tool accordingly -- `userspace/e2fsprogs` is `mke2fs` when invoked as
`mke2fs` and `e2fsck` when invoked as `e2fsck`, out of one executable. That only
works if something creates the second name: a symlink, a hard link, or a copy.

We wrote the dispatch about seventy times and **never wrote the links**. So
`e2fsck`, `mke2fs`, `strip`, `ranlib`, `xxd`, `killall`, `shred`, `visudo`,
`lpr` and dozens more are complete implementations that no build produces an
executable for, and that a user typing the name gets "command not found" for.
`e2fsck` and `mke2fs` create and check ext4, which `design.txt` makes our only
filesystem. See `known-issues.md` ->
`B-DOZENS-OF-COMMANDS-EXIST-IN-SOURCE-AND-CAN-NEVER-BE-RUN`.

Nothing detected this for months because nothing was looking. This looks.

## What it does, and what it is worth

For every `userspace/*/src/main.rs` that extracts its own invocation name, it
collects the names that program dispatches on and asks whether anything
produces an executable of that name:

    a crate       `userspace/<alias>/Cargo.toml` exists
    a coreutils bin  `userspace/coreutils/src/bin/<alias>.rs` exists
    a staged alias   `scripts/create-ext4-rootfs.sh` copies/links some binary
                     to that name (it already does this for `dash` -> `/bin/sh`)

An alias with none of the three is **unreachable**: finished code behind a
branch no invocation can select.

This is a *heuristic* about a syntactic pattern, so it has both kinds of error
and neither is silent. False positives -- a program comparing a string to
`"root"` is checking a username, not its own name -- are handled by the
explicit `IGNORE` table below, which carries a reason per entry so the list can
be audited rather than trusted. False negatives are likelier: a crate that
spells the dispatch a way `_DISPATCH` does not match is simply not seen, which
is why the count in `known-issues.md` is quoted as a range.

## The baseline, and why this is a ratchet rather than a test

The honest thing for a checker to do on finding ~50 defects is fail, and a
check that fails from the day it lands is a check somebody deletes. So the
known-unreachable set is recorded in `multicall-aliases-baseline.txt` and
`--check` fails only on a name that is *not* in it. That stops the problem
growing while the backlog is worked down, which is the property actually worth
having: every entry removed from the baseline is a command that now runs, and
the file only ever shrinks.

**Do not add to the baseline to make a red run green.** A new unreachable alias
means a new personality was written without a producer -- fix that instead, by
giving the tool its own crate. Adding a link is the wrong fix and
`coreutils-canonical-answer.md` says why: the kernel grants capabilities per
file, so one file answering to `sudo`, `visudo` and `sudoedit` must hold the
union of what all three need, which is the least-privilege problem this OS
exists to avoid.

## Which tree it reads, and why that is a flag

By default this reads the working tree, which is what a person running it by
hand means and what the boot test means. `--head <rev>` reads a git revision
instead, through the `Tree` seam in `scripts/gittree.py`.

The push hook passes it, and the difference is the gate's whole point there:
without it the gate enumerates the *pushed* commits and then judges whatever
happens to be lying on the disk, so a personality that a commit introduces is
missed whenever the working tree has since been tidied, and a push of unrelated
clean commits is blocked whenever the disk has an uncommitted one. See
`known-issues.md` ->
`TD-B-PRE-PUSH-GATES-2-6-8-11-JUDGE-THE-WORKING-TREE-NOT-THE-PUSH`.

The baseline is read through the same tree, deliberately: it is a file in the
commit like any other, and reading the code from the revision while reading the
waiver list from the disk would let an uncommitted baseline edit excuse a
committed defect. `--update-baseline` still *writes* to the disk, because there
is nowhere else to write.

Usage:
    python scripts/multicall-aliases.py             # the report
    python scripts/multicall-aliases.py --check     # exit 1 on a NEW unreachable alias
    python scripts/multicall-aliases.py --check --head <rev>   # ...as of a commit
    python scripts/multicall-aliases.py --update-baseline
"""

from __future__ import annotations

import argparse
import os
import re
import sys
from pathlib import Path

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import gittree  # noqa: E402

ROOT = Path(__file__).resolve().parent.parent
# Repo-relative, `/`-separated, because that is the only spelling the `Tree`
# seam speaks -- see `gittree.Tree`. The one `Path` left is `BASELINE`, which
# `--update-baseline` writes to the disk.
USERSPACE = "userspace"
COREUTILS_BIN = "userspace/coreutils/src/bin"
ROOTFS = "scripts/create-ext4-rootfs.sh"
BASELINE_REL = "scripts/multicall-aliases-baseline.txt"
BASELINE = Path(__file__).resolve().parent / "multicall-aliases-baseline.txt"

# Shadowing is ratcheted separately from unreachability, because they are
# different defects with different remedies. An unreachable alias is finished
# code nobody can run; a SHADOWED alias is finished code nobody can run *and* a
# second implementation of a name something else already provides, so the two
# can drift apart silently and which one a user gets is decided by whichever
# binary the rootfs installs under that name.
#
# That last part is not hypothetical. `userspace/udisks` carried a `umount`
# personality that printed "would unmount" beside `userspace/mount`, which
# actually unmounts; it was found by hand on 2026-09-10 while reading the crate
# for something else, not by this gate, which reported it and exited 0. The
# ratchet exists so the next one is found by the gate instead.
SHADOW_BASELINE_REL = "scripts/multicall-shadowed-baseline.txt"
SHADOW_BASELINE = Path(__file__).resolve().parent / "multicall-shadowed-baseline.txt"


def read_shadow_baseline(tree: gittree.Tree) -> set[str] | None:
    """The pinned shadowing set, empty if the file is absent.

    # Reads the REVISION, not the disk

    Takes the tree for the same reason [`read_baseline`] does, and the first
    version of this function did not -- it called `SHADOW_BASELINE.read_text()`
    and broke every lane's boot test.

    `scripts/test-checkers-honour-head.py` gate 2 is what caught it: the
    fixture commits four aliases with a producer of each recognised kind, then
    DELETES them from the disk, and asserts the commit still passes when read
    with `--head`. A checker that consults the working tree answers about
    whatever is lying around rather than about the commit being pushed, which
    is the whole property that test exists to hold -- and the property the
    gate needs, because `--head` is how the pre-push hook judges the commits it
    is about to send rather than the tree they were tidied in afterwards.

    (`--update-baseline` still writes to disk, which is correct: writing is a
    disk operation and has no revision to speak of.)
    """
    text = tree.read_text(SHADOW_BASELINE_REL)
    if text is None:
        # ABSENT, which is not the same as EMPTY, and the difference decides
        # whether this gate can judge the revision at all.
        #
        # An empty baseline means "no shadowing is permitted here" and every
        # shadowed alias is a new one. An ABSENT baseline means the revision
        # has no shadowing policy -- it predates this ratchet, or it is a
        # synthetic fixture -- and judging it against a file it does not carry
        # is judging it by a rule it never had.
        #
        # Returning an empty set for both is what broke every lane's boot test:
        # `test-checkers-honour-head.py` builds a repo whose four aliases each
        # HAVE a producer, which is the definition of shadowed, and that
        # fixture has no baseline. Every one of the four read as new and the
        # commit was refused.
        return None
    out = set()
    for line in text.splitlines():
        line = line.split("#", 1)[0].strip()
        if line:
            out.add(line)
    return out

# A file is only considered if it extracts its own invocation name at all.
# Without this the `==` pattern below matches any string comparison in the tree.
_EXTRACTS_NAME = re.compile(
    r"argv\[0\]|file_stem|args\(\)\.next\(\)|args_os\(\)\.next\(\)|args\.first\(\)"
)
# Two dispatch shapes are in use. The first is an explicit personality enum,
# which carries every spelling in one arm:
#     "mke2fs" | "mkfs.ext2" | "mkfs.ext4" => Personality::Mke2fs,
# The second is a direct comparison against the extracted name. The variable
# names are enumerated rather than left open because `x == "…"` matches far too
# much; a dispatch that invents a sixth name for the variable is a false
# negative, which is the safe direction.
# `=> Personality::X`, and also `=> Some(Personality::X)` / `=> Ok(...)`. The
# wrapper is not cosmetic: `userspace/cron`'s dispatch returns Option so that an
# unimplemented name cannot fall through to `at`, and when that change was made
# this regex stopped matching, silently taking SEVEN personalities and TWO
# shadowing pairs out of the count. It was caught only because the expected
# number had been predicted before the run -- "6 shadowing, down from 8" reads
# as progress otherwise, which is the failure this gate exists to detect,
# committed by the gate itself.
_CHAIN = re.compile(
    r'((?:"[a-z][a-z0-9_.+-]{0,20}"\s*\|\s*)*"[a-z][a-z0-9_.+-]{0,20}")'
    r'\s*=>\s*(?:Some\(|Ok\()?Personality::'
)
_LITERAL = re.compile(r'"([a-z][a-z0-9_.+-]{0,20})"')
_NAMEVAR = (
    r"basename|base_name|prog_name|progname|program|arg0|argv0|invoked|"
    r"invoked_as|exe_name|cmd_name|self_name"
)
_COMPARE = re.compile(rf'(?:{_NAMEVAR})\s*==\s*"([a-z][a-z0-9_.+-]{{0,20}})"')
_TESTS = re.compile(r"^#\[cfg\(test\)\]", re.MULTILINE)
# `let <ident> = <rest-of-line>` and `<ident>: &str` (a function parameter).
# Used to FOLLOW the invocation name through rebindings rather than guess what
# it is called -- see `name_vars`.
_LET = re.compile(r"\blet\s+(?:mut\s+)?([a-z_][a-z0-9_]*)\s*(?::[^=]*)?=\s*(.*)")
_PARAM = re.compile(r"\b([a-z_][a-z0-9_]*)\s*:\s*&\s*(?:str|String|OsStr)\b")
# A function header, used to keep the taint from leaking between functions that
# happen to reuse an identifier. `cron` calls the lowercased argv0 `lower` in
# `detect_personality` and the lowercased TIMESPEC `lower` in at(1)'s time
# parser, so a whole-file taint reports `noon`, `midnight` and `teatime` as
# personalities.
_FN = re.compile(r"^\s*(?:pub(?:\([^)]*\))?\s+)?(?:async\s+)?(?:const\s+)?"
                 r"(?:unsafe\s+)?(?:extern\s+\"[^\"]*\"\s+)?fn\b",
                 re.MULTILINE)

# Strings that match the dispatch shape but are not tool names. Each carries the
# reason, because an unexplained ignore list is indistinguishable from a list of
# defects somebody wanted to stop seeing.
IGNORE: dict[str, str] = {
    "root": "a username -- `sudo`/`sshd`/`useradm` check who is running them",
    "lo": "a network interface -- `ifconfig`, `dhcpcd`, `hwinfo`",
    "localhost": "a hostname -- `telnet`",
    "anonymous": "an FTP login name -- `ftpd`",
    "all": "an argument value -- `jq --args all`",
    "list": "a subcommand -- `starship`",
    "internal": "an inetd service class",
    "error": "a yacc token category, not a program",
    "cpu": "a `sysstat` report selector",
    "memory": "an `lsmem` report selector",
    "min": "a `stty` control-character name",
    "time": "a `stty` control-character name",
    "ftp": "a protocol name -- `ftpd`",
    "w.exe": "the Windows-suffixed spelling of `w`, matched alongside it",
}


def functions(text: str) -> list[str]:
    """`text` split at function headers.

    Crude -- it does not brace-match, so a chunk runs to the next `fn` and a
    nested function ends its parent early. That is fine for this use: both
    errors make a chunk SMALLER, and a smaller chunk can only lose a dispatch
    (a false negative in one crate) rather than invent one, whereas the
    whole-file version was inventing them.
    """
    starts = [m.start() for m in _FN.finditer(text)]
    if not starts:
        return [text]
    return [text[a:b] for a, b in zip(starts, starts[1:] + [len(text)])]


def name_vars(text: str) -> set[str]:
    """Every variable that holds this program's own invocation name.

    `_NAMEVAR` lists twelve spellings and its comment called a thirteenth "a
    false negative, which is the safe direction". It is not: `userspace/cron`
    calls the variable `lower`, so nine personalities -- three of them
    shadowing a standalone crate of the same name -- were invisible to this
    gate for as long as it has existed.

    A name is not a fact about a program; the assignment chain is. So seed from
    the extraction calls (and from the conventional spellings, which are still
    good evidence when they appear as a function parameter) and then propagate
    forward through `let` bindings:

        let name = argv0.rsplit(['/', '.'])...   # argv0 is seeded -> name
        let name = name.strip_suffix(".exe")...  # name -> name
        let lower = name.to_ascii_lowercase();   # name -> lower

    Forward-only and line-based, which is enough for the shape this is looking
    for: a handful of rebindings between the extraction and the comparison. It
    over-approximates -- a `let` whose right-hand side merely MENTIONS a tainted
    variable is treated as carrying it -- and that is the direction to err in,
    because the cost of a false positive here is a name reported as reachable
    that is not, while a false negative is a shadowing pair nobody sees.
    """
    seeded = set(_NAMEVAR.split("|"))
    tracked = {v for v in seeded if re.search(rf"\b{v}\b", text)}
    # A function parameter is a binding too: `fn f(argv0: &str)` is where
    # `cron`'s chain starts, and there is no `let` for it.
    tracked |= {m for m in _PARAM.findall(text) if m in seeded}

    # Iterate to a fixed point: a later binding can feed an earlier-declared
    # one in a different function, and one pass would miss the transitive step.
    for _ in range(8):
        before = len(tracked)
        for line in text.splitlines():
            m = _LET.match(line.strip())
            if m is None:
                continue
            lhs, rhs = m.group(1), m.group(2)
            if _EXTRACTS_NAME.search(rhs) or any(
                re.search(rf"\b{re.escape(v)}\b", rhs) for v in tracked
            ):
                tracked.add(lhs)
        if len(tracked) == before:
            break
    return tracked


def invocation_aliases(text: str, crate: str) -> set[str]:
    """Names this source dispatches on, other than the crate's own."""
    if (m := _TESTS.search(text)) is not None:
        text = text[: m.start()]
    names: set[str] = set()
    # The enum arm needs no corroboration: a type literally named `Personality`
    # mapped from a string is invocation-name dispatch and nothing else. Demanding
    # a name-extraction call alongside it cost a real hit -- `userspace/chpasswd`
    # spells the extraction as a bare `.first()` in a method chain, so requiring
    # `args.first()` missed that it also answers to `passwd`.
    for chain in _CHAIN.findall(text):
        names |= set(_LITERAL.findall(chain))
    # The comparison shape does need it. `basename == "x"` is a strong hint but
    # `name == "x"` is not, and without evidence that the file extracts its own
    # invocation name this matches ordinary string handling across the tree.
    if _EXTRACTS_NAME.search(text):
        names |= set(_COMPARE.findall(text))
        # ...and the same comparison against any variable the name actually
        # flows into, which is how `cron`'s `lower == "crond"` is reached --
        # but ONE FUNCTION AT A TIME, or an identifier reused elsewhere in the
        # file for something else drags its own string comparisons in with it.
        for chunk in functions(text):
            for var in name_vars(chunk):
                names |= set(
                    re.findall(
                        rf'{re.escape(var)}\s*==\s*"([a-z][a-z0-9_.+-]{{0,20}})"',
                        chunk)
                )
    return {n for n in names if n != crate and n not in IGNORE}


def _self_test() -> int:
    """Fixtures for `invocation_aliases`, all of them cases that occurred.

    The gate's whole claim is a COUNT -- "N personalities, M shadowing" -- and a
    count is the one output where a detector that stopped seeing and a tree that
    got better are spelled identically. So these pin exact sets, not "at least".
    """
    failures = 0

    def expect(label: str, got: object, want: object) -> None:
        nonlocal failures
        ok = got == want
        failures += not ok
        print(f"  {'ok  ' if ok else 'FAIL'}  {label}")
        if not ok:
            print(f"          got  {got!r}")
            print(f"          want {want!r}")

    # THE CASE THIS WAS WRITTEN FOR. Three rebindings between the parameter and
    # the comparison, and the final variable is called `lower`, which no
    # enumeration of conventional names would have contained.
    cron = """
fn detect_personality(argv0: &str) -> Personality {
    let name = argv0.rsplit(['/']).next().unwrap_or(argv0);
    let name = name.strip_suffix(".exe").unwrap_or(name);
    let lower = name.to_ascii_lowercase();
    if lower == "crond" { Personality::Crond }
    else if lower == "crontab" { Personality::Crontab }
    else { Personality::At }
}
fn main() { let args: Vec<String> = std::env::args().collect();
            let argv0 = args.first().map(|s| s.as_str()).unwrap_or("at"); }
"""
    expect("a name reached through three rebindings is still the name",
           invocation_aliases(cron, "cron"), {"crond", "crontab"})

    # THE REUSE. Same identifier, different function, different value. A
    # whole-file taint reported noon/midnight/teatime as personalities of
    # `cron`, which is a check of the wrong proposition: "is this identifier
    # ever the program name" instead of "is it the program name here".
    reuse = cron + """
fn parse_timespec(spec: &str) -> Option<u64> {
    let lower = spec.to_ascii_lowercase();
    if lower == "noon" { return Some(43200); }
    if lower == "midnight" { return Some(0); }
    if lower == "teatime" { return Some(57600); }
    None
}
"""
    expect("an identifier reused in another function does not leak the taint",
           invocation_aliases(reuse, "cron"), {"crond", "crontab"})

    # A CONFIG VALUE. `value` is not the program name however tainted the file
    # is elsewhere; it is the right-hand side of a settings line.
    cfg = cron + """
fn apply(&mut self, key: &str, value: &str) {
    if key == "use-ipv4" { self.use_ipv4 = value == "yes"; }
}
"""
    expect("a config boolean is not a personality",
           invocation_aliases(cfg, "cron"), {"crond", "crontab"})

    # A SUBCOMMAND. Argument dispatch, not invocation-name dispatch: `avahi
    # service ...` is one program with a verb, not two programs sharing a
    # binary, and only the latter can be shadowed by another crate.
    subcmd = cron + """
fn run(all: &[String]) -> i32 {
    let has_service = all.iter().any(|a| a == "service");
    if has_service { 0 } else { 1 }
}
"""
    expect("a subcommand is not a personality",
           invocation_aliases(subcmd, "cron"), {"crond", "crontab"})

    # THE WRAPPED ARM. `=> Some(Personality::X)` is what a dispatch returning
    # Option looks like, and `cron` became one so that an unimplemented name
    # could not fall through to `at`. The unwrapped regex stopped matching and
    # the personality count silently dropped by seven.
    wrapped = """
fn detect_personality(argv0: &str) -> Option<Personality> {
    let lower = argv0.to_ascii_lowercase();
    match lower.as_str() {
        "crontab" => Some(Personality::Crontab),
        "atq" | "atrm" => Some(Personality::Atq),
        _ => None,
    }
}
"""
    expect("an arm wrapped in Some() is still a dispatch arm",
           invocation_aliases(wrapped, "cron"), {"crontab", "atq", "atrm"})

    # NO REGRESSION on the two shapes that already worked.
    enum_arm = """
fn p(argv0: &str) -> Personality {
    match argv0 { "mke2fs" | "mkfs.ext4" => Personality::Mke2fs, _ => Personality::Mkfs }
}
"""
    expect("the enum-arm shape still resolves every spelling in the arm",
           invocation_aliases(enum_arm, "mkfs"), {"mke2fs", "mkfs.ext4"})

    plain = """
fn main() { let basename = std::env::args().next().unwrap();
            if basename == "tail" { tail() } }
"""
    expect("the conventional variable name still works with no chain",
           invocation_aliases(plain, "head"), {"tail"})

    # The corroboration rule: without evidence the file reads its own argv[0],
    # `x == "ls"` is ordinary string handling and must not count.
    bare = 'fn f(s: &str) -> bool { let lower = s.to_lowercase(); lower == "ls" }'
    expect("a comparison with no argv[0] extraction anywhere is ignored",
           invocation_aliases(bare, "whatever"), set())

    # A crate never shadows itself.
    expect("a crate answering to its own name reports nothing",
           invocation_aliases(cron, "crond"), {"crontab"})

    print(f"multicall-aliases: self-test "
          f"{'FAILED' if failures else 'passed'} ({failures} failure(s))")
    return 1 if failures else 0


def staged_aliases(tree: gittree.Tree) -> set[str]:
    """Names `create-ext4-rootfs.sh` installs as a second name for a binary.

    It copies rather than links (`dash` -> `/bin/sh`) so the image builder need
    not support symlinks, so this looks for a destination path under a `bin`
    directory. Deliberately loose: over-reporting a name as *reachable* only
    costs a missed defect here, whereas under-reporting it produces a false
    alarm in `--check`, and a checker that cries wolf gets switched off.
    """
    text = tree.read_text(ROOTFS)
    if text is None:
        return set()
    found: set[str] = set()
    for m in re.finditer(r"(?:cp|install|ln)\s[^\n]*?/s?bin/([a-z][a-z0-9_.+-]*)", text):
        found.add(m.group(1))
    return found


def producers(
    tree: gittree.Tree, alias: str, cu_bins: set[str], staged: set[str]
) -> list[str]:
    out = []
    if tree.is_file(f"{USERSPACE}/{alias}/Cargo.toml"):
        out.append(f"crate userspace/{alias}")
    if alias in cu_bins:
        out.append("coreutils bin")
    if alias in staged:
        out.append("staged by create-ext4-rootfs.sh")
    return out


def _leaf(rel: str) -> str:
    return rel.rsplit("/", 1)[-1]


def survey(tree: gittree.Tree) -> list[tuple[str, str, list[str]]]:
    # `entries` in place of `glob("*.rs")` + `iterdir()`: one listing, then this
    # file's own predicate. The seam offers no filtering on purpose -- see
    # `gittree.Tree` -- because "a coreutils bin" means two different shapes
    # here (a `<name>.rs` file and a `<name>/` directory) and no shared glob
    # could have meant both without an option nobody else would use.
    cu_bins: set[str] = set()
    for rel, is_dir in tree.entries(COREUTILS_BIN):
        name = _leaf(rel)
        if is_dir:
            cu_bins.add(name)
        elif name.endswith(".rs"):
            cu_bins.add(name[:-3])
    staged = staged_aliases(tree)
    rows: list[tuple[str, str, list[str]]] = []
    for rel, is_dir in tree.entries(USERSPACE):
        if not is_dir:
            continue
        crate = _leaf(rel)
        # `read_text` answers `None` for a crate with no `main.rs` -- a library
        # crate, or `coreutils` itself -- so the absence needs no separate
        # `is_file` call, and cannot race one on the disk.
        text = tree.read_text(f"{rel}/src/main.rs")
        if text is None:
            continue
        for alias in sorted(invocation_aliases(text, crate)):
            rows.append((crate, alias, producers(tree, alias, cu_bins, staged)))
    return rows


def read_baseline(tree: gittree.Tree) -> set[str]:
    text = tree.read_text(BASELINE_REL)
    if text is None:
        return set()
    out = set()
    for line in text.splitlines():
        line = line.split("#", 1)[0].strip()
        if line:
            out.add(line)
    return out


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--check", action="store_true",
                    help="exit 1 on an unreachable alias not in the baseline")
    ap.add_argument("--self-test", "--selftest", dest="self_test",
                    action="store_true", help="run this script's own fixtures")
    ap.add_argument("--update-baseline", action="store_true",
                    help="rewrite the baseline from what is found now")
    ap.add_argument(
        "--head", default=None,
        help="judge this commit instead of the working tree. The push hook "
             "passes the commit being published, so a personality introduced "
             "by a commit cannot be hidden by a tidied worktree -- nor an "
             "uncommitted one block a push of unrelated clean commits.",
    )
    args = ap.parse_args()

    # Ahead of everything else, and taking no tree: the fixtures are string
    # literals in this file, so the self-test answers the same in a checkout
    # with no history. --check passing means nothing unless this passed first.
    if args.self_test:
        return _self_test()

    check = args.check
    update = args.update_baseline

    try:
        tree = gittree.open_tree(str(ROOT), args.head)
    except gittree.GitTreeError as exc:
        # Exit 2, not 1. `scripts/run-checker.sh` reads 1 as "the checker found
        # something" and prints the gate's refusal over it; a revision that
        # cannot be opened is not a finding against anyone's code.
        print(f"multicall-aliases: cannot read {args.head!r}: {exc}", file=sys.stderr)
        return 2
    with tree:
        rows = survey(tree)
        baseline = read_baseline(tree)
        # Read here, not at the point of use: `tree` is closed by the end of
        # this block, and a closed tree answers `None` for every path -- which
        # reads as "the baseline is empty", so every pinned entry would look
        # new. That is the same defect as reading the disk, one step further
        # along: an answer about something other than the revision.
        shadow_pinned = read_shadow_baseline(tree)
    unreachable = {f"{c}:{a}" for c, a, p in rows if not p}
    shadowed = [(c, a, p) for c, a, p in rows if p]

    if update:
        body = [
            "# Command names that a program answers to but that nothing produces an",
            "# executable for. Generated by `scripts/multicall-aliases.py",
            "# --update-baseline`; see that script's docstring.",
            "#",
            "# THIS FILE SHOULD ONLY EVER SHRINK. A line here is a finished tool that",
            "# no user can run. Removing one means giving that tool a real producer --",
            "# preferably its own crate, so it gets its own capability identity.",
            "# Do NOT add a line to turn a red `--check` green: a new entry means a new",
            "# personality was written without a producer, which is the defect itself.",
            "",
        ]
        body += sorted(unreachable)
        # newline="" stops Python translating "\n" to "\r\n" on Windows. Git
        # normalises it on commit either way, so without this the file on disk
        # differs from the file in the index and every checkout shows it dirty.
        BASELINE.write_text("\n".join(body) + "\n", encoding="utf-8", newline="")
        print(f"wrote {BASELINE.relative_to(ROOT)} with {len(unreachable)} entries")

        shadow_body = [
            "# Command names answered to by one crate while ANOTHER crate or",
            "# coreutils bin actually produces the executable. Generated by",
            "# `scripts/multicall-aliases.py --update-baseline`.",
            "#",
            "# THIS FILE SHOULD ONLY EVER SHRINK, and for a sharper reason than its",
            "# sibling: every line is two implementations of one command name that",
            "# can disagree, with the winner chosen by whichever binary the rootfs",
            "# installs. `userspace/udisks` answered to `umount` with a stub that",
            "# printed \"would unmount\" while `userspace/mount` unmounted for real;",
            "# a packaging accident decided whether the command worked.",
            "#",
            "# Remove a line by deleting the shadowing branch -- the name belongs to",
            "# whichever program performs the operation. Do NOT add one to turn a red",
            "# --check green.",
            "#",
            "# THE ONE TEST FOR WHETHER A GROWTH IS LEGITIMATE: the added alias must",
            "# be REACHABLE IN A REVISION THAT PREDATES THE COMMIT ADDING IT. Two",
            "# ways that happens, and neither is a new defect:",
            "#",
            "#   the detector learned to see it -- 2026-09-10, cron:crond and",
            "#       cron:crontab, when it stopped guessing the name of the variable",
            "#       holding argv0 and started following the assignment chain.",
            "#",
            "#   the source made it explicit -- 2026-09-10, cron:at, when the",
            "#       dispatch stopped falling through to `at` in a default arm that",
            "#       carried no string literal. The crate always answered to `at`;",
            "#       there was simply nothing for a gate to match on.",
            "#",
            "# A line added with neither -- no detector change, no source change, the",
            "# alias genuinely new -- is somebody turning a red gate green.",
            "#",
            "# This paragraph lives in scripts/multicall-aliases.py, not here.",
            "# An earlier copy was written directly into this file and the next",
            "# --update-baseline deleted it, silently, because the header is",
            "# regenerated wholesale. Policy written into a generated file is one",
            "# regeneration from being lost, and nothing reports the loss: the gate",
            "# stays green, the entries stay right, and only the reasoning goes.",
            "",
        ]
        shadow_body += sorted(f"{c}:{a}" for c, a, _ in shadowed)
        SHADOW_BASELINE.write_text(
            "\n".join(shadow_body) + "\n", encoding="utf-8", newline=""
        )
        print(
            f"wrote {SHADOW_BASELINE.relative_to(ROOT)} with {len(shadowed)} entries"
        )
        return 0

    if not check:
        print(f"{'crate':<16} {'answers to':<18} produced by")
        print("-" * 78)
        for crate, alias, prod in rows:
            print(f"{crate:<16} {alias:<18} {'; '.join(prod) or 'NOTHING'}")
        print(
            f"\n{len(rows)} personalities across "
            f"{len({c for c, _, _ in rows})} crates: "
            f"{len(unreachable)} unreachable, {len(shadowed)} shadowing a real producer"
        )
        if shadowed:
            print(
                "\nShadowing a real producer -- the branch is dead code, and the two "
                "implementations can disagree:"
            )
            for crate, alias, prod in shadowed:
                print(f"  {crate} answers to {alias}, but {prod[0]} provides it")
        return 0

    shadow_now = {f"{c}:{a}" for c, a, _ in shadowed}
    # `None` means the revision carries no shadow baseline, so there is no
    # policy here to enforce. Skipping is the only honest answer; failing would
    # judge the commit by a rule it does not contain.
    shadow_new = sorted(shadow_now - shadow_pinned) if shadow_pinned is not None else []
    shadow_gone = sorted(shadow_pinned - shadow_now) if shadow_pinned is not None else []
    for name in shadow_gone:
        print(f"unshadowed: {name} -- run --update-baseline to drop the line")
    if shadow_new:
        print(
            f"\n{len(shadow_new)} NEW shadowed command name(s): a program answers to\n"
            "a name that ANOTHER crate or coreutils bin already produces. The branch\n"
            "cannot be reached, and the two implementations can disagree without\n"
            "anything noticing.\n",
            file=sys.stderr,
        )
        for name in shadow_new:
            crate, alias = name.split(":", 1)
            prod = next((p for c, a, p in shadowed if f"{c}:{a}" == name), [])
            print(
                f"  userspace/{crate} answers to `{alias}`, but "
                f"{prod[0] if prod else 'something else'} provides it",
                file=sys.stderr,
            )
        print(
            "\nDelete the shadowing branch: the name belongs to whichever program\n"
            "performs the operation. See design-decisions.md 1019.",
            file=sys.stderr,
        )
        sys.stdout.flush()
        return 1

    new = sorted(unreachable - baseline)
    left = sorted(baseline - unreachable)

    # An entry leaves the unreachable set for TWO reasons and they are not the
    # same news. Either something now produces the alias -- a fix -- or the
    # crate that answered to it is gone, in which case nothing answers to the
    # name at all and there was never anything to produce.
    #
    # This said "is now produced" for both until 2026-09-10, when the 1006
    # deletion removed `userspace/snap` and `userspace/systemd-resolved` and
    # the check reported three of their aliases as fixed. Nothing was fixed;
    # the dispatch had been deleted. `--update-baseline` would have recorded
    # that as a repair, which is a worse outcome than the stale line it
    # replaced: a baseline whose entries mean two different things cannot be
    # read at all.
    fixed, gone = [], []
    for name in left:
        crate = name.split(":", 1)[0]
        (gone if not (ROOT / "userspace" / crate).is_dir() else fixed).append(name)

    for name in fixed:
        print(f"fixed: {name} is now produced -- run --update-baseline to record it")
    for name in gone:
        crate, alias = name.split(":", 1)
        print(
            f"gone:  {name} -- userspace/{crate} no longer exists, so nothing "
            f"answers to `{alias}`; run --update-baseline to drop the line"
        )
    if new:
        print(
            f"\n{len(new)} NEW unreachable command name(s): a program answers to these,\n"
            "and no crate, coreutils bin, or rootfs alias produces an executable for\n"
            "them, so the code behind them can never run.\n",
            file=sys.stderr,
        )
        for name in new:
            crate, alias = name.split(":", 1)
            print(f"  {alias:<20} (a personality of userspace/{crate})", file=sys.stderr)
        print(
            "\nGive the tool its own crate rather than adding it to the baseline, and\n"
            "rather than adding a symlink: one executable answering to several tool\n"
            "names must hold the union of all their capabilities, which is the\n"
            "least-privilege problem this OS is designed to avoid.",
            file=sys.stderr,
        )
        return 1
    tail = []
    if fixed:
        tail.append(f"{len(fixed)} fixed")
    if gone:
        tail.append(f"{len(gone)} gone with their crate")
    suffix = f" ({', '.join(tail)})" if tail else ""
    print(f"ok -- {len(unreachable)} unreachable aliases, all known{suffix}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
