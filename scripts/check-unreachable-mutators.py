#!/usr/bin/env python3
"""Find accounting-module mutators that nothing outside their own module calls.

Why this exists
---------------
Four consecutive batches of the kshell guessed-value burn-down (rqstat, zramstat,
signalq, and rqstat's `register` before them) each turned up the *same* extra
defect: the module exposed a `pub fn` that mutated state, and the only caller in
the entire kernel was the module's own `self_test`.  The consequence is not a
dead function -- it is a **column that is monotonic by construction**.
`zramstat`'s `mem_used` could only rise, because `record_discard` was the one
operation that lowered it and nothing reachable called it.  `signalq`'s
`blocked_mask` could only gain bits, because `unblock` was unreachable.  A
counter that cannot go down does not look like a gap in the shell; it looks like
data.

The cause is consistent and worth stating: these arms were written to
*demonstrate* a feature, and demonstrating means showing a counter go up.  The
operation that brings it back down has no demo value, so it was never wired.

What it reports
---------------
For every `kernel/src/fs/*.rs` module, every `pub fn` whose name marks it a
mutator, whose only references anywhere under `kernel/src` are inside its own
file.  A function called only by its own `self_test` counts as unreachable: the
test proves the code works, which is precisely why the gap survives review.

A ratchet since 2026-09-10, and was a reporting tool that always exited 0 before
that
------------------------------------------------------------------------------
It was named `find-unreachable-mutators.py`, it always exited 0, and **nothing in
the tree ran it** -- two of the three ways a check can be inert at once (see
`check-gates-can-refuse.py`, `check-gates-are-wired.py` and
`check-gate-call-sites.py`, which exist for the three separately).

The consequence was visible in the entry it serves. On 2026-09-10 the heading of
`A-FS-MODULES-EXPOSE-MUTATORS-NOTHING-CAN-REACH` said 503 and its body said 520,
two weeks apart, and the body was the stale one. Both were also undercounts --
see `CEILING` for the suffix-collision bug that made them so, which was found by
making this run 5.6x faster and checking the answer had not changed. It had.

An entry about code a tool can see and nothing calls had its own headline number
drift away from the tool that produces it -- because the number lived in prose
and this file exited 0 whatever it found. Renaming it to `check-*` brings it under the wiring ratchet, so it
cannot quietly stop being run again.

`CEILING` is the count. **It fails when the count RISES**, which is the
regression worth blocking: a new mutator nothing can call is a new column that
can only go one way.

When the count FALLS it prints the new value and exits 0, and that asymmetry with
`check-crate-names.py` -- whose baseline must be pruned or it fails -- is
deliberate. Most of these modules are kernel-side accounting for features owned by
other lanes (`touchpad`, `loginscreen`, `notifprefs`, `bootcfg`), so a lane wiring
up one of its own recorders would otherwise have to edit lane A's pin to get a
green boot test. The cost is that the pin loosens rather than tightening itself;
the mitigation is that the exact new number is printed on every boot test, which
is a good deal more visible than a figure in a paragraph.

The population is large and clearing it is a burn-down, not a fix -- see
known-issues.md, `A-FS-MODULES-EXPOSE-MUTATORS-NOTHING-CAN-REACH`.  Run it to
pick the next module to work on, and to check the count is going down.
"""

import pathlib
import re
import sys
from collections import defaultdict

ROOT = pathlib.Path(__file__).resolve().parent.parent
FS_DIR = ROOT / "kernel" / "src" / "fs"
SRC_DIR = ROOT / "kernel" / "src"

# Name prefixes that mark a function as changing recorded state.  Deliberately
# conservative: a false positive here sends someone to read a function that
# turns out to be a getter, which is cheap, but the point of the list is to keep
# the report to things whose absence distorts a number.
MUTATOR_RE = re.compile(
    r"^pub fn ("
    r"record_\w+|set_\w+|add_\w+|remove_\w+|clear_\w+|reset_\w+|delete_\w+|"
    r"create_\w+|register_\w+|unregister_\w+|unblock\w*|unplug\w*|untrack_\w+|"
    r"unlink\w*|free_\w+|release_\w+"
    r")\s*\(",
    re.MULTILINE,
)

# A `pub fn` that only the module's own self_test calls is still unreachable for
# our purposes, so we look for references *outside the defining file* only.

# The count this tree is allowed to carry. A ratchet: it may be LOWERED when
# mutators get wired, and raising it is the one edit that makes this check mean
# nothing.
#
# 504 across 219 modules, of 1928 mutators in 430 files, measured 2026-09-10.
#
# NOT 503, which is what this file reported until the same day and what
# known-issues.md quoted. The old reachability test was the SUBSTRING
# `f"{module}::{name}("`, and `a11y` is a suffix of `inputa11y`: a real
# `inputa11y::set_filter_keys(` call in kshell.rs contains the substring
# `a11y::set_filter_keys(`, so `a11y`'s own unreachable mutator was counted as
# reachable. There is no `a11y::set_filter_keys` caller anywhere.
#
# 21 of the 429 module names are suffixes of another module's name (42 ordered
# pairs: `ar` inside `sidebar`/`taskbar`/`toolbar`/`tar`/`rar`, `cache` inside
# `pagecache`/`fscache`, `vfs`, `index`, `policy` and so on), so the class was
# live in 21 modules and had fired in one. The indexed version matches
# `\b(\w+)::(\w+)\s*\(` and compares the captured module name, which cannot
# collide on a suffix.
#
# The direction matters: the bug made the tool report FEWER problems than exist.
# The first measurement of 2026-08-26, 520, was taken with it and is an
# undercount too.
CEILING = 504


