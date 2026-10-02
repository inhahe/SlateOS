## TD-FONT-SCRIPT-FALLBACK-STOPS-BEFORE-LATIN -- FIXED

**Fixed.** Our script fallback chain was the run's script, its older OpenType
spelling, `DFLT`, `dflt`. HarfBuzz's has a fifth entry —
`hb_ot_layout_table_select_script` tries `latn` last, with the comment "some
old fonts put their features there even though they're really trying to
support Thai, for example". A face that registers no default script at all was
therefore shaped with no features by us and with `latn`'s features by
HarfBuzz.

`Gabriola.ttf` is the witness: its `GPOS` ScriptList is `cyrl`, `grek`, `latn`
and no `DFLT`. `123 456` is all digits and a space, so the run has no script
of its own and starts the chain at `DFLT` — which the face does not have. We
reached no `kern` feature and left `456` unkerned; HarfBuzz reached `latn`'s
and pulled the pairs in by 40 and 60 units.

`fallback_chain` in `otl.rs` now ends with `latn`. Both walks that use it —
`select`, at parse time, and `ByScript::for_script`, at shaping time — pick it
up together, which is the point of their sharing the function.

`a_script_the_face_does_not_register_gets_nothing` in `gsub.rs` was asserting
the old behaviour on a face that registers `arab` and `latn`; it is now
`a_script_the_face_does_not_register_falls_back_to_latin_and_then_nothing` and
checks both halves — the `latn`-registering face answers a Hebrew run, and an
`arab`-only face still says nothing. The HarfBuzz sweep goes `agree`
11826 -> 11828, `misplaced` 22 -> 20.
