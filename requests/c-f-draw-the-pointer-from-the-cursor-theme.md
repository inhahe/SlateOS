# C -> F: draw the pointer from the cursor theme the user chose

**From:** Lane C. **To:** Lane F. **Filed:** 2026-10-01.
**Status:** OPEN. Nothing changes on screen until it is done: with no cursor
theme chosen -- everyone today -- the pointer is the compositor's own drawing,
as now.

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
