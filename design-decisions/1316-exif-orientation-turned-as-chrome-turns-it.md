## 1316. EXIF orientation: turned as Chrome turns it

**Date:** 2026-09-25
**Lane:** F
**Decided by:** Claude (autonomous).

**In short:** Photographs from phones used to open lying on their side: a
phone stores its picture as the sensor saw it and notes in the file which way
up it belongs, and nothing here read that note. Now pictures are turned as
Chrome turns them -- from the note in JPEGs and PNGs, not in WebPs, GIFs or
BMPs, where Chrome ignores it -- and everything that reports a picture's size
reports it the way up it is shown.

### What it is

`gui/imagecodec/src/orientation.rs`: the `Orientation` values and the eight
ways of turning pixels, and a port of Skia's `SkExif::Parse` for the
orientation tag. `jpeg::orientation` finds it the way Blink's JPEG decoder
does (the first `APP1` segment before the scan that starts `Exif\0`);
`png::orientation` the way Skia's Rust PNG codec does (the first `eXIf`
chunk before the image data, one with a bad CRC passed over). Both decoders
apply it in `decode`, `decode_scaled` and `dimensions`.

### The choices with two sides

1. **Applied by default, everywhere.** Every caller wants the picture as it
   is shown -- the viewer, the thumbnails, a wallpaper -- and a decoder that
   leaves turning to each of them gets it wrong in the one that forgets, as
   the thumbnailer and the viewer both would have. *Against:* a program that
   edits a photograph and rewrites its EXIF would want the stored pixels;
   none exists yet, and `jpeg::orientation`, `png::orientation` and
   `Orientation::inverse` are public for the day one does. The size reported for a sideways photograph changes, which is
   the point: a size that disagrees with the picture it describes is the bug.
2. **Chrome's reading, where readers differ.** Pillow's `exif_transpose`
   looks only in the first EXIF directory, takes an orientation of any
   integer type, and reads a PNG `eXIf` wherever it is; Chrome follows the
   pointer to the EXIF sub-directory, takes only a `SHORT`, and reads only an
   `eXIf` before the image data. Chrome's is followed, and fixtures for each
   difference check that Pillow really does read them otherwise.
3. **Not for WebP.** The WebP container has an `EXIF` chunk and Chrome does
   not turn by it (Blink's WebP decoder never reads it); neither does this.

### How it is held

`tests/orientation.rs`: one picture stored under all eight orientations as a
JPEG and as a PNG, against Pillow's decode turned by `exif_transpose` (the
JPEGs to within rounding; the PNGs exactly, and each JPEG exactly the upright
decode turned); the five cases where Chrome and Pillow read the orientation
differently; and thumbnails that fit their box the way up the picture is
shown. Unit tests hold the parser to Skia's rules (byte order, entry types,
the sub-directory pointer, directories cut short) and each turn to its
definition.
