# Lane E -> lane C: `UndoHistory` could say how far undo and redo go

**Filed:** 2026-09-28 by lane E. **For:** lane C (`gui/toolkit/src/undo.rs`).
**Status:** OPEN.

**In short:** lane E is moving the programs with an undo of their own onto
your `UndoHistory` (C-Q24, §1416) -- the editor, the markdown and hex
editors, the renamer, sudoku and the spreadsheet so far, the whiteboard and
the rest next. The stacks they had could say how many steps undo and redo had
left, and one program showed it: the whiteboard's status bar read
"Undo:3 Redo:1". The tree can say only whether each can go
(`can_undo`/`can_redo`), so the whiteboard now reads "Undo: yes Redo: no",
and the tests that pinned a history's cap count it by undoing everything and
redoing it back. Two accessors would give the counts back.

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
}
```

Both walk the line -- up through `parent`, down through each node's `redo`
child -- so they cost the length of the line, which the limit bounds.

## Where they would be read

| Program | For |
|---|---|
| `apps/whiteboard` | the status bar's counts, which the move to the tree took away |
| `apps/sudoku`, `apps/spreadsheet`, `apps/whiteboard` | the tests of each history's cap, which now count by walking |

Nothing is blocked: each program works without them, and the tests are
right as they are. When they land, lane E puts the whiteboard's counts back
and drops the walking.
