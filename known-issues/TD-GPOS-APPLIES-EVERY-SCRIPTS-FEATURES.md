## TD-GPOS-APPLIES-EVERY-SCRIPTS-FEATURES

**Status:** FIXED 2026-08-16 -- the resolution note in the entry below gives the change; on `main` since. Stamped 2026-09-28 by lane F, `gui/font`'s owner since the six-lane split, so that the heading no longer reads as open.

**What.** The `GSUB` half of the table walk now selects features by the run's
script. The `GPOS` half does not: `otl::feature_subtables` takes the union over
every script the face registers, which is the behaviour
`TD-GSUB-APPLIES-EVERY-SCRIPTS-FEATURES` was filed against.

**Why this is a smaller problem than it sounds.** `GPOS` only *moves* glyphs;
it cannot change what the text says. And every positioning subtable is gated on
glyph coverage, so a face's Arabic `kern` does not cover Latin glyphs and its
pairs never fire on a Latin run. The failure mode is therefore a wasted
coverage lookup, not a wrong glyph — unlike `GSUB`, where the same fault
rewrote Calibri's slash.

**Why it is filed rather than fixed.** `GPOS` is reached through
`ScaledFont::kern(left, right)`, a public API handed two glyph ids with no run,
no string and no script behind them. There is nothing to select with without
changing that signature, and the signature is what the layout code wants: it
asks about a pair while walking a shaped run it has already produced.

**How it could actually bite.** A face registering `kern` pairs under two
scripts whose coverage sets *overlap* — shared punctuation is the realistic
case — could take the wrong script's value. Not observed on any of the 556
faces installed here.

**Proper fix.** Give positioning the same treatment as substitution: build a
`ByScript` for `GPOS` too, and change `kern` to take the script — or replace it
with a whole-run positioning pass, which is what mark attachment will need
anyway for GPOS type 5. The second is the better shape and should be done when
mark-to-ligature lands.

**Where.** `gui/font/src/otl.rs` — `feature_subtables`;
`gui/font/src/kern.rs`; `gui/font/src/mark.rs`;
`ScaledFont::kern` in `gui/font/src/scaled.rs`.

**Status: FIXED** (2026-08-16, verified rather than implemented). The entry
predicted its own fix correctly -- "a whole-run positioning pass ... is the
better shape and should be done when mark-to-ligature lands" -- and that is
what happened, in the type-5/7/8 work, without this entry being revisited.
Checked line by line today:

* `gui/font/src/gpos.rs` builds a `ByScript`, and `Positioning::apply` walks
  only `self.lookups.for_script(run.script, run.lang)`. Every lookup that moves
  a glyph in shaped text -- pair kerning, cursive, all three mark types,
  contextual -- is selected by the run's script and language, with the same
  fallback chain `GSUB` uses.
* `gui/font/src/kern.rs`'s union is reached from shaped text only for the
  *legacy* `kern` table, which has no script systems in it to select among.
  `scaled.rs` gates it on `!self.face.gpos_kerns(segment.script, lang)`, so a
  run whose script does reach a `GPOS` `kern` feature never takes it.
* The two surviving `feature_subtables` calls are in `MarkPositioning::parse`,
  and everything they feed is `MarkPositioning::is_mark` -- "is this glyph a
  combining mark", which is a property of the glyph and not of the run. The
  union is the *right* answer there: a face's Arabic anchors identify Arabic
  marks whatever script is being shaped, and narrowing it by script would make
  `is_mark` say no to a mark whose only mention is under another script's
  `mark` feature.

What is left is the entry's own stated reason for filing, unchanged and
harmless: `ScaledFont::kern(left, right)` takes two glyph ids with no run, so
it cannot select and does not. It is public API with no caller in this tree
(the compositor draws through `ShapedRun`), it is documented as the answer for
a caller that has no script, and the overlap case the entry describes -- two
scripts' `kern` coverage sets intersecting -- is still not observed on any of
the 556 host faces. Narrowing it would mean re-signaturing a public method to
serve nobody, so this closes as verified rather than as further work.
