## §431 — When the host cannot falsify a shaping pass, synthesize a font that can

**Date:** 2026-08-15
**Lane:** C
**Decided by:** Claude (autonomous)

**In short:** Everything the font crate does is checked by shaping the same
string with our code and with HarfBuzz (the reference text-shaping library
every browser uses) over all 556 fonts installed on this machine, and
comparing. That works only while some installed font actually exercises the
code being checked. For the Thai fallback below, *none* does — so the sweep
would have printed "agree" for a pass that never ran, which is worse than no
test at all. The decision: when no installed font can disprove a pass, build
one that can, with fontTools, and check it in.

### The problem, concretely

A Thai font that predates OpenType ships the shifted forms of its tone marks
(a tone mark moves down and left when it has to clear a tall consonant) as
extra glyphs in the *private use area* — the block of codepoints Unicode
reserves for "whatever the font vendor wants" — at U+F700 for Windows and
U+F880 for the Mac. Picking those glyphs is the shaping engine's job, and the
engine only does it when the font contains no Thai layout rules of its own.

Every Thai font Windows ships today *does* contain its own layout rules, which
turns the fallback off. A direct probe confirmed it: of the 556 faces
installed here, **zero** carry a single one of those private-use glyphs. So
the fallback was unfalsifiable against the host collection — the sweep asks
each face for glyphs, both engines answer with the same missing-glyph boxes,
and the report says the two agree.

### The decision

`gui/font/tools/gen_thai_legacy.py` builds three faces with fontTools that do
not exist on this machine and could not: the Thai block, the private-use
block, and deliberately **no `GSUB`/`GPOS`/`GDEF` at all**. That absence is
the whole point — it is what makes both engines take their fallback path over
the same face, so the two answers become comparable.

Three faces, and the third is as load-bearing as the first two:

| face | holds | what it proves |
|---|---|---|
| `ThaiLegacyWin` | Windows private-use forms only | the fallback fires and picks the right form |
| `ThaiLegacyMac` | Mac forms only | the vendor preference is *tested*, not assumed |
| `ThaiNoPua` | Thai glyphs, no private-use forms | the pass runs, finds nothing, and damages nothing |

`ThaiNoPua` is the case every real modern face without Thai layout rules is
in. A fallback that mangled *that* would mangle a lot of real fonts, and no
amount of agreement on the first two faces would have caught it.

Result: 78 of 78 strings agree with HarfBuzz across the three faces. The
corpus (`gui/font/tools/thai-pua-corpus.txt`) names one string per edge of the
two state machines rather than sampling real text, since a synthetic font's
only purpose is to reach edges real text reaches rarely.

*What it cost:* two generated files and a fontTools dependency for the test
tooling (not for the crate). *Alternative rejected:* hand-write unit tests
asserting specific glyph ids. Those assert what I believe HarfBuzz does; the
synthetic face asks HarfBuzz. When the two disagree, only the second one
tells me which of us is wrong — and it did, immediately, by catching that the
pass was gated on the wrong predicate and never fired at all.

**Generalization.** This is now the rule for the three shapers still to come
(Khmer, Myanmar, USE) and for anything else the host cannot exercise: if the
sweep cannot disagree with a pass, the pass is untested, and the fix is a
font that can disagree — not a weaker claim in the commit message.
`harfbuzz_sweep.py` grew a `--corpus FILE` flag so each such oracle brings its
own strings instead of bloating the built-in corpus.

**Where:** `gui/font/tools/gen_thai_legacy.py`,
`gui/font/tools/thai-pua-corpus.txt`, `gui/font/tools/harfbuzz_sweep.py`
(`--fonts`, `--corpus`), `gui/font/src/thai.rs` (`pua_shape`),
`known-issues.md` → `TD-FONT-HAS-NO-UNIVERSAL-SHAPING-ENGINE`.
