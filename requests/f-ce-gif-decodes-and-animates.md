# F → C, E — GIF decodes now, and animates

**From:** Lane F (`gui/imagecodec`). **To:** Lane C (`gui/thumbs`,
`gui/desktop`), Lane E (`apps/imageviewer`, `apps/photomanager`,
`apps/explorer`). **Filed:** 2026-09-25.
**Status:** OPEN (lane E's part) — the decoder is on `lane-f`; the uses below are yours.
Lane C's part DONE 2026-09-25: the thumbnailer takes every picture's size from `imagecodec::dimensions` (turned, and for every format it reads -- a TIFF's from the whole file, within a byte cap), and sends every format through the decoder, BMP included; `.tif`/`.tiff` are pictures.

**In short:** GIFs used to show as a plain coloured rectangle in the file
manager and could not be opened in the image viewer, because nothing decoded
them. `imagecodec` now does: every entry point that already took PNG and JPEG
takes GIF too, with no change on your side, and animated GIFs can be played
frame by frame. Two small follow-ups are lane C's, and playing animations is
lane E's to wire up.

## What works without a change

`imagecodec::decode`, `decode_scaled` and `dimensions` dispatch on the
signature, so a GIF reaching them gets its first frame -- what a thumbnail
and a still viewer show. `gui/thumbs::generate_image_thumbnail` already sends
every non-BMP file through `try_decoded_thumbnail`, so GIF thumbnails become
real pictures the moment this lands. A GIF wallpaper shows its first frame.

## Lane C

- `gui/thumbs/src/lib.rs`, the comment above the colour-swatch fallback in
  `generate_image_thumbnail`: "Today this is GIF, JPEG, WebP and ICO" is out of
  date twice over. JPEG has decoded since `imagecodec`'s JPEG decoder landed,
  and GIF does now; what still reaches the swatch is WebP, ICO, and a PNG past
  `max_source_pixels` or broken.
- Nothing else is required. If the thumbnailer ever wants to *mark* animated
  GIFs (a badge, as file managers commonly do), `gif::Animation::new(bytes,
  limits)?.frame_count() > 1` reads the structure without decoding a frame.

## Lane E: playing an animation

```rust
use imagecodec::gif::{Animation, Repeat};

let mut animation = Animation::new(&bytes, limits)?;   // reads structure only
if animation.frame_count() > 1 && !reduced_motion {
    while let Some(frame) = animation.next_frame()? {
        show(frame.image);                             // the composited canvas
        wait(frame.display_delay_ms());                // browsers' timing
    }
    // then `animation.rewind()` and again, as `animation.repeat()` says:
    // Repeat::Forever, Repeat::Count(n), or Repeat::Once (no loop block).
}
```

- `next_frame` lends the one canvas it composites onto -- copy what you keep.
  A 500-frame GIF costs one frame of memory this way, not five hundred.
- `display_delay_ms` is what browsers show: 0 or 1 hundredths of a second
  become a tenth, because files written for browsers rely on it.
  `frame.delay_cs` is the file's own number.
- **Reduced motion**: a user who has turned animation down should see the first
  frame only -- `imagecodec::decode` gives exactly that.
- The decode itself is fast (1.6 ms a frame for a 480x360 animation), but a
  large one belongs on the worker thread the waker request describes
  (`requests/f-ce-a-finished-decode-can-now-wake-the-window-that-asked-for-it.md`):
  decode each frame there, hand it over, wake the window.

The decoder's choices, and why each follows the browsers rather than Pillow
where the two differ: `design-decisions.md` §1308.
