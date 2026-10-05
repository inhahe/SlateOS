## 868. A checkbox tree records a tick as a rule on the node clicked, and a click that leaves every child alike folds into the parent

**Date:** 2026-09-24 &middot; **Decided by:** Claude (autonomous) &middot; **Lane:** C

**In short:** When you choose files in a tree of folders — what to back up,
what to index — you tick boxes. We had to decide what a tick on a *folder*
means and how the choice is stored. It is stored as a short list of rules: the
folders you ticked, plus the exceptions you made inside them ("`/home`, except
`/home/u/.cache`"). So a tick on a folder also covers the files inside it that
nobody has opened yet, and the ones created next week. One consequence is worth
knowing before it surprises anyone: **if you tick every folder inside a folder
one by one, the outer folder becomes ticked** — and from then on it covers new
folders created there, exactly as if you had ticked it directly. The widget is
`gui/toolkit/src/treeview.rs` (`CheckRules`); `design.txt` asked for it as the
"tristate checkbox treeview — good for selecting files and directories".

### The three candidates

| | stores | a folder nobody opened | a file created tomorrow | what a box shows |
|---|---|---|---|---|
| **A. A state per file** (the classic tristate tree) | a tick on every file | must be read in full before it can be ticked | not in the set | always what is under it |
| **B. Rules, and nothing more** | the clicks | covered by its folder's rule | covered | "partly" in one case with nothing visible ticked (below) |
| **C. Rules, folded** — chosen | the clicks, tidied | covered | covered | always what is under it |

**A is ruled out by the filesystem.** Ticking `/home` would mean reading every
directory under it before the box could show a tick, and a home directory can
hold hundreds of thousands of files. It also saves a list of the files that
existed on the day, which is the wrong answer for a backup set.

**B disagrees with its own display in one case.** Tick `/home`, then untick
each of its three folders in turn. The rule on `/home` is still there with three
exceptions, so the box says *partly* — the rule still covers whatever is
created in `/home` later — while every box a user can see under it is empty.
The brute-force test that compares the rules against model A caught exactly
this, which is how the case was found rather than argued.

**C folds.** When a click leaves every child of a node in one state, the node
takes that state and the children's rules are absorbed into it, and the check
repeats one level up. Untick the last of the three: `/home` becomes unticked,
exceptions and all. The invariant this buys is stated and tested
(`the_rules_agree_with_a_brute_force_model_under_random_clicks`): in a tree
whose folders are all loaded, a box is partly ticked exactly when what is under
it is mixed. A node whose children have not been read is never folded, because
"every child agrees" cannot be established.

### The consequence, stated plainly

Folding is symmetric. Ticking every child ticks the parent, and a ticked parent
means everything in it, including what does not exist yet. The alternative —
fold only towards *unticked*, so a folder is ticked only when the user ticks it
— was considered and rejected: its box would then read "partly" over a folder
whose every visible child is ticked (the excluded part being tomorrow's files),
which is B's mismatch in the other direction. The rule adopted is that **the box
and the meaning never disagree**, and what "ticked" means is written in the
module documentation where a caller reads it.

### Two smaller calls made with it

- **"Partly" costs nothing to compute.** Rules are kept normalised — none repeats
  what its node would inherit — and under that invariant a node is partly
  ticked exactly when some rule sits strictly below it, which is one probe of an
  ordered map. The argument is in `CheckRules`' documentation; two mutations of
  the code (the probe, and the normalisation) each fail the suite.
- **A folder is re-read every time it is opened** (`gui/toolkit/src/dirtree.rs`),
  not only the first. Nothing watches the disk for changes, so a cached listing
  is a listing of the past, and closing and reopening a folder is how a user asks
  for the present.

### How to reverse

Folding is one function, `CheckRules::fold_upwards`, called from one place,
`toggle_in`. Making it one-directional is a condition on the state it folds
towards. Dropping it entirely returns to B. Neither changes the saved form,
which is the list of `(path, included)` pairs either way — so a selection saved
under one rule loads under another, and only what the boxes display differs.
