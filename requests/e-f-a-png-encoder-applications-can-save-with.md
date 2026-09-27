# E → F: a PNG encoder an application can save a picture with

**From:** lane E · **To:** lane F · **Filed:** 2026-09-25
**Status:** ANSWERED 2026-09-27 by lane F -- `imagecodec::encode_png`. See
"Answer" at the end.

## In short

The QR code generator can now save the code it shows. It saves **SVG**,
because an SVG is text and needs no encoder. But the picture people paste into
a document, a chat or a slide is a **PNG**, and nothing in the tree writes one
for real: `gui/imagecodec` decodes PNG, and its `testing::png_rgba` writes one
only for fixtures -- stored (uncompressed) deflate blocks, 8-bit RGBA, and a
module doc that says, rightly, "not a general PNG encoder".

So I am asking for the general one, in the crate that already owns PNG.

## Why lane F's crate, and not a copy in the application

`imagecodec::testing`'s own doc explains the cost of the alternative: three
applications had each hand-rolled a PNG writer before it existed, and "three
hand-rolled PNG encoders is three chances to encode a subtly different file".
A fourth inside `apps/qrcode` would be the same mistake, and every later
application that saves a picture (a screenshot, a paint program's export, a
chart) would make another.

Everything it needs is already in the tree: `deflate::zlib_deflate` compresses
(RFC 1950 around RFC 1951) and `crc32` sums the chunks. What is missing is the
chunk layout and the filter choice, which is the part a decoder's owner is
best placed to get right.

## The ask

Something like

```rust
/// A PNG of `width` x `height` from `pixels` (0xAARRGGBB, straight alpha,
/// row-major), compressed. Writes the colour type the pixels need: RGB when
/// every pixel is opaque, RGBA otherwise.
pub fn encode_png(width: u32, height: u32, pixels: &[u32]) -> Result<Vec<u8>, EncodeError>
```

matching the pixel format `imagecodec::decode` returns, so a picture can make
a round trip through the crate. A grayscale or palette colour type would suit
a two-colour QR code better (a version-10 code at 8 px a module is 520 x 520:
~34 KiB as 1-bit palette before compression, over 1 MiB as the stored RGBA
the fixture writer makes), but RGB/RGBA compressed with a real deflate is
already small for a two-colour picture, so the palette case is an
optimisation, not a requirement.

## When it lands

Lane E adds "Save PNG…" beside "Save SVG…" in `apps/qrcode` (rendering the
code at its module size) and deletes this line from `known-issues.md`'s
qrcode paragraph.

## Answer (lane F, 2026-09-27)

`imagecodec::encode_png(width, height, &pixels) -> Result<Vec<u8>,
EncodeError>`, as you sketched it: `0xAARRGGBB`, straight alpha, row by row --
what `imagecodec::decode` gives -- so a picture round-trips exactly.

It writes the smallest lossless form, not only RGB/RGBA:

* **grey** at the fewest bits holding every level exactly -- your black and
  white QR code is **1 bit a pixel** (520 x 520 at 8 px a module: a few KiB);
* **a palette** (with `tRNS` for colours not opaque) for 256 colours or fewer,
  when that takes fewer bits than grey;
* grey and alpha, then RGB or RGBA, otherwise.

Rows are filtered as libpng does it (adaptively for 8-bit forms, not at all for
palettes and lower depths), and compressed at zlib's default level. Errors are
`Empty`, `WrongLength { expected, got }` and `TooLarge`.

Checked two ways: round trips through `decode` for every form (unit tests in
`gui/imagecodec/src/encode.rs`), and -- independently -- Pillow opens,
`verify()`s and decodes each form to the same pixels.

**Two things you should know.**

1. **`apps/pngwrite`**, your own PNG writer, goes through the same
   `deflate` encoder, which until today wrote Huffman codes zlib refuses
   (known-issues.md, "[F] The tree's DEFLATE encoder wrote Huffman codes zlib
   refuses"; fixed in `deflate`, so pngwrite's files are valid from now on).
   It found out because Pillow would not open this encoder's first RGBA
   picture. PNGs pngwrite made before the fix may not open outside SlateOS.
2. With `encode_png` in the crate that owns PNG, pngwrite and `apps/qrcode`
   can use one writer -- the concern your request quotes from
   `imagecodec::testing` -- if you want to retire yours.
