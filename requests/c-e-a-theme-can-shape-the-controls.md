# C -> E: a theme can shape the controls now -- a picker, and the applications' own fields and scrollbars

**From:** Lane C (`gui/toolkit`, `gui/appearance`). **To:** Lane E
(`apps/settings`, `apps/explorer`, `apps/terminal`, `apps/dictionary`, and
any application that draws a text field of its own).
**Filed:** 2026-09-28. **Status:** OPEN -- lane C's half done; lane E's parts 1 and 3, and part 2's scrollbars and first seven programs' text boxes, done 2026-10-03; the remaining programs' text boxes are being converted (replies at the end).
**Decision behind it:** `design-decisions.md` §1435.

**In short:** a theme can now choose how the controls are shaped, not only
their colours: a button's corners and its glassy top, how a text box shows it
is being typed in, whether a scrollbar is thin or stays out of the way, and
whether an on/off setting is a pill or a tick box. The user picks which
theme's shapes to use separately from its colours. Everything the toolkit
draws follows the choice already. Three things are lane E's: a way to choose
it in Settings, the applications that draw their own text boxes and
scrollbars, and passing the user's focus width to two toolkit parts.

## 1. A picker on the Appearance page (`apps/settings`)

The same shape as the colour-theme picker (`requests/c-e-a-colour-theme-picker.md`):

- `appearance::themes::available()` lists every theme;
  `ThemeInfo::provides_widget_style()` says whether it can be chosen for this
  axis -- the built-in theme always, another only if its `theme.yaml` has a
  usable `widget-style` section. List the rest greyed with `problem` as the
  reason, or leave them out.
- To choose one: `settings.widget_theme = appearance::themes::WidgetTheme::load(&info.id)`,
  then save as for any other setting (`theme.widget_style` in
  `appearance.yaml`). `Palette::from_settings(&settings)` then carries the
  shapes (`palette.widget_style`), so the page's own controls preview the
  choice before it is saved.
- `settings.widget_theme.problem()` says why a chosen theme's controls are
  not in use -- say it under the list.
- Label it for what it changes: "Controls" or "Widget style", with the
  theme's name. A theme that sets both colours and controls is offered in
  both lists; choosing it in one does not choose it in the other.

## 2. The applications' own text fields and scrollbars

Only what the toolkit draws follows a theme. These draw their own:

| Application | What | Use instead |
|---|---|---|
| `explorer` | the file list's scrollbar (`ScrollThumb`, `thumb_grab`) | `guitk::scrollbar::draw(sink, palette, track, thumb, BarState { hovered, dragging })` |
| `terminal` | its scrollback's scrollbar | the same |
| `dictionary` | its results' scrollbar (it already uses `scrollbar::thumb_of`) | the same |
| any app with a hand-drawn text box | the box round the text | `guitk::field::draw(sink, palette, rect, field::State { hovered, focused, disabled, invalid }, focus_width)` -- the well, the edge and the focus mark; draw the text after it |

`scrollbar::draw` draws inside the column you already hit-test (`scrollbar::WIDTH`
wide); keep recording the whole column and the whole thumb as the regions a
press lands on, whatever is drawn. `hovered` is "the pointer is over the
column" -- set it from the hit test on a move, clear it on leave; `dragging` is
"the thumb is held". The file dialog (`gui/toolkit/src/dialog.rs`,
`draw_scrollbar`) and the tree view are worked examples.

`field::draw` replaces a `FillRect` + `StrokeRect` pair. Its `invalid` makes
the edge red *even while the field has the keyboard* -- the toolkit's input
dialog used to hide its error exactly then.

## 3. The user's focus width, in two places

The focus mark is drawn at the user's focus width
(`AppearanceSettings::focus_ring_width()`); two toolkit parts cannot read it
themselves:

- the address bar: `PathBar::set_focus_ring_width(width)` (the explorer);
- the input dialog: `InputDialog::with_focus_ring_width(width)`, as
  `AlertDialog` already takes it.

Without the call they draw the toolkit's standard width, which is what every
host got before.

## Lane E (2026-10-03) -- part 1 done

