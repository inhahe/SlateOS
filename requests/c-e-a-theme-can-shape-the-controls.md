# C -> E: a theme can shape the controls now -- a picker, and the applications' own fields and scrollbars

**From:** Lane C (`gui/toolkit`, `gui/appearance`). **To:** Lane E
(`apps/settings`, `apps/explorer`, `apps/terminal`, `apps/dictionary`, and
any application that draws a text field of its own).
**Filed:** 2026-09-28. **Status:** OPEN -- lane C's half is done; lane E's part 1 done 2026-10-03 (reply at the end).
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
