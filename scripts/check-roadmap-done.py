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


# A Rust module path in parentheses on an `[x]` line: `(fs::bench)`,
# `(notify::read_events)`. This is the shape lane A's sections use, where lane
# B's is a command name before a colon -- measured by lane A on 2026-09-11:
# 3,025 `[x]` lines, of which only 26 name a repo path and 415 carry a distinct
# module path. Two lanes, two conventions, one file.
MODPATH = re.compile(r"\(([a-z_][a-z0-9_]*(?:::[a-z_][a-z0-9_]*)+)\)")

# Where those paths are rooted. Lane A's tree; this file only ever reads it.
MODROOT = "kernel/src"


def kernel_modules(tree: gittree.Tree) -> tuple[set[str], set[str]]:
    """`(full paths, bare module names)` for every module under `kernel/src`.

    A full path is `::`-joined and rooted at `kernel/src` -- `fs::notify` for
    `kernel/src/fs/notify.rs` or `kernel/src/fs/notify/mod.rs`. The bare set is
    every individual segment, which the elision rule below needs.
    """
    full: set[str] = set()
    bare: set[str] = set()
    for rel in tree.files_under(MODROOT):
        if not rel.endswith(".rs"):
            continue
        parts = rel[len(MODROOT) + 1:].removesuffix(".rs").split("/")
        if parts and parts[-1] in ("mod", "lib", "main"):
            parts = parts[:-1]
        if not parts:
            continue
        full.add("::".join(parts))
        bare.update(parts)
    return full, bare


def modpath_resolves(path: str, full: set[str], bare: set[str]) -> bool:
    """Whether a roadmap module path names something real.

    Three ways, and the second and third are not slack -- they are the
    convention. Lane A measured this and reported the rule, having first written
    the strict version and had it call `notify::read_events` missing when it is
    `kernel/src/fs/notify.rs` line 439 and entirely real:

      1. the whole path is a module -- `fs::bench`;
      2. the path minus its last segment is, because the last segment is often
         a FUNCTION or TYPE rather than a module -- `notify::read_events`;
      3. the leading segments may be ELIDED, so the remainder need only appear
         as a module somewhere under `kernel/src`. The roadmap writes
         `notify::read_events` for what is really `fs::notify::read_events`.

    Rule 3 is why the bare-name set exists. One false positive in 415 is the
    rate at which a gate gets switched off, so the elision rule matters more
    than it looks.
    """
    if path in full:
        return True
    head, _, _ = path.rpartition("::")
    if head and head in full:
        return True
    # Elided leading segments: any suffix of the path that is itself a known
    # path, or -- for a two-segment `mod::item` -- the module alone.
    segs = path.split("::")
    for i in range(len(segs)):
        tail = "::".join(segs[i:])
        if tail in full:
            return True
        if len(segs) - i > 1 and "::".join(segs[i:-1]) in full:
            return True
    return segs[0] in bare or (len(segs) > 1 and segs[-2] in bare)


def modpath_sightings(text: str) -> list[tuple[int, str, str]]:
    """Every `[x]` module path in `text`, resolved or not.

    Separate from the violation list so the pass message can report how many
    paths were actually examined. "0 violations" out of 415 and "0 violations"
    out of 0 print the same word otherwise, and the second one means the regex
    stopped matching.
    """
    out: list[tuple[int, str, str]] = []
    for n, line in enumerate(text.splitlines(), start=1):
        if "- [x]" not in line:
            continue
        for path in MODPATH.findall(line):
            out.append((n, path, line.strip()))
    return out


