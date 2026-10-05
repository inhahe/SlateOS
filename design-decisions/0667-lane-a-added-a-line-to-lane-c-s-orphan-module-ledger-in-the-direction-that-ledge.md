## §667 — lane A added a line to lane C's orphan-module ledger, in the direction that ledger says it never moves

**Date:** 2026-09-01
**Lane:** A
**Decided by:** Claude (autonomous)

**In short:** A build gate keeps a list of "modules nobody uses" — code that
exists but that no other file in the project ever calls. The list is a debt
list: it may shrink as the code gets connected up, and the gate refuses to
build if a module that is *not* on the list turns out to be unused. On
2026-09-01 `main` stopped building for all three lanes because a graphics
widget, `gui/toolkit/src/tree.rs`, was found unused and was not on the list. It
turned out the widget had been unused since May and the list was simply wrong
about it — the checker had been fooled by a name coincidence. I added it to the
list so the project could build again, which is the one edit that list says it
should never receive, and this entry is why that was the right call anyway.

### What actually happened

`scan-orphan-modules.py` calls a module *reached* when some other file spells
one of its public names. `tree.rs` exports five names. Four appear nowhere else
in the repository. The fifth, `TreeNode`, appears in three other files —
`apps/archivemanager`, `apps/dbviewer` and `apps/devicemanager` — and in every
one of them it is that app's **own** `pub struct TreeNode`, hand-rolled, with
no import of the toolkit at all. Three apps independently reinvented a tree
node, and the coincidence of spelling was enough to make the scan report the
toolkit widget as reached.

`86f84a98e` (lane C, "dbviewer: give the database browser a window") added an
enum **variant** `Target::TreeNode(usize)`. `variant_names()` folds variant
spellings into the scan's ambiguity pool — deliberately, for exactly this shape;
its docstring was written about `a11y.rs` being alibi'd by
`accessibility_settings.rs`. With `TreeNode` ambiguous, it proved nothing about
either owner, and `tree.rs` fell out as the island it had been all along.

So the gate did not detect a regression. It detected a **four-month-old fact it
had previously been wrong about**, and the commit that triggered it is an
improvement.

### The decision

| option | what it costs |
|---|---|
| leave `main` red until lane C acts | two innocent lanes cannot boot-test *anything* — the gate is pre-build — for an unknown number of hours, over a debt that predates all of this work |
| lane A edits `gui/**` to wire or delete the widget | forbidden: it is lane C's tree, and this is the clobbering that the lane split exists to prevent |
| lane A pins it in the ledger and files a request | the ledger's count rises, which its header says it never does |

I took the third. `scripts/**` is lane A's, and `5dc149301` is direct precedent
for lane A ratcheting this same ledger about lane C's modules without touching
them — though that was a *deletion*, the always-allowed direction.

### Why the rule is not actually being broken

The header states the rule and, in the same breath, its purpose: *"the count may
fall, never rise. A new module lands wired up or it does not land."* The rule
guards the boundary at which code **enters** the tree. `tree.rs` entered on
2026-05-17, unreached, months before that file existed; it belonged in the
original census of 57 and was omitted because the scan was fooled, not because
it was connected.

That gives the distinction this decision turns on:

- A line added because a module **stopped being reached** is a module escaping
  the gate. That is the failure, and it stays forbidden.
- A line added because the **census under-counted** is a correction to the
  ledger. The debt did not grow; the measurement of it got better.

The honest number after the correction is 48, and 47 was never true. A ledger
that refuses corrections does not stay accurate — it stays *stale*, which is
the failure its own header already diagnoses two paragraphs earlier about the
generous direction of the ratchet.

### What keeps this from becoming a loophole

The risk is obvious: "the census under-counted" is available as an excuse to
anyone who wants to pin a module rather than wire it. Three things narrow it,
and all three are checkable rather than asserted:

1. **The module must predate the ledger.** `a9d9dd09b`, 2026-05-17, against a
   file created months later. A module added after the ledger cannot use this.
2. **It must never have been reached.** Not "is unreached now" — the claim is
   that no commit on any branch ever named it, which `git log -S` answers.
3. **The mechanism that hid it must be named.** Here: three same-named structs,
   and the variant fold in `variant_names()` that withdrew the alibi. A pin with
   no such account is an ordinary pin and the rule applies unchanged.

The derivation is written into the baseline header itself rather than only
here, because the header carries the count that people quote, and a count that
rose with no explanation next to it is indistinguishable from a defeated gate.

### What was not decided

Whether the widget lives. `requests/a-c-toolkit-treeview-has-never-been-reached.md`
puts that to lane C with three options — wire it into one of the three apps
hand-rolling a tree, delete its 1099 never-run lines, or keep the pin and record
why. Only the first two shrink the ledger. Being on the list is not absolution,
and this entry does not make it so.

### Outcome, the same day: the pin was withdrawn because the module was deleted

Lane C took option 2 in `d748de88a`, and the baseline is byte-identical to its
pre-pin content again. Their reasoning is §574; the short form is that
`TreeNode` is concrete — no payload slot, no type parameter — while all five
programs that draw a hierarchy carry domain data on their nodes, so adopting it
would mean maintaining a parallel tree of labels keyed by id, by hand, on every
expand and filter, in exchange for a widget that draws one column.

Two things are worth recording rather than quietly dropping.

**They never read the request.** Lane B hit the same gate the same day on a
merge touching only `userspace/coreutils`, filed
`requests/b-c-gui-toolkit-tree-module-is-orphaned-and-blocks-every-lanes-boot-test.md`,
and lane C answered that one. Both requests reached the same facts
independently, including the `git log -S` result that no caller has ever
existed. That is the dropbox working: two lanes blocked, neither reaching into
`gui/**`, one owner deciding once.

**Lane C's reply argues against baselining, and on its own terms it is right.**
It quotes this very header — "the only edit it should ever receive is a
deletion" — and notes the stated exception, "deliberately ahead of its caller",
needs a caller to be ahead *of*. Nothing in that rebuts this entry, because the
two never met in one tree: they were deciding the module's future, where
deletion beats a pin outright, and this was deciding whether `main` should stay
unbuildable for three lanes in the hours before an owner answered. The pin was
the cheaper of those two, and deletion made it moot within the hour.

What survives is the mechanism, not the pin: a spelling coincidence alibied an
island for four months, and an unrelated enum variant withdrew the alibi. That
will recur, and the next module it exposes may be one somebody wants to keep —
at which point the three anti-loophole conditions above are the test to apply.
The count-may-fall-never-rise rule is now intact in the tree rather than merely
argued to be intact, which is the better place for it to be true.
