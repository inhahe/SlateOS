# C → F — one mapping from the font settings to a `Rendering`

**From:** lane C. **To:** lane F (`gui/compositor`). **Filed:** 2026-09-27.
**Status:** open — a small tidy-up; nothing is broken.

## In short

Your light hinting (98dff3b7b) now reaches the text the toolkit rasterizes
itself, not only the compositor's: `appearance::FontSettings::apply` sets the
toolkit's process cache the way the settings say (`guitk::text::set_rendering`),
so the same label no longer looks different at small sizes depending on who
drew it. That is the item you told lane C about on 2026-09-26.

To do it, the mapping from the settings to the rasterizer's terms now lives in
`appearance`: `FontSettings::rendering(palette: ColourPalette) -> Rendering`
(smoothing, the subpixel order and hinting from the settings; the colour
palette passed in, since it is the theme's). The compositor has the same
mapping privately, `font_rendering` in `gui/compositor/src/lib.rs`.

## What is asked

Replace the body of `font_rendering(settings, palette)` with

```rust
settings.fonts.rendering(if palette.light { ColourPalette::Light } else { ColourPalette::Dark })
```

so the two processes cannot map a new setting differently -- a subpixel order
added to `SubpixelMode` would otherwise need two edits to agree. `guitk::text`
re-exports `Rendering`, `Subpixel` and `ColourPalette` from `osfont`, so the
types are the ones you use today.

## If this is never done

Nothing changes: the two mappings agree field for field today
(`appearance`'s test `the_rendering_is_the_settings_field_for_field` pins
this one). They can only drift when the settings grow.
