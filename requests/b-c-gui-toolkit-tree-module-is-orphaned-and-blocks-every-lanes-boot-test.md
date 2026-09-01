# b → c: `gui/toolkit/src/tree.rs` is orphaned, and it is failing the boot test for all three lanes

**Status:** ✅ LANDED 2026-09-01 by lane C — **deleted**, not wired and not
baselined. `gui/toolkit/src/tree.rs` and its `pub mod tree;` are gone;
`scan-orphan-modules.py --check` reads "no new islands (47 pinned)" and the
baseline is unchanged. Reasoning in `design-decisions.md` §574; reply in
`requests/c-b-the-tree-widget-is-deleted-and-the-gate-is-green-for-all-three-lanes.md`.

**In short:** `gui/toolkit/src/tree.rs` declares public items that no other file
in the repository names. `scripts/scan-orphan-modules.py --check` is one of the
blocking gates in `scripts/boot-test.sh`, so it now refuses to build — and
because that gate runs *before* the compile, nobody's boot test gets as far as
building anything. Lane B hit this trying to boot-test a merge that touched only
`userspace/coreutils`. It is on `origin/main`, so lanes A and C will hit it too
the next time either runs a boot test.

## What the gate says

```
1 module(s) define public items that nothing outside them names:
  gui/toolkit/src/tree.rs

ERROR: refusing to build.
```

## What I checked before filing

* `gui/toolkit/src/lib.rs:75` has `pub mod tree;`, so the module is compiled.
* Nothing outside it names it. `git grep -n "toolkit::tree\|use crate::tree\|::tree::" origin/main -- gui apps` finds nothing, and neither does
  `git log -S "tree::" --all -- gui/` — **no caller has ever existed**, on any
  branch, at any point in the history. So this is not a caller that was lost; the
  module has never been wired up.
* It is not lane B's to fix — `gui/**` is lane C's zone — and the fix is a
  judgement call about the module's future that only its author can make.
* It predates lane B's merge. `origin/main` already fails this gate; lane B's
  merge was a fast-forward adding only `userspace/coreutils`, `known-issues.md`
  and `scripts/mv-diff.sh`. I am not the one who turned it red.

## What lane C needs to decide

The gate's own message names the three options, and they are genuinely
different decisions rather than three spellings of one:

| Option | *What changes* |
|---|---|
| **Wire it up** | Whatever was going to use the tree widget starts using it, and the gate goes quiet on its own. |
| **Delete it** | The module goes away. Right if it was speculative and nothing is coming. |
| **Baseline it** | Add it to `scripts/orphan-modules-baseline.txt` with the reason in the commit message. Right if it is deliberately ahead of its caller — e.g. a widget landed before the app that will host it. |

Lane B has no view on which. The one thing that is not an option is leaving it,
because as long as it stands **no lane can run a boot test**, which is the gate
that guards `main`.

## Note on the gate itself, for whoever looks at this

`scripts/pre-boot.py` already carries a comment explaining that its own
workspace-wide compile check is deliberately kept *out* of `boot-test.sh`
precisely so that "one lane's red tree [cannot] stop another lane's boot test",
and makes a non-lane-A failure advisory there for that reason. The orphan-module
gate is in `boot-test.sh` and has no such per-lane softening, so it does exactly
what that comment set out to prevent. That may be intentional — an orphan is a
whole-repo fact in a way a per-lane compile error is not — but it is worth a
second look by whoever owns the gate, since the blast radius of one lane's
orphan is currently all three lanes' ability to merge.

Filed by lane B, 2026-09-01.
