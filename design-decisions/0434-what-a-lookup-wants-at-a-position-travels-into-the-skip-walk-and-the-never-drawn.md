## §434 — What a lookup wants at a position travels *into* the skip walk, and the never-drawn characters get a third answer

**Date:** 2026-08-15
**Lane:** C
**Decided by:** Claude (autonomous)

**In short:** Some characters are typed to instruct the text engine and are
never meant to appear on screen — the soft hyphen you put in a long word to
say "you may break it here", the zero-width joiner that asks two letters to be
drawn as one. Until now every one of them acted as a wall: the font's rule for
turning `f` and `i` into a single `fi` shape could not see past it, so writing
`f`, joiner, `i` — which is literally a request for that shape — produced the
opposite of what was asked. The fix is to let a rule *step over* such a
character. The decision recorded here is where the "is this the character I
wanted?" test has to live for that to work: inside the walk, not after it.

### The problem the obvious design cannot express

A lookup asks the run "give me the next glyph I may look at". Before this
change that question had two answers: **hidden** (the lookup's own flags say
to pass over marks, or ligatures, or a named set) and **not hidden**. A
never-drawn character needs a third: *step over it unless it is exactly what
the rule asked for at this position.* HarfBuzz calls this `SKIP_MAYBE`, and
resolves it against the rule's criterion — `matcher_t::may_match` — in the
same loop iteration as the skip test.

Try to keep the criterion outside the walk and it cannot be expressed at all:

```rust
let pos = skip.next(glyphs, at)?;        // walk returns a position
if glyphs[pos].gid != want { return None }   // caller judges it
```

When the caller rejects a glyph it has no way to say *carry on* rather than
*fail* — and "carry on" is the correct answer for precisely the ignorable
case, while "fail" is correct for every other. The walk is the only place that
still knows which of the two the position was. A caller that re-entered the
walk from `pos + 1` would get the other error: it would hop over an ordinary
glyph that failed the criterion, and ligate across a letter.

### The decision

`Skipper::scan` — one private loop — takes the criterion as
`Option<impl FnMut(usize) -> bool>` and returns the first position that both
the flags admit and the criterion accepts, stopping at the first admitted
position that is neither ignorable nor accepted. `next`/`prev` pass `None`,
which reproduces HarfBuzz's `MATCH_MAYBE`: no criterion means an ignorable is
always stepped over. `next_matching` passes the ligature component. And
`walk_forward`/`walk_backward` pass the caller's existing `each` closure —
which already returned `Option<()>` and already deferred its side effects
until after the match succeeded, so the criterion was in the API all along and
only needed routing one level deeper.

*Cost accepted:* `each` now has a load-bearing contract — it may be called at
positions that are not part of the final match, so it must record nothing
before it returns `Some`. That is written on `walk_forward`, and every
existing caller in `context.rs` already honoured it, which is the evidence
that the contract fits the callers rather than being imposed on them.

### The three-way table, and why the joiners are not symmetric

`Joiners` carries which kind of lookup is asking. The answers differ per kind
of character:

| | `GSUB` input | `GSUB` context | `GPOS` |
|---|---|---|---|
| ZWNJ | never | if `auto_zwnj` | always |
| ZWJ | if `auto_zwj` | always | always |
| hidden (CGJ, Mongolian FVS, tags) | never | never | always |
| everything else ignorable | always | always | always |

ZWJ is stepped over and ZWNJ is not, because that asymmetry *is* what the two
characters mean: ZWJ asks for the ligature, so the walk looking for one should
reach past it; ZWNJ forbids it, so the same walk must stop dead. Reading the
row the other way makes `f ZWNJ i` ligate, which is exactly what the writer
typed the character to prevent.

`auto_zwnj`/`auto_zwj` go *off* for features whose own subject is the joiners
— the Indic and Khmer basic features, where a joiner selects between a
conjunct and a half-form rather than decorating a ligature. This is
HarfBuzz's `F_MANUAL_JOINERS`, and it had to be modelled rather than
hard-coded per shaper, because one lookup can be reached by two features:
`manual_joiners` is a mask on the plan, and a lookup whose feature mask
intersects it is manual. Merging that way (manual wins) is the direction
HarfBuzz's `hb_ot_map_builder_t::compile` merges it (`auto_zwj &=`). Note that
`locl` and `ccmp` are *not* in the manual set for either shaper — they are
enabled with `F_PER_SYLLABLE` alone — and that is observable: it decides
whether a `ccmp` ligature may form across a ZWJ.

