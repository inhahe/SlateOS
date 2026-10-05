## 807. A scrollbar is one geometry object shared by the renderer and the drag, and paging beats jump-to-click

**Date:** 2026-09-03
**Lane:** C
**Decided by:** Claude (autonomous)

**In short:** The spreadsheet drew scrollbars that could not be dragged. Making
them work needs the program to answer two questions -- "where do I draw the
thumb?" and "where did the user just drag it to?" -- which are the same
question run in opposite directions. I made one object answer both, rather
than writing the answer out twice. Separately, I chose what a click on the
empty part of the bar does: it moves the view one screenful towards the click,
which is what Windows, macOS and every browser do, rather than teleporting the
view to that spot.

### Decision 1: one geometry object, not two derivations

`ScrollbarGeometry::new(track, vertical, max_scroll, offset)` computes the
thumb's rect, and `ScrollbarGeometry::offset_at(lead)` maps a thumb position
back to a scroll offset. `render_scrollbars` uses the first; `drag_scrollbar`
uses the second. Neither computes a ratio of its own.

| | One object (chosen) | A ratio in each place |
|---|---|---|
| Renderer and drag agree | By construction | Only while both are edited together |
| Cost of a layout change | One function | Two, and the second is easy to miss |
| How a divergence shows up | It cannot | Thumb drifts from the pointer, error zero at the top and growing downward |
| Testable as a law | Yes -- round-trip `offset -> lead -> offset` | Only end-to-end, at whatever points a test happens to sample |

The alternative is not a straw man: it is what the file already did in three
other places, and all three were wrong. This same task uncovered a horizontal
scrollbar drawn over the sheet tabs, a tab strip whose hit boxes sat a status
bar's height above the tabs, and a tab layout hit-tested at a different stride
than it was drawn at. Every one was two derivations of a number that should
have had one, and every one was invisible at zero and grew with the offset.
That is the signature, and it is why the round-trip test checks eleven points
across the range rather than one: a wrong scale still agrees at zero.

### Decision 2: a press in the gutter pages, it does not jump

A press on the track outside the thumb scrolls one viewport towards the press
and starts no drag.

| | Page towards the press (chosen) | Jump the thumb to the pointer |
|---|---|---|
| *What changes:* | The view moves one screenful per click, so you can read what you pass | The view lands wherever you clicked, skipping everything between |
| Matches | Windows, macOS, GTK, every browser | macOS with a non-default setting; some touch UIs |
| Misclick cost | One screenful, one click to undo | Anywhere in the document, and the place you were is gone |
| Fine positioning | Repeat clicks, or drag the thumb | Immediate |

Paging wins on familiarity and on the cost of being wrong. A user who wants
the jump can drag the thumb there, which is one gesture; a user who wanted a
page and got a jump has lost their place. The one real argument for jumping --
reaching a distant part of a long sheet quickly -- is already served better by
Ctrl+End and by the name box.

### Reversing either

Decision 1 should not be reversed; if a future scrollbar needs geometry this
object cannot express, widen the object rather than letting a caller compute
its own. Decision 2 is a user-visible behaviour and a plausible thing to make
configurable later; it lives entirely in the non-thumb arm of
`press_scrollbar`, which is the only place that would have to change.
