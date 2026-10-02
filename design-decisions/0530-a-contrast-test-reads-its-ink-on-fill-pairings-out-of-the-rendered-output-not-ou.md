## 530. A contrast test reads its ink-on-fill pairings out of the rendered output, not out of a table written by hand

**Date:** 2026-08-24
**Lane:** C
**Decided by:** Claude (autonomous)

**In short:** Several shell modules carry a test that checks their text is dark
enough against the colour behind it to be readable. Those tests were written as a
list: "the title is drawn on the card, the caption is drawn on the row", and so
on, typed out by the same person who wrote the drawing code. Five modules in, the
measured result is that such a test has never caught a single thing wrong with
the module it lives in — it *cannot*, for a reason that is structural rather than
accidental. This entry changes the shape: the test now walks the drawing commands
the module actually produced, pairs each block of colour with the text drawn on
top of it, and measures what it finds. In the first module written that way it
caught fourteen of thirty-four seeded bugs.

**Why the hand-written table cannot catch anything.** A test of the form "check
that `text` on `base` clears 4.5:1" is a claim about *the palette*. It reads two
palette values, computes a ratio, and asserts. Nothing in it calls the renderer,
so no change to the renderer can make it fail. Break the module completely —
paint the caption in the fill's own colour, transpose an ink and a fill, delete a
site — and the test still reads the same two palette entries and still passes.
The reintroduction harness measured this precisely across modules 42, 43 and 44:
each module's contrast test caught **0** of its own seeded defects, while every
other test in the same file caught between 1 and 33.

That zero is not a bug in those tests. They are genuinely useful — as *palette*
tests, they would fail if a role were retuned in a way that broke a pairing
somebody had written down. But they were being counted as coverage of the
drawing code, and they were never that.

There is a second, quieter failure. A hand-written table lists the pairings its
author thought of. Module 44 had three sites drawing `overlay0` as an ink at
**3.59:1** in Mocha and **2.14:1** in Latte, and its contrast test passed
throughout — the pairing was simply not in the table. **A test over a
hand-written table catches a role moving underneath a pairing; it cannot catch a
pairing nobody listed.** Reading the pairings out of the output makes the list
exhaustive by construction.

**The shape.** Render the scene, walk the command list in pairs, and for every
`FillRect` immediately followed by a `Text`, measure the ratio:

```rust
for pair in cmds.windows(2) {
    let (RenderCommand::FillRect { color: fill, .. },
         RenderCommand::Text { color: ink, .. }) = (&pair[0], &pair[1])
    else { continue };
    assert!(contrast(*fill, *ink) >= 4.5, "…");
    checked += 1;
}
assert_eq!(checked, 24, "the pairing walk did not reach every chip");
```

Run it over both modes and over every variant of the scene that changes a colour
(here: the four drop effects), so a defect that only shows in Latte, or only on
one of four badge fills, is still measured.

**Three properties this has to keep, and how.**

1. **It must not become an echo** (the standing rule: an expectation derived
   from the code under test asserts nothing). Everything asserted comes from
   outside the module — the 4.5:1 floor is WCAG's, and the expected pair count is
   written by hand. What is *read* from the code is only which pairs exist, which
   is the question, not the answer.
2. **It must notice a site that stops being drawn.** Without the `checked` count,
   deleting a site makes the walk find fewer pairs and pass more easily — the
   classic shape where removing the code removes the test. The count is what
   turns an absence into a failure, and it did: "the tooltip is never drawn" is
   caught by this test and by nothing else that measures contrast.
3. **It cannot see its two operands exchanged.** Contrast is a symmetric
   function, so drawing the fill in the ink's role and vice versa yields exactly
   the ratio it yielded before. Legibility survives a transposition; the design
   does not. That defect belongs to the module's *ordered site table*, which
   pins which role goes where — and the sweep confirms the two tests divide the
   space cleanly between them.

**The cost.** Pairing by adjacency is an approximation: it assumes a `Text`
command that follows a `FillRect` is drawn on it, which is true of the shell's
renderers because they emit each chip as fill-then-label, and false in general.
It also flattens alpha, so a fill drawn at 220/255 is measured as if opaque.
Both are acceptable here and both are checked rather than assumed: the module
asserts the exact number of pairs the walk should find, so a renderer that
stopped emitting in that order would fail loudly rather than silently measure
the wrong pairs; and where a flattened alpha could matter, the flattened pairing
clears the floor by such a margin (11.34 and 7.06 against 4.5) that no realistic
blend could drop it under. The alternative — a real compositing model in a unit
test — would be an echo of the compositor.

**Consequence for what is left.** Four shell modules remain, and the
~2,258-constant application conversion after them. Every one gets the walking
form, and the four modules already converted with the table form keep their
tables (they are correct as palette tests) but are not counted as covering their
own drawing code. The census in `known-issues.md` records the per-module catch
counts so that a zero is read as a structural property and not as a gap.
