## TD-C-THE-CALENDARS-EVENT-COLOUR-ONLY-SHOWED-WHEN-A-DAY-WAS-SELECTED (lane C, 2026-08-23) -- FIXED 2026-08-23

**Status:** FIXED 2026-08-23, commit `c36aeb469`, as part of the module-43
palette conversion.

**In short:** The calendar lets you give each event its own colour. You could
write `color: FF0000` next to an event in the calendar file, and the calendar
would read it, store it, and write it back unchanged. But the only place it ever
*drew* that colour was the little bar on the detail card — the panel that opens
when you click a specific day. The month grid's per-day dot, which is the mark
you actually look at when scanning the month, was a fixed lavender for every
event regardless. So a colour you set was invisible unless you first clicked the
day it was on, which is the one moment you no longer need a colour to tell
events apart.

A second, quieter half: an event with no `color:` line in the file was given a
hardcoded Mocha blue by the parser, and the serializer then wrote that invented
blue back out. Opening the calendar was enough to edit the user's file.

**Correction to the first version of this entry.** It was originally filed as
"the colour is read only by the serializer and never rendered at all". That was
wrong, and wrong for an instructive reason: it was derived from
`grep '\.color' | grep -v 'color:'`, and the one production read —
`color: event.color,` in `render_event_detail`'s colour bar — was excluded by
the second filter, which was there to drop struct-literal *writes*. A filter
that removes the noise can remove the signal with it; the shape of a read and
the shape of a write were the same shape.

**Where it was:** `gui/desktop/src/calendar.rs`. `CalendarEvent::color` was a
plain `Color`; the parser assigned `parse_hex_color(val).unwrap_or(theme::BLUE)`
at three sites; `render_day_cell` hard-coded the dot to `theme::LAVENDER` (or
`theme::BASE` on today's disc) and only ever asked
`events_for_date(...).is_empty()`, never looking at the event it found.

**How the dot half survived:** the field was covered by a round-trip test
(`assert_eq!(e.color, Color::from_hex(0x89B4FA))`), which pinned the *default*
and proved the value survived a save/load cycle. That test passed and would have
passed forever, because **a value that round-trips correctly and a value that is
used are unrelated properties**. The colour bar's existence is what made this a
half-bug rather than a whole one, and it is also what made the field look
covered: something drew it, so nothing asked what else should have.

**The fix, in two parts, and why part 1 is not "point the default at `p.blue`".**
The tempting move during the conversion was to swap `theme::BLUE` for `p.blue`.
That is wrong on its own terms, because the assignment is in a **parser**, and a
parser that consults the palette makes the same file parse to different data
depending on which theme is active — after which the serializer writes the
theme-dependent value back, so merely opening the calendar in light mode
rewrites every event the user never gave a colour. A display setting must not be
able to edit user data. So:

1. **`color` is now `Option<Color>`.** `None` means "the user did not choose
   one", which is not the same as any colour and must not be spelled as one —
   the identical reasoning as design-decisions.md §526 for the peek popup's
   sampled colour. `export_text` emits the `color:` line only for `Some`, so a
   file without the key round-trips to a file without the key.
2. **`render_day_cell` uses it**, via the new
   `CalendarEvent::dot_color(&self, p: &Palette)`, which resolves `None` to
   `p.lavender` and `Some(c)` to the user's colour. The detail card's bar goes
   through the same method, so the two marks for one event cannot disagree.
   This makes the dot the one place in the module drawing a colour deliberately
   *not* from the palette, so `palette_check::assert_drawn_from` is handed it via
   `derived` — and the fixture uses an off-palette event colour, or the test
   could not tell the two paths apart.

**Still open, deliberately: the dot on today's disc.** An arbitrary user colour
can land on the accent-coloured today disc at any contrast, including none. The
`None` path handles it (`readable_on(today_disc)`), but a `Some(c)` that happens
to equal the accent draws an invisible dot. Fixing that properly needs a design
answer — a contrasting ring, a brightness-nudged variant, or a rule that the
user's colour is simply honoured and the collision is theirs — and it is logged
separately rather than guessed at here. See
`TD-C-A-USER-CHOSEN-EVENT-COLOUR-CAN-VANISH-INTO-THE-TODAY-DISC` below.

**While you are in there — a second, unrelated hole in the same module.**
`CalendarPopup::render_tray_clock` (line ~2531) is a one-line delegate to
`clock.render(...)`, and it is called from **nowhere in the tree** — not by
production code, not by any of the module's hundred-odd tests, every one of
which calls `clock.render` directly. So the delegate could drop an argument,
transpose `x` and `y`, or (after the conversion) hand the clock the wrong
palette, and nothing would fail. This is the same shape as the `window_peek`
manager delegate found during module 41: *a test that calls the inner function
cannot see a defect in the outer delegate — a delegate is a site.* The
conversion needs at least one test that goes through `render_tray_clock`
itself, or the sweep will report a defect there as escaped and be right to.
