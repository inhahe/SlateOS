# Lane E -> lane F: a picture's colour depth, read from its header as its size is

**Filed:** 2026-09-26 by lane E. **For:** lane F (`gui/imagecodec`).
**Status:** OPEN.

**In short:** the file manager's picture columns show a picture's size and
shape from `imagecodec::dimensions`, and its Colour Depth column is blank,
because nothing reports how many bits a pixel of the file holds. It used to
say "24-bit" for every picture, which was invented; blank is honest but
empty. The decoder reads every header this would need.

## What would do it

Beside `dimensions`, from the same bytes:

```rust
/// What a picture's header says about its pixels.
pub struct PixelFormat {
    /// Bits of each channel: 8 for most pictures, 16 for a deep PNG or
    /// TIFF, 1 for a fax.
    pub bits_per_channel: u8,
    /// Channels a pixel holds, alpha counted: 1 grey, 2 grey+alpha,
    /// 3 colour, 4 colour+alpha (or CMYK -- see `model`).
    pub channels: u8,
    /// Indexed into a palette of at most `1 << bits_per_channel` colours.
    pub palette: bool,
    pub has_alpha: bool,
}

pub fn pixel_format(head: &[u8]) -> Result<PixelFormat, ImageError>;
```

What lane E would show: "8-bit grey", "24-bit colour", "32-bit colour with
alpha", "48-bit colour" (16 a channel), "8-bit palette" (256 colours), "1-bit"
-- one label function in `apps/explorer/src/columns.rs`, and a sort by total
bits.

Where each format keeps it, for what it is worth: PNG's IHDR (bit depth,
colour type), JPEG's SOF (precision, components), GIF (always a palette; the
global table's size), BMP's `biBitCount`, WebP's VP8L header / alpha chunk,
TIFF's `BitsPerSample` / `SamplesPerPixel` / `PhotometricInterpretation`, ICO's
directory entry.

## If it is never done

Nothing breaks: the column stays blank, as `known-issues.md` "[E] The
explorer's file-type columns showed the same invented values for every file"
records.
