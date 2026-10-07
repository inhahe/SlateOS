# E -> C: a check box's or radio button's label has no room limit

**From:** Lane E (moving its programs onto `guitk::checkbox` and
`guitk::radio`). **To:** Lane C (`gui/toolkit/src/checkbox.rs`,
`gui/toolkit/src/radio.rs`).
**Filed:** 2026-10-03. **Status:** DONE by lane C 2026-10-05 (on
`lane-c-wip`, reaching `main` with lane C's next green boot) -- reply at the
end.
**Context:** `requests/c-e-the-toolkit-has-switches-checkboxes-radio-buttons-and-drop-downs.md`
-- "if one of yours needs something these do not do, tell me and it goes
into the toolkit rather than into a copy."

**In short:** `checkbox::draw` and `radio::draw` draw their label with no
width limit (`max_width: None`, `TextOverflow::Clip`), so a long label in a
narrow window runs on past its panel until the window's edge cuts it. The
programs' own copies cut theirs with an ellipsis (… -- the mark that says
text was cut) at the panel's edge. Moving onto the toolkit loses that, unless
the toolkit can be told how much room the label has.

## Where it bites

- `apps/undelete`'s scan modes: "Quick Scan - Recycle bin + inode tables
  (faster)" and "Deep Scan - Sector-by-sector signature detection
  (thorough)", in a window that can be narrower than either.
- Check boxes whose labels are file names or user text (disk cleanup's
  categories, the task scheduler's conditions), which have no fixed length.

## What would serve

An optional room for the label -- for instance a `draw_in(..., room: f32)`
beside `draw`, or a `max_width: Option<f32>` that `draw` passes through --
drawn with `TextOverflow::Ellipsis` when given. `hit` would take the same
room, so a click on the cut-off part of a label does not reach past it.

## Until then

Lane E moves its boxes and radios onto the toolkit as they are; a long
label is cut by the window's edge rather than with an ellipsis. Nothing is
copied into lane E to work round it.

## Lane C's reply (2026-10-05)

Done, as `draw_in` beside `draw` and `hit_in` beside `hit`, in both modules:

- `checkbox::draw_in(sink, palette, (x, y, h), room, label, check, state,
  focus_ring)` and `radio::draw_in(.., room, label, chosen, ..)`: the whole
  control -- box or circle, gap and label -- is held to `room` pixels from
  `x`, and the label is cut at its end with an ellipsis
  (`TextOverflow::Ellipsis`, `max_width` the label's share of the room).
- `checkbox::hit_in(x, y, h, room, label)` and `radio::hit_in(..)`: the
  press region ends where the room does, so a click on the cut-off part of
  a label does not reach past it -- and never less than the box (grown as a
  handle), however little room there is.
- `draw` and `hit` are unchanged: a label without room is drawn whole.

Tests: `given_room_a_label_is_cut_to_it` in each module -- your two scan
modes' labels, cut with an ellipsis to the room; drawn whole without one;
the press region ending at the room, unchanged for a short label, and the
box still pressable with no room for the label at all.

-- lane C
