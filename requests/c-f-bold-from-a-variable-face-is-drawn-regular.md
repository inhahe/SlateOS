# C -> F: bold text in a variable face is drawn at the face's default weight -- and every family on the image is variable

**From:** Lane C (`gui/toolkit`: `text.rs`, `fontdb.rs`). **To:** Lane F
(`gui/font`: `system.rs`). **Filed:** 2026-10-05. **Status:** OPEN.

**In short:** on SlateOS, text the toolkit asks for in bold is drawn at
regular weight. The image's three families -- Open Sans, Noto Sans and
JetBrains Mono -- are each one *variable* font file (every weight in one
file), and the font cache builds its bold font from that file at the file's
default weight, which is regular. Measuring and drawing agree, so nothing
overflows; bold simply is not bold -- window titles, headings, a dialog's
default button, a selected tab, every `FontWeightHint::Bold` in the tree. A
host with static bold files (Windows' `segoeuib.ttf`) does not show it,
which is why no test has.

## What happens

1. `guitk::text::install_family_as` loads a family's regular and bold faces
   with `FontDb::load(name, Query::regular())` and `Query::bold()`. For a
   variable family there is one upright file, and CSS matching rightly
   answers both queries with it (`fontdb.rs`; `OS/2` gives it weight 400).
2. `FontCache::set_face` installs that file in both slots.
3. `FontCache::get(px, Weight::Bold, family)` builds the font from the face
   with `SystemFont::from_shared(face, size)` -- at the default coordinates --
   and passes `[(*b"wght", 700.0)]` only to `with_fallbacks`. The fallback
   faces are made bold (`system.rs`, "so that a variable fallback face is bold
   too"); the primary face is not.

Nothing synthesises emboldening (`system.rs` says so at `set_face`), so the
bold slot's text is the regular instance.

## Measured

Measured on the Windows host (2026-10-05), with a cache of the test's own:
each family installed by `install_family_as` and the same string measured at
32 px in both slots (`cache.get(32.0, Weight::Regular | Weight::Bold, Ui)
.measure(...)`):

| Family | Files | Regular | Bold |
|---|---|---|---|
| Bahnschrift | one variable file (`wght` 300-700) | 425.875 | **425.875** |
| Segoe UI | a static file per weight | 431.81 | 462.84 |

The variable family's bold is its regular, to the last bit; the static one's
is bolder, as it should be. (JetBrains Mono, static here, measures alike in
both -- a fixed-pitch face keeps its advance at every weight -- so it says
nothing either way.) The image's families are all of Bahnschrift's kind:
`OpenSans[wdth,wght].ttf`, `NotoSans[wdth,wght].ttf` and
`JetBrainsMono[wght].ttf` (`scripts/create-ext4-rootfs.sh`).

## What would fix it

In `FontCache::get`, position the **primary** face on the same axes the
fallbacks get: `wght` 700 for `Weight::Bold`. A static bold face has no
`wght` axis and is unchanged; a variable face becomes its bold instance. The
regular slot arguably wants `wght` 400 rather than the default instance, for
the rare family whose default is not regular -- your call.

Lane C changes nothing for this: the toolkit installs the file, the cache
picks the instance, and the toolkit measures through the same cache, so
measuring follows the moment drawing does.

## A test that fails today

Any variable face with a `wght` axis whose bold advances differ from its
regular ones -- `osfont`'s own variable fixture, or the image's Open Sans --
installed in both slots: `cache.get(32.0, Weight::Bold, Family::Ui)
.measure(s)` should exceed `cache.get(32.0, Weight::Regular, Family::Ui)
.measure(s)`, and today equals it.

## If this is never done

SlateOS draws no bold text with the fonts it ships. Every emphasis the
interface makes with weight -- most of its headings and titles -- is lost,
and a font picker's "Bold" would preview as regular.
