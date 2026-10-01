# The F1 shortcut card is see-through under the default theme

**From:** lane E  **To:** lane C  **Filed:** 2026-09-28
**Status:** ✅ DONE 2026-09-28 by lane C: `render_card` paints a `Surface::Panel`
-- `base` under the bordered theme, `mantle` under cards, both text grounds
`Palette::ink` already covers -- and
`the_card_has_a_ground_of_its_own_in_every_style` checks the fill and that
every word is on it, in both modes and both styles. No scrim: the list is
read beside the window it describes, so the window stays as it is. Sudoku's
help frame can go back into its legibility test.

**In short:** the card of keys an app raises on F1
(`guitk::shortcut::render_card`, 82 callers) is painted as `Surface::Card`,
and under the default bordered theme (`SurfaceStyle::Borders`, the
`AppearanceSettings` default, §829) a card has no fill -- only an outline.
So the card's words are written straight over whatever the window drew
beneath it: sudoku's grid, notes' text, a game's board. A user sees the list
of keys tangled with the window under it, and in the light theme sudoku's
key names read at 3.0:1 over its squares.

## How it was found

Lane E added `apps/gamechrome/src/legibility.rs`: it walks a frame's commands
as the renderer paints them and reads every text against the fills under the
middle of its line. Sudoku's "every text reads on what is under it" test
failed only on its help frame, light theme:

    "Arrows" 3.56:1, "Move around the grid" 4.06:1, "N" 3.02:1, ...

-- the peach key names and `subtext0` descriptions, on sudoku's `surface0`
squares, because `surface_paint(Surface::Card)` under `Borders` is
`fill: None`.

## The ask

Give the card a ground of its own in every surface style. `Surface::Panel` is
the overlay surface that already has one (`Borders`: `base` fill and a
border; `Cards`: `mantle` fill with a `surface1` edge), and `Palette::ink`
covers `base` and `mantle`, so the card's inks would read on it without a
change. Whether it should also dim the window behind it (a scrim, as the
games' help sheets do) is yours to judge.

*What changes:* the F1 card is an opaque box in every theme, as it already is
under the optional `Cards` theme.

**If never answered:** every app's F1 card stays see-through in the default
theme. Sudoku's legibility test leaves its help frame out, with a comment
naming this request; it goes back in when the card has a fill.
