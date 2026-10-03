## §445 — Syntax state entering a line is memoized per line and invalidated from the first edited line

**Date:** 2026-08-16
**Lane:** C
**Decided by:** Claude (autonomous)

**In short:** A block comment opened near the top of a file colours everything
below it, so to draw the lines currently on screen the editor has to know
whether the line above the screen was inside a comment — and that answer depends
on every line above it, all the way to line 1. Scrolling to line 4000 of a file
therefore costs re-reading 4000 lines, every frame, unless the answer is
remembered. The editor now keeps one small "what was I in the middle of when
this line started" value per line, and throws away everything from the first
line an edit touched.

**The three options.**

| | Cost to draw a frame | Cost of an edit | Can it be wrong? |
|---|---|---|---|
| Recompute the prefix each frame | O(lines above the viewport) | free | no |
| Memoize, invalidate on edit *(chosen)* | O(visible lines), amortised | O(lines below the edit), lazily | only if a new edit path forgets to invalidate |
| Recompute the whole file on each edit | O(visible lines) | O(file) eagerly | no |

Recomputing per frame is the honest one and it is what the editor would have
done had highlighting simply been switched on. It is also the one that gets
slower the further down a file the user scrolls, which is the wrong shape: the
user's experience of a large file would degrade with position in it, for no
reason they could see. At 4000 lines and 60 Hz it is a quarter of a million line
tokenizations per second to draw fifty lines.

Recomputing the whole file on each edit has the opposite shape — drawing is
cheap, but every keystroke pays for the whole document, and the common case
(typing at the top of a 50,000-line file) is the worst case.

The memo pays only for what changed. `hl_entry[i]` is the highlighter state
*entering* line `i`; `entry_state(line)` extends the vector lazily from the last
known entry, and every mutating operation calls `invalidate_highlight(first_line)`,
which truncates to `first_line + 1`. Typing on line 3 of a 50,000-line file
discards 49,997 entries and recomputes only as far down as the viewport actually
reaches. This is what Vim, VS Code, Sublime and Emacs all do, in the same shape.

**The cost is the third column of that table, and it is real.** The memo is the
only one of the three that can disagree with the buffer, and it does so
silently: a forgotten `invalidate_highlight` in some future edit path shows up
as stale colours in one region of one file, which is exactly the kind of bug
that gets reported as "the highlighting is a bit flaky sometimes" and never
reproduced. So the invalidation is not left to reviewer attention.
`the_syntax_cache_agrees_with_a_recomputation_after_every_edit` walks all ten
editing operations and asserts, after each, that the cached entry state for
every line equals a from-scratch recomputation. Adding an edit path without
invalidating fails that test, which is the property that makes the memo
affordable.

**Why `RefCell`.** `entry_state` is called from the render path, which has
`&Document`, and it mutates the memo. The alternatives were to make rendering
take `&mut Document` — which spreads a mutable borrow across the whole draw
call for the sake of a cache — or to fill the memo eagerly on edit, which is the
third row of the table. Interior mutability is the narrow answer: the memo is
not part of the document's value, it is a derived fact about it, and
`&Document` promising "I will not change the document" is still true.

**Where it lives.** `apps/editor/src/main.rs` — `Document::hl_entry`,
`entry_state`, `invalidate_highlight`, `EditorState::render_editor`, and
`highlight_render_tests`.
