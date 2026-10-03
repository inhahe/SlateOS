## TD-A-A-BOOT-GATE-SCANNED-ANOTHER-LANES-WORKTREE (lane A, 2026-09-14) -- FIXED

**In short:** `scripts/check-collapsed-messages.py` had its scan root hardcoded
to `E:/visual studio projects/os-lane-c`. Every lane's boot ran it, and every
lane's boot therefore checked **lane C's** working tree instead of its own.

**How it surfaced.** Lane A's boot failed at gate 60 naming
`gui/desktop/src/session/tests.rs:122` and a message about a rebound shortcut.
That text does not exist anywhere in lane A's tree, and the file's line 122 is
`.collect()`. A gate had refused a build over a string the tree does not
contain.

**Two consequences, and the second is worse.**

1. Another lane's **uncommitted** edits can refuse your build. Nothing needs to
   be committed, pushed or merged; it is read straight off their disk.
2. The gate's pass never described the tree being built. A green from it cleared
   lane C's tree no matter who ran it -- so it could not have cleared lane A
   either, in any of the runs where it passed.

**The fix**, and it was already written down elsewhere: `check-eol.py` carries a
comment stating that `ROOT` derived from `__file__` means
`os-lane-a/scripts/...` checks `os-lane-a` and nothing can redirect it. The same
derivation now applies here. After it, lane A's run reports 384 source files
scanned and clean.

**Not systemic, and the check for that is worth recording because the first
answer was wrong.** Grepping `scripts/*.py` for `os-lane-[abc]` returns five
files, which reads as a four-gate cross-lane defect. Four of the five are
**prose**: `check-eol` mentions it in the comment quoted above, `ctest-fixtures`
in a paragraph about a past bug, `prune-build-cache` in a usage example. Only
`check-collapsed-messages` had it in executable code. A grep that matches
comments and docstrings counts documentation as defect.

**Still live in lane C's tree:** the collapsed message the gate originally found
is real and is theirs to repair with
`python scripts/check-collapsed-messages.py --apply`. It was never lane A's.
