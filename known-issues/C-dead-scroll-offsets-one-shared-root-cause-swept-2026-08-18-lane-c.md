## Dead scroll offsets, one shared root cause — swept 2026-08-18 (lane C)

**In short:** fifteen lists across the toolkit and eleven apps had a scroll
position that *nothing that draws ever read*. The field was declared,
initialised to zero, and then never mentioned again — so the list drew from the
top always, and everything past the bottom edge was unreachable by any means
the app offered. All fifteen are now fixed; this section is the record of what
the class looks like, so the next one is recognised faster. Two further
instances have since been fixed on the same pattern
(`apps/diskimager`, `apps/devicemanager`), and the sixteenth — the free-form
one that needed a different fix — is done too: see
`C-KANBAN-CARD-DETAIL-PANEL-HAS-A-DEAD-SCROLL-OFFSET` below, which also records
why the fix this file originally proposed for it was the wrong one.

The dead-field signature is cheap to test for and worth reusing: **grep the
field name and count the hits.** Two — a declaration and an initialiser — means
nothing reads it. That is how all of them were found, after the first turned up
by accident.

Two refinements the sweep added to that rule:

- **A field that is *read* but never *written* is worse than a dead one**, and
  the grep test does not catch it. radio's `station_scroll` had four hits —
  declaration, initialiser, and two reads in the renderer — so it looked alive.
  Nothing ever assigned it, so the list was frozen at row 0 while the selection
  walked off the bottom: with 20 preset stations and 10 rows visible, half the
  list was unreachable. When a grep turns up reads, check that at least one hit
  is on the left of an `=`.
- **Pulling on a dead field finds live ones.** settings' dead `dropdown_scroll`
  led to the discovery that every dropdown in the app was decorative; netscan's
  dead `sidebar_scroll` led to three further defects in the same screen. The
  dead field is a marker for a list nobody ever exercised, and a list nobody
  exercised usually has more than one thing wrong with it. Budget for that
  before starting, rather than treating each find as a one-line change.

### The instances

