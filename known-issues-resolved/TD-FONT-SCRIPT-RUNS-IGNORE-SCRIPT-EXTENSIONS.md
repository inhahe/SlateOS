## TD-FONT-SCRIPT-RUNS-IGNORE-SCRIPT-EXTENSIONS

**Status: FIXED** (2026-08-16, lane C). `gen_script_tables.py` now emits
`SCRIPT_EXT_RANGES` / `SCRIPT_EXT_POOL` — the `Script_Extensions` property
wherever it says something `Script` does not, which on Unicode 16.0.0 is 669
characters in 177 ranges, 119 distinct sets, widest 23, stored in 452 pooled
rows because a set that appears verbatim inside a longer one is stored once.
`script::runs` carries the intersection of the open run's set with each new
character's, exactly as the proper fix below asked.

Three things the fix had to settle that the entry did not anticipate:

* **`SCRIPT_TAGS` is now one row per OpenType *tag pair*, not per Unicode
  script.** `kana` is the only pair two scripts share, and it is the one that
  matters: comparing scripts rather than rows would cut Japanese at every
  change between hiragana and katakana, for a difference no font can act on.
* **A character with no `Script` of its own may narrow a run but never end
  one** — it has an affinity, not an identity. Without this, U+0301 COMBINING
  ACUTE (used by eight scripts, none of them Hebrew) starts a run of its own
  after a Hebrew letter, and a mark in its own run is a mark whose base's
  `GPOS` never attaches it.
* **Script is resolved over the whole text first, and the direction boundaries
  are cut into that answer afterwards** (`by_script`, then `runs`). Resolving
  per directional run instead re-derives the script of `"ހ٠ހ"`'s middle piece,
  which bidi rule I2 makes a run of its own, as Arabic — measured as a real
  disagreement with HarfBuzz on `SansSerifCollection.ttf`, which has an Arabic
  `locl` and no Thaana one. The one exception is a character with no script at
  all: it belongs to the directional run it is *drawn* in, so the space in
  `"hello שלום world"` joins the Latin after it rather than trailing the
  Hebrew before it.

Measured against HarfBuzz over 556 faces × 95 strings: `agree` 51422 → 51423,
`differ` 1179 → 1178, `misplaced` and `reordered` unchanged. 711 unit tests and
18 host-font tests pass. Kept here (not archived) until it has survived a boot
on `main`.

**What.** `script::runs` resolves a character's script from the Unicode
`Script` property alone. UAX #24 defines the real algorithm over
`Script_Extensions`, which lists *every* script a shared character is used
with — the danda U+0964 is `Script=Common` but `Script_Extensions` names
Devanagari, Bengali, Gurmukhi and a dozen more.

**What the difference actually is.** Our rule is the one UAX #24 calls the
starting point: a scriptless character extends whatever run is open, and a
scriptless prefix joins the first real script after it. That gives the right
answer whenever a shared character is adjacent to text of a script it belongs
to, which is nearly always. The full algorithm differs only for a character
that is ambiguous *and* sits at a boundary between two scripts that both claim
it — then it should join the one it is actually adjacent to under the
extension set, rather than simply continuing the open run.

**Why it is filed rather than fixed.** It needs a second generated table
(`Script_Extensions` is a set per character, not a scalar) and a resolution
pass that carries a candidate set forward, and it changes the answer only for
cases that are already vanishingly rare in the text this OS renders. The
generator already reads `fontTools.unicodedata`, which exposes
`script_extension`, so the data side is small; the algorithm side is not.

**Proper fix.** Emit a second table of extension sets, and replace the
"extends whatever is open" rule with UAX #24's: carry the intersection of the
open run's script set with each new character's, and close the run when the
intersection empties.

**Where.** `gui/font/src/script.rs` — `runs` and the module doc;
`gui/font/tools/gen_script_tables.py`.
