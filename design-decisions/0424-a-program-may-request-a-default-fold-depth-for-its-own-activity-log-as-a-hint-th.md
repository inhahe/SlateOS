## §424 — A program may request a default fold depth for its own activity log, as a hint the user overrides

**Date:** 2026-08-14
**Lane:** C
**Decided by:** Operator (Claude proposed this option and recommended it)

`roadmap-detailed.md` §4.14 gives every process an activity stream whose
`enter`/`exit` and `phase` records form a tree, and specifies that the viewer
folds it: open frames expanded, completed frames folded to a summary line,
failures expanded regardless. That left one question genuinely open — may a
*program* say how its own output should start out folded? An installer would
like to say "start me collapsed to phases."

### The tension

Both sides are real, which is why it was filed as an ambiguity rather than
decided in passing.

**For allowing it.** The program is the only party that knows which of its
nesting levels are semantically meaningful. Depth is a poor proxy for
importance: one program's level 3 is its phase boundary, another's is a
logging helper's inner frame. A viewer choosing purely by depth will collapse
the interesting level of one program and expand the noise of another, and no
global default fixes both.

**Against.** The program is also the party with an incentive to hide its own
noise, and the party most likely to get it wrong — a default fold depth chosen
by a developer watching their own program on their own machine is tuned for
someone who already knows what the levels mean. A hint mechanism is also a
mechanism, and every "the app can influence the system UI" affordance ever
shipped has been abused by somebody.

### What was decided

Allow it, strictly as a default the user's per-app preference overrides. The
constraints are what make this safe, and they are specified rather than left
implied:

* it cannot survive an explicit user setting for that app;
* it cannot hide a frame from expand-all;
* it cannot fold a frame that the failure rules (a frame that failed, or
  contains an error or warning) would expand;
* the viewer indicates when the active fold depth came from the program rather
  than from the user or the global default, so "why is this collapsed" always
  has a visible answer.

The reasoning that settles it: with those constraints, a program that abuses
the hint costs the user **one click, not the data**. The failure mode is
bounded and self-correcting, whereas the failure mode of refusing the hint —
every program's log folding at a depth that is meaningless for that program —
is unbounded and has no recourse at all.

### The alternative that was rejected

Depth-only folding chosen entirely by the viewer, with no program input. It is
simpler and unabusable, and it is what most log viewers do. It was rejected
because it makes the fold depth a property of the *viewer* when the thing being
folded is a property of the *program*, and because §4.14 already establishes
that adoption is the hard part of this feature: a facility that ignores what
instrumented programs tell it gives authors one less reason to instrument.

**Where it bites:** `roadmap-detailed.md` §4.14 → "Grouping and nesting —
subtasks under tasks" (the fold-state bullets) and "Settings" (the per-app
"whether this app's requested default fold depth is honoured" toggle).
Implementation is lane **C**, in the Process Explorer activity pane and the
call-tree view.