| Where | Symptom | Fix |
|---|---|---|
| `gui/toolkit/src/listview.rs` — `ListViewport::visible_range` | Not a dead field but the same class: the method re-derived the window itself instead of using `scroll_window`, and lacked its last-page clamp. A list that *shrank* between the call that set `first_visible` and the render that asked what to draw returned an **empty** range — a blank panel, not the last page. | `visible_range` now literally calls `scroll_window::visible_count`. The duplicate implementation is gone, so the two cannot drift again — that is a property of the code now, not of a test. |
| `apps/diskanalyzer` — `DiskAnalyzerUI::scroll_offset` | `f32`, never read. The list view cut off at the last row that fit and offered no way to reach the rest. | Changed to `usize` (a row index — the list draws whole rows, so a pixel offset can only express positions the renderer rounds away), given `scroll_list_by`/`scroll_list_to_top`, and the row loop now windows through `scroll_window::visible`. |
| `apps/kanban` — `KanbanApp::scroll_offset` | `f32`, never read. Worse than the others: the card loop was **unbounded**, so a column with more cards than fit drew *past the bottom of the window*, over whatever was beneath it. | Changed to `usize`; the column now windows through `scroll_window::visible_variable` and says how many cards it is hiding (`+N more`). See design-decisions.md §471 for the one-offset-for-all-columns choice. |
| `apps/tmux` — `TerminalBuffer::scroll_offset` **and** `Pane::copy_scroll` | Two fields for one concept, both dead. `copy_scroll` was only ever *reset* to 0 by `enter_copy_mode`; nothing incremented it and nothing rendered read it. The module doc's advertised "copy mode for scrollback browsing" drew a `[COPY MODE]` badge and nothing else — the scrollback was entirely unreachable. | `TerminalBuffer::scroll_offset` **deleted**: how far back a *view* is looking is a property of the view, not of the buffer, and two panes over one buffer would have to disagree about it. `copy_scroll` survives, and is now read by `TerminalBuffer::visible_lines`/`view_rows` and written by `Pane::scroll_back`/`scroll_forward`/`scroll_to_top`/`scroll_to_bottom`, bound to `k`/`j`/`b`/`f`/`g`/`G` (and `q` to leave) in `process_prefix_key`. |
| `apps/defrag` — file-list offset | Never read. The list measured against the raw panel height, so the overflow was cut by the *window edge* rather than by a scroll position — which is what made the hidden rows unreachable rather than merely off-screen. | Windows through `scroll_window::visible`, with an unconditionally-reserved "N more" footer per design-decisions.md §470. |
| `apps/podcast` — sidebar offset | Never read, and the sidebar needed more than a viewport: its *fixed* content alone (six library entries, twelve categories, two headings, two dividers) is 732px, so a 600px window could not show it with **no subscriptions at all**. | The column below the title became a flat `SidebarRow` list measured by `scroll_window::visible_variable` and scrolled as one. See §472 for the tradeoff against the usual pinned-sidebar convention. |
| `apps/podcast` — `episode_list_scroll` | Never read. The list drew rows until one *started* past the content height, so it drew that straddling row whole and past the bottom, and everything after it was cut by the window edge. | `scroll_window::visible` against the content height the caller already reduces by the now-playing bar, same reserved footer. |
| `apps/taskscheduler` — `task_list_scroll` **and** `history_scroll` | Both never read. Both renderers took a height parameter named `_height` and **ignored it**, drawing every row at a computed y with no bound. A `PushClip` hid the overflow, which is what kept it from being obvious — but a clipped row is exactly as unreachable as one drawn off the window when there is no offset to bring it back. | Both window through `scroll_window::visible`. `recent(100)` looked like a bound and was not (a hundred rows is 3200px); it is now `HISTORY_ROWS_OFFERED`, which is what it always was — how far back the tab reaches, not a viewport. |
| `apps/vpnmanager` — `log_scroll_offset` | Never read. The log stopped drawing at a hard-coded `py + 500.0`, a number with no relation to the panel it was drawing into: it overran a short panel and wasted a tall one. | `render_tab_log` is now handed the panel height instead of inventing one. Also fixed: the row-stripe parity was found by scanning the whole log for the row's own address with `std::ptr::eq` — O(n²) per frame over a 500-row log, with `.unwrap_or(0)` silently guessing on failure. `enumerate()` already has the index. `clear_log()` now resets the offset. |
| `apps/netmanager` — `sidebar_scroll` | Declared `f32` with the comment `(future: scroll offset)`, read by nothing. The interface loop had **no break of any kind**, so a long list drew straight through the status bar and off the bottom of the window. | Windows through `sh` — the height `render_sidebar` already computed and threw away. The selection highlight now compares against the *absolute* row index, so it follows the selected interface as the list scrolls instead of staying on whichever row happens to be there. |
| `apps/netscan` — `sidebar_scroll` (+ three more it exposed) | Dead. Pulling on it found three more in the same screen: the results table shifted rows by a pixel offset and drew *all* of them behind a clip, so the bottom row was sliced in half and an unclamped offset could push the table out of its own panel; the port list compared each row's y against a `dy` it was advancing *inside the same loop*, so its `break` could never fire and its `continue` skipped the advance — one notch past a row's height made the entire port list vanish; and the click hit-test computed the table's first row itself, getting a different answer from the renderer (no 26px summary bar, no scroll offset), so clicking a row on a scrolled table opened some earlier host's panel. | Table and hit-test both read `RESULTS_ROWS_TOP` and call one `results_visible_rows()`, so they cannot disagree. `sidebar_scroll` was **deleted** rather than wired up — the only scrollable thing in that sidebar is the port list, which has its own offset, so inventing a meaning for a second one would be worse than removing it. `topology_zoom`, dead for the same reason, went with it. |
| `apps/radio` — `genre_scroll` (dead) **and** `station_scroll` (read, never written) | `station_scroll` is the read-but-never-written case above. `genre_scroll` was dead, and the genre loop bounded itself against the sidebar's bottom edge rather than against the search hint pinned 20px above it, so a window shorter than ~468px drew genres straight over the hint. The genre filter is cycled by Left/Right with no regard for what is on screen, so the highlighted genre could also sit below the fold with no key that brought it back. | Both lists now use the toolkit types written for these rules: `ListViewport` for the station list and the genre selection, `scroll_window` for the genre list's window. Every distance is a named constant and both renderers `debug_assert!` that the geometry they walk is the geometry the capacity was computed from — the two were previously spelled out twice with different values. A page is now a windowful rather than a fixed five rows. |
| `apps/settings` — `dropdown_scroll` (+ the hit-test it exposed) | Dead, and the popup was **unbounded**: `popup_h` was `item_count * 36 + 8` with no reference to `window_height`, and the device dropdowns are built from runtime lists, so "taller than the window" is not hypothetical. Pulling on it found the larger defect — the popup's geometry existed only inside `render_open_dropdown`, so the click handler had nothing to test against and settled for `// For simplicity, any click closes the dropdown`. `apply_dropdown_selection` — correct, complete, covering all ten dropdowns — was reachable **only from tests**. | `DropdownLayout` holds the geometry, computed once by `dropdown_layout()` and used by both the renderer and the hit-test; `item_at` is the literal inverse of `row_top` and sits beside it in the same `impl`, so a change to one is a change to the other. The popup is pulled up to fit the window and scrolls when it cannot, opening a dropdown reveals the choice it already has, and a popup hiding items says how many. |

