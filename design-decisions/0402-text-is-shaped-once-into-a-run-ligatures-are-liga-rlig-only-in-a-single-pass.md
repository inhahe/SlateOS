## §402 — Text is shaped once into a run; ligatures are `liga` + `rlig` only, in a single pass

**Date:** 2026-08-14
**Lane:** C
**Decided by:** Claude (autonomous)

**Decision.** Every consumer of a string's layout — measuring, drawing,
hit-testing, truncation — goes through one `ScaledFont::shape` /
`SystemFont::shape` call that returns a `ShapedRun`. On top of that run,
GSUB ligature substitution is enabled for exactly two features, `liga`
(standard ligatures) and `rlig` (required ligatures), read from LookupType 4
in a single left-to-right pass with longest-match-first within a ligature
set.

**Why one run.** The recurring bug class in this code is two loops over the
same string arriving at different answers. It has now happened twice for
real: kerning was added to `measure` but not to `fit`/`char_index_at`, so a
prefix chosen to fit *N* px measured wider than *N* px and a click landed on
one character while the caret drew at another; and `measure` expanded `\t`
to four cells while `draw_text` drew it as the missing-glyph box. Neither
was a hard fault — both are text that is quietly wrong, which is the kind of
defect that survives every test that looks at a single function. A single
`ShapedRun` walked by all four callers makes the divergence unrepresentable
rather than merely discouraged, and it is what made ligatures expressible at
all: the old `SystemFont::glyph(ch)` API was 1:1 char→glyph, so no ligature
could have been returned through it. That API was deleted rather than
documented.

*Cost:* shaping allocates a `Vec` per call where the old loop allocated
nothing, and a widget that measures then draws shapes twice. Accepted:
`shape` touches `cmap`/`hmtx`/GPOS/GSUB only, never the rasterizer or the
glyph cache, and the alternative — a cache keyed by (string, size, weight) —
buys a memcpy-scale saving at the price of an invalidation problem. Revisit
if profiling of a real desktop frame says otherwise.

**Why kerning is charged to the preceding glyph.** A `ShapedGlyph`'s
`advance` includes the kern against the glyph that follows it, so
`sum(advance) == run.width()` and the draw loop is exactly `pen +=
advance` — no separate "look at the next glyph" step that a caller could
forget. The part of the advance that is a kern is kept in `kern_next`
because it has to be *recoverable*: `fit` cuts a prefix that the caller then
draws **alone**, so the kern pulling the last surviving glyph toward a
dropped one must come off. Without that, `fit("AVATAR", 10.0)` keeps the `A`
on the strength of a −0.4 px correction for a `V` that is no longer there,
and the result measures 10.32 px — wider than the budget it was asked to
fit. `fit_end` needs no such correction, which is the same fact seen from
the other side.

**Why only `liga` and `rlig`.**

- `liga` is what a face means by "this is how I am meant to be set": `fi`,
  `fl`, `ffi`. Leaving it off means the `f`'s hood collides with the `i`'s
  dot in every serif face on the desktop — the font ships a fix for its own
  defect and we were declining it.
- `rlig` is not optional in the sense that matters: Arabic lam-alef is a
  *required* ligature, and text that omits it is not a stylistic variant,
  it is wrong.
- `dlig`, `hlig` and `swsh` are discretionary by definition — the spec says
  they are off unless the *document* asks. There is no per-run feature list
  in this stack to ask with, so enabling them would apply a typographic
  choice to every button label on the system.
- `clig` (contextual ligatures) is excluded for a different reason: it needs
  LookupType 6 (chained context) to be honest, and a half-implementation
  that fires context-dependent lookups without their context is worse than
  none.

**Why a single pass.** The spec's model is iterative — a lookup's output can
feed a later lookup. In practice `LigatureSet`s are ordered longest-first by
convention, so trying every record at a position and taking the first match
gets `ffi` right without a second pass. The alternative (loop until
fixpoint) costs a bounded-iteration guard and a re-walk of the run for a
gain no Latin face demonstrates. Documented here so the next session knows
it is a decision, not an oversight: a script that genuinely needs multi-pass
substitution (Indic reordering) needs far more than this anyway.

**What is deliberately left out.** Script and language selection (the
`ScriptList` is not consulted — features are taken from the `FeatureList`
by tag, which is the default-script behaviour every Latin face wants and is
wrong for a face whose `liga` differs by script); GPOS mark attachment;
contextual and chained-context substitution; bidi. Each is a separate
roadmap step, not a gap this one should have closed.

**Measured.** Of 556 faces installed on the development host, 169 carry
ligature lookups this reads and 114 substitute `fi`. Notably `times.ttf` and
`segoeui.ttf` do **not** — the shipped Microsoft core versions carry no
`liga` ligature lookup for it — which is why the host-font test treats its
oracle list as "at least one of these" rather than "all of these".

**Where it lives.** `gui/font/src/shape.rs` (`ShapedRun`, `ShapedGlyph`,
`GlyphKey`, `fit`/`fit_end`/`offset_at`/`x_of`), `gui/font/src/gsub.rs`
(ligature parsing), `gui/font/src/otl.rs` (the walk shared with GPOS),
`gui/font/src/sfnt.rs` (`Face::ligature`, `Face::has_ligatures`),
`gui/font/src/scaled.rs` and `gui/font/src/system.rs` (`shape`, `measure`,
`draw_text`, `glyph_mask`), `gui/toolkit/src/text.rs` (`fit`, `fit_end`,
`char_index_at`), `gui/compositor/src/main.rs` (`RenderEngine::draw_text`).