def main() -> int:
    if not FS_DIR.is_dir():
        print(f"[unreachable-mutators] no such directory: {FS_DIR}", file=sys.stderr)
        return 0

    # Indexed once, rather than searched per mutator.
    #
    # The original shape was `for each mutator: for each file: substring search`,
    # which is 1928 x 430 scans of whole files and took 70 s measured. That is too
    # slow to wire into anything, and an unwired checker is how this one came to
    # sit unrun for two weeks in the first place -- so the speed is not a nicety,
    # it is what lets the ratchet exist.
    #
    # Three indexes, one pass:
    #   qualified[(module, name)] -> files containing `module::name(`
    #   bare[name]               -> files calling `name(` unqualified
    #   imports[file]            -> modules that file names as `fs::<module>::`
    #
    # The reachability question is then answered by set membership. The count was
    # the check on that, and it earned its keep: it moved from 503 to 504, which
    # is how the suffix-collision bug in the old substring test was found. See
    # `CEILING`.
    qualified: dict[tuple[str, str], set[pathlib.Path]] = defaultdict(set)
    bare: dict[str, set[pathlib.Path]] = defaultdict(set)
    imports: dict[pathlib.Path, set[str]] = {}

    qual_re = re.compile(r"\b(\w+)::(\w+)\s*\(")
    call_re = re.compile(r"\b(\w+)\s*\(")
    imp_re = re.compile(r"\bfs::(\w+)::")

    for path in SRC_DIR.rglob("*.rs"):
        body = path.read_text(encoding="utf-8", errors="replace")
        for mod, fn in qual_re.findall(body):
            qualified[(mod, fn)].add(path)
        for fn in call_re.findall(body):
            bare[fn].add(path)
        imports[path] = set(imp_re.findall(body))

    findings: dict[str, list[str]] = defaultdict(list)
    total_mutators = 0

    for mod_path in sorted(FS_DIR.glob("*.rs")):
        module = mod_path.stem
        if module == "mod":
            continue
        text = mod_path.read_text(encoding="utf-8", errors="replace")
        names = MUTATOR_RE.findall(text)
        if not names:
            continue
        total_mutators += len(names)
        for name in names:
            # Qualified (`zramstat::record_discard`) in any file but this one...
            reachable = any(
                p != mod_path for p in qualified.get((module, name), ())
            )
            # ...or called bare in a file that actually imports this module. The
            # import test is load-bearing: without it a bare `record_read(` in an
            # unrelated module would count, and `record_read` exists many times
            # over in this tree.
            if not reachable:
                reachable = any(
                    p != mod_path and module in imports.get(p, ())
                    for p in bare.get(name, ())
                )
            if not reachable:
                findings[module].append(name)

    unreachable = sum(len(v) for v in findings.values())
    print(
        f"[unreachable-mutators] {unreachable} mutator(s) with no caller outside "
        f"their own module, across {len(findings)} module(s) "
        f"(of {total_mutators} mutator(s) in {len(list(FS_DIR.glob('*.rs')))} file(s))"
    )
    for module in sorted(findings, key=lambda m: (-len(findings[m]), m))[:15]:
        names = findings[module]
        # ASCII only: this prints to a console whose code page is not UTF-8, and
        # an em dash or an ellipsis character comes out as mojibake there.
        print(f"  {module}: {len(names)} -- {', '.join(sorted(names)[:6])}"
              + (" ..." if len(names) > 6 else ""))
    if len(findings) > 15:
        # "..." and not "…": the comment eight lines above says an ellipsis
        # character comes out as mojibake on this console's code page, and this
        # line used one anyway -- printing a literal replacement character in the
        # middle of the summary. A rule stated next to the line that breaks it.
        print(f"  ... and {len(findings) - 15} more module(s)")

    if unreachable > CEILING:
        print()
        print(
            f"REFUSING: {unreachable} unreachable mutators, and the ceiling is "
            f"{CEILING}. {unreachable - CEILING} more than the tree is allowed to "
            "carry."
        )
        print(
            "A mutator nothing outside its own module can call is a counter that "
            "can only move one way, and a counter that cannot fall does not look "
            "like a gap -- it looks like data. Wire the new one to a caller, or "
            "if it is genuinely internal, make it private so it stops counting."
        )
        print(
            "Do NOT raise CEILING to make this pass. It is a ratchet; raising it "
            "is the one edit that makes the check mean nothing."
        )
        return 1

    if unreachable < CEILING:
        print(
            f"[unreachable-mutators] the ceiling is {CEILING} and the count is "
            f"{unreachable}: {CEILING - unreachable} have been wired since it was "
            f"last pinned. Lower CEILING to {unreachable} in "
            "scripts/check-unreachable-mutators.py when convenient -- not failing "
            "on this is deliberate, see this file's header."
        )

    return 0


if __name__ == "__main__":
    sys.exit(main())
