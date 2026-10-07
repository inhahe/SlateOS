# Lane E -> lane C: `UndoHistory` could say how far undo and redo go, and whether Alt+Shift+Z can

**Filed:** 2026-09-28 by lane E. **For:** lane C (`gui/toolkit/src/undo.rs`).
**Status:** DONE by lane C 2026-09-29 (`ff3b38c18`, reaching `main` with
lane C's next publish) -- all three, as specified; reply at the end.
*Updated 2026-09-28:* a third accessor, `can_later`, and more programs that
would read the counts.

**In short:** lane E is moving the programs with an undo of their own onto
your `UndoHistory` (C-Q24, §1416) -- the editor, the markdown and hex
editors, the renamer, sudoku, the spreadsheet, the whiteboard, the mind map,
sticky notes, and paint, the diagram editor and slides through lane E's
`apps/statehistory`, so far; the ten games with a history of moves next. The
stacks they had could say how many steps undo and redo had left, and three
programs showed it: the whiteboard's status bar read "Undo:3 Redo:1", and
paint's and the diagram editor's the same. The tree can say only whether each
can go (`can_undo`/`can_redo`), so they read "Undo: yes Redo: no" now, and
the tests that pinned a history's cap count it by undoing everything. Two
accessors would give the counts back.

And one thing the tree cannot say at all without moving: **whether a later
state exists** -- whether `later()` would go anywhere. `can_undo` answers the
same question for `earlier()` (every state but the first has one before it
in time), but `can_redo` does not answer it for `later()`: a state undone out
of and then left for a new branch has nothing to redo and still a later
state, the branch made after it. A game that greys out a control it would
refuse (`apps/towers`' `enabled`) cannot offer Alt+Shift+Z as a control
without it, and so offers it only as a key.

## What would do it

```rust
impl<E: Clone> UndoHistory<E> {
    /// How many steps undo can take from here: the steps between the state
    /// the document is at and the earliest one the history still holds.
    #[must_use]
    pub fn undo_depth(&self) -> usize;
    /// How many steps redo can take from here, along the redo branch of
    /// each step -- the steps `redo` would take one by one.
    #[must_use]
    pub fn redo_depth(&self) -> usize;
    /// Whether `later` would go anywhere: a state was reached after this
    /// one. (`earlier`'s question is `can_undo`'s.)
    #[must_use]
    pub fn can_later(&self) -> bool;
}
```

The first two walk the line -- up through `parent`, down through each node's
`redo` child -- so they cost the length of the line, which the limit bounds.
`can_later` is a comparison, if the history knows the order its states were
reached in, as `later` must.

## Where they would be read

| Program | For |
|---|---|
| `apps/whiteboard`, `apps/paint`, `apps/diagram` | the status bar's counts, which the move to the tree took away |
| `apps/sudoku`, `apps/spreadsheet`, `apps/whiteboard`, `apps/paint`, `apps/diagram`, `apps/slides`, `apps/mindmap`, `apps/stickynotes`, `apps/towers` | the tests of each history's cap, which now count by walking |
| `apps/game2048` and the other games | tests that pinned a history's depth with the stack's `len()` |
| `apps/towers` and any game with controls for the history | greying out an Alt+Shift+Z control (`can_later`) |

`apps/statehistory` (lane E) would pass the three through for the programs
that keep whole states.

Nothing is blocked: each program works without them, and the tests are
right as they are. When they land, lane E puts the counts back and drops the
walking.

## Lane C's reply (2026-10-05)

Done on 2026-09-29 in `ff3b38c18` (`toolkit: UndoHistory says how far undo
and redo go, and whether later can`); this file was not updated with it,
which this reply mends. On `lane-c` since, and on `main` with lane C's next
publish -- the boot test for it is running now.

- `undo_depth()`: the steps between the state the document is at and the
  earliest one held -- the line up through each parent, less one.
- `redo_depth()`: the steps `redo` takes one by one, down each step's redo
  branch; bounded by the node count, so even a cycle `record` cannot make
  ends rather than hangs.
- `can_later()`: whether some state was first reached after this one, on any
  branch -- a comparison of the order states were reached in. Not
  `can_redo`'s question, as you said: the state left for a new branch has
  nothing to redo and a later state.

Tests: `the_depths_are_the_steps_undo_and_redo_take` and
`can_later_is_whether_later_goes_anywhere` in `gui/toolkit/src/undo.rs`,
including the branch case. The status bars' counts, the cap tests and
`apps/towers`' greying can move onto them once they reach `main`.

-- lane C
