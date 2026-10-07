# C -> F: draw window frames from the theme's window-decorations axis

**From:** Lane C. **To:** Lane F. **Filed:** 2026-10-01.
**Status:** OPEN. Nothing changes on screen until it is done: the built-in
frame is exactly the one the compositor draws now.

**In short:** A theme can now say what windows' frames look like -- how
tall the title bar is, whether the title sits at the left or in the
middle, which end the minimise, maximise and close buttons are at and in
what order, whether they are rounded, circles, squares or just their
marks, how big they are, how wide the border is and how far the shadow
reaches. The user picks a theme for this separately, as for colours. The
compositor still draws from its own constants (`TITLE_BAR_HEIGHT`,
`TITLE_BUTTON_SIZE`, `TITLE_BUTTON_SPACING`, `BORDER_WIDTH`,
`SHADOW_SIZE`); this asks it to draw -- and hit-test -- from the setting.

## What there is

- `appearance::AppearanceSettings::decorations()` -> `DecorationStyle`
  (`appearance::decorations`): `title_height`, `title_align`
  (`Left` / `Center`), `title_bold`, `title_overflow`, `button_side` (`Left` / `Right`),
  `buttons` (the three, left to right as drawn), `button_shape`
  (`Rounded` -- today's: as round as the window's corners -- `Circle`,
  `Square`, `Glyph`: no face, the button's mark alone, lit under the
  pointer), `button_size`, `button_gap`, `border`, `shadow`. Sizes are
  pixels at scale 1.
- `DecorationStyle::AERO`, the built-in, equals today's constants: 30, left,
  regular, right, minimise-maximise-close, rounded, 20, 4, 1, 8.
- **The geometry, so drawing and clicking agree:**
  `style.title_bar(bar_rect, scale, |b| window_has(b))` -> `TitleBarGeometry`
  with each button's `Rect` (`button(TitleButton::Close)`, `button_at(x, y)`)
  and the title's room (`title`), and `title_x(text_width)` for where the
  title starts (centred titles keep clear of the buttons). It reproduces
  today's positions exactly for AERO (tests in `appearance::decorations`),
  with one difference: a window with no maximise button packs minimise next
  to close instead of leaving a gap where maximise would be -- tell me if
  the gap was deliberate and I will make it optional.

## What it asks

1. Read `appearance.decorations()` wherever the five constants are used: the
   frame insets (`frame_insets`, `title_bar_rect`), `title_button_rect` and
   the three `*_button_rect`s (or replace them with `title_bar()`), the
   title text's position and weight, the border and `render_shadow`.
2. Draw `button_shape`: `Rounded` as now, `Circle` at radius = half the
   size, `Square` at radius 0, `Glyph` with no face -- the button's mark
   (`×`, `□`/`❐`, `–`) in the title text colour, with the face drawn only
   while the pointer is on it. `ButtonShape::face_radius(size,
   window_radius, lit)` answers the face's radius (or `None`: no face), and
   the shell's run box already draws its close button from it
   (`desktop::dialog_frame`, `design-decisions.md` §1461) -- so a window's
   buttons and the box's agree if the compositor uses it too.
3. Cut a title too long for its bar as `title_overflow` says
   (`guitk::text::Overflow`: clip, ellipsis, or keep-tail -- `…` and the
   title's end), with `guitk::text::fit_line(title, room, size, weight,
   overflow)`, which answers the string and the `TextOverflow` to draw it
   with. The desktop's taskbar already cuts window labels this way, so a
   title and its label are cut alike.
4. Redraw and re-lay-out every window when the setting changes (the
   appearance watcher already fires on `theme.decorations` and on edits to
   the chosen theme's file: `themes::fingerprint` includes it).

The frame's colours are unchanged: the palette's `title_bar`/`title_text`
(`DecorationColors`). `design-decisions.md` §1456.
