### [F] A variable font without `HVAR` keeps its default advances at every weight -- 2026-09-26 -- **FIXED 2026-09-27**

**Status:** FIXED 2026-09-27 (lane F) -- `Face::advance_at` takes such a
face's advance from its phantom points (`glyf::advance`), as HarfBuzz 14.3.0
does. The fixture's Bold master now varies its widths (600 to 630), so the
test can fail: `glyf::tests::a_face_without_hvar_advances_between_its_phantom_points`
pins HarfBuzz's advances at weights 610 and 401, and its `HVAR` twin those of
the face with the table. FreeType's hinted points for both variable faces were
regenerated from the new masters and the hinter still matches them.

**In short:** a variable font may leave out its table of advance-width
changes (`HVAR`) and let each glyph's outline data carry them instead, as
the positions of its "phantom points". HarfBuzz reads the advance from
there; this crate ignores them and uses the default advance at every weight,
so text in such a font keeps its regular letter spacing when set bold. Every
variable font on this host has `HVAR`, so nothing here shows it.

**Where:** `Face::advance_at` (`gui/font/src/sfnt.rs`) returns the unvaried
advance whenever the face has no `HVAR`.

**What HarfBuzz 14.3.0 does** (`hb_ot_get_glyph_h_advances`, and
`glyf_accelerator_t::get_advance_with_var_unscaled`): with `gvar` but no
`HVAR`, the advance is the right phantom point's x less the left's, where
`gvar` moved them, by HarfBuzz's `roundf`, clamped at zero; half an em for a
glyph it cannot read; and read through the same shared-tuple scalar cache as
drawing (`gvar::Scalars::Drawn`). A `CFF2` face without `HVAR` keeps its
default advances there too.

**The fix is ready but for its test.** Since 2026-09-26 the phantom points
carry their real values (`crate::glyf`), so the advance is
`roundf(right.x - left.x)` over `glyf::points(.., Scalars::Drawn)`, taken in
`advance_at` when the face has `gvar` and `glyf` but no `HVAR` -- about ten
lines. (A patch doing so, and one recording HarfBuzz's advances into the
hint fixture after drawing every glyph to warm its cache, may survive in lane
F's worktree under `target/glyfpts/deferred/`; `target/` is not kept.) What is missing is a test that can fail: the fixture's two masters
are both 600 units wide everywhere, so its advances do not vary. The Bold
master needs other widths for some glyphs -- which changes the `VAR` and
`VAR_NOHVAR` faces and so every FreeType expectation drawn from them, to be
regenerated and re-verified with `tools/hint_oracle.py`.
