## TD-C-THE-SIZE-CELL-AGREES-WITH-CONVENTION-AND-NOT-WITH-THE-SPEC

**Date:** 2026-09-16. **Lane:** C.
**Where:** `apps/explorer/src/main.rs` — `a_folder_row_leaves_the_size_cell_blank`;
`roadmap-detailed.md` §4.1 → "Directories have a size column too".

**In short:** in the file list, a folder's Size column is empty. The design says
it should show how much the folder contains, added up — and says so having
considered the reason every other file manager leaves it blank, and rejected
it. The code does the conventional thing and its comment cites the convention
as justification, so anyone reading the code believes it is finished and anyone
reading the design believes it is unstarted. Neither can see that they
disagree.

**The two texts, side by side:**

| | says |
|---|---|
| the test | "A directory has no meaningful byte count, so its Size cell stays blank — which is what the hand-written view did, and what every file manager does." |
| `roadmap-detailed.md` §4.1 | "**Directories have a size column too** — recursive total of contents. Most file managers leave this blank because computing it on every directory listing is expensive; **we cache instead**." |

**Why this is not simply a bug to fix.** The spec's answer is a cache, and the
cache is not buildable in this lane today: its invalidation rides on the
filesystem change-notification stream (roadmap-detailed line ~1242,
unstarted, lane A) and its pressure-shrinking on the kernel shrinker
subsystem, also lane A. A recursive walk with no invalidation would be worse
than blank -- a number that is confidently wrong the moment anything inside the
folder changes, which is the shape design-decisions 856 is about.

**What was changed now:** the test's comment, which no longer argues from
convention. The behaviour is unchanged and still pinned; what it says about
itself is different, because "this is what everyone does" reads as a decision
that has been made, and this one has been made the other way.

**The general point.** A comment justifying behaviour by appeal to what is
normal is worth checking against the spec, because a project that wrote its own
spec usually did so to depart from normal somewhere -- and those are exactly
the places where an implementer's instinct and the design disagree without
either noticing.