### What the reintroduction checks confirmed

Each fix was verified by restoring the defect and re-running:

- `listview` — exactly 3 of the new tests failed;
  `every_sequence_of_moves_leaves_the_selection_visible` correctly did **not**,
  since `reveal` maintains the clamp under mutating calls and the bug only
  showed when the list changed behind the viewport's back.
- `diskanalyzer` — all 4 new tests failed.
- `kanban` — 3 of 5 failed. The other two are aimed elsewhere (the
  measure/draw agreement, and the shrunk-column clamp, which lives in
  `scroll_window` and has its own tests there).
- `tmux` — `scrolling_back_changes_what_the_pane_draws` failed, which is the
  only one of the eleven new tests that goes through `render()`. The other ten
  are about numbers; this one is about pixels, and it is the one that would
  have caught the original bug.
- `podcast` — 4 of 7 sidebar tests failed with the sidebar unbounded again; the
  3 that did not are testing the clamp and reachability, which that particular
  defect does not violate. The episode list: 4 of 6.
- `taskscheduler` — 6 of 8. `vpnmanager` — 4 of 6. `netmanager` — 4 of 7 (the
  other three test the last-page clamp and reachability, which an *unbounded*
  list satisfies trivially — an unbounded list does reach everything, it just
  draws it in the wrong place).
- `netscan` — each of the four defects was reintroduced separately: 1 test for
  the hit-test, 3 for the table window, 3 for the port loop, 1 for the topology
  notice.
- `radio` — three separate reintroductions: the frozen scroll fires 2, the hint
  overdraw 1, the unfollowed genre filter 2.
- `settings` — three separate reintroductions: the missing hit-test fires 1,
  the unbounded popup 3, the dead scroll offset 3.

### Two things the sweep settled, worth not re-deciding

