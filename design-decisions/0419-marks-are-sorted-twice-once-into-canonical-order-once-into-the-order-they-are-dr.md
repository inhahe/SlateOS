## §419 — Marks are sorted twice: once into canonical order, once into the order they are drawn

**Date:** 2026-08-14
**Lane:** C
**Decided by:** Claude (autonomous)

Unicode's canonical combining classes are an *ordering*, chosen so that two
spellings of the same text normalize to one string. They are not a description
of where a mark sits, and for the fixed-position classes — Hebrew 10–26, Arabic
27–35, Syriac 36, Telugu 84 and 91, Thai 103, Tibetan 129–132 — the order they
impose is close to the opposite of the order the marks are stacked in. A Hebrew
vowel is typed before the shin dot and drawn under the letter while the dot goes
over it. Arabic shadda is typed after a vowel and drawn nearer the letter than
it; Unicode's own normalization FAQ names that case and declines to renumber,
because the classes are a stability guarantee and cannot move.

So a renderer that sorts marks only into canonical order stacks them wrong. Ours
did, and it was 465 of the sweep's 841 `misplaced` cases — one string,
`שָׁלוֹם` typed with the qamats before the shin dot.

**Decision.** Sort twice. `norm::sort_marks` takes the class function as a
parameter; `nfc` calls it with `combining_class` and the shaper's entry point,
`norm::pieces`, calls it a second time with `display_class` — a permutation of
the fixed-position blocks whose *numeric* order is stacking order, bottom to
top. This is HarfBuzz's `_hb_modified_combining_class`, which exists for exactly
this and does exactly this.

**Where the second sort goes, which is the actual decision.** §410 says `nfc()`
is pure Unicode and never sees a font, and that is the promise worth keeping:
it is the function a caller with a *text* question asks, and its answer must be
NFC or it is not an answer. HarfBuzz has no such constraint — its normalizer is
private to the shaper — so it substitutes the modified classes inside
normalization and sorts once. Three options were live:

1. *Sort with the modified classes inside `nfc`,* as HarfBuzz does. Smallest
   change and one pass instead of two. Rejected: `nfc` would return something
   that is not NFC, silently, and the name would be a lie. §410's two-layer
   split exists precisely so that "what the text is" has a home separate from
   "what this face can show".
2. *Sort in the shaper, in `scaled.rs`.* Keeps `norm.rs` entirely about
   Unicode. Rejected: the sort is a Unicode fact — a table of classes and a
   stable insertion sort — and putting it in `scaled.rs` means `scaled.rs`
   grows a second copy of the loop `norm.rs` already has, and a second answer to
   "which characters are starters".
3. **Chosen:** sort in `norm::pieces`, the shaper's entry point, leaving `nfc`
   exactly NFC. `pieces` already promises less than `nfc` does — it is
   *"the characters a face should actually be asked for"*, and has been
   font-dependent since §410 — so display order is the same kind of claim as
   `fit_to_face`'s, made in the same place.

**Pro:** one implementation of the sort, one table, one definition of a
starter; `nfc` keeps its meaning; the fallback's `attach_class` needs no change
at all, because it matches on real classes and the permutation is injective
within each block.
**Con:** `pieces` output is not NFC for five scripts, which a reader could
assume it is. Stated in its doc comment, in `display_class`'s, and here.

**The bijectivity is load-bearing and is tested.** If two classes the canonical
sort keeps apart mapped onto one display class, the second sort would merge them
and their *typed* order would start deciding their stacking order — silently,
and only for the colliding pair. `each_permuted_block_is_a_bijection_onto_itself`
checks it. It also found the one place the claim is narrower than it looks:
Tibetan 132 maps to 131, which is legal only because Unicode assigns no
character class 131. HarfBuzz's table collides there too.

**Measured.** 556 faces x 23 strings: `agree` 10917 → 11223, `misplaced`
841 → 625, `reordered` 32 → 0, `differ` 998 → 940. The pointed-Hebrew string
went 465 → 249. Nothing regressed.

**Where.** `gui/font/src/norm.rs` — `display_class`, `permute`, `sort_marks`
and the second call in `pieces`. `gui/font/src/fallback.rs` — `attach_class`,
whose doc records why matching real classes still selects the right characters.
