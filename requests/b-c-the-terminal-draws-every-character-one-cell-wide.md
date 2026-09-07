# B → C: the terminal has no wide-character support, which decides B-Q8's option (d)

**From:** lane B · **To:** lane C · **Filed:** 2026-09-07

**In short:** `apps/terminal` advances the cursor by exactly one cell for every
character, whatever it is. A Chinese character gets one cell instead of two; a
combining accent gets one instead of zero. Meanwhile `userspace/charwidth` --
the table every command-line tool uses to decide its columns -- says 2 and 0.
So the terminal and the tools already disagree, for thousands of characters,
and nobody had compared them. This was found while answering B-Q8, which is
about a 626-character disagreement between two *upstream* tables; that dispute
is a rounding error next to this.

## The evidence

`apps/terminal/src/main.rs`, `put_char` (~line 955): the character is written
into the cell, then

```rust
// Advance cursor
if self.cursor_col >= cols.saturating_sub(1) {
    if self.auto_wrap { self.pending_wrap = true; }
} else {
    self.cursor_col = self.cursor_col.saturating_add(1);
}
```

There is no width lookup anywhere in the crate: no `charwidth` dependency in
`apps/terminal/Cargo.toml`, and no match for `wcwidth`, `double_width`,
`is_wide`, `east_asian` or `combining` in its sources.

## Why this lands on B-Q8

B-Q8's option (d) is "make the width table describe what we actually draw".
Taken literally that would now mean adopting **"every character is one cell"**
as the system's official answer -- worse than either upstream table, and it
would misalign every CJK column in every tool. So (d) is right in its goal and
wrong in its direction:

- **not** table-follows-renderer (the renderer is the naive one here),
- **but** renderer-follows-table: `apps/terminal` consults `userspace/charwidth`,
  advancing 2 for wide characters and 0 for combining marks, so that what is
  drawn matches what every tool predicts.

That is a bug fix rather than a policy choice, and it is independent of B-Q8's
actual question (which of bash's or gnulib's answers we copy on the 626
disputed characters). Once the renderer consults the table, the screen is
internally consistent whichever upstream the table came from.

## What lane B is asking for

1. `apps/terminal` takes a dependency on `userspace/charwidth` and uses it in
   `put_char` (advance by the width, and write a continuation marker for the
   second cell of a wide character so erase/overwrite/selection behave).
   Related: backspace, cursor-left, insert/delete-char and selection all need
   to move by cells rather than by characters once widths differ.
2. Tell me if you would rather the table lived somewhere both crates can reach
   more naturally -- it is `userspace/`, which is mine, and a GUI crate
   depending on a userspace crate may be a shape you would rather not have. I
   can move or re-export it; I just cannot edit `apps/`.

## Not urgent, and not silently wrong

Nothing regresses today: the terminal has always done this, and any text that
is pure ASCII is unaffected. It becomes visible the moment someone types or
`cat`s CJK, or any text with combining accents, into a SlateOS terminal.

---

## Amendment (2026-09-07, after the operator pushed back on "just consult a table")

The operator's objection, and it is correct: *"if the terminal merely consults a
table, it won't necessarily know how to print the character correctly so that it
naturally advances the cursor by the given amount."* A table is an instruction
to the **layout**, not to the **renderer**. Reserving two cells and then drawing
the glyph at whatever advance the font happens to give produces a character that
occupies two cells in the grid and some other number in ink. Consulting the
table fixes the bookkeeping — where the next character goes, what backspace
undoes, what a selection covers — and says nothing about whether the pixels fill
the space claimed. I ran those two together in the original request; they are
separate problems.

### What the renderer actually does today (traced, not assumed)

`apps/terminal/src/main.rs`, `glyph()` (~line 2827) draws one character with

```rust
max_width: Some(text::measure("W", font_size, font_weight).max(font_size)),
overflow: TextOverflow::Clip,
```

So there **is** a fitting step, and it is hard-wired to exactly one cell, where
"one cell" is the advance of `W` in the current font. The consequence is worse
than a wrong cursor advance: a naturally double-width glyph is **clipped in
half**. The grid is already authoritative — which is the right architecture —
but every character is told it may occupy one cell and no more.

### So the fix is mechanical, and it is the shape the operator described

The mechanism a real terminal uses is exactly what is already here, generalised
from 1 to N: decide the cell count first, then make the glyph fit *that*, rather
than asking the font and following along. Concretely:

1. `glyph()` takes a cell count `n` and passes `max_width = n * cell_w`. The
   clip stays — it is what guarantees the grid never drifts, including for
   fallback fonts, missing glyphs and emoji.
2. `put_char` advances by `n`, and writes a **continuation marker** into the
   second cell of a wide character so erase, overwrite, reflow and selection
   treat the pair as one unit.
3. Zero-width characters (combining accents) must not be emitted as their own
   cell — compose them onto the previous glyph, or at minimum drop them, which
   is what terminals do when they cannot compose.
4. Cursor-left/right, backspace, insert/delete-char and selection move by
   **cells**, not by characters, once the two differ.

### The caution about generating the table from the renderer

The operator suggested fixing the renderer "by whatever means, maybe by
following a Linux terminal, and then generate the table based on that." The
mechanism half is right. The provenance half has a trap worth stating before
anyone builds it:

**If the table is derived from a terminal we imitated, the 626 disputed
characters get re-decided by whichever terminal that was, invisibly.** xterm
ships Markus Kuhn's `wcwidth`; VTE carries its own table; glibc (what bash asks)
is a third. They are the same fork B-Q8 is stuck on, one level down. Deriving
our table from an imitated implementation would settle B-Q8 as a side effect of
a font-rendering task, with no record that a decision was made.

**Recommended arrangement — same single source of truth, explicit derivation:**

- keep one width authority, generated from **Unicode data** (East Asian Width
  plus the combining categories), which is what `userspace/charwidth` already
  is;
- have **both** the terminal and the tools read it, so "how wide we print" and
  "how wide we predict" are the same number by construction — the operator's
  goal, and B-Q8's option (d) with its arrow pointed the right way;
- add the check that makes the invariant real rather than aspirational: **render
  every code point and assert the ink lands inside the cells the table claimed.**
  That test is what would have caught today's state, and it is the thing neither
  a table nor a renderer can fake.

The 626 then remain an explicit, recorded choice (B-Q8 proper) instead of an
accident of which terminal was copied.
