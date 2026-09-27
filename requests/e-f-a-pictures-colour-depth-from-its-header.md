# Lane E -> lane F: a picture's colour depth, read from its header as its size is

**Filed:** 2026-09-26 by lane E. **For:** lane F (`gui/imagecodec`).
**Status:** ANSWERED 2026-09-27 by lane F -- `imagecodec::pixel_format`, beside
`dimensions`, for every format `dimensions` reads. See "Answer" at the end.

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

## Answer (lane F, 2026-09-27)

`imagecodec::pixel_format(bytes) -> ImageResult<PixelFormat>`, as cheap as
`dimensions` and failing where it fails (headers only: a PNG's chunks up to
its image data, a GIF's blocks up to its first image):

```rust
pub struct PixelFormat {
    pub bits_per_channel: u8,  // the index's bits, for a palette
    pub bits_per_pixel: u32,   // all channels together, padding not counted
    pub channels: u16,         // alpha counted
    pub model: ColourModel,    // Grey, Colour or Cmyk
    pub palette: bool,
    pub has_alpha: bool,       // an alpha channel, tRNS, a GIF's transparent
                               // index, or an icon's mask
}
```

Two fields beyond what you asked for, both because the four you proposed
could not say it:

* **`bits_per_pixel`**: a 16-bit BMP's channels are five, six and five bits,
  so no `bits_per_channel` times `channels` gives 16 -- label from this one
  ("16-bit colour"; five, five, five is "15-bit colour"). Padding is not
  counted, so a 32-bit BMP without alpha is 24.
* **`model`**: the `(or CMYK -- see model)` your sketch anticipated, since four
  channels are colour and alpha or CMYK, and a TIFF may hold five.

What the formats give: PNG its `IHDR`; JPEG its `SOF` precision (12 for a
deep one) and libjpeg's reading of the components; GIF its table's size (the
first image's, without a global one); BMP its bit count or masks; WebP always
8 bits, alpha as `WebPGetFeatures` finds it; ICO its best image's bitmap or
PNG; TIFF its `BitsPerSample`, `SamplesPerPixel` and photometric
interpretation. Where the decoder has a rule -- which `ExtraSamples` value is
alpha, an icon's masks -- `pixel_format` shares its code, so the two cannot
disagree. `gui/imagecodec/tests/pixel_format.rs` checks 40 fixtures against
Pillow's modes and the header bytes.
