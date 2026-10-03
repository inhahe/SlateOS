## §425 — A monospace face is a *scoped render-tree state*, not a field on every text command

**Date:** 2026-08-15
**Lane:** C
**Decided by:** Claude (autonomous)
**Zone:** gui-toolkit / gui-core / apps

**Context.** The toolkit had no way to ask for a fixed-pitch face at all.
Every `text::measure` call resolved against the one installed UI face, so the
terminal-shaped apps — `apps/tmux` above all — sized their character cell from
`text::digit_advance`, i.e. the advance of `'0'` *in a proportional face*. At
13px that is 7.55px, while `'W'` in the same face measures 13.08px. The grid
was laid out on one width and filled with glyphs of another: characters
overhung their neighbours' cell backgrounds, and the block cursor was drawn
beside the character it was supposed to mark rather than on it. This is a real
rendering bug, not a test artifact, and it cannot be fixed by picking a
different measuring character — a proportional face has no single advance to
pick.

So `osfont` grew a `Family { Ui, Mono }` axis on its cache key, and the
question became how a *caller* names the family it wants, given that the
render tree is the interface between an app and whatever draws it.

**Options.**

1. **A `family` field on `RenderCommand::Text`.** Each string carries its own
   face. Maximally explicit; no state to get out of balance.
2. **Scoped `PushFont { family }` / `PopFont` commands**, following the
   existing `PushClip`/`PopClip` and `PushTranslate`/`PopTranslate` grammar.
   A region of the tree is drawn in a family; commands inside inherit it.

**Decision: (2), scoped.**

**Reasoning.**

- *It matches what the property actually is.* A family is a property of a
  **region** — a terminal pane, a code block, a hex dump — not of each
  individual string inside it. The clip rectangle and the translate are scoped
  for exactly this reason, and a family behaves the same way: it is set once
  on entering a region and restored on leaving.
- *The cost of (1) is paid by everyone and the benefit is collected by almost
  nobody.* The tree contains **4570 `RenderCommand::Text {` construction sites
  across 208 files**. Option (1) puts a new field on every one of them so that
  a handful — one terminal, one editor gutter, one hex view — can set it to
  something other than the default. Even with `..Default::default()` that is
  4570 sites of churn and 4570 places to get it subtly wrong later.
- *Backends that do not implement it degrade correctly.* A renderer that
  ignores `PushFont`/`PopFont` draws everything in the UI face — precisely
  what it did before the commands existed. Option (1) has the same property
  only if the field is `Option`-shaped, which weakens its one advantage.

**Cost we accept.** Scoped state can go unbalanced: a `PushFont` on a path
that returns early leaks the family into whatever is drawn next. The
mitigations are the same ones the clip stack already uses — the compositor
clears its `font_stack` at both ends of `execute()`, so a leak cannot survive
a frame — plus a per-app test. `apps/tmux`'s
`the_grid_is_drawn_in_the_family_it_was_measured_in` walks the command list,
asserts the depth returns to zero, that the scope was opened exactly one deep,
and that cell glyphs were actually drawn *inside* it. That last assertion is
the one that matters: without it the test passes vacuously on a fresh
multiplexer, whose buffer is all spaces and whose cell loop skips them.

**Wire protocol.** `guiremote` encodes the two commands as tags `0x0B`/`0x0C`
with a one-byte `FontFamilyTag`. `PROTOCOL_VERSION` stays at **1**: the change
is purely additive, every frame a v1 encoder could produce still decodes
identically, and the only mismatch — a new encoder against an old decoder —
already fails cleanly with `DecodeError::BadTag` rather than misrendering.

**Fallback semantics.** A family with no installed face falls back to the
built-in 8x16 bitmap face, which is *itself* monospace — never to the other
family's face. This matters because `Mono` is a promise about **metrics**, not
about looks: a caller may treat the text as a grid. Falling back from `Mono` to
a proportional UI face would silently reintroduce the exact bug this entry
exists to fix, so the fallback preserves the promise even when it cannot
preserve the appearance.

**Where it bites:** `gui/font/src/system.rs` (`Family`, the cache key),
`gui/toolkit/src/render.rs` (`FontFamily`, `PushFont`/`PopFont`),
`gui/toolkit/src/text.rs` (`measure_in`, `cell_advance`, `line_height_in`,
`ascent_in`, mono face installation), `gui/remote/src/lib.rs` (tags
`0x0B`/`0x0C`), `gui/compositor/src/main.rs` (`font_stack`), `apps/tmux`.
