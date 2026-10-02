## 1435. A theme chooses the shapes of the controls as an axis of its own; colour, and anything that would move a click, stay out of it

**Date:** 2026-09-28 &middot; **Decided by:** Claude (operator-approved scope:
`roadmap-detailed.md` → *Tier 2 — Widget Styling* asks for the axis and its
four items; which settings it has, their spellings and their limits are
Claude's call) &middot; **Lane:** C

**In short:** A theme can now say how the controls are shaped, not only what
colours they are: how round a button's corners are, how much room its label
has and whether its face has the glassy brighter top, how a text box shows it
is being typed in, how wide a scrollbar is and whether it stays out of the way,
and whether an on/off setting is a pill or a tick box. The user picks which
theme's shapes to use separately from its colours, the way icons already work.
A few things were kept out on purpose: a theme cannot choose the colour of the
"this is selected" ring (it could make it invisible), cannot make a scrollbar
wider than the strip that answers the mouse, and under high contrast cannot
keep the choices that make things harder to see.

**The file.** A `widget-style` section in `theme.yaml`, named as the axis is
in `meta.supports`; `theme.widget_style: <name>` in `appearance.yaml` chooses
it, next to `theme.colors` and `theme.icons` (and named after the capability
`ui.theme.widget_style`). The shipped `aero/theme.yaml` writes every setting
out as the template:

| Setting | Values | The built-in theme's | From |
|---|---|---|---|
| `button.radius` | 0-14 px (14 is a pill) | 4 | `.aero-srch-btn` |
| `button.padding` | 4-24 px either side of the label | 14 | its `padding: 0 14px` |
| `button.gloss` | the upper half a shade brighter | true | its gradient |
| `button.shadow` | a soft shadow under it | false | it has none |
| `field.radius` | 0-13 px | 3 | `.aero-srch-in` |
| `field.border` | `box` or `underline` | box | its 1px border |
| `field.focus` | `ring`, `glow` or `underline` | glow | `.aero-srch-in:focus` |
| `check.radius` | 0-7 px (7 is a circle) | 2 | the toolkit's box |
| `toggle` | `pill` or `checkbox` | pill | every switch drawn so far |
| `scrollbar.width` | `thin` (6) or `normal` (10) | normal | the toolkit's 10 |
| `scrollbar.visibility` | `always`, or `overlay` (a thin line until the pointer comes to it) | always | the browser's |

As with colours, what a section leaves out is the built-in theme's, and a value
that cannot be read costs that value and is listed for the theme's author. A
radius past the roundest is drawn as the roundest, with a note, since that is
plainly what was meant. Every measure is a whole number of pixels: a `Palette`
carries the style and is compared for equality, so the style must be `Eq`.

**How it reaches a control.** `AppearanceSettings::read_from` loads the chosen
theme's section with the settings, as it loads the colours, and
`Palette::from_settings` puts the result on the palette
(`Palette::widget_style`). Every control is already handed a palette, so none
needs a new argument, and every application gets the style with its colours
through `oswindow`. The settings watcher's fingerprint covers the widget
theme's file as it covers the colour theme's.

**What was kept out, and why:**

| Left out | Why | What would bring it in |
|---|---|---|
| **Focus colour** | a focus mark is drawn over grounds nobody can list in advance, so a theme's colour could be one that vanishes; `guitk::style::FOCUS_RING_WIDTH`'s note already refuses the hue as a user setting for this reason. The mark is always the accent, at least the user's focus width -- or red, on a field whose content is wrong: one signal, rather than a red edge inside a ring of the accent, which reads as a field both fine and wrong | nothing: this is the rule |
| **Scrollbar colour** (the roadmap's item names it) | it is the colours axis's already: the thumb is `surface2`, whose documented job is "a scrollbar thumb" | nothing: a theme sets `surface2` |
| **A field with no edge** | a well on a page of nearly its own shade is a field nobody can find; `underline` keeps a line where the typing goes | nothing |
| **A scrollbar wider than its column, or one that takes no column** | the toolkit answers a click by laying a widget out again with any palette to hand -- "where things land does not depend on colour" -- so a style that moved a hit region would put the click somewhere other than the drawing. The column is the same in every theme; the style draws the bar inside it, and an overlaid bar is a thin line rather than nothing, still saying where the view is | a widget that keeps the style it last drew with for its clicks, as the dialogs will have to for padding |

**High contrast.** A high-contrast scheme is chosen for need and replaces the
colours whole. It keeps the theme's shapes -- round or square, pill or box, a
user who needs contrast can see either -- but puts back the four choices that
make a control show less plainly: no gloss (a second ground under a label), no
shadow (a soft edge), a ring for focus (a glow or an underline says less), and
a scrollbar that stays (`WidgetStyle::for_high_contrast`).

**The alternatives for the model:**

| Option | For | Against |
|---|---|---|
| **A fixed set of named settings per control** (chosen) | each is checked, bounded and documented; a theme cannot ask for what the toolkit cannot draw | a new look needs a new setting |
| A CSS-like property sheet (`design.txt` 772 floats "a subset of CSS") | open-ended | `design.txt` itself records the recommendation against CSS; every property is a promise every control must keep, and themes are "pure data" that must not reach layout |
| Free numbers for the scrollbar width | finer choice | the bar lives in a fixed column, so there is room for two looks -- thin, and the column itself -- and a theme gains nothing from a third |

**Button padding, and why it could be let in.** A button's padding decides
its width, and so where every button after it in a row begins -- exactly the
kind of choice the "where things land" rule forbids a style. The one row that
measures its buttons is the toolkit's alert dialog, and it already tests a
click against the rectangles its last `render` recorded rather than against a
second layout; so it adopts the palette's button style at the start of
`render` and measures the row with it, and the click follows the drawing in
any theme. (Kept out at first, then built the same day, once that was
checked.)

**What honours it:** the toolkit's button, its padding in the alert dialog's
row; every toolkit text field, drawn
by one function now (`guitk::field` -- the drop-down, the address bar, the
input dialog, the Save box and the colour picker's hex field had each drawn
their own, and had drifted); the check box; the switch, drawn as a box in
the pill's room under `toggle: checkbox`, the shell's settings pages
included; the file dialog's and the tree view's scrollbars
(`guitk::scrollbar::draw`); and the shell's own text fields -- the Run box,
the start menu's search, the login screen's password, the shortcut editor
and an icon being renamed -- which now also draw their focus marks at the
user's focus width, which the shell had never read. The start menu's search
takes the theme's box but no focus mark: it has the keyboard whenever the
menu is open, and the reference draws none. Still to follow: lane E's
applications and Settings picker
(`requests/c-e-a-theme-can-shape-the-controls.md`) -- each noted in
`roadmap-detailed.md` → *Tier 2 — Widget Styling*.
