## TD-FONT-FALLS-BACK-PAST-A-SCRIPT-THE-FACE-REGISTERS -- FIXED

**Fixed.** Script selection and language-system selection are two separate
steps in OpenType, and only the first one falls back. We ran them as one: a
script table whose DefaultLangSys offset was zero was treated as *not found*,
and the fallback chain read on to `DFLT`, handing the run every feature the
face filed under the default script.

`NotoSansLisu-Bold.ttf` is the witness. Its `GPOS` ScriptList registers
`DFLT`, `latn` and `lisu`; `latn` has **no DefaultLangSys at all** — its
features are filed under the language systems `MOL ` and `ROM ` alone — while
`DFLT` names `kern`, `mark` and `mkmk`. HarfBuzz picks `latn` for a Latin run
(`hb_ot_layout_table_select_script` stops at the first *registered* tag,
whatever that script then contains), then asks it for the default language
system and gets the null offset, which resolves to `Null(LangSys)` with a
feature count of zero. So HarfBuzz shapes this face's Latin text with no
`GPOS` feature whatsoever — and, because `apply_gpos` is a property of the
*face* rather than of the plan's script, it does not run the mark fallback
either. `c` + U+0327 + U+0301 comes out as `ccedilla` + `acutecomb` with the
mark at offset zero. We ran the same face's MarkBasePos lookup 1 and placed
the accent at x=31 — arithmetically right for the anchors (base 287, mark
-258, advance 514), and wrong because the font never asked for it.

The fix is in three places in `gui/font/src/otl.rs`:

- `select` returns at the first script tag the ScriptList *has*, with
  `LangSys::lang_sys` an `Option` that is `None` when the DefaultLangSys
  offset is zero. `lookup_indices` then reads a feature count of zero.
- `lookup_indices` no longer collapses an empty result to `None`. `None` now
  means "no such script", which is the only thing the caller may treat as a
  reason to keep looking.
- `ByScript::parse` records a script even when it selects no lookups, and
  returns `None` on an empty *lookup* list rather than an empty *script*
  list. The empty entry is what stops `for_script`'s own fallback chain at a
  registered-but-empty script.

The last of the three matters as much as the first: `for_script` walks the
same chain a second time at shaping time, so leaving `latn` out of the map
would have let the run fall through to `DFLT` again after `select` had
correctly refused it.

Covered by `a_registered_script_with_no_features_does_not_fall_back_to_dflt`
in `gpos.rs`. The HarfBuzz sweep goes `agree` 11806 -> 11820, `misplaced`
42 -> 28.

**Still open.** HarfBuzz's script chain has a fourth entry ours does not:
after `DFLT` and `dflt` it tries `latn`, on the grounds that old fonts filed
everything there. Reached only by a run whose script the face registers under
none of the first three, so it is a narrower case than this one, and
separately measurable.
