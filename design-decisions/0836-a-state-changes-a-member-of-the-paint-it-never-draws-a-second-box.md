## 836. A state changes a member of the paint; it never draws a second box

**Date:** 2026-09-12
**Lane:** C
**Decided by:** Claude (autonomous)

**In short:** some boxes on screen change their outline to say something is
happening to them -- a password field turns red when the password was wrong, a
search box turns blue while you are typing in it. The old way to do that was to
draw the box, then draw a coloured rectangle on top of its edge. That worked
while boxes were filled, because the coloured rectangle was the only edge there
was. Now that the standard theme outlines every box, it is two rings, one inside
the other, in two different colours. The rule from here on is that a state
**replaces** the box's outline rather than adding one.

**What it looks like in code.** `push_surface` asks the theme what to paint and
paints it. A site that needs a state to speak now starts from the same answer
and changes the one part the state owns:

```rust
let mut paint = p.surface_paint(Surface::Card);
if focused {
    paint.border = Some(p.blue);
}
p.push_paint_radii(out, x, y, w, h, CornerRadii::all(4.0), paint);
```

`push_surface_radii` is now exactly this with `paint` taken straight from
`surface_paint`, so there is one drawing path and not two.

**The alternatives**

**A. Keep drawing a second rectangle, and have the theme not outline boxes that
have their own border.** Rejected: the theme would have to know which sites
those are, and the only way to tell it is a flag at each site -- which is the
draw site choosing its own appearance again, the thing 829 exists to stop.

**B. Add a `Surface::Focused` and a `Surface::Invalid` beside `Selected`.**
Tempting, and right for `Selected`, which is genuinely a kind of surface. Wrong
for these: "rejected" is not a kind of box, it is a thing that happened to a
box, and the two compose -- a rejected password field is still a card. Modelling
every combination as its own `Surface` member multiplies the match arms by the
number of states.

**C. What was chosen.** The state owns one member of a paint the theme
otherwise decides. It composes, it needs no new vocabulary, and the one thing
it cannot express -- a site inventing a colour from nothing -- is the thing that
should not be expressible.

**Why this needed deciding at all.** Nineteen sites in the tree had the second
rectangle, and none of them looked wrong to whoever wrote them, because the
theme they had in front of them did not draw an outline. That is the shape of
the whole conversion's risk: code that is correct under one theme and wrong
under the other, with nothing in the type system to say which one you were
thinking about. A rule that says *change the paint, do not add a rectangle*
turns that into something a reader can check locally.

**What it cost.** Twelve of the nineteen strokes were not states at all, just
structural edges restating what the theme now says; they became
`paint.border.unwrap_or(the old colour)`, which keeps the card theme's edge
because that theme fills without outlining. Two were not borders at all --
login's power menu restated an edge `Surface::Panel` already carries, and
screenrecorder's countdown circle was a disc with a ring over it, where the disc
is the ring's ground and vanished when outlined.
