#!/usr/bin/env python3
"""Gate: a roadmap entry marked `[x]` must not name a crate that does not exist.

# Why this exists

On 2026-09-11 `roadmap.md` -- which `CLAUDE.md` calls "the live source of truth
for task progress/status" -- was reporting **2,229 programs complete**, with
descriptions and line counts, that commit `ccefac978` had deleted. That commit
is *"userspace: delete the 2,285 commands that reported success for work they
never did"*. So the status marker was making the same claim the deleted code
had made, about the same programs.

Nobody wrote a lie. Somebody deleted 2,285 crates for an excellent reason,
nothing came back to the status file, and nothing anywhere was looking. That is
not a failure of care at the keyboard and no amount of care at the keyboard
prevents it -- which is the argument for a checker rather than a rule. It was
found by accident, while checking whether a real CMake would collide with our
own `cmake` reimplementation. It could not: that reimplementation was one of
the 2,229.

# What it checks

Every line of the shape

    - [x] <name>: <description>
    - [x] <name>/<alias>/<alias>: <description>

whose `<name>` looks like a crate name is checked against
`userspace/<name>/Cargo.toml`. If the manifest is absent, the entry is claiming
a program that is not there.

# What it deliberately does NOT check

**That an entry marked `[ ]` has no crate.** The reverse direction is not a
defect: a crate can exist while its roadmap line is still open, because the
line may describe more than the crate currently does. Flagging that would
punish honest under-claiming, and the whole point here is that over-claiming is
the dangerous direction.

**Entries whose first token is not a crate name.** Plenty of roadmap lines are
prose (`- [x] Direct .pkg installation from anywhere`). Only a first token that
is a plausible crate name -- lowercase, no spaces -- is considered, and only
when the line has the `name:` or `name/alias:` shape that the userspace
inventory uses. A checker that guessed at prose would produce noise, and a
noisy gate is one that gets bypassed.

# The baseline, and why the first draft of this file was wrong about it

This file first said "there is no baseline, on purpose, because the correct
value is zero". That was wrong, and the run proved it: after correcting the
2,229, **352 entries still name nothing**, and they are not all defects.

The roadmap uses ONE line shape for at least three different inventories --
command names in 2.7, libc function names in 2.5 (`pread`, `strsignal`,
`getpeername`), kernel API entries elsewhere -- and a command name does not
reliably map to a crate directory: `wpa_supplicant/wpa_cli` is
`userspace/wpa`, `polkitd/pkexec` is `userspace/polkit`. Some of the 352 are
real (`ethtool`, `smartctl`, `mdadm` exist nowhere); others are that
mismatch.

So the population is real, large, and mixed, which is exactly the situation
this tree answers with a ratchet rather than a rule. `roadmap-done-baseline.txt`
records the entries that do not resolve today; `--check` fails on any name that
is not in it. Deleting a crate without touching the roadmap adds a name, and
the push stops.

Recording the NAMES and not a count, because a count cannot see a swap: one
entry repaired and another broken leaves the total unmoved, and the total is
the only thing a count is watching.
"""

from __future__ import annotations

import os
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

import gitenv  # noqa: E402,F401  (imported for its side effect; see gittree)
import gittree  # noqa: E402
import selftestflag  # noqa: E402


def _load_multicall():
    """`multicall-aliases.py`, whose name has a hyphen in it."""
    import importlib.util

    path = Path(__file__).resolve().parent / "multicall-aliases.py"
    spec = importlib.util.spec_from_file_location("_multicall_aliases", path)
    if spec is None or spec.loader is None:
        raise ImportError(f"cannot load {path}")
    mod = importlib.util.module_from_spec(spec)
    sys.modules["_multicall_aliases"] = mod
    spec.loader.exec_module(mod)
    return mod


_multicall = _load_multicall()

ROADMAP_REL = "roadmap.md"
BASELINE = Path(__file__).resolve().parent / "roadmap-done-baseline.txt"
CRATE_ROOT = "userspace"
COREUTILS_BIN = "userspace/coreutils/src/bin"
POSIX_SRC = "posix/src"
ROOTFS_SH = "scripts/create-ext4-rootfs.sh"
STAGED_RE = r"(?:cp|install|ln)\s[^\n]*?/s?bin/([a-z][a-z0-9_.+-]*)"

