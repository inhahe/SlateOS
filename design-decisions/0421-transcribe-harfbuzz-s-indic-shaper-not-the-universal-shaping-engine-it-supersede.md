## §421 — Transcribe HarfBuzz's Indic shaper, not the Universal Shaping Engine it superseded

**Date:** 2026-08-14
**Lane:** C
**Decided by:** Claude (autonomous)

`TD-FONT-HAS-NO-JOINING-OR-REORDERING-SHAPER` proposed "a Universal Shaping
Engine pass for Indic and South-East Asian" as the second of its two shapers.
That is the shape the Unicode spec suggests — USE was written to be the one
shaper every complex script could share, and the Indic scripts are complex
scripts — and it is what a reading of the standards would produce. It is
nevertheless the wrong target here, and the reason is a fact about the oracle
rather than about the spec.

**HarfBuzz does not run USE for Devanagari.** `hb-ot-shaper-indic.cc` and
`hb-ot-shaper-use.cc` are separate modules, and `hb_ot_shaper_categorize`
sends the nine Indic scripts to the first one. The split is not an accident of
history that USE will eventually absorb: the Indic scripts' reordering is
specified against **Uniscribe**, whose behaviour predates USE and whose quirks
Microsoft's fonts were built around — the `dev2`/`deva` tag revision, the
old-spec halant move, the reph position table, Malayalam's zero-context
exception. USE's cluster grammar deliberately does not reproduce them.

So the two options were not "USE or a narrower USE".

**Write USE and point Devanagari at it.** One shaper instead of two, covering
~90 further scripts as a side effect, and the standards-blessed model. But it
would have matched neither oracle: not HarfBuzz, which runs the Indic shaper,
and not the fonts, which were built for Uniscribe. Every disagreement it
produced would have needed a judgement about whether HarfBuzz or the spec was
right, which is exactly the diagnosis cost the sweep exists to avoid. And it
would not have fixed the measured symptom, since the sweep's only complex-script
strings are Arabic and Devanagari.

**Transcribe the Indic shaper.** More total code — USE still has to be written
afterwards for the scripts it covers, and is now filed as
`TD-FONT-HAS-NO-UNIVERSAL-SHAPING-ENGINE` — and a second syllable grammar to
maintain. In exchange every disagreement is a bug in the transcription and can
be diagnosed by reading one file of HarfBuzz beside one file of ours, which is
the property that took `हिन्दी` from 5 disagreeing faces to 0 and the whole
`misplaced` bucket to 0.

The second is what was done. The parts that are *not* Indic-specific were
written to be reused: `apply_stages` (a lookup set per stage, the shaper's own
features confined to one syllable), the syllable stamping in `setup_syllables`
(a byte per glyph rather than a range, so a `ccmp` ligature cannot invalidate a
boundary), and `Plan`'s once-per-run probing of what the face declares. USE
needs all three and none of them assumes Indic.

**Where.** `gui/font/src/indic.rs`, `indic_machine.rs`, `indic_shape.rs`;
`gui/font/src/gsub.rs` — `apply_stages`; `gui/font/src/sfnt.rs` —
`Face::substitute`, which dispatches on `Script::shaping`.
