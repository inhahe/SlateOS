# F → C, E — WebP decodes now, animations included

**From:** Lane F (`gui/imagecodec`). **To:** Lane C (`gui/thumbs`), Lane E
(`apps/imageviewer`, `apps/photomanager`, `apps/explorer`). **Filed:**
2026-09-25.
**Status:** OPEN — the decoder is on `lane-f`; the uses below are yours.

**In short:** WebP pictures -- the format browsers save most images in --
used to show as a plain coloured rectangle in the file manager. `imagecodec`
now decodes every kind: lossless, lossy (photographs), with transparency,
and animated. Every entry point that already took PNG, JPEG and GIF takes
WebP too with no change on your side, so thumbnails and the viewer already
show them; what is left is a stale comment (lane C), telling the viewer's
info panel that the format has a name (lane E), and playing animations (lane
E).

## What works without a change

`imagecodec::decode`, `decode_scaled` and `dimensions` dispatch on the
signature (`RIFF....WEBP`). A still WebP decodes to exactly what Chrome,
Firefox and Pillow show -- every pixel is held to libwebp in the crate's
tests. An animated one decodes to its first frame, which is what a thumbnail
and a still viewer show.

## Lane C

- `gui/thumbs/src/lib.rs`, the comment above the colour-swatch fallback in
  `generate_image_thumbnail` ("Today this is GIF, JPEG, WebP and ICO"): only
  ICO still reaches the swatch that way, plus a PNG over `max_source_pixels`
  or any broken file. (The same comment was already out of date for GIF and
  JPEG -- `requests/f-ce-gif-decodes-and-animates.md`.)
- If the thumbnailer marks animations (a badge), `imagecodec::webp::Animation::new(bytes,
  limits)?.frame_count() > 1` reads the structure without decoding a frame,
  exactly as for GIF.

## Lane E

**The viewer's info panel.** `apps/imageviewer/src/main.rs`'s `ImageFormat`
has no WebP, so a WebP that displays perfectly is labelled with no format
and no header size. `RIFF` at 0 and `WEBP` at 8 identify it
(`imagecodec::webp::is_webp`), and `imagecodec::webp::dimensions` reads its
size from the headers alone -- for a file cut short after its `VP8X` chunk,
the canvas that chunk declares, as libwebp's `WebPGetFeatures` does.

**Playing an animation** is the GIF recipe with WebP's types:

```rust
use imagecodec::webp::{Animation, Repeat};

let mut animation = Animation::new(&bytes, limits)?;   // reads structure only
if animation.frame_count() > 1 && !reduced_motion {
    while let Some(frame) = animation.next_frame()? {
        show(frame.image);                             // the composited canvas
        wait(frame.display_duration_ms());             // browsers' timing
    }
    // then `animation.rewind()` and again, as `animation.repeat()` says:
    // Repeat::Forever, or Repeat::Times(n) -- n plays in all, the first
    // included (WebP counts plays; GIF's count is looser).
}
```

- As with GIF, `next_frame` lends the canvas it composites onto; copy what
  you keep. Playing costs two canvases of memory (three for a file shown
  without its alpha), whatever the frame count.
- `display_duration_ms` is what browsers show: 10 ms or less becomes 100 ms.
  `frame.duration_ms` is the file's own number.
- One difference from GIF: a frame whose data is broken is an **error**
  (`next_frame` returns `Err`, and again if asked again), where a broken GIF
  frame draws what it had. That is libwebp's behaviour and Pillow's; the
  canvas is left as the last good frame drew it, so showing that and
  stopping is the natural response.
- Reduced motion: `imagecodec::decode` gives the first frame, as for GIF.

The decoder's choices: `design-decisions.md` §1311 (lossless), §1312
(lossy) and §1313 (the container and animations).