# `- [x] name: …` or `- [x] name/alias/alias: …`, where the first token is a
# plausible crate directory name. The trailing `: ` -- colon AND SPACE -- is
# required, and the space is not decoration.
#
# Requiring only the colon matched `- [x] sched::suspend/resume for task
# pause/unpause` on the first `:` of the Rust path separator, and reported 426
# kernel API entries as missing userspace crates. That is the over-matching
# direction, which is worse than missing a real entry: it buries the true
# findings in noise, and a noisy gate is one that gets bypassed. Colon-space is
# what the inventory writes and `::` never is.
ENTRY = re.compile(r"^\s*- \[x\] ([a-z0-9][a-z0-9_.+-]*)((?:/[A-Za-z0-9_.+-]+)*): ")


def _staged_names(tree: gittree.Tree) -> set[str]:
    """Names `create-ext4-rootfs.sh` installs into a `bin` directory.

    Same shape as `multicall-aliases.py`'s `staged_aliases`, and deliberately
    loose for the same reason: over-reporting a name as real costs a missed
    defect, under-reporting produces a false alarm, and a checker that cries
    wolf gets switched off.
    """
    text = tree.read_text(ROOTFS_SH)
    if text is None:
        return set()
    return set(re.findall(STAGED_RE, text))


def real_names(tree: gittree.Tree) -> set[str]:
    """Every name the tree can actually produce or describe.

    FIVE HOMES, not one, and getting this wrong is the whole difficulty. The
    first version of this gate checked only `userspace/<name>/Cargo.toml` and
    reported 413 false positives on its first run, in three distinct flavours:

      * a **multicall alias** — `mkswap/swapon/swapoff` is one crate named
        after the second alias, so the first name has no directory;
      * a **coreutils personality** — `head` and `tail` are files under
        `coreutils/src/bin`, not crates;
      * a **libc module** — the roadmap's POSIX section uses the identical
        `name: description` shape for `fcntl`, `ctype`, `stdlib`, which are
        `posix/src/*.rs` and were never meant to be commands.

    A gate is worth having only if its findings are worth reading, so the
    predicate is "does this name correspond to anything in this tree", which
    every one of the 2,229 deleted crates fails and none of the 413 does.
    """
    names: set[str] = set()
    for rel, is_dir in tree.entries(CRATE_ROOT):
        if not is_dir:
            continue
        crate = rel.rsplit("/", 1)[-1]
        if not tree.is_file(f"{CRATE_ROOT}/{crate}/Cargo.toml"):
            continue
        names.add(crate)
        # THE NAMES THE PROGRAM ANSWERS TO, which are frequently none of the
        # ones its directory is called. `wpa_supplicant/wpa_cli/wpa_passphrase`
        # is `userspace/wpa`; `mkswap/swapon/swapoff` is `userspace/swapon`.
        # Checking directory names alone reported 274 working programs as
        # missing.
        #
        # `multicall-aliases.py` already extracts this and is already a
        # pre-push gate, so this reuses its extractor rather than writing a
        # second one — a second implementation of the same question drifts, and
        # this file would be the one to drift, being the one nobody runs.
        main_rs = tree.read_text(f"{CRATE_ROOT}/{crate}/src/main.rs")
        if main_rs is not None:
            names |= _multicall.invocation_aliases(main_rs, crate)
    # Workspace crates outside `userspace/` — `deflate`, `quoting`, `sha2`.
    for rel, is_dir in tree.entries(""):
        if not is_dir:
            continue
        leaf = rel.rsplit("/", 1)[-1]
        if tree.is_file(f"{leaf}/Cargo.toml"):
            names.add(leaf)
    for rel, _is_dir in tree.entries(COREUTILS_BIN):
        leaf = rel.rsplit("/", 1)[-1]
        names.add(leaf[:-3] if leaf.endswith(".rs") else leaf)
    for rel in tree.files_under(POSIX_SRC):
        leaf = rel.rsplit("/", 1)[-1]
        if leaf.endswith(".rs"):
            names.add(leaf[:-3])
    names |= _staged_names(tree)
    return names


def violations(
    text: str, tree: gittree.Tree | None = None, known: set[str] | None = None
) -> list[tuple[int, str, str]]:
    """`(line number, first name, source line)` for every `[x]` entry none of
    whose names corresponds to anything in the tree."""
    if known is None:
        if tree is None:
            raise ValueError("violations() needs a tree or a known-name set")
        known = real_names(tree)
    out: list[tuple[int, str, str]] = []
    for n, line in enumerate(text.splitlines(), start=1):
        m = ENTRY.match(line)
        if m is None:
            continue
        # `a/b/c:` is one program with three names. It is satisfied if ANY of
        # them is real, because the crate is often named after a later one.
        aliases = [m.group(1)] + [a for a in m.group(2).split("/") if a]
        if not any(a in known for a in aliases):
            out.append((n, m.group(1), line.strip()))
    return out


