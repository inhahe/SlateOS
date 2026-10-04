# Third-party code in `imagecodec`

Several of this crate's decoders are ports -- translations into Rust -- of
the decoders the browsers and Pillow run, because matching their pixels and
their refusals exactly is the point (see each module's documentation). A port
is a derivative work, and each upstream's licence asks that its notice travel
with the code. This file says what derives from where, and the licences are
beside it; each ported file also carries its upstream's copyright lines in its
module documentation, and says there what it was translated or adapted from.

The decoders not listed here -- PNG, GIF, the scaler, the PNG encoder -- are
this project's own, written to their specifications and tested against the
programs named in their documentation, not translated from them.

`notices.yaml` is the same list for `scripts/gather-notices.py`, which puts
the notices in every image (design-decisions.md §1433).

**For a binary that contains this crate** (anything that decodes JPEG or
TIFF), libjpeg-turbo's licence asks that its documentation say:

> This software is based in part on the work of the Independent JPEG Group.

and the University of California's notice on libtiff's LZW decoder (below)
asks that it acknowledge the University's work:

> This product includes software developed by the University of California,
> Berkeley.

`notices.yaml` gives both as attributions, which the notices page shows.

## libjpeg-turbo 3.1.1

Portions of this software are copyright (C) 1991-1998 Thomas G. Lane;
(C) 1997-2018 Guido Vollbeding; (C) 1999 Ken Murchison; (C) 2009-2024
D. R. Commander; (C) 2009 Pierre Ossman for Cendio AB; (C) 2013 Linaro
Limited; (C) 2014 MIPS Technologies, Inc.; (C) 2015, 2020 Google, Inc.;
(C) 2018 Matthias Räncker; (C) 2019-2020 Arm Limited; (C) 2023 Aliaksiej
Kandracienka; and, for the SIMD arrangement, (C) 1999-2006 MIYASAKA Masaru.
Each ported file names its own. They are used under
the IJG License (`libjpeg-turbo-README.ijg`, which must travel unaltered),
the Modified BSD License and the zlib License, as libjpeg-turbo's
`LICENSE.md` (`libjpeg-turbo-LICENSE.md`) explains.

| File | Derived from (libjpeg-turbo 3.1.1) |
|---|---|
| `src/jpeg.rs` | `jdapimin.c`, `jdapistd.c` |
| `src/jpeg/arith.rs` | `jdarith.c`, `jaricom.c` |
| `src/jpeg/coef.rs` | `jdcoefct.c` |
| `src/jpeg/color.rs` | `jdcolor.c` |
| `src/jpeg/color/sse2.rs` | `jdcolor.c` (its integers, computed in SSE2) |
| `src/jpeg/decompress.rs` | `jdapimin.c`, `jdapistd.c`, `jdinput.c`, `jdmaster.c`, `jdmainct.c`, `jdpostct.c` |
| `src/jpeg/error.rs` | `jerror.h` (the fatal errors, as values) |
| `src/jpeg/huffman.rs` | `jdhuff.c`, `jdphuff.c` |
| `src/jpeg/idct.rs` | `jidctint.c`, `jidctred.c` |
| `src/jpeg/idct/sse2.rs` | `jidctint.c`; `simd/x86_64/jidctint-sse2.asm` (its arrangement) |
| `src/jpeg/lossless.rs` | `jdlhuff.c`, `jddiffct.c`, `jdlossls.c` |
| `src/jpeg/marker.rs` | `jdmarker.c` |
| `src/jpeg/source.rs` | `jdatasrc.c` |
| `src/jpeg/tables.rs` | `jutils.c` (`jpeg_natural_order`), `jdapimin.c` (`jpeg_abort`) |
| `src/jpeg/upsample.rs` | `jdsample.c` |

## libtiff 4.7.1

Portions of this software are copyright (c) 1988-1997 Sam Leffler,
(c) 1991-1997 Silicon Graphics, Inc., (c) 1997 Greg Ward Larson (LogLuv),
(c) 1996 Pixar (PixarLog), (c) Joris Van Damme and AWare Systems (old-style
JPEG) and (c) 2022 Even Rouault (LZW), and are used under libtiff's licence,
`libtiff-LICENSE.md`, whose notices must appear in all copies. libtiff's
CIE L*a*b* conversion (`tif_color.c`) is from the VIPS library, with the
permission of its author, John Cupitt. libtiff's LZW decoder (`tif_lzw.c`,
ported as `src/tiff/lzw.rs`) derives from the `compress` program, from
software contributed to Berkeley by James A. Woods, derived from original
work by Spencer Thomas and Joseph Orost, copyright (c) 1985, 1986 The Regents
of the University of California; its notice, in the same file, asks that
documentation acknowledge that the software was developed by the University
of California, Berkeley.

| File | Derived from (libtiff 4.7.1, `libtiff/`) |
|---|---|
| `src/tiff.rs` | `tif_getimage.c`, `tif_dirread.c` |
| `src/tiff/color.rs` | `tif_color.c` |
| `src/tiff/dir.rs` | `tif_open.c`, `tif_dirread.c`, `tif_dir.c`, `tif_dirinfo.c`, `tif_strip.c`, `tif_tile.c` |
| `src/tiff/fax.rs` | `tif_fax3.c`, `tif_fax3.h`, `tif_fax3sm.c` |
| `src/tiff/luv.rs` | `tif_luv.c` |
| `src/tiff/lzw.rs` | `tif_lzw.c` |
| `src/tiff/next.rs` | `tif_next.c` |
| `src/tiff/ojpeg.rs` | `tif_ojpeg.c` |
| `src/tiff/pixarlog.rs` | `tif_pixarlog.c` |
| `src/tiff/read.rs` | `tif_read.c`, `tif_packbits.c`, `tif_predict.c`, `tif_zip.c`, `tif_jpeg.c`, `tif_swab.c` |
| `src/tiff/rgba.rs` | `tif_getimage.c` |
| `src/tiff/thunder.rs` | `tif_thunder.c` |

