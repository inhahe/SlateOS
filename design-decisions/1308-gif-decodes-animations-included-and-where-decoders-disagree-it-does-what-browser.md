## 1308. GIF decodes, animations included, and where decoders disagree it does what browsers do

**Date:** 2026-09-25
**Lane:** F
**Decided by:** Claude (autonomous). Mechanism inside `gui/imagecodec`; the
calls with a user-visible side are how the file's ambiguities are resolved,
recorded below.

**In short:** GIFs -- still the web's animation format and common in any
downloads folder -- used to show as a plain coloured rectangle in the file
manager and could not be opened at all, because nothing decoded them. They
now decode: a thumbnail or still viewer gets the first frame, and a viewer
that animates gets every frame, composited as it should be, with its timing.
Where the format leaves a choice to the decoder, this makes the choice
browsers make, so a GIF looks here as it does in a web page.

### What it is

`gui/imagecodec/src/gif.rs` and `gif/lzw.rs`. The LZW decoder handles the
three places decoders go wrong -- the code not yet in the table, when the code
width grows (one code after the encoder), and a table that fills without a
clear -- each with its own unit test. `gif::Animation` walks the file's
structure once without decoding (frame count, loop count, the canvas size),
then composites one frame at a time onto one reused canvas: each image drawn
over what the ones before it left, the previous image's disposal applied first
(keep, clear, or restore what was under it). `decode` is its first frame;
`decode_scaled` averages that by the same rule as a PNG thumbnail, which moved
from `png.rs` into a shared `scale.rs` so that the two cannot drift apart.

### The choices with two sides

1. **Browsers' answers, not Pillow's, where the two differ.** The format
   leaves several things open, and the two references at hand disagree on
   four: the colour the canvas starts as and is cleared to (browsers:
   transparent, ignoring the header's background colour; Pillow: the
   background colour), disposal 3 on the first frame (browsers: the empty
   canvas; Pillow: keeps the frame), a first image smaller than the screen
   (browsers: the rest transparent; Pillow: palette entry 0), and an image
   past the canvas after the first (browsers: clipped; Pillow: grows the
   canvas). *For browsers:* that is how the files were authored and viewed;
   a GIF whose animation leaves stray colour behind here, when it does not in
   any browser, is a bug report. *Against:* Pillow cannot answer those cases,
   so they are held to a small reference compositor in the fixture generator
   instead -- and the generator checks that Pillow really does decode each of
   them differently, so none of them passes either way by accident.
2. **An image with a minimum code size outside 2 to 8 is skipped.** 1 is below
   the format's minimum and giflib's own encoder never writes it; the decoders
   that accept it disagree about when its codes widen (giflib after the first
   code, Firefox and Chrome after four entries), and Pillow rejects it. With no
   agreed picture to reproduce, drawing any is a guess. Above 8, codes name
   indices no byte holds.
3. **The first image may enlarge the canvas; later ones are clipped.** A first
   image bigger than its logical screen is a broken encoder's file, and
   Firefox and Pillow both grow the canvas to show it; growing it again for a
   later frame would change the picture's size mid-animation.
4. **The display delay is a helper, not applied.** `Frame::delay_cs` is the
   file's number; `display_delay_ms` gives what browsers show (0 or 1
   hundredths become a tenth of a second). A viewer that wants the file's word
   can have it.
5. **One canvas, lent per frame** (`next_frame` returns a borrow of it), not a
   list of frames. A 500-frame GIF at 500x500 is 500 MB as a list; as a
   canvas it is 1 MB, and a viewer keeps only what it shows.

### How it is held

`tests/gif.rs` over nineteen fixtures (`tests/data/generate_gif.py`), every
frame compared exactly -- GIF is lossless: Pillow-written photographs (256
colours, interlaced and not), four colours, animations with each disposal and
transparency, answered by Pillow's decode where it agrees with browsers; and
hand-written files -- a full table left uncleared, clears mid-string, uneven
interlace, 87a, an image bigger than its screen, no colour table, indices past
it, frames off the canvas, disposal 2 without transparency, disposal 3 first --
answered by the indices they were made from. The hand-written files come from a
writer validated by round-tripping 630 files through Pillow's decoder. Also:
rewind replays identically, loop counts, delays, a file cut off mid-image draws
a prefix equal to its whole twin's, the canvas is refused past the byte budget
before it exists, and no truncation or bit flip panics. Removing the disposal,
the interlace mapping or the first-frame enlargement each fails the suite.

### Measured

Release, a 480x360, 100-frame, 256-colour animation of 3 MB: the first frame
in 2 ms; every frame, composited, in 150 to 170 ms, 1.6 ms a frame, where
Pillow takes 320 ms. Playing at 25 frames a second leaves a frame 40 ms.
