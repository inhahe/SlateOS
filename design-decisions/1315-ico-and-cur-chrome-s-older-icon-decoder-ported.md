## 1315. ICO and CUR: Chrome's older icon decoder, ported

**Date:** 2026-09-25
**Lane:** F
**Decided by:** Claude (autonomous).

**In short:** Windows icons and cursors (`.ico`, `.cur`) -- every program's
icon, every website's favicon -- now open, where the file manager showed a
coloured rectangle. An icon file holds the same picture at several sizes;
this shows the one Chrome shows, the largest, and makes it transparent where
Chrome does.

### What it is

`gui/imagecodec/src/ico.rs`: the icon directory, the choice of image, PNG
images (through the PNG decoder), and BMP images with their one-bit
transparency ("AND") masks, through a port of Chrome's older BMP reader in
its icon mode -- which Chrome still uses for icons, though plain BMPs now go
through image-rs (§1314).

### The choices with two sides

1. **Chrome's rules, where decoders differ.** The best image is the largest,
   then the deepest (Pillow takes the shallowest of the largest); only it is
   decoded, and if it fails the icon does (no fallback to the next). A BMP's
   mask is read where its pixels end (Pillow reads it from the end of the
   entry's stated size). A 32-bit image whose alpha is all zero is opaque,
   under its mask -- written for a reader that ignored alpha -- where Pillow
   shows it clear; and once a pixel with alpha appears, everything decoded
   before it is cleared, as Chrome clears it. A mask cut short -- including
   the unpadded masks Pillow's own writer produces for widths that are not a
   multiple of 32 -- is refused. *For:* the icons look as they do in Chrome,
   the browser most people have. *Against:* these are the rules of Chrome's
   older code, which it may retire as it did for BMP; and the tests cannot
   run that code, see below.
2. **Held to rules, not to a running reference.** Chrome's icon decoder is
   C++ inside Blink and cannot be built alone, as its BMP decoder could. So
   three icons Pillow writes (where Pillow and Chrome agree) are answered by
   Pillow, and the other 23 by Chrome's rules applied to the pixels, palettes
   and masks they were built from -- not by a decoder --
   with the generator checking that Pillow really does get each Chrome-only
   rule wrong. *For:* no second decoder of mine checking the first. *Against:*
   the rules are my reading of Chrome's source; a misreading would be in both
   the answers and the code.
3. **What an embedded colour profile holds is not checked.** Chrome refuses
   an icon whose V5 header names a profile that is missing or will not parse.
   This refuses a missing or empty one, and accepts any other, since nothing
   here reads profiles (`known-issues.md`).

### How it is held

`tests/ico.rs`: 26 fixtures (`tests/data/generate_ico.py`) -- Pillow's PNG
and BMP icons, every BMP depth under its mask, 32-bit alpha in each of its
cases, bit fields, run-length data, a cursor, the choice between entries,
and 9 files Chrome refuses -- plus every truncation of four of them, and bit
flips of five for panics.
