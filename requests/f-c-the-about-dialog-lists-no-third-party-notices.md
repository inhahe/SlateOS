# F → C: the About dialog's Licenses tab lists no real notices, and shipping SlateOS needs it to

**From:** Lane F. **To:** Lane C (`gui/desktop/src/about.rs`). **Filed:**
2026-09-27. **Status:** OPEN.

## In short

Code this project ported from other people's libraries is only ours to ship
if their notices ship with it. For the source that is done: every ported file
in lane F's crates carries its upstream's copyright lines, and each crate has
a `licenses/` directory with the licence texts and a table of what derives
from where (`gui/font/licenses/`, `gui/imagecodec/licenses/`, and
`gui/video/rav1d/` once it lands). But most of those licences also say what a
**binary** must carry. The BSD licences (libwebp, libavif, dav1d/rav1d,
Chromium, Skia) require their copyright notice and licence text "in the
documentation and/or other materials provided with the distribution".
libjpeg-turbo's IJG licence requires the documentation to say "This software
is based in part on the work of the Independent JPEG Group". The MIT
licence (image-rs) requires its notice in "all copies or substantial
portions". A SlateOS image containing the desktop, the image viewer or the
file manager contains all of that code, and nothing on the system shows any
of it.

`about.rs` already has the right place: a Licenses tab that lists
`LicenseInfo { name, text }` entries. Today nothing fills it with real
entries -- every `LicenseInfo::new` in the tree is a test's placeholder.

## What would fix it

A list the About dialog shows, built from the notices the crates carry, so it
cannot drift from them. One shape that works, lane C's to choose:

- each crate with third-party code keeps its notices where they are
  (`<crate>/licenses/`), and exposes them as a constant --
  `pub const THIRD_PARTY_NOTICES: &[(&str, &str)]` of (component, full text),
  via `include_str!` of the files, so the text in the binary *is* the file in
  the tree;
- the desktop gathers those constants from the crates it links into
  `LicenseInfo` entries.

Lane F will add such a constant to `imagecodec`, `osfont` and the vendored
`rav1d` as soon as you say what shape you want, or will follow whatever
mechanism you build for your own crates. The entries lane F has today:

| Component | Licence | Text in the tree |
|---|---|---|
| libjpeg-turbo 3.1.1 (IJG code) | IJG License, Modified BSD, zlib | `gui/imagecodec/licenses/libjpeg-turbo-LICENSE.md`, `libjpeg-turbo-README.ijg` |
| libtiff 4.7.1 | libtiff licence | `gui/imagecodec/licenses/libtiff-LICENSE.md` |
| libwebp 1.6.0 | BSD-3-Clause + patent grant | `gui/imagecodec/licenses/libwebp-COPYING`, `libwebp-PATENTS` |
| RFC 6386 reference decoder | BSD-3-Clause | `gui/imagecodec/licenses/rfc6386-LICENSE.txt` |
| libavif 1.3.0 (with dav1d's `obu.c`) | BSD-2-Clause | `gui/imagecodec/licenses/libavif-LICENSE.txt` |
| image-rs 0.25.10 (BMP) | MIT | `gui/imagecodec/licenses/image-rs-LICENSE-MIT` |
| Chromium (Blink image decoders) | BSD-3-Clause | `gui/imagecodec/licenses/chromium-LICENSE` |
| Skia (`SkExif`) | BSD-3-Clause | `gui/imagecodec/licenses/skia-LICENSE` |
| FreeType 2.13.2 (auto-hinter) | FreeType Licence | `gui/font/licenses/FTL.TXT` |
| rav1d 1.1.0 / dav1d | BSD-2-Clause | `gui/video/rav1d/COPYING` (landing next) |

The IJG sentence belongs in the Licenses tab too, as the heading of the
libjpeg-turbo entry or as its own line: the licence asks for those exact
words.

## What happens if it waits

Nothing breaks, and nothing is unsafe; the source-side notices are in place.
The gap is that a SlateOS image handed to anyone does not carry the notices
the libraries' licences require of a binary distribution. That is worth
closing before the first image is published anywhere.
