## TD-C-THREE-OF-THE-CALENDARS-COLOUR-PAIRINGS-ARE-BELOW-THE-CONTRAST-FLOOR (lane C, 2026-08-23) -- **CLOSED; entry was stale**

**Closed 2026-09-07 (lane C), on verifying it.** All three fixes are in
`calendar.rs`, and its module doc records them with the measured numbers:
adjacent-month day numbers moved `surface2` → `subtext0`, the selected day's
disc `surface1` → `surface0`, and the event-detail body `subtext0` → `text`.
The role-category diagnosis this entry made -- a *fill* role used as an *ink*
is guaranteed low-contrast, so 2.46 and 1.91 were not unlucky values -- is
quoted in the code where the choice is made.

**The test the entry insisted on is there too, and in the form it insisted
on.** `every_pairing_the_calendar_draws_clears_the_contrast_floor` iterates
both modes, reads the roles off `Palette::for_mode`, and *computes* each
ratio -- the numbers in the table above are not copied into any assertion, as
this entry required. It covers eighteen pairings, not the three that were
broken.

**Nothing was done to the code for this closure.** Sixth stale entry closed
today; `todo.txt` carries the note about what that costs and two preventions
neither of which is mine to make.

Original entry follows.

**In short:** Three places in the calendar draw text too close in brightness to
what is behind it to be comfortably readable. The worst is the greyed-out day
numbers from the previous and next month that fill the corners of a month grid:
they measure **2.46:1** against the popup background, where the accepted floor
for readable text is 4.5:1. That one is broken **in the dark theme the shell
actually starts in**, not merely in light mode — it is on screen right now.

**The measurements.** Contrast ratio (WCAG relative luminance), computed for the
pairings as they will read once each side is a palette role. The Mocha column is
what ships today, since the module is currently hard-coded to Mocha:

| Site | Ink / behind | Mocha | Latte |
|---|---|---|---|
| adjacent-month day number | `surface2` on `base` | **2.46** | **1.91** |
| selected day's number | `text` on `surface1` | 6.31 | **4.39** |
| event-detail body text | `subtext0` on `surface0` | 5.65 | **3.40** |
| today's number | `base` on `blue` | 7.79 | 4.63 |
| ordinary day number | `text` on `base` | 11.34 | 7.06 |
| day-of-week header | `subtext0` on `base` | 7.37 | 4.64 |

**Where:** `gui/desktop/src/calendar.rs`. The adjacent-month ink is the `else`
arm of the `text_color` choice in `render_day_cell` (~line 2220); the selected
day is `text` drawn over the `SURFACE1` disc chosen at ~line 2196; the detail
body is ~line 2318/2345 over the `SURFACE0` panel at ~line 2279.

**The root cause of the worst one is a role-category error, not a bad shade.**
`surface2` is a *fill* role — the colour of a raised panel — and it is being
used as an *ink*. Surfaces sit near the background by construction, so a surface
used as text is guaranteed to be low-contrast in whichever mode you look at; the
2.46 and the 1.91 are not unlucky values, they are what the role is for. The
de-emphasised-text roles are `overlay0` and `subtext0`. Note that `overlay0`
does not rescue it either (3.19 Mocha / 2.09 Latte) — it is also below floor —
so the fix has to be `subtext0` (7.37 / 4.64) plus, if the adjacent-month days
must still look recessed, recessing them by something other than contrast:
lighter weight, or simply omitting them.

**The other two are ordinary near-misses** and want a role step: the selected
day's disc moving `surface1` → `surface0` (which darkens the gap in Latte), and
the detail body moving `subtext0` → `text` on its panel.

**How it went unnoticed:** the module has no contrast test of any kind, and
membership sweeps cannot find this class at all — *contrast is not a membership
property*, so both colours in an unreadable pair can be perfectly good palette
members and every sweep will pass. Only a test that computes the ratio finds it.
Module 41 (`window_peek`) is the precedent: its 1.60:1 close-button X had been
shipping since the module was written.

**Trigger:** fix during the `calendar.rs` palette conversion (module 43 of part
2 of `TD-C-FORTY-NINE-SHELL-MODULES-CARRY-THEIR-OWN-COPY-OF-THE-PALETTE`), and
add the contrast test in the same change — the numbers above are predictions
from the role table and must be re-derived by a test that reads the palette, not
copied from this table into an assertion.
