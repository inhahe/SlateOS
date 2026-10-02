## 1314. BMP: Chrome's decoder, ported and held to the original

**Date:** 2026-09-25
**Lane:** F
**Decided by:** Claude (autonomous).

**In short:** Windows bitmap pictures (`.bmp`) now open -- the image viewer
used to say "BMP images cannot be displayed yet", and the file manager had a
separate, partial reader of its own. Every kind Chrome shows is read, and each
comes out exactly as Chrome shows it: over 69 test pictures and more than
22,000 damaged or deliberately odd files, this decoder and Chrome's own agree
on every pixel, and on every file neither will show.

### What it is

`gui/imagecodec/src/bmp.rs`: the file header; every bitmap header (OS/2
1.x's 12 bytes, Windows' 40, 52, 56, 108 and 124, OS/2 2.x's 16 to 64);
palettes of 1, 2, 4 and 8 bits; 16- and 32-bit bit fields, narrow, wide and
with alpha; 24- and 32-bit colour; RLE8, RLE4 and OS/2's RLE24, both row
orders.

### The choices with two sides

1. **Chrome's answers, and Chrome's current decoder's.** BMP's
   documentation leaves most corner cases open, and the decoders people use
   answer them differently: what pixels a run-length stream skips become
   (black in Windows and now Chrome; transparent in Firefox and Chrome's
   older decoder); whether a 32-bit picture's fourth byte is alpha; what a
   file that stops short shows (nothing, in Chrome). Chrome is most of the browsers people use, and it now reads
   BMP through image-rs 0.25.10 with four patches of Chromium's own
   (inside Skia, `rust_bmp`); this module ports that decoder in its lenient
   mode. *For:* the pictures look as they do in the browser most people
   have, and the reference is exact and runnable -- the generator builds it
   from the same crate and patches, so every answer in the tests is the real
   decoder's. *Against:* Chrome changed decoders recently and may change
   answers again; and some of its answers are not the friendliest -- a file
   missing one byte of its last row's padding is refused outright, where
   Pillow shows it. Following the browser means following it
   there too; the tests would say at once if Chrome moved (by rebuilding the
   reference from a newer Chromium).
2. **Refuse what Chrome refuses, including short files.** Chrome's decoder
   reports a short file as incomplete, and Blink fails an image whose data
   has all arrived and is still incomplete. So a truncated BMP -- even one
   whose run-length stream is only missing its end code before the last row
   is finished -- is `Truncated` here, not half a picture. *For:* no picture
   here that Chrome would not show. *Against:* GIF, by contrast, draws what a
   short file has (§1308), because there browsers do; the two formats differ
   because the browsers do.
3. **Alpha only where the header asks for it.** A 32-bit picture with a
   40-byte header is opaque whatever its fourth byte, since most such files
   leave it unset or fill it with junk; a V4 or V5 header's alpha mask makes
   it alpha even uncompressed, and so does any bit-field picture's alpha
   mask. Chrome's older decoder also rescued pictures whose alpha was all
   zero (showing them opaque); the current one does not, and neither does
   this. *For:* Chrome. *Against:* a V5 picture written by a program that set
   the alpha mask and left the channel empty is fully transparent -- as it
   is in Chrome today.
4. **Colour spaces and profiles are not applied.** V4 and V5 headers can
   carry calibrated primaries or an ICC profile; like the PNG and JPEG
   decoders here, this one does not apply them, since the desktop has no
   colour pipeline yet. It still refuses a file whose V5 header names an
   embedded profile lying past the end of the file, as Chrome does.

### How it is held

`tests/bmp.rs` decodes 69 fixtures (`tests/data/generate_bmp.py`) and
compares each with Chrome's answer to the pixel, or to the refusal: every
header, depth, mask shape and run-length escape, Pillow's own files, and 13
files Chrome refuses. The generator downloads image-rs 0.25.10 and
Chromium's four patches (pinned to a commit, checked against SHA-256 sums),
builds them into a small program, and has it answer every fixture; it also
reports where Pillow differs (it widens 5-bit channels differently, ignores
V4/V5 alpha masks, forgives short files and misreads several headers).
Outside the suite, 12,240 bit-flipped, cut and header-rewritten files and a
sweep of 10,368 combinations of header size, compression and bit depth were
decoded by both: every one agrees. Two mistakes in the port were caught on
the way -- a Windows header's masks read from past its end (by the
fixtures), and an OS/2 header's compression code taken to mean a fourth mask
(by the corrupted files; now a fixture and a unit test too).