**Test what is *drawn*, and scope the helper by shape, not by pixel position.**
A test that only asks what the selection index is would have passed throughout
radio's frozen-scroll bug — the index was always right; it was the window that
never moved. The helpers key on the text shape (`H000` hostnames, `I000`
interfaces, the `" more"` suffix) or on command provenance (settings scopes to
commands from the popup's `BoxShadow` onward) rather than on an x/y box,
specifically so that moving a panel cannot silently turn a helper into a filter
that matches nothing and a test into one that asserts about an empty list.

**The strongest test of a hit-test is the renderer.** settings' best test asks
the renderer what it drew and the hit-test what is under each drawn row, and
requires them to agree. A hit-test checked against its own arithmetic checks
nothing — that is exactly the bug netscan had, where hit-test and renderer each
computed the first row and got different answers.

### A seventeenth instance, found later

`apps/remotedesktop`'s `content_scroll` (fixed in `ffd9d7b25`, while converting
that app's wheel handler for `C-SCROLL-DELTA-UNITS-ARE-DOCUMENTED-WRONG`). It
had the standard four grep hits — declared, initialised, written by the wheel,
never read — so the file-transfer and history lists drew every row and let the
clip cut the tail off, and everything past the bottom edge was unreachable.

Two things about it are worth adding to the sweep's record:

- **The same app had a second, differently-shaped instance the grep signature
  does not catch.** Its session list ignored `sidebar_scroll` — an offset that
  *is* live, and correctly read by the neighbouring profiles list in the same
  sidebar. A per-field grep cannot see that, because the field's hits are all
  present and correct; what is missing is a hit in one particular renderer.
  **The check that finds it is per-*list*, not per-field:** for every list that
  clips, does the loop that draws it consult an offset at all?
- **This one was reachable.** remotedesktop has a real `handle_event`, so
  unlike most of the sweep's instances this was a live user-visible bug rather
  than a latent one waiting on `C-NO-APP-IS-WIRED-TO-AN-EVENT-LOOP`. That is
  the second app in that position, after kanban.

### Two more, found the same way — one fixed, one not

Turned up while surveying the rest of the scroll-delta consumers. Both are the
plain shape: declared, initialised, written by the wheel handler, **never
read**.

| App | Field | Hits | Why the sweep's grep missed it | Status |
|---|---|---|---|---|
| `apps/diskimager` | `scroll_offset` (`main.rs:1015`, written at 1551) | 3 | Three hits, not four — it has no `.max()`/clamp helper of its own, so it falls below the signature's threshold. | **fixed, `344301f1c`** |
| `apps/devicemanager` | `properties_scroll` (`main.rs:777`, written at 3145) | 5 | Five hits, not four: two extra *writes* (`= 0.0` resets at 878 and 3080) push it over. Resets look like liveness and are not. | **fixed, `676fa35b0`** |

**diskimager turned out to be a matched pair, and that is a new shape.** Its
`scroll_offset` was written by the wheel and read by nobody — but its ISO 9660
file tree read `iso_scroll_offset`, which was *written* by nobody. A dead
writer and a dead reader, in the same app, for the same list: two halves of one
feature that were never joined. The visible result was worse than an offset
that does nothing, because the tree is the app's whole browse tab and could
show a few thousand files of which the user could reach one screenful.

Fixing it meant deleting both fields for a single `iso_scroll: usize`, and the
tree gained the interaction it was already *drawn* for while it was reachable:
`expanded` was rendered as a `v`/`>` arrow but nothing could ever change it, so
no directory below the root's own children had ever been visible either. See
`C-RENDERER-AND-HIT-TEST-DERIVE-THE-SAME-LAYOUT-SEPARATELY` — the same commit
named the drive list's duplicated `64.0` row height, which the renderer and the
hit-test each carried their own copy of.

**Look for the dead-reader half too.** The sweep so far has only ever grepped
for offsets that are written and not read. An offset that is *read and not
written* renders a list that can never move, which looks identical to a list
that simply fits. The grep is the mirror image: for each scroll-offset field,
does anything outside `new()`/`Default` assign to it?

That is the lesson worth carrying: **a hit count is a bad test for a dead
field, in both directions.** What identifies one is the absence of a *read*,
and both of these have zero. The reliable grep is for the field name in the
crate's render functions, not a count over the crate.

Their live siblings in the same two apps are fine and read correctly
(`diskimager::sidebar_scroll` at 1576/1810, `devicemanager::tree_scroll` at
1722/3054/3115), which is what makes the dead one easy to overlook: the app
visibly scrolls, just not in that pane.

### Left over

`gui/desktop`'s touchpad-gesture, Bluetooth and window-rules panels are now
*correct* for any offset a caller sets, but still have **no input wired to set
one** — tracked separately as
`C-TOUCHPAD-GESTURE-LIST-DRAWS-PAST-THE-PANEL`. The panels render from `&self`
and cannot write a position back, so this needs settings-shell plumbing rather
than a change in the panels.

**And the larger thing this sweep walked into.** Chasing the last few offsets
turned up *why* a scroll position can sit dead in eleven separate apps without
anyone noticing: for most of them there is **no way to scroll at all**, because
the app is never connected to an input source. Of the 140 app crates, 59 define
no event handler anywhere in the crate, and `podcast` — three of this sweep's instances —
is one of them. Its `scroll_episode_list_by` is correct, tested, and called
only by tests. The same is true of `defrag`, `diskanalyzer`, `netmanager`,
`taskscheduler`, `vpnmanager` and `tmux`: this sweep gave each of them a
working viewport that no user can currently reach. (`kanban`, `netscan`,
`radio` and `settings` do have handlers, so their fixes are reachable the
moment an app is hosted at all.) That is tracked as
`C-NO-APP-IS-WIRED-TO-AN-EVENT-LOOP` below, and it is the reason the fixes here
were still worth making — the viewport logic is the part that is hard to get
right and easy to test, and it is now right and tested in all of them.
