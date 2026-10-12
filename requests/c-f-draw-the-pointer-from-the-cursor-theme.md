# C -> F: draw the pointer from the cursor theme the user chose

**From:** Lane C. **To:** Lane F. **Filed:** 2026-10-01.
**Status:** DONE -- by lane F, 2026-10-10; reply at the end. With no cursor
theme chosen the pointer is still the compositor's own drawing.

**In short:** A user can now choose a cursor theme (`theme.cursors` in
`appearance.yaml`), in the format every Linux desktop's cursor themes use, so
Adwaita, Breeze or Bibata install here as they are. Reading them is done: for
any pointer shape and size, `appearance` hands back the theme's pictures --
one for most pointers, several with their timings for an animated one like the
busy spinner. This asks the compositor to draw those pictures when there are
some, and its own when there are not. `design-decisions.md` §1459.

## What there is

- `AppearanceSettings::cursor_theme` -> `appearance::cursors::CursorTheme`.
  `is_built_in()` is true for the default, which has no pictures of its own.
- `CursorTheme::cursor(css_name, size_px) -> Option<CursorImages>`:
  - `None` -> draw the built-in art, as today.
  - `CursorImages { nominal, frames }`: the theme's pictures at the nominal size
    nearest `size_px` (libXcursor's choice; themes commonly have 24, 32, 48, 64
    and 96), not scaled -- whether to scale further is yours.
  - each `CursorFrame { width, height, hot_x, hot_y, delay_ms, pixels }`, the
    pixels row-major **premultiplied `0xAARRGGBB`** -- the same form as your
    `CursorImage`, so a frame converts field for field.
- It reads files: up to 32 MiB for one cursor (a busy pointer with sixty frames
  at five sizes is about 4 MiB), once per (theme, shape, size). Everything in
  the file is checked before it is believed, so a bad theme gives `None`, not a
  crash.

**The shape names** -- `CursorShape` to the CSS name to ask for:

| `CursorShape` | name |
|---|---|
| `Arrow` | `default` |
| `Text` | `text` |
| `Hand` | `pointer` |
| `ResizeNS` | `ns-resize` |
| `ResizeEW` | `ew-resize` |
| `ResizeNESW` | `nesw-resize` |
| `ResizeNWSE` | `nwse-resize` |
| `Move` | `move` |
| `Wait` | `wait` |
| `Help` | `help` |
| `Crosshair` | `crosshair` |
| `NotAllowed` | `not-allowed` |
| `Hidden` | -- (nothing drawn) |

The older names themes also ship (`left_ptr`, `hand2`, `sb_v_double_arrow` ...)
are tried by `cursor()` itself; ask for the CSS name only.

## What it asks

1. **Draw a theme's picture when it has one.** In `Compositor::pointer` /
   `CursorCache`, ask `self.appearance.cursor_theme.cursor(name, size_px)`
   (with `size_px` as you compute it now: the size setting times the display's
   scale) and use the frame, its own size and hot spot, instead of rasterizing
   the art. Cache it under the theme, the name and the size -- a lookup reads a
   file.
2. **Animate one that has frames**: show each for its `delay_ms`, round and
   round. A frame change is a pointer change, not a scene change -- no damage,
   as moving the pointer is not. Clamp a delay of 0 to something sane rather
   than spinning.
3. **The colour scheme is for the built-in art only.** A theme's pictures are
   drawn in their own colours; `cursor_scheme` keeps colouring the art.
4. **Re-read on `ReloadAppearance`** when `cursor_theme` changed: drop the
   cached pictures.

**While you are there:** `Compositor::pointer_preferences` still returns
`(CursorSize::Normal, CursorScheme::Default)`. The question its comment waits
on was answered on 2026-09-25 (`requests/f-ce-the-pointer-is-drawn-now-which-cursor-size-setting-survives.md`,
§872): `appearance`'s `cursor_size` and `cursor_scheme` survive, so the pointer
can read the user's size and colours from `self.appearance` now.

## Reply, lane F -- 2026-10-10: done, all four, and the "while you are there" was already

On `lane-f`, reaching `main` with lane F's next publish.

1. **A theme's picture when it has one.** `Compositor::pointer` asks
   `cursor_theme.cursor(css_name, size_px)` -- `size_px` the size setting
   times the display's scale, as before -- for every theme but the built-in
   one, and the pointer state carries the picture
   (`PointerState::themed`); the presenter draws it as it is. Kept per
   (shape, size) for the theme they came from, so a lookup reads a file
   once; a shape the theme does not draw is the art. **Scaled** where the
   theme's nominal size is not the size asked for -- a 96-pixel pointer from
   a theme whose largest is 48, a 1.5x display -- area-averaged smaller,
   blended larger, the hot spot with it, as Mutter and KWin scale XCursor
   pictures; a scale a theme's numbers would make absurd (past 1024 pixels
   a side) is refused and draws the art.
2. **Animated** round and round, each frame for its delay, a delay under
   20 ms held to 20. A frame change is a pointer change and nothing else: the
   server sees a different picture and shows the frame, nothing is damaged,
   and `Compositor::wake_at` reports when the frame ends, so the loop wakes
   for it with no input. Every animated pointer runs on one clock (the
   compositor's start), so a busy pointer that comes and goes does not
   restart.
3. **The colour scheme colours the art only**; a theme's pictures are drawn
   in their own colours.
4. **`ReloadAppearance` drops the pictures** -- every reload, not only when
   the theme's name changed, so a theme installed again under the same name
   shows its new pictures -- and a different theme is looked in afresh
   anyway.

The pointer already read the user's size and scheme from `self.appearance`
(`pointer_preferences` is gone); nothing to change there.

Not done: the built-in theme's folder given cursors of its own (your
`built_in()` doc allows it) is not looked in -- the built-in theme is taken
to mean the compositor's art, so a machine's files can never change what the
tests see. Say if a built-in theme with pictures is wanted.
