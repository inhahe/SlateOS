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

Usage:
    python scripts/multicall-aliases.py             # the report
    python scripts/multicall-aliases.py --check     # exit 1 on a NEW unreachable alias
    python scripts/multicall-aliases.py --update-baseline
"""

from __future__ import annotations

import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
USERSPACE = ROOT / "userspace"
COREUTILS_BIN = USERSPACE / "coreutils" / "src" / "bin"
ROOTFS = ROOT / "scripts" / "create-ext4-rootfs.sh"
BASELINE = Path(__file__).resolve().parent / "multicall-aliases-baseline.txt"

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
_CHAIN = re.compile(
    r'((?:"[a-z][a-z0-9_.+-]{0,20}"\s*\|\s*)*"[a-z][a-z0-9_.+-]{0,20}")\s*=>\s*Personality::'
)
_LITERAL = re.compile(r'"([a-z][a-z0-9_.+-]{0,20})"')
_NAMEVAR = (
    r"basename|base_name|prog_name|progname|program|arg0|argv0|invoked|"
    r"invoked_as|exe_name|cmd_name|self_name"
)
_COMPARE = re.compile(rf'(?:{_NAMEVAR})\s*==\s*"([a-z][a-z0-9_.+-]{{0,20}})"')
_TESTS = re.compile(r"^#\[cfg\(test\)\]", re.MULTILINE)

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
    return {n for n in names if n != crate and n not in IGNORE}


def staged_aliases() -> set[str]:
    """Names `create-ext4-rootfs.sh` installs as a second name for a binary.

    It copies rather than links (`dash` -> `/bin/sh`) so the image builder need
    not support symlinks, so this looks for a destination path under a `bin`
    directory. Deliberately loose: over-reporting a name as *reachable* only
    costs a missed defect here, whereas under-reporting it produces a false
    alarm in `--check`, and a checker that cries wolf gets switched off.
    """
    if not ROOTFS.is_file():
        return set()
    text = ROOTFS.read_text(encoding="utf-8", errors="replace")
    found: set[str] = set()
    for m in re.finditer(r"(?:cp|install|ln)\s[^\n]*?/s?bin/([a-z][a-z0-9_.+-]*)", text):
        found.add(m.group(1))
    return found


def producers(alias: str, cu_bins: set[str], staged: set[str]) -> list[str]:
    out = []
    if (USERSPACE / alias / "Cargo.toml").is_file():
        out.append(f"crate userspace/{alias}")
    if alias in cu_bins:
        out.append("coreutils bin")
    if alias in staged:
        out.append("staged by create-ext4-rootfs.sh")
    return out


def survey() -> list[tuple[str, str, list[str]]]:
    cu_bins = {p.stem for p in COREUTILS_BIN.glob("*.rs")}
    cu_bins |= {p.name for p in COREUTILS_BIN.iterdir() if p.is_dir()}
    staged = staged_aliases()
    rows: list[tuple[str, str, list[str]]] = []
    for main in sorted(USERSPACE.glob("*/src/main.rs")):
        crate = main.parent.parent.name
        text = main.read_text(encoding="utf-8", errors="replace")
        for alias in sorted(invocation_aliases(text, crate)):
            rows.append((crate, alias, producers(alias, cu_bins, staged)))
    return rows


def read_baseline() -> set[str]:
    if not BASELINE.is_file():
        return set()
    out = set()
    for line in BASELINE.read_text(encoding="utf-8").splitlines():
        line = line.split("#", 1)[0].strip()
        if line:
            out.add(line)
    return out


def main() -> int:
    check = "--check" in sys.argv[1:]
    update = "--update-baseline" in sys.argv[1:]
    rows = survey()
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

    baseline = read_baseline()
    new = sorted(unreachable - baseline)
    fixed = sorted(baseline - unreachable)
    for name in fixed:
        print(f"fixed: {name} is now produced -- run --update-baseline to record it")
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
    print(f"ok -- {len(unreachable)} unreachable aliases, all known ({len(fixed)} fixed)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
