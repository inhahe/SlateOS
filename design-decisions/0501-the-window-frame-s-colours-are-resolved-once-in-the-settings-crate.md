## 501. The window frame's colours are resolved once, in the settings crate

**Date:** 2026-08-21
**Lane:** C
**Decided by:** Claude (autonomous)

**In short:** Two programs draw the title bar at the top of every window — the
compositor (the program that owns the screen) and the desktop shell (the program
that draws the taskbar). Until now each one had its own private list of colours,
and they did not match: if you switched the desktop to light mode, the shell
switched and the compositor did not, so you got a dark title bar on a light
desktop. The same was true of the accent colour and of the interface font size.
The fix is that neither program is allowed to choose any more. There is now one
list of frame colours, in the crate that owns the settings file, and both read
it.

### The problem, precisely

The compositor's `DecorationTheme` was twelve hardcoded constants — a dark navy
desktop, a blue-gray title bar — with a `Default` impl and no other constructor.
It was marked `#[allow(dead_code)]`, which is what let one of its twelve fields
(`close_button_hover`) sit unread for the whole life of the type.

Three of the user's settings had no route into it:

| Setting | What the shell did | What the compositor did |
|---|---|---|
| `theme_mode` (light/dark) | switched between Latte and Mocha | dark navy, always |
| `accent_titlebars` + `accent_color` | recoloured the focused title bar | ignored |
| `fonts.ui_size` | drew titles at the user's size | 16px, always |

Increment 4b/4c had already routed the two *shape* settings (window corners,
drop shadows) into the compositor. Colours and the title font were what was
left, and they are the reason `known-issues.md`'s
`TD-C-THE-DESKTOP-AND-THE-COMPOSITOR-BOTH-DRAW-WINDOW-TITLE-BARS` could not
simply be closed by deleting the shell's duplicate: deleting first would have
shipped a visible regression for every user in light mode, every user with an
accent colour, and every user who had enlarged the interface font.

### Why the resolution lives in `gui/appearance` and not in the compositor

The obvious cheap fix was to copy `DesktopTheme::from_settings` — the light/dark
table and the accent derivation — into the compositor. That was rejected. The
copy would have been a second, independently-editable answer to "what colour is
a title bar", which is exactly the defect
`TD-THREE-INDEPENDENT-APPEARANCE-MODELS` describes, reintroduced inside the very
entry that exists to remove a duplicate. Two renderers agree about a colour only
if neither of them decides it.

So `appearance::DecorationColors` holds the eleven resolved colours, with
`for_mode(light)` for the base palette and `from_settings` for the palette after
the user's accent choice. `gui/appearance` is where it belongs because it is
the crate that already owns the settings, already depends on `guitk` for
`Color`, and is already depended on by both renderers — adding it there created
no new edge in the dependency graph.

`readable_on` and `emphasized` moved there with it (the shell still needs them
for the taskbar), and the shell's private `with_alpha` was deleted in favour of
`guitk::theme::with_alpha`, which had been there all along.

### Why the compositor still has a type of its own

`DecorationTheme` survives, reduced to `from_settings` plus eleven `u32` fields.
It is `DecorationColors` with the ARGB packing already done. The alternative —
holding `DecorationColors` and packing at each blit — would convert five colours
per window per frame for a value that changes only when the user changes it.
`set_appearance` re-resolves it, which is the one place that already knows the
settings changed.

Its `Default` now defers to `AppearanceSettings::default()` rather than
restating a palette. That is not tidiness: a compositor that has never loaded a
settings file and one that loaded a file saying nothing unusual used to look
different, and the difference was only ever visible on screen.

### The desktop background is in the frame palette, and the shadow is real now

`desktop_bg` is not part of a frame. It is in `DecorationColors` because it is
the surface a frame is seen against, it is painted by the same process from the
same palette, and splitting it out would mean a caller had to find two answers
to assemble one screen.

The shadow colour was the dead field's neighbour and turned out to be a third
opinion: the dead constant said alpha `0x40`, and `render_shadow` actually drew
with a local constant of `40` decimal. The palette now states `rgba(0, 0, 0, 40)`
— what is actually drawn — and `render_shadow` reads it, so the colour of a
shadow is stated in the same place as the colour of the bar above it. Nothing
changed on screen.

### The title font follows `ui_size`, and cannot follow `ui_font`

`(DEFAULT_FONT_SIZE * scale)` became `(appearance.fonts.ui_size * scale)`. A
window title is interface text, and someone who enlarged the interface font
because they could not read 13pt has said something about window titles too.
The default is 13.0 rather than the old 16.0, which is a visible change and the
correct one: the shell's own decorator has drawn titles at 13pt × the role ratio
all along, so 16 was one half of the disagreement rather than a considered size.

`fonts.ui_font` still reaches nothing. `osfont::Family` is `Ui` or `Mono` and
has no lookup by name, so no renderer in this tree can honour a font *family*
— the shell's decorator does not either. That is a missing capability, not a
missing wire, and it is tracked separately.

### Verification

Every test below was confirmed to be a real regression test by putting its bug
back and watching it fail with the message that names it.

| Test | Bug reintroduced |
|---|---|
| `the_two_modes_disagree_about_every_colour_a_frame_is_drawn_with` (appearance) | light `desktop_bg` copied from dark |
| `a_focused_bar_is_legible_against_whatever_accent_it_was_given` (appearance) | accent bar keeps the base foreground |
| `accented_title_bars_leave_the_unfocused_windows_in_the_base_palette` (appearance) | accent applied to the unfocused bar too |
| `the_mode_still_decides_the_palette_when_the_accent_is_off` (appearance) | accent applied unconditionally |
| `readable_on_answers_with_the_palettes_own_extremes` (appearance) | pure black and white instead of the palette's |
| `emphasis_stays_visible_at_both_ends_of_the_range` (appearance) | emphasis darkens unconditionally |
| `the_users_theme_mode_reaches_the_desktop_behind_the_windows` (compositor) | the twelve hardcoded colours |
| `the_users_accent_reaches_the_title_bar_it_asked_for` (compositor) | as above |
| `an_accent_title_bar_leaves_every_other_window_alone` (compositor) | as above |
| `the_users_interface_font_size_reaches_the_window_title` (compositor) | the title font back to a constant |
| `the_shells_window_colours_are_the_compositors_window_colours` (desktop) | the shell recolours a title bar itself |

The last one is the load-bearing one. It fails the moment someone recolours a
window in `DesktopTheme`, which is the natural place to do it and the wrong one,
and it will keep failing for as long as the shell's duplicate decorator exists.
When that decorator is deleted, the test goes with it.

The compositor tests take their expected colours from `appearance`, never from
`comp.theme` — reading the answer back out of the thing under test is precisely
what the old hardcoded palette would also have passed.
