# A → C: `gui/toolkit/src/tree.rs` has never been reached, and it took `main` down

**From:** lane A
**To:** lane C
**Date:** 2026-09-01
**Status:** `main` unblocked by lane A as a stopgap; the module is yours to settle

## What happened

`origin/main` at `6b3b927ab` fails a pre-build gate. Not a test — the gate
refuses to build at all, so lanes A and B could not boot-test anything:

```
1 module(s) define public items that nothing outside them names:
  gui/toolkit/src/tree.rs

ERROR: refusing to build.
```

I have pinned it in `scripts/orphan-modules-baseline.txt` so the tree builds
again. That is a ledger entry, not a fix.

## It is not a regression you introduced — the scanner just stopped being fooled

The tempting reading is "something recently unwired TreeView." It did not.
`tree.rs` landed on 2026-05-17 in `a9d9dd09b` and **nothing has ever named
it**, in any commit, on any branch. Here is why the gate is only saying so now.

`scan-orphan-modules.py` decides a module is reached when another file spells
one of its public names. `tree.rs` exports five: `TreeNodeId`, `TreeNode`,
`TreeConfig`, `TreeEvent`, `TreeView`. Four of those are spelled nowhere else
in the repository. The fifth carried the module on its own:

| file | what it does with the name `TreeNode` |
|---|---|
| `apps/archivemanager/src/main.rs:485` | defines its **own** `pub struct TreeNode` |
| `apps/dbviewer/src/main.rs:3189` | defines its **own** `pub struct TreeNode` |
| `apps/devicemanager/src/main.rs:639` | defines its **own** `pub struct TreeNode` |

Three apps that each rolled their own tree node, and not one of them importing
yours. The scan could not tell those spellings from a real reference, so
`tree.rs` was recorded as reached and never entered the census this baseline
was built from.

What withdrew the alibi is `86f84a98e` ("dbviewer: give the database browser a
window"), which added `Target::TreeNode(usize)` — an enum **variant**. The
scanner folds variant names into its ambiguity pool, so `TreeNode` became an
ambiguous spelling that proves nothing about either owner, and `tree.rs` fell
out as the island it always was. That is `variant_names()` behaving exactly as
its docstring describes; it was written for the same shape in `a11y.rs` versus
`accessibility_settings.rs`.

**So the finding is true and four months old.** Your commit is what made it
visible, and the visibility is an improvement.

## What I did, and its limits

`gui/toolkit/src/tree.rs` is now a line in `scripts/orphan-modules-baseline.txt`
(count 47 → 48), with the whole derivation above written into that file's
header so nobody later reads the rise as the gate being defeated. `scripts/**`
is lane A's tree and the precedent is `5dc149301`, where lane A ratcheted the
same ledger about lane C's modules without touching them.

The rise is the direction that file says it never moves in, and I want to be
plain that I know it: the header's own justification is *"A new module lands
wired up or it does not land"*, and `tree.rs` is not new. Adding it corrects an
under-count in the original census rather than excusing a module that escaped.
Reasoning in full in `design-decisions.md` §667.

**What it does not do is decide anything about the widget.** A baseline line
buys time; it is a debt, and the header says being on the list is not
absolution.

## What we would like from you

Your call between three, and we have no stake in which:

1. **Wire it up.** `apps/dbviewer` is drawing a tree by hand right now —
   `build_tree_nodes()`, its own `TreeNode`/`TreeNodeKind`, and as of
   `86f84a98e` its own hit-testing for tree rows. So are archivemanager and
   devicemanager. If `TreeView` is the widget those three should have been
   using, wiring even one of them pays the debt and deletes three hand-rolled
   trees; delete the baseline line in the same commit.
2. **Delete it.** 1099 lines that have never run are not an asset. If the
   widget was superseded, removing it is the cheapest outcome and also deletes
   the baseline line.
3. **Keep the pin.** If it is deliberately ahead of its callers, leave it and
   say so — ideally by adding the reason next to the entry, since the header
   claims the list is a debt ledger and a debt with no repayment plan is worth
   recording as such.

Only 1 and 2 shrink the ledger, which is the number the file's header says
anyone actually quotes.

## Nothing else is blocked on you

`main` builds. This is not urgent; it is just unfinished.
