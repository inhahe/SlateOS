## §469 — Snapped windows have a 6-pixel gap, given up when the screen cannot afford it

**Date:** 2026-08-19 · **Lane:** C · **Decided by:** Claude (autonomous)

**In short:** When you snap two windows side by side, they now sit six pixels
apart instead of touching. The desktop had two separate pieces of code that
knew how to snap a window — one that ran and left no gap, one that was never
called and left six pixels — and wiring the better one up meant picking which
look is the real one. Six pixels won, because it is the one the richer
implementation and every desktop we are competing with already use. On a
screen too narrow for the gap to fit, it is dropped rather than shrunk, since
subtracting six pixels from a three-pixel screen produces a negative window.

### The situation

`gui/desktop/src/snap.rs` had seven layout presets, a zone picker, drag-time
overlays, and per-window restore history. `mod snap;` was the only reference to
it in the tree. What actually ran when the user pressed `Super+Left` was
`main.rs::snap_window`, twenty lines of integer arithmetic that split the work
area in half and knew nothing about any of that.

They disagreed about one visible thing: `snap.rs` separates adjacent zones by
`ZONE_GAP = 6.0`; `main.rs` made the halves touch. Two answers to one question,
only one of which a user could see, and the invisible one was the more capable.
`known-issues.md`
`C-TWO-SNAP-IMPLEMENTATIONS-WITH-DIFFERENT-GAP-POLICIES` recorded that whoever
wired the module up would also have to settle the gap, because a desktop where
some snaps have a gap and others do not is worse than either policy.

`design.txt` says nothing about window snapping at all, so there is no spec to
defer to. That is what makes this a decision rather than a lookup.

### The decision

**A 6-pixel gap, and one implementation.** `main.rs::snap_window` is now a
wrapper over `snap_window_to_zone`, which asks `SnapManager` for the rectangle.
The shell computes no snap geometry.

**The gap is an affordance, not an invariant.** Every preset subtracts the gap
from the area before dividing it, which is only meaningful when the area is big
enough to give it away. `build` tries the gap, keeps it only if every resulting
zone is at least `ZONE_GAP` in both dimensions, and otherwise rebuilds the
layout edge-to-edge. The rule is applied to the *built zones* rather than to
each preset's arithmetic, so a preset added later inherits it without knowing it
exists.

**The threshold is stated as a property, not a number.** "Every zone is at
least one gap wide" rather than "the screen is at least 18 pixels wide": the
latter is right only for `TwoEqualHalves`, and the six other presets divide the
area differently.

### Alternatives considered

**No gap — adopt `main.rs`'s policy into `snap.rs`.**
*What changes:* snapped windows touch, as they do today.
For: nothing about today's behaviour changes, so no user is surprised; touching
halves tile the screen exactly, with no strip of wallpaper showing through. It
is also the cheaper edit — one constant.
Against: it throws away information. The gap was written deliberately into the
implementation that has seven layouts and a picker, and it is what Windows 11,
GNOME and KDE all do, because a visible seam is how a user tells two snapped
windows apart from one window with an internal divider. Deleting it to match
the simpler implementation is letting the code that happened to be wired up win
on the grounds that it happened to be wired up.

**A gap that shrinks proportionally on small screens.**
*What changes:* on a 40-pixel-wide screen the halves would be one pixel apart
instead of six.
For: never loses the seam entirely; no threshold to get wrong.
Against: it makes the gap a function of screen width, so a window's snapped
position is no longer predictable from the layout alone, and the seam becomes a
sub-pixel smear at exactly the sizes where it is hardest to see anyway. It also
does not actually solve the bug: a proportional gap still has to decide what to
do at width 0.

**Clamp the width to zero instead of choosing a gap.**
*What changes:* nothing visible; the negative-width zone becomes a zero-width
zone.
For: a one-line fix at the point of the defect.
Against: it treats the symptom. The negative width is not the problem — the
problem is that the layout is being asked to give away space it does not have,
and a clamped zero-width zone is a zone the user can snap a window into and
then not find it. Dropping the gap keeps every zone usable.

### Consequences

- **Two existing tests encoded the old policy and were rewritten**, not
  patched: `the_two_snapped_halves_cover_the_work_area_with_no_gap` became
  `..._span_the_work_area_with_one_gap_between_them`, and
  `snapping_and_switching_desktops_are_different_shortcuts` stopped asserting a
  literal x. The second is the more useful lesson: it exists to keep two key
  chords apart and had no business pinning a coordinate that belongs to
  `snap.rs`, and it failed for a reason unrelated to what it tests.
- **`ZONE_GAP` is now `pub`.** The shell's tests assert the policy, and a
  private constant would force them to re-state `6.0` — which is the shape of
  the mistake that produced two snap implementations in the first place.
- Anything that later wants a gapless desktop (a tiling mode, a "maximise all"
  view) sets the gap in one place rather than reimplementing snapping. The gap
  being a parameter of `zones_with_gap` rather than a constant read inside each
  arm is what makes that a one-line change.
- The drag-time half of `snap.rs` — `hit_test`, `edge_snap_hit`,
  `render_overlay`, the picker — is still unwired. This decision does not
  settle whether dragging to a screen edge should snap; it settles only what a
  snap looks like once it happens.
