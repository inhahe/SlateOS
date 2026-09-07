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