HEADER = """# Roadmap `[x]` entries whose name resolves to nothing in this tree.
#
# Written by `scripts/check-roadmap-done.py --update-baseline`. THIS FILE IS A
# RATCHET AND ONLY EVER SHRINKS: a name here is an entry claiming a program is
# done when nothing by that name exists.
#
# It is not all defects, and that is why it exists rather than a bare zero. The
# roadmap uses one line shape for command names, libc function names and kernel
# API entries alike, and a command name does not reliably map to a crate
# directory -- `wpa_supplicant` is `userspace/wpa`. Some of these are real
# (`ethtool`, `smartctl`, `mdadm` exist nowhere); others are that mismatch.
#
# NAMES and not a count, because a count cannot see a swap: one entry repaired
# and another broken leaves the total unmoved.
#
# Do NOT add a name here to turn a red `--check` green. A new name means a
# program was deleted or renamed and the status file was not told -- which is
# how 2,229 entries came to report done for programs deleted BECAUSE they
# reported success for work they never did.
"""


def read_baseline() -> set[str] | None:
    try:
        text = BASELINE.read_text(encoding="utf-8")
    except OSError:
        return None
    return {
        line.strip()
        for line in text.splitlines()
        if line.strip() and not line.lstrip().startswith("#")
    }


def write_baseline(names: list[str]) -> None:
    nl = chr(10)
    tail = "#" + nl + f"# {len(names)} unresolved." + nl + nl
    body = HEADER + tail + "".join(n + nl for n in names)
    # newline="" so Python does not translate to CRLF on Windows; git would
    # then see a file that differs from its index on every checkout.
    BASELINE.write_text(body, encoding="utf-8", newline="")


def selftest() -> int:
    """The shapes this must catch and the shapes it must leave alone."""
    failures: list[str] = []
    checked = 0

    import tempfile

    with tempfile.TemporaryDirectory() as td:
        fake = Path(td)
        (fake / CRATE_ROOT / "realcrate").mkdir(parents=True)
        (fake / CRATE_ROOT / "realcrate" / "Cargo.toml").write_text(
            "[package]\n", encoding="utf-8", newline=""
        )

        cases = [
            # (line, should_be_flagged, label)
            ("  - [x] ghostcrate: a thing that is not there (~190 lines)", True,
             "the defect: done, and no crate"),
            ("  - [x] realcrate: a thing that is there", False,
             "done, and the crate exists"),
            ("  - [x] ghost/alias/other: multi-personality, none of it present", True,
             "the alias form still names its crate first"),
            ("  - [ ] ghostcrate: not claimed done", False,
             "an open entry claims nothing"),
            ("  - [x] Direct .pkg installation from anywhere", False,
             "prose is not an inventory entry"),
            ("  - [x] Port Chromium (~35M lines C++)", False,
             "prose beginning with a capital"),
            ("  - [x] realcrate/alias: present under its first name", False,
             "alias form, crate present"),
            # No colon: the inventory shape is what makes the first token a
            # crate name, and without it this is a sentence.
            ("  - [x] ghostcrate is coming along nicely", False,
             "no colon, so not an inventory entry"),
            # The over-match that cost 426 false positives on the first run.
            ("  - [x] sched::suspend/resume for task pause/unpause", False,
             "a Rust path separator is not the inventory colon"),
            ("  - [x] channel::recv_timeout (SYS_CHANNEL_RECV_TIMEOUT 205)", False,
             "kernel API entry, not a crate claim"),
        ]
        # Through the same seam the real run uses, so the self-test cannot pass
        # against a code path the gate does not take.
        with gittree.WorkTree(str(fake)) as tree:
            known = real_names(tree)
            checked += 1
            if "realcrate" not in known:
                failures.append(f"real_names missed the crate on disk: {sorted(known)}")

            for line, want, label in cases:
                checked += 1
                got = bool(violations(line, known=known))
                if got != want:
                    failures.append(f"{label}: want flagged={want}, got {got}\n    {line}")

            # Line numbers must point at the line a reader will open.
            checked += 1
            doc = "intro\n\n  - [x] realcrate: fine\n  - [x] ghostcrate: not fine\n"
            hits = violations(doc, known=known)
            if [(n, c) for n, c, _s in hits] != [(4, "ghostcrate")]:
                failures.append(f"line numbering: {hits}")

    for f in failures:
        print(f"selftest FAIL {f}")
    print(f"selftest: {checked - len(failures)}/{checked} cases pass")
    return 1 if failures else 0


