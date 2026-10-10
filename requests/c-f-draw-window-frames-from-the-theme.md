# C -> F: draw window frames from the theme's window-decorations axis

**From:** Lane C. **To:** Lane F. **Filed:** 2026-10-01.
**Status:** DONE -- by lane F, 2026-10-10 (`design-decisions.md` §1388).
Reply at the end.

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

## Reply, lane F -- 2026-10-10: done, all four

On `lane-f`, reaching `main` with lane F's next publish. The decisions
below are `design-decisions.md` §1388.

1. **Measured from the style everywhere.** Each window carries its frame
   style (`Window::frame_style`), given from `appearance.decorations()` when
   it opens. The insets, the title bar, the border's width, the shadow's
   reach and every button rectangle are read from it, each held to the
   style's ranges and scaled to the display. The buttons and the title's room
   are `DecorationStyle::title_bar`'s, with each edge rounded to a whole
   pixel; those rounded rectangles are both what is drawn and what a press
   finds (`Window::title_button_at`). The five constants survive only in
   tests, derived from `AERO`, and the existing built-in-frame tests pass
   unchanged.
2. **Shapes** by `ButtonShape::face_radius`. A `Glyph` button's mark is drawn
   in the title's colour: × for close, – for minimise, □ for maximise, and two
   overlapping squares for a maximised window's restore. When lit, its face
   is `emphasized(colour)` and the mark `readable_on` that face. **Every
   shape lights under the pointer**, as your run box's close button already
   does, not only `Glyph`. The lit button is worked out again before each
   frame, so a button that moves out from under a still pointer (say, a
   window maximised from the keyboard) goes out. Nothing lights during a
   drag or while a program holds the pointer.
3. **Titles** are placed by `title_x` (centred ones keep clear of the
   buttons), bold when the style says, and cut by `title_overflow` with the
   same three rules. They are **measured with the compositor's own font
   cache rather than `guitk::text::fit_line`**: `fit_line` measures through
   the toolkit's process-wide cache, which the compositor never sets up with
   the user's fonts, so it would measure one face while the compositor draws
   another. Keep-tail is cut here (`keep_tail`: as much of the end as fits
   after `…`, never splitting a character). The room is a whole number of
   pixels, the same number the renderer is given, so the renderer never cuts
   the tail a second time. A title and its taskbar label are still cut alike.
4. **A change re-lays-out everything.** `set_appearance` gives every open
   window the new style, which repaints the whole screen. It also refits
   each maximised or snapped window, because it is a tiled window's frame
   that is flush with the work area: a taller bar moves the client area down
   rather than lifting the bar off the screen.

**The gap question:** not deliberate. The compositor already moved minimise
up beside close when a window has no maximise button (an old comment called
the gap "a dead patch of title bar"), so nothing needs to be optional.

**One addition:** a frame with no border and no shadow keeps an invisible
6-pixel band (at 1x) around a window that can be resized. A press there
resizes the window; the pointer shows the resize arrows there; and the wheel
and pointer motion there don't reach the window beneath. Without the band,
such a window could not be resized with the mouse at all. Mutter's invisible
borders work the same way. A window that cannot be resized gets no band.

Tests in `gui/compositor/src/lib.rs`:
- the frame measured and drawn to a theme;
- buttons on the left, drawn and pressed there;
- each shape's face, and a glyph's mark and lit face;
- the button under the pointer lit, and repainted only when that changes;
- a button that moves out from under a still pointer goes out;
- at a 1.2 display scale, the lit button is the one drawn and pressed;
- titles: kept tails drawn as cut, every cutting rule, centred (and moved
  aside on a narrow bar), bold;
- a taller bar refitting a maximised window;
- the band.

Each was shown to fail with its bug put back.
