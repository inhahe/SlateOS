## TD-FONT-FALLBACK-CLASSES-SCRIPTS-IT-NEVER-PLACES

**What.** `fallback::attach_class` carries position classes for scripts
`fallback::positions_marks` always refuses, so those arms can never run.
`scaled.rs` calls `attach_class` only when the run's script is *not* in
`COMPLEX_SCRIPTS`; `thai`, `lao ` and `tibt` are all in it. That makes dead:

- the whole `if cp & !0xFF == 0x0E00` block (Thai and Lao vowel signs, and the
  phinthu), and
- the `103 | 107` (Thai sara u/uu and mai), `118 | 122` (Lao) and `129 | 132`
  (Tibetan) arms of the `match klass` below it.

**Why it is not simply a bug.** The mappings are *correct*; they are just
unreachable, and they became unreachable when `COMPLEX_SCRIPTS` grew to the
full list of scripts with a non-default HarfBuzz shaper. HarfBuzz agrees that
these marks should not be fallback-placed — its Thai, Lao (via the default
shaper's Thai path), Myanmar and Tibetan/USE shapers all set
`fallback_position = false` — so deleting the arms changes no output.

**Proper fix.** Delete them, and say in `attach_class`'s doc that it is only
ever asked about a script the fallback places, so an arm for one it does not is
a claim that can never be checked. Not merged into the mark-ness fix because
that change had to be measurable against the sweep and this one is provably a
no-op; a commit that mixes the two cannot be bisected.

**Risk of not doing it.** Low, but it is the kind of debt that misleads: the
next reader of `attach_class` will believe Thai vowels are placed here, and
will debug the wrong function when they are not.

**Where.** `gui/font/src/fallback.rs`, `attach_class` (~line 244) and
`COMPLEX_SCRIPTS` (~line 115).

**Fixed** (2026-08-14). Deleted, and the doc comment now says *why* there is no
Thai arm rather than leaving a reader to notice there isn't one. The sweep is
byte-for-byte unchanged across 556 faces x 23 strings — 10917 / 32 / 841 / 998
before and after — which is the whole claim of the entry, measured.

The test that covered the deleted arms was replaced rather than dropped:
`the_scripts_the_class_map_omits_are_scripts_it_is_never_asked_about` asserts
`positions_marks` is false for `thai`, `lao ` and `tibt`, so that taking one of
them out of `COMPLEX_SCRIPTS` fails here instead of silently sending marks the
map has no classes for through it — U+0E34 SARA I would arrive as class 0 and
be taken for a base.