def main() -> int:
    for s in (sys.stdout, sys.stderr):
        try:
            s.reconfigure(encoding="utf-8", errors="replace")
        except (AttributeError, ValueError):
            pass

    args = sys.argv[1:]
    if selftestflag.wants_selftest(args):
        return selftest()

    # `--head <sha>` judges the revision being PUSHED rather than the files on
    # disk, which is the difference between a gate and a suggestion: a commit
    # that breaks this can be repaired in the working tree and pushed anyway,
    # and a worktree-reading gate calls that clean. `scripts/quote-names.py`
    # carries the same flag for the same reason, and that is not a coincidence
    # -- reading the worktree is how two unformatted commits reached origin.
    head: str | None = None
    if "--head" in args:
        i = args.index("--head")
        if i + 1 >= len(args) or args[i + 1].startswith("--"):
            print("--head needs a commit-ish argument", file=sys.stderr)
            return 2
        head = args[i + 1]

    if head is not None and "--update-baseline" in args:
        # Refused rather than ignored: recording a past commit's unresolved
        # names as the current allowance would re-permit everything repaired
        # since.
        print("--head cannot be combined with --update-baseline", file=sys.stderr)
        return 2

    try:
        maker = (
            gittree.RevTree(head, str(ROOT))
            if head is not None
            else gittree.WorkTree(str(ROOT))
        )
        with maker as tree:
            text = tree.read_text(ROADMAP_REL)
            if text is None:
                print(
                    f"check-roadmap-done: {ROADMAP_REL} is not in the tree"
                    f"{' at ' + head if head else ''} — refusing to judge.",
                    file=sys.stderr,
                )
                return 2
            # A tree with no crates would make every entry look stale. That is
            # the no-corpus failure the other gates here learned to refuse: an
            # empty answer about an empty subject is not a verdict about
            # anybody's code.
            known = real_names(tree)
            present = sum(
                1
                for rel, is_dir in tree.entries(CRATE_ROOT)
                if is_dir and tree.is_file(f"{CRATE_ROOT}/{rel.rsplit('/', 1)[-1]}/Cargo.toml")
            )
            if present < 50:
                print(
                    f"check-roadmap-done: only {present} crates found under"
                    f" {CRATE_ROOT}/ — refusing to judge, since that is a broken"
                    " checkout rather than a roadmap full of stale entries.",
                    file=sys.stderr,
                )
                return 2
            found = violations(text, known=known)
    except (gittree.GitTreeError, OSError) as e:
        # Loud and non-zero: a checker that cannot read its subject must not
        # report the clean answer, because "no violations" is byte-identical to
        # a healthy tree.
        print(f"check-roadmap-done: cannot read the tree: {e}", file=sys.stderr)
        return 2

    names = sorted({name for _n, name, _s in found})

    if "--update-baseline" in args:
        write_baseline(names)
        print(f"wrote {BASELINE.name} with {len(names)} unresolved name(s)")
        return 0

    allowed = read_baseline()
    if allowed is None:
        print(
            f"check-roadmap-done: {BASELINE} is missing. Run --update-baseline"
            " once to record what does not resolve today; without it this"
            " cannot tell a new break from the existing backlog.",
            file=sys.stderr,
        )
        return 2

    new_names = [n for n in names if n not in allowed]
    fixed = sorted(allowed - set(names))
    for f in fixed:
        print(f"fixed: {f} now resolves — run --update-baseline to record it")

    if not new_names:
        print(
            f"ok — {len(names)} known unresolved, 0 new ({present} crates,"
            f" {len(fixed)} improved)"
        )
        return 0

    found = [(n, name, s) for n, name, s in found if name in set(new_names)]

    print(
        f"{len(found)} roadmap entr{'y' if len(found) == 1 else 'ies'} marked `[x]`"
        f" nam{'es' if len(found) == 1 else 'e'} something that does not exist:\n"
    )
    for n, name, line in found[:40]:
        print(f"  roadmap.md:{n}: {CRATE_ROOT}/{name}/ is absent")
        print(f"      {line[:100]}")
    if len(found) > 40:
        print(f"  … and {len(found) - 40} more")
    print(
        "\n`roadmap.md` is the source of truth for task status, so an `[x]` on a"
        "\nprogram that is not there is a fabricated done-status — the thing"
        "\nCLAUDE.md forbids, arrived at by not revisiting a truth after the"
        "\nground moved rather than by writing a lie."
        "\n\nIf the crate was deleted, mark the entry `[ ]`: its description is"
        "\nstill the specification, and §1006's ruling is to add a name back WHEN"
        "\nit is implemented. If it was renamed, rename it here too."
    )
    return 1


if __name__ == "__main__":
    raise SystemExit(main())
