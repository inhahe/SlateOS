## 1324. The font settings' `smoothing` and `subpixel` now rasterize text: LCD planes with FreeType's filter, on/off pixels without smoothing, grey kept beside both

**Date:** 2026-09-26
**Lane:** F
**Decided by:** Claude (autonomous).

**In short:** the appearance settings have offered "font smoothing" and a
subpixel order (none, RGB, BGR, and the two vertical ones) since the settings
existed, defaulting to smoothing on and RGB -- and neither changed a pixel:
every glyph was drawn grey-anti-aliased whatever was chosen. Now both are
honoured wherever the compositor draws text. Subpixel rendering lights a
glyph's edge pixels a stripe at a time, sharper on an LCD at the price of faint
colour fringes, the way Windows' ClearType and FreeType's LCD mode do;
smoothing off draws every pixel fully on or off.

**Decision.**

* **Rasterized at three times the resolution along the stripes, then
  FreeType's default LCD filter** (`FT_LCD_FILTER_DEFAULT`, taps 8, 77, 86,
  77, 8 of 256), folded three samples to a pixel. The filter is FreeType's so
  that text looks as it does in the programs people compare against; the
  weights sum to 256, so solid runs stay solid.
* **`GlyphMask` keeps `coverage` and gains optional `lcd` planes**: for an LCD
  mask `coverage` is the stripes' mean, so a consumer that blends one alpha a
  pixel (anything outside lane F) still draws the glyph correctly, in grey.
  `lcd_at` reads either kind as three stripes.
* **The mode belongs to the font, not to a draw call** (`ScaledFont`,
  `SystemFont`, `FontCache::set_rendering`): every cached mask was rasterized
  under it, so a change drops the cache, and the compositor sets it from the
  settings at construction and on every `set_appearance`.
* **Smoothing off ignores the subpixel order**: with no partial pixel there is
  nothing to split.
* **Blending per channel** in `osfont::scaled::blit_mask` and the compositor's
  `Framebuffer::draw_glyph` (`blend_pixel_lcd`), each channel at the text
  colour's alpha times its own stripe's coverage.
* **The default stays what the settings default to: RGB.** The compositor
  draws straight into the framebuffer that is scanned out, so it knows the
  destination pixels exactly -- the condition LCD rendering needs.

**Alternatives.**

| | For | Against |
|---|---|---|
| Keep grey only, remove the setting | simpler | the settings page (lane C's) offers it, and it is the platform norm on LCDs |
| A lighter or no filter | sharper | visible fringes (FreeType's "light" filter exists for that choice) |
| Subpixel in the toolkit's own text too | apps drawing their own buffers get it | they cannot know what is under a translucent window; the compositor can |

**Not done:** `hinting` (fitting outlines to the pixel grid) had nothing to
act on when this was written -- there was no hinter. It has one now: §1325.
Subpixel *positioning* (placing glyphs at fractions of a pixel) is a separate
change.

**How to reverse.** `Rendering::default()` is grey; a cache never given a mode
draws as before.
