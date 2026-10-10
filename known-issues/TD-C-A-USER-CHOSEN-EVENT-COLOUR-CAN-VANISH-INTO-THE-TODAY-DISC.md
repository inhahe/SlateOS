## TD-C-A-USER-CHOSEN-EVENT-COLOUR-CAN-VANISH-INTO-THE-TODAY-DISC (lane C, 2026-08-23)

**Status:** OPEN for lane E only. Decided 2026-09-27 (`design-decisions/1424-...`,
the operator's answer to C-Q19): the colours stay exactly as chosen, and the
two pickers warn instead -- the calendar's event-colour picker and Settings'
accent picker, both lane E's. Lane C's part is built: the one test of "too
close to see one on the other" both warnings use,
`guitk::palette::hard_to_tell_apart` (re-exported by `appearance`). The
desktop's calendar drawing stays as it is, by the decision. Close this when
lane E's two warnings land.

**In short:** Each calendar event can carry a colour you picked, and the month
grid draws a small dot in that colour on the event's day. Today's date is drawn
as a filled disc in your accent colour. If you give an event a colour close to
your accent — or exactly it — the dot lands on the disc and disappears. The
event is still there; the mark saying so is not visible.

**Where:** `gui/desktop/src/calendar.rs`, `render_day_cell`. The dot's colour is

```rust
let dot_color = match (first.color, is_today) {
    (Some(chosen), _) => chosen,          // <-- no contrast check
    (None, true) => readable_on(today_disc),
    (None, false) => p.lavender,
};
```

The `None` branches are safe by construction: `readable_on` derives the ink from
the disc's own brightness, and `p.lavender` is only ever drawn on the card. The
`Some` branch honours the user's colour unconditionally, which is the correct
*default* instinct and the wrong *only* rule.

**Why it is filed rather than fixed:** the three available answers are each
defensible and each visible to the user, so this is a design choice, not an
oversight to patch.

**2026-09-13: now in the operator's queue as `open-questions.md` → C-Q19, and
the reason it moved is that one option got cheap.** `appearance::legible_on`
landed on 2026-09-12: it moves a colour the smallest distance that clears
4.5:1, does nothing when it already does, and preserves hue. So "nudge the
brightness" is no longer a rule somebody has to invent, it is one call.

**I started implementing it and stopped at the call site's own comment**,
which says *"An event the user coloured keeps that colour everywhere, even on
today's disc: it is their data and the calendar does not get to overrule it."*
That is the same principle that left `apps/whiteboard`'s strokes and
`apps/screenshot`'s annotations unfloored earlier the same day, and overruling
it here on the strength of a mechanism the decision predates would have been
inconsistency dressed as progress. The comment is the reason this is a
question rather than a commit.

**One thing the aborted attempt did leave behind, and it is worth having.** The
test written for it passed *without* the fix, because the fixture's event never
landed on the rendered cell and no dot was drawn at all. A negative control --
assert that a dot exists before asserting anything about its colour -- turned a
vacuous pass into a loud failure. Whatever is decided here, the test for it
needs that control first.

| Option | *What changes:* |
|---|---|
| Honour it always (today) | Nothing. A colour matching the accent is invisible on today's cell, and the user arguably asked for that. |
| Draw a ring around the dot | Every coloured dot gains a thin outline in the cell's own background colour, on every day, not just today. |
| Nudge the dot's brightness when contrast is below a floor | A dot on today's disc is drawn slightly lighter or darker than the colour the user set — so the file and the screen disagree. |

**If it is never answered:** nothing gets worse. It affects one cell out of
forty-two, only when a colour was explicitly chosen, and only when that colour
is near the accent. The event remains in the detail card, whose colour bar sits
on `mantle` and is unaffected.

**Note the shape of it:** this is a *contrast* defect, and contrast is not a
membership property — the user's colour is not a palette member at all, so
`palette_check::assert_drawn_from` is told to accept it via `derived` and can
never see this. A membership test cannot check a value it was told to accept.
Only a test that computes a ratio can, and it would need a policy to compare
against, which is exactly what is missing.
