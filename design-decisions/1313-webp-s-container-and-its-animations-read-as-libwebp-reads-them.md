## 1313. WebP's container and its animations, read as libwebp reads them

**Date:** 2026-09-25
**Lane:** F
**Decided by:** Claude (autonomous).

**In short:** Animated WebP pictures -- the stickers, reactions and short
clips the web is full of -- now play, and every frame comes out exactly as
libwebp (the library Chrome, Firefox and Pillow read WebP with) composites
it. Whether a WebP file is acceptable at all is now decided the way libwebp
decides it, so a damaged file shows here as it shows elsewhere, or is refused
where it is refused elsewhere: of 4,500 damaged test files, the six this
decoder used to accept and libwebp refused are now refused, and 3,387
damaged animations play, stop, or are refused frame for frame as in libwebp.

### What it is

`gui/imagecodec/src/webp/riff.rs` ports libwebp's three readers of the
container: `WebPGetFeatures` (a quick look at the headers, which is what
`webp::dimensions` now returns), the demuxer (`WebPDemux`: every chunk
walked, each frame's place found, the whole checked), and the header parse a
frame's decode starts with. `webp.rs` ports the animation decoder
(`anim_decode.c`) as `webp::Animation`; `webp::decode` gives a still picture,
or an animation's first frame.

### The choices with two sides

1. **libwebp's readers, all three, exactly.** RFC 9649 leaves much to the
   reader: whether a `VP8X` chunk may be longer than ten bytes, whether flags
   the format does not define make a file invalid, what a chunk after the
   picture inside an `ANMF` means, whether a frame's `ANMF` size or its
   picture's wins. libwebp answers each -- differently in its different
   readers, sometimes (the demuxer takes a long `VP8X` chunk,
   `WebPGetFeatures` refuses it) -- and a file is shown only if all of them
   accept it. *For:* a file is acceptable here exactly when it is acceptable
   to the programs people use, and corrupted files make a regression suite
   for it (below). *Against:* libwebp's quirks become this decoder's: a
   frame's `ANMF` size is ignored for its picture's; a chunk after the
   picture inside an `ANMF` is read as if it followed the `ANMF`; a running
   size total wraps as a 32-bit number does in C. Each is commented where it
   is, and `riff.rs`'s unit tests pin each one -- and each of their
   expectations was checked against libwebp itself, through its Python
   bindings.
2. **The alpha shown is the alpha `WebPGetFeatures` reports.** Pillow and
   Firefox show a picture with its alpha only if `WebPGetFeatures` says the
   file has any: a lossless stream's own flag, or for an animation the
   `VP8X` chunk's. So an animation whose `VP8X` chunk lacks the flag is
   shown opaque, its uncovered canvas black. *For:* one rule for stills and
   animations, the two reference programs' rule. *Against:* Chrome decides
   from the demuxer's flags; the two differ only on files no encoder writes
   (encoders set the flag whenever any frame has alpha).
3. **libwebp's compositing, not the specification's formula.** RFC 9649
   describes blending by the ideal alpha formula. libwebp's animation decoder
   approximates it in integers (the canvas's share is `a·(256−s)>>8`, and a
   reciprocal scale), which can come out a level off the ideal; inside a
   rectangle the frame before cleared to transparent, it does not blend at
   all, which is not quite the same as blending with transparency; and it
   starts from a cleared canvas for any frame that owes nothing to the one
   before. All of that is ported. *For:* Pillow and libwebp's own tools are
   this decoder, so every frame can be tested to the bit. *Against:* the
   browsers composite with the same rules but their own arithmetic (Chrome,
   premultiplied), so a partly transparent pixel blended over another can
   differ from a browser's by a level. Opaque frames -- most frames -- are
   the same everywhere.
4. **A broken frame is an error.** libwebp's animation decoder and Pillow
   stop with an error at a frame that will not decode; so does
   `next_frame`, and again if asked again, with the canvas left as the last
   good frame drew it. A broken GIF frame, by contrast, draws what it had,
   because browsers do that (§1308). *For:* libwebp. *Against:* what
   browsers do with a broken WebP frame was not checked; they decode frames
   with libwebp's incremental decoder, which might show part of one
   (`todo.txt`, Judgment Calls).
5. **Memory: two canvases, three for a file shown opaque.** libwebp keeps
   the canvas and the canvas after the last frame's disposal. A file shown
   without its alpha needs a third to show it in, since the canvas itself
   must keep the true alpha for blending the next frame. `Limits` is checked
   against all of them before anything is decoded. A still decode needs one.

The API mirrors `gif::Animation` (`new`, `size`, `frame_count`, `repeat`,
`next_frame`, `rewind`, `into_canvas`), plus `has_alpha`. `Repeat::Times(n)`
counts plays, the first included, as WebP defines its loop count; GIF's
`Repeat::Count` is reported as written, because GIF decoders disagree on it.
`Frame::display_duration_ms` shows 10 ms or less as 100 ms, the browsers'
rule for every animated format.

### How it is held

`tests/webp.rs` plays nine animations frame by frame against Pillow's frames
(libwebp 1.6's animation decoder): five written by libwebp's encoder
(lossless; lossy, alpha in `ALPH` chunks; both mixed; key frames forced on a
clear canvas; opaque, and so shown without alpha), four by hand for what no
encoder is made to do -- each of the four reasons a frame is a key frame,
blending beside a rectangle the frame before cleared, clear pixels that are
not blended, a missing alpha flag, and the demuxer's reading of chunks where
an encoder would not put them. A census test in `webp.rs` fails if the
fixtures stop reaching any of those rules; the blending arithmetic is checked
against a transcription of libwebp's C, types included, for every pair of
alphas.

Outside the suite: the 4,500 corrupted lossless files of §1312's addendum,
which had left six differences (all containers the demuxer refuses), leave
none; 2,700 bit-flipped animations and 687 cut ones leave none -- 1,195 play
every frame the same, 1,012 stop at the same frame, 1,167 are refused by
both, and 13 whose flipped canvases were too large to write an answer for
were only played, for panics.

### Measured

Release, Windows, best of five, a 480x270 animation of 60 frames: lossy,
6.0 ms a frame against libwebp's 3.0 (through Pillow); lossless, 4.4 against
3.5. The lossy gap is the lossy decoder's (`known-issues.md`); a 25 fps clip
has 40 ms for each frame.
