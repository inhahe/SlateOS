## TD-C-BOLD-TEXT-IN-A-VARIABLE-FACE-IS-DRAWN-REGULAR (lane C, 2026-10-05) — FIXED

**Status:** FIXED 2026-10-05 by lane F (801d9d8d2, on main and boot-tested
with lane F's publish 05a998ca9): `FontCache::get` positions every face a
font is built from at the weight asked for where it has a `wght` axis -- 700
for bold, 400 for regular. Asked in
`requests/c-f-bold-from-a-variable-face-is-drawn-regular.md`.

**In short:** on SlateOS no text is drawn bold. Every family the image ships
(Open Sans, Noto Sans, JetBrains Mono) is a single variable font file, and the
font cache builds its bold font from that file at the file's default weight,
which is regular. Measuring and drawing use the same cache, so nothing
overflows -- the emphasis is simply gone: window titles, headings, a dialog's
default button, every `FontWeightHint::Bold`.

**Where:** `osfont::system::FontCache::get` builds the primary font with
`SystemFont::from_shared(face, size)` at the default coordinates and passes the
bold axes (`[(*b"wght", 700.0)]`) only to the fallback faces
(`with_fallbacks`). `guitk::text::install_family_as` fills both slots with the
one variable file, which is what CSS matching rightly answers for a family
that has one upright file.

**How to see it:** install a variable family in a `FontCache` with
`install_family_as` and measure a string in both slots: Bahnschrift on the
Windows host measures 425.875 px at 32 px in each, where Segoe UI (a static
bold file) measures 431.81 regular and 462.84 bold.

**The fix:** position the primary face on the same axes as the fallbacks in
`FontCache::get` -- `wght` 700 for bold. A static face has no `wght` axis and
is unchanged. Nothing in the toolkit changes: it measures through the same
cache, so measuring follows the drawing.
