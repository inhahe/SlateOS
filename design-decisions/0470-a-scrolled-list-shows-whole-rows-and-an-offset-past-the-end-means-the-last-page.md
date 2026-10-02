## §470 — A scrolled list shows whole rows, and an offset past the end means the last page

**Date:** 2026-08-19
**Lane:** C
**Decided by:** Claude (autonomous)

**In short:** three settings panels each drew a list that could be longer than
the box it was drawn in, and each got it wrong differently — two drew straight
through the bottom edge, and the third went blank if you scrolled down and then
deleted things. They now share one piece of code that answers "which rows fit?".
Deciding that required picking three rules that a user can see the effects of:
a row that only half fits is not shown at all, scrolling too far shows the last
page rather than an empty box, and a list that is hiding rows says so.

### The situation

`touchpad.rs`, `bluetooth.rs` and `window_rules.rs` all render a variable-length
list of rows into a fixed-height panel, and all three carry a public
`scroll_offset` field. Two of them never read that field and never compared
their position against the panel height at all. The third did bound its list,
using arithmetic that had already produced one underflow panic.

Three copies of one calculation, in three states of brokenness, is the
band-aid-accumulation pattern `CLAUDE.md` warns about — so the question was not
"how do I bound this loop" but "what is the one right answer, and what does it
have to decide?". It turns out to have to decide three things, none of which is
obviously correct on its own.

### The decision

**1. A partially-visible row is not visible.** Capacity truncates: 100 pixels of
room at 26 pixels a row shows three rows and leaves 22 pixels blank.

*What changes:* the bottom of the list is always a clean edge, never a row
sliced in half by the panel border.

The alternative — show the partial row, clipped — needs a clipping mechanism the
render command list does not have, so the "clip" would in practice be a row
drawn over whatever is beneath the panel. That is the bug being fixed, in a
smaller size.

**2. An offset past the end means the last page, not an empty list.** The stored
offset is treated as a *request*, clamped to `total - capacity` when the window
is computed. The caller's field is not modified.

*What changes:* delete enough rules while scrolled to the bottom of the list and
you see the remaining rules, instead of an empty panel you have to scroll back
up out of.

This is the case that used to underflow in `window_rules.rs`, and the local fix
there had settled on "render nothing". Nothing is defensible — it is literally
what is at that offset — but it is a blank panel arrived at by a route the user
did not take, and the recovery (scroll back up) is only obvious if you know why
it happened. Clamping the *effect* rather than the *field* is what makes this
safe to do from `render(&self)`, and it also means a panel that is temporarily
too short does not permanently destroy the user's scroll position.

**3. A list that is hiding rows says so.** Both panels draw a short line beneath
the list — `Showing 3-9 of 40 - scroll for more`, or `12 more - scroll to see
the rest`.

*What changes:* a truncated list is visibly truncated instead of looking like
the whole list.

Without this, correctly bounding the list makes the bug *less* visible while
leaving the user equally stuck: rows no longer spill over the panel edge, they
simply are not there, and nothing suggests scrolling would help. The space for
this line is reserved unconditionally, including when the list fits, so that the
budget arithmetic does not depend on its own result and the control below the
list does not move as the list grows.

### Alternatives considered

- **Fix each panel locally.** *What changes:* nothing visible now; the next
  panel with a list makes the same mistake a fourth time. Rejected — the three
  existing copies had already diverged into three different behaviours, which is
  precisely the argument for one implementation.
- **A scrollbar widget instead of a row window.** *What changes:* a draggable
  bar appears beside long lists. Rejected for now as a larger change to the
  toolkit rather than to these panels, and orthogonal: a scrollbar still needs
  something to tell it which rows are visible. This decision does not preclude
  one; it supplies the arithmetic one would need.
- **Round capacity up and clip the last row.** *What changes:* the list appears
  to continue under the panel edge. Rejected — see rule 1.

### Consequences

- `gui/desktop/src/scroll_window.rs` is the only copy of this arithmetic. It has
  three entry points because the callers genuinely differ: `visible` (uniform row
  height, capacity from a pixel height), `visible_count` (caller states its own
  row capacity — `window_rules.rs` has a `visible_rules` setting), and
  `visible_variable` (per-row heights, for `bluetooth.rs`, whose section
  headings are shorter than its device rows). A test asserts the first two agree
  with the third wherever their inputs coincide, so a panel's behaviour cannot
  depend on which one its author reached for.
- **Section headings are rows.** Flattening `bluetooth.rs`'s three sections into
  one sequence is what lets a single offset govern all of them, and the headings
  had to become entries in that sequence — otherwise scrolling can land the list
  mid-section, showing devices under no label. The visible consequence is that
  the first scroll step retires the "Connected (2)" heading rather than a
  device, which is asserted rather than left to chance.
- **Degenerate sizes are answers, not panics.** A zero, negative, `NaN` or
  infinite row height or panel height yields "no rows fit" rather than a
  division by zero or an unbounded count. These reach the code through a window
  resize, so they have to be survivable; the `NaN` case is specifically guarded
  because `used + NaN > avail` is *false*, which without the guard admits the
  entire list into a panel that fits three rows.
- Nothing here writes a `scroll_offset`. The panels are now correct for any
  value a caller sets, but no input is wired to set one yet; that is settings-
  shell plumbing, tracked in `known-issues.md` under
  `C-TOUCHPAD-GESTURE-LIST-DRAWS-PAST-THE-PANEL`.