### The divergence from HarfBuzz that is kept

After this, the host sweep's `misplaced` count is 170, and all 170 are corpus
strings containing an ignorable. In every one, the glyphs agree and every
*visible* glyph's position agrees; what differs is the x of the erased,
zero-advance glyph itself.

The cause is where a legacy `kern` table's adjustment is charged. HarfBuzz
puts it on the right-hand glyph, as both an advance and an x-offset; we charge
it to the pair's left glyph. For adjacent glyphs the two are indistinguishable
— same drawn positions, same total width. They separate only when a
zero-advance glyph stands between the pair, which is exactly the erased
ignorable. For `a` CGJ `b` in Arial Rounded, HarfBuzz's erased glyph sits at
the *unkerned* pen, 1203, while `b` is drawn at 1190 — 13 units inside the
following letter's image. Ours sits at 1190, where the next glyph actually
starts.

Ours is kept. The x of an invisible zero-advance glyph is good for one thing
— placing a caret on that character's cluster — and for that, "where the next
glyph is drawn" is the right answer and "inside the next glyph" is not.
Matching HarfBuzz here would also mean adopting its representation of a kern
throughout, which fights `ShapedGlyph`'s own model, where `advance` and
`kern_next` deliberately describe the gap *after* a glyph.

*Cost accepted:* the sweep will report 170 `misplaced` forever, and a future
reader must not take that as a regression. That is what this section and the
`known-issues.md` entry are for; if the number moves, or a string that is not
in the ignorable set appears in that list, something real broke.

**Update 2026-09-21 — the number moved, to 1, and nothing broke.** This
paragraph asked a future reader to investigate that, so: host sweep, 556
faces × 111 strings, `agree 60490`, `reordered 0`, `differ 0`,
`misplaced 1`.

The 169 went because **the kern-charging difference this section describes no
longer exists.** On this section's own worked example — `a` CGJ `b` in Arial
Rounded — both shapers now report `1203;0;0, 0;0;0, 1267;-13;0`: the kern is
charged to the right-hand glyph as an x offset, which is what HarfBuzz was
said here to do and we were said not to. The prose above is a record of what
was true in August and is left standing as that; it is no longer a
description of the code.

**The one that survives is a different mechanism with the same conclusion.**
`a` CGJ `b` in `SegUIVar.ttf`, whose `GPOS` attaches U+034F to its base as a
mark. HarfBuzz keeps that attachment through hiding and reports the blank at
x_offset −1042 — minus the *base's* advance, so it sits back on the `a`.
`hide_ignorables` zeroes the offset, so ours sits at the pen, 1042. No kern
is involved: `a`'s advance is 1042 and `b` starts there in both. Glyph 2's own
advance is 561, so this is not HarfBuzz's `adjust_mark_offsets` either — it is
mark-to-base attachment, kept rather than discarded.

Ours is still kept, on this section's own argument: the x of an invisible
zero-advance glyph is good for placing a caret on that character's cluster,
and the pen is where the next glyph starts. But the *cost* is now one case,
not 170, and a reader who finds `misplaced` above 1 should look at mark
attachment before kerning.

Corrected with it: `hide_ignorables` carried "The x offset goes for the same
reason HarfBuzz zeroes it". HarfBuzz does not zero it. The comment now says
what HarfBuzz measurably does and why we differ.

**Measured.** Host sweep, 556 faces × 60 strings: `differ` on `f\u200di` from
76 faces to 0, `misplaced` from 331 to 170. Khmer probe: 45/45 before and
after — the Indic-family features read the joiners themselves and had to come
through untouched, and that they did is the check on `manual_joiners`.

**Where:** `gui/font/src/skip.rs` (`Joiners`, `steps_over`, `scan`,
`next_matching`, and the walk pair), `gui/font/src/norm.rs` (`Ignorable`, the
five-way classification), `gui/font/src/gsub.rs` (`Staging`, `Ctx`,
`skipper`, `ligature_matches`), `gui/font/src/scaled.rs` (the kerning
transparency of an erased glyph), `gui/font/src/gpos.rs` and `kern.rs` (the
`POSITIONING` call sites), `gui/font/src/khmer.rs` and `indic_shape.rs`
(`manual_joiners`), `known-issues.md`
(`TD-FONT-DOES-NOT-HIDE-DEFAULT-IGNORABLES`).