## libwebp 1.6.0, and RFC 6386's VP8 decoder

Portions of this software are copyright 2010-2025 Google Inc., from libwebp,
used under its BSD licence (`libwebp-COPYING`) with its additional patent
grant (`libwebp-PATENTS`); and copyright (c) 2010, 2011 Google Inc., from the
reference decoder in RFC 6386, used under the BSD licence the RFC prints
(`rfc6386-LICENSE.txt`).

| File | Derived from |
|---|---|
| `src/webp.rs` | libwebp `src/demux/anim_decode.c`, `src/demux/demux.c`, `src/dec/webp_dec.c` |
| `src/webp/riff.rs` | libwebp `src/dec/webp_dec.c`, `src/demux/demux.c` |
| `src/webp/alpha.rs` | libwebp `src/dec/alpha_dec.c`, `src/dsp/filters.c` |
| `src/webp/lossless.rs` | libwebp `src/dec/vp8l_dec.c`, `src/utils/bit_reader_utils.c`, `src/utils/huffman_utils.c`, `src/utils/color_cache_utils.c`, `src/dsp/lossless.c` |
| `src/webp/lossy.rs` | libwebp `src/dec/vp8_dec.c`, `frame_dec.c`, `tree_dec.c`, `quant_dec.c`; RFC 6386 |
| `src/webp/lossy/reader.rs` | libwebp `src/utils/bit_reader_utils.c`, `bit_reader_inl_utils.h` |
| `src/webp/lossy/filter.rs` | libwebp `src/dsp/dec.c`; RFC 6386 `dixie_loopfilter.c` |
| `src/webp/lossy/predict.rs` | libwebp `src/dsp/dec.c`; RFC 6386 section 12 |
| `src/webp/lossy/transform.rs` | libwebp `src/dsp/dec.c`; RFC 6386 sections 14.3 and 14.4 |
| `src/webp/lossy/tables.rs` | RFC 6386 (the tables it prints) |
| `src/webp/lossy/yuv.rs` | libwebp `src/dsp/yuv.h`, `src/dsp/upsampling.c` |

## Chromium, image-rs and Skia

Portions of this software are copyright The Chromium Authors and copyright
2023 Google LLC (Skia), used under their BSD licences
(`chromium-LICENSE`, `skia-LICENSE`), and are from image-rs, copyright its
contributors, used under the MIT licence (`image-rs-LICENSE-MIT`; image-rs is
MIT or Apache-2.0 at the user's choice).

| File | Derived from |
|---|---|
| `src/bmp.rs` | image-rs 0.25.10 `src/codecs/bmp/decoder.rs`, with Chromium's patches to it (`third_party/rust/chromium_crates_io/patches/`) |
| `src/ico.rs` | Blink `image-decoders/ico/ico_image_decoder.cc`, `image-decoders/bmp/bmp_image_reader.cc` |
| `src/orientation.rs` | Skia `src/codec/SkExif.cpp` (`SkExif::Parse`) |

## libavif 1.3.0

Portions of this software are copyright 2019-2020 Joe Drago, 2023 Google
LLC, and 2018 VideoLAN and dav1d authors and Two Orioles, LLC. They are used
under the BSD-2-Clause licence, reproduced with libavif's other notices in
`libavif-LICENSE.txt` (libavif v1.3.0's `LICENSE`).

| File | Derived from (libavif v1.3.0) |
|---|---|
| `src/avif.rs` | `src/read.c` (`avifDimensionsTooLarge`, the decoder's defaults) |
| `src/avif/stream.rs` | `src/stream.c` |
| `src/avif/container.rs` | `src/read.c`: `avifParse` and the `meta` box parsers |
| `src/avif/movie.rs` | `src/read.c`: `avifParseMovieBox` and the track parsers |
| `src/avif/setup.rs` | `src/read.c`: `avifDecoderParse`, `avifDecoderReset`; `src/avif.c`, `src/utils.c`: the clean-aperture crop; `src/gainmap.c`: `avifGainMapValidateMetadata` |
| `src/avif/obu.rs` | `src/obu.c`, which libavif took from dav1d |
| `src/avif/decode.rs` | `src/read.c`: `avifDecoderNextImage`, `avifDecoderDecodeTiles` and the grid tile copy; `src/codec_dav1d.c` |
| `gui/video/yuv/src/reformat.rs` (a crate of its own, which video frames share) | `src/reformat.c`, `src/reformat_libyuv.c`, `src/alpha.c`, `src/colr.c` (the matrix coefficients) |

The AV1 decoding itself is rav1d, a separate crate with its own notices
(`gui/video/rav1d/COPYING`).

The test fixtures `tests/data/avif_*.avif` are libavif's own test files
(`tests/data/` at v1.3.0), under the same licence; `tests/data/generate_avif.py`
names each one and pins it by SHA-256.

## libyuv

The YUV-to-RGB conversions and the scaling AVIF pictures go through are
libyuv's, ported in their own crate, `gui/video/yuv`, whose `licenses/`
carries libyuv's licence, patent grant and notice. That crate also carries
libavif's choice among them (`src/reformat.rs`, above), under the libavif
notice here.
