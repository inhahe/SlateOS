## §408 — A lookup type is a rule about one position; the pass belongs to a shared driver (GSUB 5/6)

**Date:** 2026-08-14
**Lane:** C
**Decided by:** Claude (autonomous)

### Context

GSUB lookup types 5 (contextual) and 6 (chained contextual) do not
substitute anything themselves. They match a pattern and then say "run
lookup 12 at input glyph 0, then lookup 7 at input glyph 2." The lookups
they name are named *by index into the font's LookupList* — not by
feature, not by anything a feature walk would have found. That is exactly
how a font hides a helper lookup that only makes sense inside one
context.

Until now every lookup type in `gsub.rs` owned its own loop over the run:
`apply_single` walked the buffer, `apply_ligature` walked the buffer, and
so on. Types 5/6 cannot be written that way, because what they need is to
apply some *other* lookup at one position.

### Decision

Invert it. Each lookup type became a function that answers one question —
"what, if anything, do you do at position `i`, and how far does that
carry the cursor?" — and a single shared driver (`apply_lookup`) turns
any such rule into a pass over the run. `apply_at` dispatches on the
type.

Consequences that fall out of the shape rather than being decided
separately:

- **Termination is structural.** The driver resumes past what a match
  produced, so a lookup is never offered its own output. A font whose
  ligature is also its own input, or whose Multiple Substitution
  decomposes a glyph to itself, cannot loop. Because this lives in the
  driver, no lookup type can forget it — which is the real argument for
  the refactor, over and above types 5/6 needing it.
- **Recursion is bounded explicitly**, by `MAX_NESTING = 6` carried in
  `Ctx`, since a font may have lookup 5 invoke lookup 6 invoke lookup 5.

### The hard part: nested position bookkeeping

A `SequenceLookupRecord` names a glyph of the input *as it was matched*.
But the lookup it invokes may grow the run (Multiple Substitution) or
shrink it (Ligature), and a later record in the same list still refers to
the original numbering. Tracking a single offset is not enough, because a
ligature does not just shift later glyphs left — it *swallows* some of
them, and a later record must not silently slide onto a glyph the context
never matched.

So positions are a `Vec<Option<usize>>`, one entry per matched input
glyph. Growth shifts later entries right. Shrinkage shifts them left
*and* sets the swallowed ones to `None`, and a record naming a `None`
position is skipped. `None` is "this glyph no longer exists," which is a
different fact from "this glyph moved," and conflating them is the bug
this representation exists to prevent.

### Alternatives considered

- **Whole-run passes only, with types 5/6 re-entering `apply_lookup`.**
  Rejected: a nested lookup must apply at *one* position, not everywhere.
  Re-running a lookup across the whole run would apply it to text the
  context never matched.
- **Re-matching the context after each nested lookup**, instead of
  tracking positions. Simpler to write, but quadratic, and it changes
  meaning: the spec matches the context once and then edits it.
- **Decoding the whole LookupList up front** so nested lookups are a
  table index. Rejected: most of it is never reached by a given run, and
  re-reading a lookup header on invocation is a handful of offsets.

### Format details worth recording

- Format 1 rules name glyph ids, format 2 rules name *classes*. It is one
  rule layout read two ways, which is why they share the `By` enum. This
  bit us in testing: a rule written `&[11]` (a glyph id) in a format-2
  context is a class number, and matched the wrong thing.
- Chained format 2 has **three separate ClassDefs**. A glyph may be one
  class as lookahead and another as input. Reusing one ClassDef for all
  three would be wrong on real fonts.
- Backtrack is stored **closest-glyph-first**: entry 0 is the glyph
  immediately before the input, not the leftmost of the context.
- A null (zero) offset means "absent" everywhere in OpenType. Following
  one lands back on the subtable's own header, where a format number
  would be read as a coverage format and match plausible-looking
  garbage. `sub_offset` refuses null for this reason.

### `clig` and `calt` are on by default

Both are `On by default` in the OpenType spec, and `calt` in particular
is what makes many faces look right rather than merely legible. Turning
them on changed shaping on 292 of the installed faces, so it is a real
behaviour change, not a formality.

It immediately broke `installed_fonts_ligate_fi`, which demanded that a
pair that does not ligate come back as *the same two glyphs*. Cambria
resolves the `f`+`i` collision contextually instead — a `calt` swaps in a
short-hooked `f` and leaves two glyphs. HarfBuzz produces the identical
`[976, 139]`, so our answer was right and the test's assumption was
stale. The test now checks length, clusters and glyph range, and counts
the faces taking the contextual route.

### Where this lives

`gui/font/src/gsub.rs` (`Ctx`, `apply_lookup`, `apply_at`,
`apply_nested`, `context_match`, `chain_match`, `By`, `Nested`);
`otl::{lookup_list, lookup_at}` in `gui/font/src/otl.rs`;
`installed_fonts_ligate_fi` in `gui/font/tests/host_fonts.rs`.
