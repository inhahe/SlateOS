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

This is a *reporting* tool, not a gate.  It always exits 0.  The population it
measures is large and clearing it is a burn-down, not a fix -- see
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
def main() -> int:
    if not FS_DIR.is_dir():
        print(f"[unreachable-mutators] no such directory: {FS_DIR}", file=sys.stderr)
        return 0

    other_sources = []
    for path in SRC_DIR.rglob("*.rs"):
        other_sources.append((path, path.read_text(encoding="utf-8", errors="replace")))

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
            # Qualified (`zramstat::record_discard`) or re-exported/imported
            # bare (`use crate::fs::zramstat::record_discard; record_discard(`).
            qualified = f"{module}::{name}("
            bare = re.compile(r"\b" + re.escape(name) + r"\s*\(")
            reachable = False
            for path, body in other_sources:
                if path == mod_path:
                    continue
                if qualified in body:
                    reachable = True
                    break
                # A bare call only counts if this module is actually imported
                # there, otherwise we would match an identically-named function
                # in an unrelated module -- `record_read` exists many times over.
                if f"fs::{module}::" in body and bare.search(body):
                    reachable = True
                    break
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
        print(f"  … and {len(findings) - 15} more module(s)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
