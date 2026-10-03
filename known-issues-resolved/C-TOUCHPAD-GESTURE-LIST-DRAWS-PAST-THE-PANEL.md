## C-TOUCHPAD-GESTURE-LIST-DRAWS-PAST-THE-PANEL (lane C, 2026-08-19) — FIXED 2026-08-19

**Where:** `gui/desktop/src/touchpad.rs` — `TouchpadSettingsUI::scroll_offset`
(declared line ~649) and the gesture loop in `render` (line ~977).

The Gestures section of the touchpad settings panel draws **every** binding in
`mgr.config.gestures`, advancing `cy` by ~22px each, and never compares `cy`
against `h` — the panel height it is handed as a parameter and uses only to
fill the background. `TouchpadConfig::set_gesture` pushes new bindings without a
bound, so the list is user-growable, and past roughly `h / 22` entries the rows
are emitted below the panel, drawn over whatever is underneath.

**`scroll_offset` is the field that exists to prevent exactly this, and it is
dead.** It is `pub`, it is initialised to 0, and it is never read anywhere in
the crate — grep finds only the declaration and the initialiser. So a caller
that wires a scroll wheel to it sees nothing happen, which is worse than the
field not existing: the API advertises a scrollable list and silently ignores
the request.

This is the same shape as
`C-TWO-SNAP-IMPLEMENTATIONS-WITH-DIFFERENT-GAP-POLICIES` below — state written
into a struct as though the feature were finished, with nothing calling it —
and it was found the same way, by asking what *reads* a public field rather
than what writes one.

**What the proper fix looks like.** Have the gesture loop start at
`self.scroll_offset` clamped to the list length, as `window_rules.rs::render`
already does at line ~940 (that one was fixed and has a regression test at
~2251), and stop emitting once `cy` would exceed `y + h`. Both halves are
needed: clamping alone still overdraws a long list, and the height check alone
leaves the tail unreachable. Tests should assert (a) a list longer than the
panel emits no command below `y + h`, (b) a `scroll_offset` past the end
renders the last page rather than nothing or a panic, and (c) scrolling by one
moves the first visible row by one — the last being the one that fails if the
offset is read but ignored.

**Not fixed in the commit that found it** only because that commit's workspace
gate was already running and editing the crate would have invalidated it. It is
the next thing lane C picks up.

### It was not one bug, it was three copies of one bug

Fixing it started with the question "who else has a dead `scroll_offset`?", and
the answer was **`bluetooth.rs`, identically**: `BluetoothSettingsUI` also takes
`h`, also spends it only on the background rectangle, also loops (three times —
connected, paired, nearby) with no height check, and also carries a
`pub scroll_offset: usize` that nothing in the crate reads. A machine that has
seen a lot of nearby devices drew them straight through the bottom of the panel,
unscrollably. `window_rules.rs` was a third copy: it *did* bound its list, but
with its own `end - start` arithmetic that had already underflowed once.

So the fix is a shared primitive rather than three local patches —
`gui/desktop/src/scroll_window.rs`, which answers "given a list, a row height, a
panel height and a requested offset, which rows are visible?" in one tested
place. Its three policies are recorded as `design-decisions.md` §470. The three
panels now call it:

| Panel | Before | After |
|---|---|---|
| `touchpad.rs` gestures | whole list, no height check, offset dead | `scroll_window::visible` |
| `bluetooth.rs` devices | three lists, no height check, offset dead | `scroll_window::visible_variable` over one flattened list |
| `window_rules.rs` rules | own arithmetic; stale offset → blank list | `scroll_window::visible_count`; stale offset → last page |

Two things fell out of doing it this way. Bluetooth's three sections had to be
flattened into one sequence *including their headings* before they could share
one offset — otherwise a list scrolled to start mid-section shows devices under
no label. And `window_rules` got a behaviour upgrade for free: an offset left
over from a longer list now pulls back to the last page instead of rendering
nothing.

Both panels also gained a line saying how much is hidden. A truncated list that
looks complete is worse than a long one, because the user has no reason to try
scrolling.

### What the reintroduction checks found

Eleven defect variants were restored one at a time and the suite re-run; all
eleven were caught by the test named for them. Two things needed correcting to
get there, and both are worth recording because both were *tests* being wrong
rather than code:

- **`the_pinch_control_stays_on_screen_however_long_the_gesture_list_is` could
  pass with the height budget removed entirely.** It asserted the control was
  *emitted*, and it always is — being drawn off the bottom of the panel is still
  being drawn. It now reads the command's `y` and asserts it lands inside the
  panel. This is the same failure mode as the snap entry below records: a test
  asserting a property the implementation happens to have rather than the one
  the caller depends on.
- **The sweep's first run reported three "pattern not found" misses that were
  not misses at all** — the source files are CRLF and the harness's multi-line
  patterns were written with `\n`, so they silently matched nothing. A sweep that
  quietly tests fewer defects than it claims is worse than no sweep, so the
  harness now takes its newline from the file.

`window_rules.rs`'s own regression test was also renamed and strengthened: it was
called `scrolling_past_the_end_of_a_shrunken_rule_list_renders_nothing` but only
asserted that *something* drew, so it would have passed under either policy. It
now names the row it expects to see.

### Still open: nothing writes these offsets

The fix makes `scroll_offset` *work*; it does not wire a scroll wheel to it. All
three fields are still only ever written by `new()`, so today the lists are
bounded and truthful but not yet scrollable in practice. That is input plumbing
in the settings shell rather than in these panels, and it is tracked separately —
the panels are now correct for any value a caller sets, which is the half that
had to come first.
