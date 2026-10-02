### TD-C-JSONVIEWER-CAN-EDIT-A-VALUE-BUT-NOT-ADD-ONE — 2026-09-04 — OPEN

**In short.** The JSON viewer can now change a value that already exists — click
a number, type a new one, press Enter. It cannot *add* a key, and it cannot
address its tabs by anything but their position. Both halves are written; what
is missing is small and named here.

**Adding a key.** `add_key_at_path` exists, has a test, and has no caller,
because adding a key needs somewhere for the user to type the name and this
program has no such field. Editing an existing value did not need one — the
value is already selected, and the buffer starts from what is there. The fix is
a one-line prompt in the tree view: a key name, then the same commit path
`set_value_at_path` already uses. The function keeps its test so that wiring it
is a small job rather than a rewrite.

**Tabs by index.** `Document::id` is assigned by `next_tab_id` and never read;
every operation addresses tabs by position in `documents`. That is the exact
arrangement that put the selection on the wrong row in `finance`, `reminders`
and `filesearch` — closing a tab shifts every index after it. It has not bitten
here yet because nothing holds a tab index across a close. `id` is kept rather
than deleted because it is what to switch to, and it is already maintained
correctly.

*2026-09-25 (lane E):* the first things to hold a tab across user
interaction -- the unsaved-changes question and the picker choosing where to
save -- name their tab by `id`, and `id` is read at last. `close_tab` now moves
the active index when a tab before it closes, which a click on another tab's
close mark made reachable. Everything else still addresses the *active* tab by
position, which is safe within one event.

**Neither is urgent.** No data is lost: an edit that cannot be made is a feature
that is absent rather than a feature that is wrong, and the tab indices are
consistent within a single operation. Written down because both look done from
the outside — the differ and the editor each had every piece except one call,
and that is exactly the shape that survives a reading.
