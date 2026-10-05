## 1420. The redo tree: one history for the toolkit, walked in time order with Alt+Z

**Date:** 2026-09-27 &middot; **Decided by:** Claude (operator-approved scope: the
operator asked for "an actual redo tree whenever possible", §1416; the shape and
the keys are Claude's) &middot; **Lane:** C, with E

**In short:** Undoing and then typing something new used to throw away what was
undone, for good. Now it is kept on a branch of its own. Ctrl+Z and
Ctrl+Shift+Z work exactly as before, along the branch you are on. To get back to
a branch you left, Alt+Z steps back through every version the text has been in,
in the order you made them, and Alt+Shift+Z steps forward again -- so every
version is reachable with two keys, without having to picture the tree. The
toolkit's text area uses it now; the programs with undo of their own can adopt
the same history.

| Question | Chosen | The alternative | Why |
|---|---|---|---|
| Where the tree lives | `guitk::undo::UndoHistory<E>`, generic over a program's step | a tree inside the text area only | twenty-two programs keep an undo of their own, twelve of them with a redo (the paint program, the spreadsheet, the diagram editor ...); one tested history is what "whenever possible" needs, and the text area is its first user |
| How an old branch is reached | Alt+Z / Alt+Shift+Z walk every state in the order it was first reached, across branches (Vim's `g-` / `g+`) | a key to choose which branch redo takes | a branch key needs the user to know where the forks are; time order needs nothing but "back" and "forward". Choosing a branch stays in the API (`select_branch`) for a program that draws its history |
| The keys | Alt+Z and Alt+Shift+Z | Ctrl+Alt+Z | Ctrl+Alt is AltGr, which types letters on several layouts (AltGr+Z is ż on a Polish keyboard); the text widgets already refuse Ctrl+Alt chords for that reason |
| What plain redo takes at a fork | the branch last undone out of, or the one just made | always the newest | undo-undo-redo-redo returns to where it started even where the tree forks |
| The bound | 500 steps on all branches together; past it, whole branches the user is not on go first, oldest first, then the oldest step of the user's own line | a bound per branch | the line being worked on keeps its depth; a branch abandoned long ago goes before the undo the user is using |

**Judgment call, easy to change:** the two keys. They are in one `match` in
`TextArea::edit_key`.
