# Third-party code in `imagecodec`

Several of this crate's decoders are ports -- translations into Rust -- of
the decoders the browsers and Pillow run, because matching their pixels and
their refusals exactly is the point (see each module's documentation). A port
is a derivative work, and each upstream's licence asks that its notice travel
with the code. This file says what derives from where; each ported file also
carries its upstream's copyright line in its module documentation.

## libavif

Portions of this software are copyright 2019 Joe Drago, 2023 Google LLC, and
2018 VideoLAN and dav1d authors and Two Orioles, LLC. They are used under the
BSD-2-Clause licence, reproduced with libavif's other notices in
`libavif-LICENSE.txt` (libavif v1.3.0's `LICENSE`).

| File | Derived from (libavif v1.3.0) |
|---|---|
| `src/avif.rs` | `src/read.c` (`avifDimensionsTooLarge`, the decoder's defaults) |
| `src/avif/stream.rs` | `src/stream.c` |
| `src/avif/container.rs` | `src/read.c`: `avifParse` and the `meta` box parsers |
| `src/avif/movie.rs` | `src/read.c`: `avifParseMovieBox` and the track parsers |
| `src/avif/setup.rs` | `src/read.c`: `avifDecoderParse`, `avifDecoderReset`; `src/avif.c`, `src/utils.c`: the clean-aperture crop; `src/gainmap.c`: `avifGainMapValidateMetadata` |
| `src/avif/obu.rs` | `src/obu.c`, which libavif took from dav1d |

The test fixtures `tests/data/avif_*.avif` are libavif's own test files
(`tests/data/` at v1.3.0), under the same licence; `tests/data/generate_avif.py`
names each one and pins it by SHA-256.