def modpath_violations(
    text: str, full: set[str], bare: set[str]
) -> list[tuple[int, str, str]]:
    """`(line, path, source line)` for every `[x]` module path that resolves to
    nothing under `kernel/src`."""
    out: list[tuple[int, str, str]] = []
    for n, line in enumerate(text.splitlines(), start=1):
        if "- [x]" not in line:
            continue
        for path in MODPATH.findall(line):
            if not modpath_resolves(path, full, bare):
                out.append((n, path, line.strip()))
    return out


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

            # -- lane A's module-path shape -------------------------------
            #
            # The fixture mirrors the real thing: `fs/notify.rs` nested under a
            # directory, plus a top-level module, because the elision rule is
            # only exercised when the path's head is NOT at the root.
            mod_full = {"fs", "fs::notify", "fs::bench", "sched"}
            mod_bare = {"fs", "notify", "bench", "sched"}
            mod_cases = [
                ("  - [x] thing (fs::notify)", False, "the whole path is a module"),
                ("  - [x] thing (fs::bench)", False, "ditto, sibling module"),
                # Rule 2: the last segment is a function, not a module.
                ("  - [x] thing (fs::notify::read_events)", False,
                 "path minus its last segment is a module"),
                # Rule 3, and THE FALSE POSITIVE LANE A HIT with the strict
                # resolver: the roadmap writes `notify::read_events` for what is
                # really `fs::notify::read_events`. One bad call in 415 is the
                # rate at which a gate gets switched off.
                ("  - [x] thing (notify::read_events)", False,
                 "leading segments elided, last segment a function"),
                ("  - [x] thing (notify::nosuchfn)", False,
                 "elided head resolves; the leaf need not be a module"),
                # The defect it is for.
                ("  - [x] thing (nosuchmod::whatever)", True,
                 "neither the path, its head, nor any segment is a module"),
                ("  - [x] thing (ghost::one::two)", True,
                 "nothing in it resolves at any depth"),
                # Not claimed done, so not this gate's business.
                ("  - [ ] thing (nosuchmod::whatever)", False,
                 "an open entry claims nothing"),
                # A single segment in parentheses is not a path, and matching it
                # would flag every parenthesised word in the file.
                ("  - [x] thing (notes)", False, "one segment is not a path"),
            ]
            for line, want, label in mod_cases:
                checked += 1
                got = bool(modpath_violations(line, mod_full, mod_bare))
                if got != want:
                    failures.append(
                        f"module path, {label}: want flagged={want}, got {got}"
                        f"\n    {line}")

            # The sighting count is what the pass message reports, so it has to
            # count paths rather than lines.
            checked += 1
            two = "  - [x] a (fs::notify) and b (sched::run)\n  - [ ] c (x::y)\n"
            if len(modpath_sightings(two)) != 2:
                failures.append(
                    f"modpath_sightings counted {len(modpath_sightings(two))},"
                    " want 2 (both on the [x] line, none from the [ ] line)")

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
            # Lane A's shape, checked in the same pass over the same file.
            # Deliberately NOT a second script: two ratchets counting adjacent
            # populations is how "N remain" stops meaning anything, and lane A
            # asked for exactly one so it would not be duplicated.
            mod_full, mod_bare = kernel_modules(tree)
            mod_found = (
                modpath_violations(text, mod_full, mod_bare)
                if len(mod_full) >= 100
                else None
            )
            mod_total = len(mod_full)
    except (gittree.GitTreeError, OSError) as e:
        # Loud and non-zero: a checker that cannot read its subject must not
        # report the clean answer, because "no violations" is byte-identical to
        # a healthy tree.
        print(f"check-roadmap-done: cannot read the tree: {e}", file=sys.stderr)
        return 2

    names = sorted({name for _n, name, _s in found})

    # -- lane A's module paths, judged before the name ratchet ----------------
    #
    # NO BASELINE, and that is the point rather than an omission. A baseline
    # exists to hold a backlog, and lane A measured this population before
    # asking for the check: 415 distinct module paths on `[x]` lines, all 415
    # resolving. An empty backlog needs no allowance, and giving it one would
    # only create somewhere to put the first failure.
    #
    # The floor is the usual refusal: a tree with almost no kernel modules
    # would make every path look fabricated, and an empty answer about an empty
    # subject is not a verdict.
    if mod_found is None:
        print(
            f"check-roadmap-done: only {mod_total} module(s) found under"
            f" {MODROOT}/ — refusing to judge the module paths, since that is a"
            " broken checkout rather than a roadmap full of invented ones.",
            file=sys.stderr,
        )
        return 2
    if mod_found:
        print(
            f"{len(mod_found)} roadmap entr"
            f"{'y' if len(mod_found) == 1 else 'ies'} marked `[x]` name"
            f"{'s' if len(mod_found) == 1 else ''} a Rust module path that"
            f" resolves to nothing under {MODROOT}/:\n",
            file=sys.stderr,
        )
        for n, path, line in mod_found[:40]:
            print(f"  roadmap.md:{n}: ({path})", file=sys.stderr)
            print(f"      {line[:100]}", file=sys.stderr)
        if len(mod_found) > 40:
            print(f"  … and {len(mod_found) - 40} more", file=sys.stderr)
        print(
            "\nThe path may elide leading segments and may end in a function or"
            "\ntype, so all three of those resolve. One that resolves by none of"
            "\nthem names a module nobody wrote, or one that has since moved.",
            file=sys.stderr,
        )
        return 1

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
        # The module-path population is named in the pass line, not just in the
        # failure. A gate that says nothing when it passes cannot be told from
        # one that did not run -- which is how a checker goes quietly blind
        # after a refactor moves what it was reading.
        mod_seen = len({p for _n, p, _s in modpath_sightings(text)})
        print(
            f"ok — {len(names)} known unresolved, 0 new ({present} crates,"
            f" {len(fixed)} improved); {mod_seen} module path(s) all resolve"
            f" against {mod_total} module(s) under {MODROOT}/"
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
