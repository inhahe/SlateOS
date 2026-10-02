## TD-C-AN-INSERTED-IMPORT-SILENTLY-REATTACHED-AN-ATTRIBUTE-IN-TEN-APPS -- FIXED the same day

**Date:** 2026-09-08. **Lane:** C. **Fixed the same day.**
**Where:** `wire_app.py`, the conversion kit's import step; ten applications
under `apps/`.

**In short:** the tool that adds one line to a file put it in the wrong place —
between a `#[allow(...)]` marker and the line that marker was written to cover.
The marker then silently applied to the *new* line instead. Nothing broke and
nothing looked wrong; in nine of the ten applications there was no visible
symptom at all.

**What happened.** `wire_app.py` inserts `use appearance::Palette;` before the
file's first `use`. Ten applications have an `#[allow(unused_imports)]`
immediately above that first `use`, so the insertion landed *between* the
attribute and its item. Rust attaches an outer attribute to whatever follows
it, so the allow silently moved onto the Palette import — which is used, so it
suppresses nothing — and the import it had been covering became unguarded.

**Why it is worth an entry rather than a one-line fix.** In `filediff` the
unguarded import produced a fresh warning (`Modifiers`, `MouseButton`) and that
is how it was noticed. In the other nine, the covered import happened to be in
use, so removing its guard changed nothing visible. The failure mode is
therefore *silent by default*: a suppression that has quietly moved to a
different item produces no diagnostic, and the next person to touch the file
gets a warning with no idea why it appeared. Nine of ten instances would never
have been found by building.

**The general lesson.** A tool that inserts a line into Rust source cannot
treat "before the first item of kind X" as a position — attributes and doc
comments bind forward to the item beneath them, so the real insertion point is
before *the attribute run*, not before the item. `wire_app.py` now walks back
over any `#[...]`, `///` or `//!` lines above its anchor.

**It applies to deletion too, and the same day proved it.** Removing
`pub mod palette_check;` from `gui/desktop/src/lib.rs` — when that module moved
to `appearance` — left its `#[cfg(test)]` and doc comment attached to the next
declaration down, `pub mod power;`. Forward binding makes a *deletion* exactly
as dangerous as an insertion: whatever the attribute governed is gone, so it
silently governs its new neighbour. This one would have compiled the power
module out of every release build. The compiler caught it only because `power`
is used elsewhere; had the stranded attribute been an `#[allow]`, nothing would
have said a word — which is the nine-out-of-ten case above. **Deleting an item
means deleting its attribute run with it**, which is the same rule
`convert_app.py` learned for constants and their doc comments.

**Found by:** a clippy warning in `filediff` that had no business being there,
followed by grepping every application for an attribute directly above
`use appearance::Palette;`. All ten fixed; all ten build clippy-clean and pass
their suites.
