# F → B, C, E — AVIF pictures decode now, animated ones included

**From:** Lane F (`gui/imagecodec`). **To:** Lane B (`userspace/file`), Lane C
(`gui/thumbs`, `gui/toolkit`), Lane E (`apps/imageviewer`, `apps/explorer`,
`apps/fileassoc`, `apps/filesearch`). **Filed:** 2026-09-27.
**Status:** OPEN for lanes B and E; lane C's half DONE 2026-09-28
(`6d3293f33`, `891f6dbae`; reply at the end). The decoder is on `main`; the
uses below are yours.

**In short:** AVIF -- the picture format more and more websites serve -- had
no decoder here. `imagecodec` now decodes
it (design-decisions.md §1333): still pictures, grids, transparency, and
animated sequences, every pixel held to libavif's through Pillow in the
crate's tests. The generic entry points already take it, so anything that
calls `imagecodec::decode` shows AVIF today. What is left is the places that
decide by extension or magic bytes *before* reaching the decoder, and playing
the animations.

## What works without a change

`imagecodec::decode`, `decode_scaled`, `dimensions` and `pixel_format`
recognise an AVIF by its `ftyp` box (brand `avif`, or `avis` for a sequence;
`imagecodec::avif::is_avif(bytes)`). A sequence decodes to its first frame,
which is what a thumbnail or a still viewer shows. Sizes are as shown: cropped
by a `clap` box and turned by `irot`/`imir` as Chrome does -- AVIF keeps its
rotation in the container, not in EXIF, so there is no EXIF turn to apply on
top.

## Lane B -- `userspace/file`

`detect_media`'s `ftyp` branch names every brand it does not list "ISO Media,
MPEG-4 compatible", `video/mp4`, so `file photo.avif` calls a picture a video.
GNU `file` says `ISO Media, AVIF Image` (brand `avif`) and `ISO Media, AVIF
Image Sequence` (brand `avis`), both `image/avif`. (`heic`/`heix`/`mif1` are
HEIF, `image/heic`/`image/heif`, if you want them at the same time.)
`userspace/xdg` already maps `.avif` to `image/avif`.

## Lane C

- `gui/thumbs/src/lib.rs`, `parse_image_dimensions`: still BMP, PNG, GIF and
  JPEG parsers of its own, so an AVIF -- like a WebP
  (`requests/f-ce-webp-decodes-and-animates.md`) -- gets the placeholder
  before any decoder is tried. Falling back to `imagecodec::dimensions`, which
  reads every format `imagecodec` decodes from the headers alone, is the whole
  fix for both.
- `gui/toolkit/src/filetypes.rs`: no `.avif` entry, so file dialogs and
  anything else that asks it get no description, MIME type or category.
  `".avif"`, `"AVIF Image"`, `"image/avif"`, `FileCategory::Image`.

## Lane E

**Recognising the file.** `apps/imageviewer/src/main.rs`: `IMAGE_EXTENSIONS`
and `ImageFormat` (add `Avif`, detected by `imagecodec::avif::is_avif`, named
"AVIF"). The same extension lists in `apps/explorer/src/columns.rs`
(`ImageColumns`), `apps/fileassoc/src/main.rs` and `apps/filesearch`.

**Playing a sequence** is the WebP recipe with AVIF's types, with two
differences. There is no canvas: each frame is the whole picture, already
cropped and turned. And the repetition counts *after* the first play, as a
GIF's does:

```rust
use imagecodec::avif::{Animation, Repeat};

let mut animation = Animation::new(&bytes, limits)?;    // reads structure only
if animation.frame_count() > 1 && !reduced_motion {
    loop {
        while let Some(frame) = animation.next_frame()? {
            show(frame.image);
            wait(frame.display_duration_ms());          // browsers' timing
        }
        match animation.repeat() {
            Repeat::Forever => {}
            Repeat::Count(more) => { /* plays after the first: stop after 1 + more */ }
        }
        animation.rewind();
    }
}
```

`Repeat::Forever` also covers a file that does not say (no edit list), which
Chrome and Firefox both loop. `next_frame` returning `Ok(None)` before
`frame_count()` frames means the alpha track ran out first -- libavif stops
there too -- and is the end of a play, not an error. A frame that fails is an
error, and stays one until `rewind`, matching the viewer's "a broken frame ends
the animation" rule. `animation.seek(n)` reaches any frame from its nearest key
frame, if the viewer ever steps through frames.

In `apps/imageviewer/src/player.rs` that is a third `Kind` beside `Gif` and
`WebP`, with `play_avif` following `play_webp` and the loop test above.

## Reply from lane C -- 2026-09-28

Both done, on `lane-c`, reaching `main` with lane C's next publish:

- **`gui/toolkit/src/filetypes.rs`** (`6d3293f33`): `.avif` is "AVIF Image",
  `image/avif`, `FileCategory::Image` -- and the content sniffing that had
  matched every ISO media `ftyp` box as `.mp4` now reads the major brand
  first: `avif`/`avis` are AVIF, `heic`/`heix` HEIC and `mif1` HEIF, each
  with a row of its own, so a photo is no longer sent to the video player.
  (Nothing decodes HEIC; naming it a picture lets whatever opens it say so
  honestly.)
- **`gui/thumbs`** (`891f6dbae`): the thumbnailer already fell back to
  `imagecodec::dimensions` and `imagecodec::decode` for any format it routes
  as a picture (that was the WebP change), so the fix was `avif` joining the
  picture extensions. An animated AVIF shows its first frame, as a GIF does.
