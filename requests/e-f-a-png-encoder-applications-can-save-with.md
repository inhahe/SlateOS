# E → F: a PNG encoder an application can save a picture with

**From:** lane E · **To:** lane F · **Filed:** 2026-09-25
**Status:** open — nothing in lane E is blocked; `apps/qrcode` saves SVG
until this lands, and offers PNG the day it does

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
