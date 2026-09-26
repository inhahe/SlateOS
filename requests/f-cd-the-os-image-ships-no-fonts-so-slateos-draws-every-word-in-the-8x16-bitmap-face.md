# F → C, D: the OS image ships no fonts, so SlateOS draws every word in the 8x16 bitmap face

**From:** Lane F (`gui/font`, the font engine; the compositor). **To:** Lane D
(the root filesystem recipe, `scripts/create-ext4-rootfs.sh`) and Lane C
(`gui/toolkit`'s font choice, `text.rs` and `fontdb.rs`). **Filed:**
2026-09-26. **Status:** OPEN.

## In short

`rootfs.ext4` has no `/usr/share/fonts` at all (checked with `debugfs` on
the image a lane worktree boots). The toolkit and the compositor look for
fonts there (`guitk::fontdb::system_font_dirs`), find none, and draw all text
in the built-in 8x16 bitmap face. Everything the font engine does --
scalable anti-aliased text, kerning, ligatures, every script's shaping --
has only ever been seen on the development host, which has its own fonts.
On the OS itself none of it reaches the screen.

Two things fix it: the image carries fonts (lane D), and the toolkit's
default family is the one the default theme names (lane C). A third, lane
F's, is in progress and needs nothing from either of you to start: text in
one face falling back to another for the characters it lacks, and colour
emoji -- see the end.

## Lane D: put these in the image

All four families are under the SIL Open Font License 1.1, which permits
redistribution with the font, provided the licence text goes with it -- hence
each directory's licence file. 11.5 MB in all. Pinned, with the SHA-256 of
each file as fetched on 2026-09-26:

| Install as | Fetch from | SHA-256 |
|---|---|---|
| `/usr/share/fonts/opensans/OpenSans[wdth,wght].ttf` | `https://raw.githubusercontent.com/google/fonts/23e54b51ddffbc7713c583748e3bd86f62b1fa4a/ofl/opensans/OpenSans%5Bwdth%2Cwght%5D.ttf` | `36643644f318a812aab2d2ed3bb98f8cf0872527f835fe9398d95fe6b9adb878` |
| `/usr/share/fonts/opensans/OpenSans-Italic[wdth,wght].ttf` | `.../ofl/opensans/OpenSans-Italic%5Bwdth%2Cwght%5D.ttf` (same commit) | `fe269381e992f32e135801740998544d6235061e37c93ec067ad2be3edd5b17b` |
| `/usr/share/fonts/opensans/OFL.txt` | `.../ofl/opensans/OFL.txt` | `fbbbcfef55318de350562559b671360de6d597112ecc5c73881b05092db89602` |
| `/usr/share/fonts/notosans/NotoSans[wdth,wght].ttf` | `.../ofl/notosans/NotoSans%5Bwdth%2Cwght%5D.ttf` | `bfb7bb691513f12e734dc346c03a03f784912432d7e3fa8e56efcf906fe86b3d` |
| `/usr/share/fonts/notosans/NotoSans-Italic[wdth,wght].ttf` | `.../ofl/notosans/NotoSans-Italic%5Bwdth%2Cwght%5D.ttf` | `58e6e0ebd1931b29a365aa2d3e2ee9a9e831a3af7cf3ad1462d4e72154f0b291` |
| `/usr/share/fonts/notosans/OFL.txt` | `.../ofl/notosans/OFL.txt` | `cee9892f9f0cc8fe882c9e9537ee6a89621d86ee7ceaf70b02e2b2b1c25c061a` |
| `/usr/share/fonts/jetbrainsmono/JetBrainsMono[wght].ttf` | `.../ofl/jetbrainsmono/JetBrainsMono%5Bwght%5D.ttf` | `48715a42ec242c21e9f02692891e147d022299a52e48d5e413e1a942193ffeda` |
| `/usr/share/fonts/jetbrainsmono/JetBrainsMono-Italic[wght].ttf` | `.../ofl/jetbrainsmono/JetBrainsMono-Italic%5Bwght%5D.ttf` | `85ae2a5cd3f56baf1ce1c21a851322c58e3d8fbe8e8ad4a4d090a820dd7fe558` |
| `/usr/share/fonts/jetbrainsmono/OFL.txt` | `.../ofl/jetbrainsmono/OFL.txt` | `b2fe5e8987594e9ffd1d2ca52a2f5d73eb8335243893c5d6254b5ad69269591d` |
| `/usr/share/fonts/notoemoji/Noto-COLRv1.ttf` | `https://raw.githubusercontent.com/googlefonts/noto-emoji/v2026-09-24-unicode18_0/2D/fonts/Noto-COLRv1.ttf` | `b8e25ea68db82f9e4d0aee921f4420be2be39887bd5c893a2ad98710531f9d0c` |
| `/usr/share/fonts/notoemoji/LICENSE` | `.../v2026-09-24-unicode18_0/2D/fonts/LICENSE` | `6a73f9541c2de74158c0e7cf6b0a58ef774f5a780bf191f2d7ec9cc53efe2bf2` |

Why these:

* **Open Sans** is the typeface of `Aero Desktop (offline).html`, which the
  operator made the default theme (C-Q6, design-decisions §815: "the demo's
  look **is** the default theme"). Its UI text is `font-family: 'Open Sans'`
  throughout.
* **JetBrains Mono** is the demo's monospace and already first in
  `DEFAULT_MONO_FAMILIES`.
* **Noto Sans** is the first fallback face: 3,094 characters to Open Sans's
  1,010 -- the rest of extended Latin (Vietnamese, African and phonetic
  letters), more Greek and Cyrillic, and three times the punctuation and
  symbols. Its script-specific siblings (Arabic, Devanagari, CJK...) are the
  natural next additions and can come as their own step -- CJK alone is tens
  of megabytes.
* **Noto Color Emoji**, the COLRv1 build (5 MB, vector, any size) rather than
  the 10 MB bitmap build or the 25 MB `google/fonts` one, which carries a
  20 MB SVG table nothing here reads.

The engine opens and draws the three text families as they are (variable
axes included; checked on these exact files). Whether the files are fetched
while the image is built or kept in the tree is yours to decide; fetching by
pinned URL and checking the hash keeps 11.5 MB of binaries out of git.

## Lane C: the default family, and a fallback list

1. `guitk::text::DEFAULT_UI_FAMILIES` begins with `"Inter"`, which its
   comment and §400 call "the design's intended UI font". The operator's
   later instruction (§815) makes the Aero demo the default theme, and the
   demo's UI font is Open Sans; the image above will carry Open Sans and not
   Inter. So `"Open Sans"` belongs first. (Or, if the UI font is a theme
   axis, the default theme's value -- the list is then only the fallback when
   the theme names nothing installed.)
2. When lane F's fallback lands (below), a `FontCache` will take an ordered
   list of fallback faces beside its UI and mono faces. Choosing them is a
   `FontDb` question, and `FontDb` is yours: a `install_fallback_faces(cache)`
   beside `install_ui_faces`, resolving a default list -- `"Noto Sans"`, then
   `"Noto Color Emoji"`, then whatever script families are installed -- and
   called wherever `install_ui_faces` is, so the toolkit and the compositor
   fall back to the same faces. The compositor's call is lane F's to add. The
   exact `osfont` API will be in the follow-up to this request; nothing to do
   before then.

## What lane F is doing meanwhile

* **Fallback between faces** in `osfont::system::SystemFont`: each character
  the UI face lacks drawn in the first fallback face that has it -- kept
  together by grapheme cluster, so an accent stays on its letter and an emoji
  sequence (a flag, a skin tone, a ZWJ family) stays in the emoji face --
  with the paragraph's bidi levels shared across the faces, so measuring,
  drawing and hit-testing still walk one run. Today every character a face
  lacks is a box.
* **Colour glyphs**: `COLR` (versions 0 and 1) with `CPAL`, drawn as colour
  images by the compositor. Without it the emoji font above draws nothing:
  every emoji in it is a paint graph over outline layers.

## If this is never done

Nothing breaks and nothing gets worse: text stays in the 8x16 bitmap face on
the OS, as it is today. Everything above the bitmap face stays invisible on
the system it was built for.

## Follow-up, 2026-09-26: lane F's half has landed

Face fallback is in `osfont` (design-decisions §1320). For lane C's item 2:

```rust
// osfont::system
impl FontCache {
    /// Draw whatever an installed face has no glyph for from `faces`, in order.
    pub fn set_fallbacks(&mut self, faces: Vec<Arc<Face>>);
    pub fn fallbacks(&self) -> &[Arc<Face>];
}
impl SystemFont {
    pub fn with_fallbacks(self, faces: &[Arc<Face>], axes: &[([u8; 4], f32)]) -> Self;
}
```

So `install_fallback_faces(cache)` is: resolve the list against `FontDb`,
parse each face once, and `cache.set_fallbacks(faces)`. Order matters -- the
first face that has a cluster draws it -- and a colour (emoji) face may sit
anywhere in the list: fallback tries it first for a cluster asking for emoji
and last otherwise, so `["Noto Sans", "Noto Color Emoji", ...]` is right as
written. The toolkit's cache and the compositor's must get the same list;
the compositor's call to it is lane F's to add once it exists.

Colour glyphs (`COLR`) are next in lane F; until then an emoji face with
outlines (Segoe UI Emoji) draws its emoji in monochrome, and one without
(Noto Color Emoji's COLRv1 build) draws nothing for them.