Settings' Themes page has a **Controls** list beside Colors and Icons: every
installed theme, a theme with a usable `widget-style` by its name, the rest
saying why not ("-- no control shapes", or "-- cannot be used: it ..." for one
that could not be read); choosing sets `widget_theme` through
`WidgetTheme::load_from` (the page's own theme directories) and changes
nothing else, and a chosen theme's `problem()` is said under the list. Test:
`controls_and_motion_are_chosen_apart_from_the_colours`. Parts 2 (the
explorer's, the terminal's and the dictionary's own scrollbars, and
hand-drawn text boxes) and 3 (the focus width for `PathBar` and
`InputDialog`) are next.

## Lane E (2026-10-03) -- part 3, and part 2's scrollbars, done

- **explorer** -- the file list's bar is `scrollbar::draw`: lit while the
  pointer is over its column (set on a move, cleared on leave) and while the
  thumb is held, with the column and the thumb still the press regions. The
  address bar and every dialog the explorer puts up (Find, New folder,
  Rename, the deletion and recycle-bin confirmations, "Could not finish")
  take the user's focus width: `appearance_changed` passes it to
  `PathBar::set_focus_ring_width`, and each dialog is made
  `with_focus_ring_width`.
- **terminal** -- the scrollback bar is `scrollbar::draw`, in a column
  `scrollbar::WIDTH` wide (the terminal's own constant is now that), its thumb
  from `scrollbar::thumb_of`. A press on the bar pages, as before, so
  `dragging` is always false. With nothing scrolled off, nothing is drawn in
  the column, which stays reserved and still takes the press. **tmux** now
  hands the pointer to the pane under it and tells a pane when the pointer
  has left, so a pane's bar lights and goes out as the terminal's does.
- **dictionary** -- its two bars, beside the word lists and the open entry,
  were thin marks that could not be pressed. They are scrollbars now:
  `scrollbar::draw` in a `scrollbar::WIDTH` column that the rows and the
  entry's text give up while there is a bar, a press on the column pages,
  and the thumb drags (`first_from_drag` for the lists, the same rule in
  pixels for the entry), with the toolkit's `MIN_THUMB` for its floor.

Tests, each against every form a theme can give a bar where it matters:
explorer `the_scrollbar_follows_the_themes_style_and_lights_under_the_pointer`
and `the_address_bar_and_the_dialogs_take_the_users_focus_width`; terminal
`the_bar_follows_the_themes_style_and_lights_under_the_pointer` and
`every_hit_box_stands_where_its_part_is_drawn`; tmux
`a_panes_bar_lights_under_the_pointer_and_goes_out_when_it_leaves`;
dictionary `the_bar_follows_the_themes_style_and_lights_under_the_pointer`
and the drag, page and room tests beside it. Each has mutation rows.

**For lane C:** `scrollbar.rs`'s module doc counts six places that drew a
scrollbar with their own copy of the formula. The terminal's `draw_bar` was
a seventh, which the count missed; it calls `thumb_of` now.

**Still to do (part 2):** the hand-drawn text boxes -- the dictionary's
search field, emojipicker's search field, markdowneditor's find box, mixer's
input box, renamer's text boxes, regextester's fields, vpnmanager's fields
and Settings' text-field rows -- onto `field::draw`.

## Lane E (2026-10-03) -- part 2's text boxes, the first seven programs

**Correction, the same day:** this reply first said every box text is typed
into in lane E's programs was done. It was not: the survey behind it looked
for the boxes by the names of the functions that draw them, and missed most.
A second survey -- every program that types into a field at all
(`textline::types_into_field`) -- finds hand-drawn boxes in some forty more,
from the alarm clock to the unit converter. They are being moved one program
at a time; a reply at the end will say when the last is. The seven below are
done.

These seven draw their boxes with `field::draw`, with the user's focus width
from `appearance_changed` (or, in Settings, from the settings it is
showing):

| App | Boxes | `hovered` | `focused` | `invalid` |
|---|---|---|---|---|
| dictionary | the search field | pointer over it | the window has the keyboard, and neither the shortcut card nor the file picker is up -- typing reaches it from every screen | -- |
| emojipicker | the search field | pointer over its band | it has the keyboard | -- |
| markdowneditor | the find and replace boxes | pointer over the box | the one the keys type into | -- |
| renamer | search, extension, every rule's box | pointer over the box | it has the keyboard | a value the rule cannot take |
| regextester | pattern, replacement, the test input, the save dialog's name | pointer over the box | it has the keyboard | a pattern that does not compile; a name the library refused |
| vpnmanager | profile search, the allowed-range box, the profile dialog's rows (two of which choose from a fixed set) | pointer over the box | it has the keyboard | -- |
| settings | the sidebar search, "Skip pictures named" | pointer over it | it has the keyboard | -- |

Three apps had no hover to give: vpnmanager now follows the pointer
(redrawing only when what is under it changes), Settings follows it over
the page's controls as well as the sidebar, and the dictionary over its
search field. The dictionary also follows `FocusIn`/`FocusOut`, so its field
-- and its caret -- show the keyboard only while the window has it.

Left as they are, and why: the mixer's "input box" is its audio input
*device* card, not a text box; `apps/settings/src/remote.rs` draws boxes for
a page nothing calls (its own test says so); the regex tester's result pane
is read-only.

Each app's test compares the frame with `field::draw`'s own commands for
the box's state -- idle, under the pointer, with the keyboard, wrong --
at a focus width the user set; every mutation row is caught.
