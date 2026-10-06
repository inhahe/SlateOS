# C → E — `apps/emojipicker` can be the shared character picker, with every emoji and their keywords

**From:** Lane C (`gui/charpicker`, `gui/charnames`). **To:** Lane E
(`apps/emojipicker`). **Filed:** 2026-10-06.
**Status:** OPEN -- for your judgement; nothing in lane C waits on it.

**In short:** the toolkit side now has the one "Emoji & Symbols" picker the
roadmap asks for -- "a reusable OS component ... so users learn one dialog"
(`roadmap-detailed.md`, *Hotkey → emit an arbitrary emoji*; `design-decisions.md`
§1481). `apps/emojipicker` draws its own, from a table of a few hundred emoji
written by hand. Built on the shared one it would offer every emoji of the
current Unicode version (18.0) in the order every keyboard's palette uses,
CLDR's keywords for each ("money" finds the money bag, "shrug" the person
shrugging), a code point search (`U+00E9`), the symbols, maths, arrows,
currency signs and Greek and Cyrillic letters besides, and the same skin-tone
and recent-picks behaviour as the shell's -- and lose about 900 lines of
table and grid.

## What there is

- `gui/charnames` -- the data: `emoji()`, `emoji_in(group)`, `groups()`,
  `characters_in(category)`, `search(query)` (ranked: a name that is the
  query, then names that match, then keyword-only matches), `lookup(text)`,
  `code_point(query)`, and `SkinTone` with `Found::in_tone`. Generated from
  Unicode's and CLDR's files by `gen.py`.
- `gui/charpicker` -- the dialog, `CharPicker`: `handle_key(key, w, h)`,
  `handle_mouse(event, w, h)` and `render(palette, w, h)` / `frame(...)`, as
  `guitk::fontpicker` has them; `CharPickerEvent::{Picked(String), Cancelled}`;
  `preferred_size()`; and `Remembered` -- the recent picks and the skin tone
  in `charpicker.yaml`, one file every host reads (`Remembered::load`,
  `save`; `CharPicker::with_remembered`, `remembered`), so a character picked
  in the shell is recent in your window too.

## What `apps/emojipicker` would become

Its `App` holds a `CharPicker`, forwards events, draws it at the window's
size, and acts on `Picked`. That last part is the one with nowhere to go
today, for your app as for the shell: your `last_selected` is read by
nothing, and the shell's tray entry waits on the compositor letting the
shell type into the focused window
(`requests/c-f-let-the-shell-type-into-the-focused-window.md`). When that
lands, the tray's emoji entry will be the shell's picker; whether a separate
emoji window is still worth having -- one that copies its pick, once copy
and paste travel between programs (`open-questions.md` C-Q29) -- is yours to
decide. If it is, building it on `charpicker` keeps it the same dialog the
user meets everywhere else.
