## C-TWO-PROCESS-LISTS-HIT-TEST-UNDER-THEIR-OWN-STATUS-BAR (lane C, 2026-08-20) — **FIXED**

**Status:** FIXED the same day. The sweep grew from two instances to five while
being fixed, one commit each:

| Commit | App |
|---|---|
| `f0343e329` | `sysmonitor: the process list was hit-tested under its own status bar` |
| `0d0c8dc1b` | `procexplorer: the process list was hit-tested under its own status bar` |
| `a0539b77c` | `credmanager: clicking the "60 entries" caption opened the first credential` |
| `d72793429` | `musicplayer: the track under the pointer was not the track that got played` |
| `bea1a3bd7` | `hexeditor: clicking the status bar jumped the cursor to the end of the file` |

**The fix is the same collapse in all five**: three helpers — a `const fn`
top, a `fn …_height()` that clamps at zero, and a `fn row_at()` that is the
renderer's arithmetic inverted — with every pointer path *and* the renderer's
clip routed through them, so the region drawn **is** the region clicked. Where
an app had a second renderer for a second tab (musicplayer's playlist), that
one goes through them too.

`scripts/reintro-row-hit-tests.py` puts each of the twelve resulting defects
back one at a time and checks the suite goes red for it. All twelve are
pinned — but only after the sweep itself found the gap: **musicplayer's
Playlists tab is drawn by a second renderer with the same arithmetic in it,
and the snap could be put back there alone with all 52 tests still green.**
That is the reintroduction check earning its keep; a test count cannot see a
duplicated renderer.

**Four faults were found only while fixing, not while auditing** — worth
noting, because the audit that opened this entry looked for exactly one shape
and walked past these:

- **musicplayer's renderer snapped to whole rows while the hit test was
  continuous.** A wheel-only user never sees this (a notch is a whole number
  of rows); every trackpad user plays a different track than the one they
  pointed at.
- **musicplayer clamped `scroll_offset` only inside the wheel handler**, so a
  resize, a search that shortened the library, or a tab switch could leave the
  list wound off its own end, painting nothing at all.
- **hexeditor's wheel read only the *sign* of `dy`** and moved a flat three
  lines per event — and read it backwards relative to every other view in the
  tree, since `dy > 0` is away from the user and so scrolls *up*.
- **hexeditor's click path had no *right* edge**, and the data inspector is
  painted over the dump's right-hand side, so clicking an inspector field
  moved the cursor in the file behind the panel. (hexeditor also carried a
  dead second copy of `show_inspector` on `HexView`, beside the `HexEditor`
  one that actually drives the renderer. Removed.)

**Two lessons for the next sweep of this family**, both learned the expensive
way here:

1. **A test that probes row *tops* cannot catch a snapping-vs-continuous
   divergence.** With musicplayer's old snapped renderer,
   `from_top = (36 + i*36) - 36 + 18 = i*36 + 18`, so `idx == i` at *every*
   top edge — the error is zero at the origin of each row and grows down it.
   The tests now sweep eight points across each painted row. This was found by
   patching the snap back in and watching the top-edge version stay green,
   which is the only way to know a regression test discriminates.
2. **A negative `f32` cast to `usize` saturates to 0 in Rust; it does not
   wrap.** That is why credmanager's missing lower bound was *quiet*: a click
   on the "60 entries" caption produced a negative offset, saturated to row 0,
   and decrypted and displayed the first credential rather than crashing.

**Where:** `apps/sysmonitor/src/main.rs` (hit tests at ~1099, ~1116, ~1157) and
`apps/procexplorer/src/main.rs` (~1159, ~1174, ~1216).

The same defect as the speed test's stats strip
(`C-A-SPEED-TEST-HISTORY-DRAWN-UPSIDE-DOWN-RELATIVE-TO-ITS-HIT-TEST`, fault
family 2), in the two apps whose whole purpose is a long clickable list of
processes. Both were found by the query that family suggests: an app that both
handles a mouse event and *divides* by a row height somewhere, then checking
whether the divisor's base and bound match the renderer's.

Both apps paint an **opaque** status bar across the bottom of the window:

| | sysmonitor | procexplorer |
|---|---|---|
| status bar | `y = window_height - 24`, `CRUST` (`:1386`) | `y = window_height - 24`, `COLOR_STATUS_BG` (`:1505`) |
| renderer's row area | `content_h = window_height - TAB_BAR_HEIGHT - 24` (`:1724`) | `content_h = window_height - content_y - 24` (`:1517`) |
| rows clipped to | `tree.clip(0, rows_y, w, row_area_h)` (`:1830`) | `row_area_h = content_h - HEADER_HEIGHT` (`:1568`) |
| scroll bound | `visible_row_count` subtracts `STATUS_BAR_HEIGHT` (`:1276`) | `content_h` subtracts it (`:1334`) |
| **hit test** | **`if my >= rows_start`** — no lower bound | **`if my >= content_y + HEADER_HEIGHT`** — no lower bound |

So in each app the renderer, the clip *and* the scroll bound all know the list
stops 24 px above the bottom of the window, and the hit test is the one place
that does not. With `ROW_HEIGHT` at 22 in both, the 24-px status bar covers
**just over one full row**: clicking the status bar selects a process that is
not on screen, and moving the pointer across it highlights one. In sysmonitor
the right-click path does it too, so the context menu — *Kill process* among its
actions — can open against a row the user cannot see.

That last point is why this is filed as a defect rather than a cosmetic
mismatch: the selection it makes is invisible, and the action offered on it is
destructive.

**Why the tests miss it.** Both suites probe row *centres*, computed from the
same `content_y + HEADER_HEIGHT` base the hit test uses, so they cannot observe
a bound the hit test never applies. The lesson is the one already recorded for
the speed test: read the row positions out of the `RenderCommand`s the renderer
actually emits, and probe the *edges* of each rectangle rather than its middle.

**What the proper fix looks like.** Identical in both, and identical to the fix
already landed in `apps/speedtest`: one description of the row area — a
`rows_top()` and a `rows_height()` on the app — read by the renderer, by
`visible_row_count`, and by all three hit tests, with the hit test rejecting
`my >= rows_top() + rows_height()`. sysmonitor additionally spells the same
number two ways (`content_y = TAB_BAR_HEIGHT` then `+ HEADER_HEIGHT` in the
left-click arm; `content_y = TAB_BAR_HEIGHT + HEADER_HEIGHT` in the right-click
and move arms), which is not itself a bug but is the state a list is in
immediately before two copies drift apart — the helpers remove it.

Tests should assert, for each app and each of the three pointer paths, that a
click at `rows_top() + rows_height() - 0.5` still selects and one at
`rows_top() + rows_height()` does not, with the row area's extent read from the
emitted clip command rather than recomputed.

**Not fixed in the commit that found it** only because that commit's workspace
gate was already running and editing the crates would have invalidated it —
the same reason `C-TOUCHPAD-GESTURE-LIST-DRAWS-PAST-THE-PANEL` waited a day.
It was the next thing lane C picked up; see the **Status** block at the top of
this entry for the five commits that closed it. Everything from here down is
the original write-up, kept because the *shape* of the defect — a top edge
spelled once per pointer path plus once in the renderer, and a bottom edge
spelled only in the renderer's clip — is worth recognising elsewhere in
`apps/`.

### The same sweep, third instance: credmanager's list header selects row zero

`apps/credmanager/src/main.rs` `handle_list_click` (`:5130`) has no guard of
its own. `handle_mouse` sends it every left click in the entry-list column that
is not in the toolbar (`:5024`), so the 32-px "N entries" header strip reaches
it, and:

```rust
let y_start = TOOLBAR_HEIGHT + LIST_HEADER_HEIGHT;   // 48 + 32
let row_idx = ((my - y_start + state.list_scroll) / ROW_HEIGHT) as usize;
```

For `my` inside that strip the numerator is negative — between −32 and 0 over a
`ROW_HEIGHT` of 52 — and a negative `f32` cast to `usize` **saturates to 0**
rather than wrapping. So clicking the header selects the first entry and opens
its detail panel. With the list scrolled it picks some other wrong row instead
of none.

Below the list it is safe by luck rather than by bound: `filtered_ids.get(row_idx)`
returns `None` past the end, so an over-large index selects nothing. The fix is
the same helper pair as the other two, which turns both the top and the bottom
into an explicit rejection rather than one accident and one saturation.

Lower severity than the process lists — the action is a selection, not a kill —
but the same root cause, and the constant it gets wrong (`LIST_HEADER_HEIGHT`)
is one that was *already* extracted to stop exactly this. Extracting the
constant fixed the two spellings and left the missing bound.

### Fourth instance: the music player's list snaps when drawn and doesn't when clicked

`apps/musicplayer/src/main.rs`. `scroll_offset` is a **continuous pixel**
offset — `wheel::pixels(*dy, TRACK_ROW_HEIGHT)` writes it (`:2296`) and a test
pins a fractional value deliberately (`a_fraction_of_a_notch_moves_now_rather_than_being_banked`,
`:2553`, asserts `0.3 * TRACK_ROW_HEIGHT`). The two render paths then throw the
fraction away:

```rust
let scroll_start = (state.scroll_offset / TRACK_ROW_HEIGHT) as usize;  // :1364, :1501
...
let row_y = TRACK_ROW_HEIGHT + i as f32 * TRACK_ROW_HEIGHT;            // snapped
```

while the two hit tests keep it:

```rust
let row_idx = ((rel_y - TRACK_ROW_HEIGHT + state.scroll_offset)
    / TRACK_ROW_HEIGHT) as usize;                                      // :2239, :2311
```

So at any offset that is not a whole number of rows the list is drawn snapped
and hit-tested continuously. At 0.3 rows of scroll the top 70% of every drawn
row selects the right track and the **bottom 30% selects the one below it** —
and because `MouseEventKind::DoubleClick` uses the same arithmetic (`:2311`),
double-clicking near the bottom of a row *plays* the wrong track.

Two defects, not one. The second is that the single click has no bounds check
at all:

```rust
state.selected_index = Some(row_idx);   // :2241 — row_idx unbounded
```

The renderer clamps `display_count` to the list length, but the click handler
does not, so clicking the empty strip below the last track selects a track
index that does not exist. (The double-click path is safe here by luck — it
goes through `filtered.get(row_idx)`.)

Note the asymmetry worth keeping: the fraction is not merely dropped, it is
*stored and tested*. A test asserts the app accepts a third of a notch, and the
renderer cannot show it. Whichever way that is resolved — snap the offset on
write, or let the renderer draw at the fractional position — both sides must be
resolved the same way, which is the whole point of deriving them from one
helper.

`sysinfo`'s `tree_hit_test` already carries a doc comment describing exactly
this bug in the past tense ("The old form added the scroll offset as **pixels
before dividing**, so any list scrolled to a position that was not a whole
multiple of `TREE_ROW_HEIGHT` selected the row above or below the one drawn
under the pointer"). It was fixed there and never swept for elsewhere.

### Fifth instance: the hex editor's status bar jumps the cursor to end-of-file

`apps/hexeditor/src/main.rs` `handle_mouse_click` (`:2164`) bounds the click
above and not below:

```rust
let content_y = y - TOOLBAR_HEIGHT - TAB_BAR_HEIGHT;
if content_y < 0.0 { return; }
let line = (content_y / LINE_HEIGHT) as usize;
```

`visible_line_count` (`:1645`) subtracts `STATUS_BAR_HEIGHT` and the click
handler does not, so the status bar is live: a click on it produces a line past
the last one drawn. The offset is then clamped — `doc.cursor = offset.min(max)`
— which makes the symptom *quiet rather than absent*: clicking the status bar
moves the cursor to the last byte of the file, discarding any selection through
`update_selection`. Milder than the process lists (no destructive action) but
the same missing bound, and the clamp is what stops it being noticed.

Unlike musicplayer, the scroll offset here is a `usize` row index
(`line.saturating_add(view.scroll_offset)`), so there is no fractional
divergence — only the missing lower bound.

### Checked and clear

All fifteen apps that both handle a mouse event and divide by a row height have
now been swept. Clear:

* `ebook` — library `list_top` is `TOOLBAR_HEIGHT`, matching the hit test's
  subtraction.
* `partmanager` — `y >= top && y < bottom`, bounded at both ends.
* `netscan`, `remotedesktop` (profile sidebar) — both already carry regression
  tests for an earlier mis-selection.
* `benchmark` — `history_row_at` rejects outside `content_top()..content_bottom_edge()`,
  guards non-finite input, and carries a doc comment recording a previous fix of
  this exact family.
* `devicemanager`, `sysinfo` — both route every caller through one
  `tree_hit_test` that rejects `offset >= pane_height()`; both doc-comment why.
* `filediff` — has `MouseEventKind::Scroll` handlers but no click-to-row
  mapping at all, so there is nothing to diverge. Its `visible_range` helper
  already centralises the render-side arithmetic.
* `settings` — the sidebar category list is fixed-length and unscrolled, and
  `idx < SettingsCategory::ALL.len()` bounds it correctly.

Recorded so the next sweep does not re-derive them.

**Debt, not a bug, in `settings` — FIXED, see below:** `list_y = HEADER_HEIGHT + SEARCH_BAR_HEIGHT + 16.0`
is written out longhand four times — the renderer (`:1422`, as
`search_y + SEARCH_BAR_HEIGHT + 16.0`), the click handler (`:3264`), the hover
handler (`:3343`) and `test_sidebar_click` (`:4049`). All four agree today. The
test is the notable part: it recomputes the constant rather than reading the
emitted render command, and probes the row *centre* (`+ 10.0` into a 44-px
row), so it would pass unchanged if the renderer and the hit test drifted
apart. That is the same blind spot that let the speed test's mirrored history
survive sixty-six tests.

**Settings sidebar: fixed.** The four copies are now one. `SettingsState` grew a
`search_top()`, a `category_list_top()`, a `category_row_top(idx)`, a
`CATEGORY_ROW_PAINTED_HEIGHT` and a `category_at(mx, my)` that is
`category_row_top` inverted; the renderer draws from the former and both hit
tests — click and hover — answer from the latter.

Collapsing it turned up one small live fault that the four-copy arrangement had
been hiding in plain sight. The renderer paints each highlight
`CATEGORY_ITEM_HEIGHT - 4.0` tall, so there are four blank pixels between one
row and the next; both hit tests claimed the full 44-pixel slot. Hovering those
four pixels therefore lit up the row *above* the pointer — a highlight sitting
one to four pixels clear of the cursor that summoned it. `category_at` now
returns `None` there, which is what the renderer's own output says, and matches
the precedent `notif_pane`'s `card_at` set for the gaps between notification
cards.

`test_sidebar_click` is gone, replaced by five tests that read the rectangles
`render()` actually emitted (signature: a `FillRect` at `x == 8.0` of width
`SIDEBAR_WIDTH - 16.0`, which nothing else in the sidebar shares) and probe
*those*:

| test | pins |
|---|---|
| `every_category_is_clickable_exactly_where_it_was_painted` | eight probes across each painted row; click and hover must agree, and each starts from a different category so a hit test that does nothing cannot pass |
| `the_gaps_between_category_rows_belong_to_no_row` | every whole pixel of every four-pixel gap |
| `nothing_outside_the_category_list_selects_a_category` | above the list, past its last row to the window's bottom, right of the sidebar, and NaN/±inf on both axes |
| `the_hover_highlight_lands_under_the_pointer` | hovers an *unselected* row and asserts the second rectangle that appears contains the pointer |
| `the_whole_category_list_fits_the_window` | tripwire: the sidebar has no clip and no scroll, so a list taller than the window has rows that cannot be reached at all |

Measured, not assumed: a three-pixel shift applied to the renderer alone fails
`every_category_is_clickable_exactly_where_it_was_painted` and
`the_hover_highlight_lands_under_the_pointer`. The test they replace passed that
same shift, which is the whole point — it clicked ten pixels into a
forty-four-pixel row and never looked at the renderer. Shifting the *shared*
constant, by contrast, fails nothing, because after the collapse a drift is no
longer expressible in one edit.

Three settings defects were added to `scripts/reintro-row-hit-tests.py`
(fourteen in total now): the renderer drifting three pixels, the gap check
deleted, and the hover handler given back its own copy of the arithmetic.

**Still open in `settings`:** the sidebar cannot scroll. Eight categories need
464 px and the default window is 800 px tall, so it fits today; the tripwire
above fires when it stops fitting. The fix at that point is a
`guitk::scroll_window` plus a `wheel::Accumulator`, as `notif_pane` has — not a
smaller `CATEGORY_ITEM_HEIGHT`.

**Also noted — FIXED, see below:** `gui/desktop/src/notif_pane.rs` duplicates the
quick-settings layout numbers between `render_quick_settings` (`:1008`) and
`handle_quick_settings_click` (`:1536`) — title, `+20.0`, N × `QS_ROW_HEIGHT`,
`+4.0`, volume slider, `+QS_ROW_HEIGHT`, brightness slider, written out twice
and inverted by hand the second time. They agree today, and the dispatcher
bounds the area at both ends, so this is debt rather than a bug. The card list
in the same file has already been collapsed (`card_tops` / `card_at` /
`content_height` / `max_scroll`), so the quick-settings block is the last
hand-inverted layout in that pane.

**Quick settings: fixed, and it was hiding a live fault after all.** The
duplication was real — `render_quick_settings` walked caption, toggles, gap,
sliders adding heights up while `handle_quick_settings_click` walked the same
list subtracting them back off, both spelling `20.0` and `4.0` by hand even
though `QS_TITLE_HEIGHT` and `QS_SLIDER_GAP` already existed — and the
hand-inverted walk had a hole in it:

```rust
let slider_y = content_y - toggle_area_end - 4.0;
...
if slider_y < QS_ROW_HEIGHT {
    self.quick_settings.volume = value;
```

`slider_y` is negative for the four pixels the renderer leaves blank between
the last toggle and the volume slider, and nothing asked whether it was above
zero. A negative `f32` divided into a row height is still negative, and
`slider_y < QS_ROW_HEIGHT` is true of every negative number — so **clicking the
blank gap set the volume**, to wherever along the track the pointer happened to
be. There is no visual cue that those four pixels do anything.

Collapsed the same way as everything else in this sweep: `qs_start_y()`,
`qs_toggle_top(idx)`, `qs_slider_top(slot)` and a `qs_at(local_y) ->
Option<QsHit>` that inverts them, with `QsHit` naming what is there
(`Toggle(idx)` / `Volume` / `Brightness`). The renderer places every row from
the helpers, `handle_quick_settings_click` matches on `qs_at`, and
`PANE_PADDING + HEADER_HEIGHT` — which was itself written out three times, in
the renderer, the click dispatcher and `list_start_y` — is now `qs_start_y()`
once.

Four new tests, all reading rows out of the commands `render_quick_settings`
pushed (toggle pills by `TOGGLE_WIDTH`/`TOGGLE_HEIGHT` less the six-pixel
inset; slider rows by the track's width, height *and* colour, so a slider at
100% is not counted twice by its own filled portion):

| test | pins |
|---|---|
| `every_quick_setting_row_answers_where_it_was_drawn` | eight probes across every painted toggle and slider row |
| `the_gap_above_the_volume_slider_does_not_set_the_volume` | every whole pixel of the gap, clicking the far end of the track so a hit would be loud |
| `the_caption_and_the_space_past_the_last_slider_belong_to_nothing` | the caption, past the last slider, negative, NaN, ±inf |
| `a_click_through_the_pane_reaches_the_toggle_that_was_drawn` | the whole path — pane-local coordinates, the dispatcher's bounds, the pill's x range, and the emitted `QuickSettingToggled` |

Measured: deleting the gap check fails
`the_gap_above_the_volume_slider_does_not_set_the_volume`; shifting the toggle
renderer three pixels alone fails
`every_quick_setting_row_answers_where_it_was_drawn`. 2404 tests pass, rustfmt
clean, clippy unchanged at 82.

That closes the last hand-inverted layout in `notif_pane` — the card list
(`card_tops`/`card_at`) was collapsed earlier, in the fix that found the 76-px
drift.

### The toolkit menus: eight walks of one list become two — FIXED

The same fault family, in its hardest form, and the only instance so far that
sits in the toolkit rather than in one app: `gui/toolkit/src/menu.rs` (every
right-click menu in the OS) and `gui/toolkit/src/menubar.rs` (the dropdowns
under File/Edit/View in every windowed app).

A list whose rows all share one height needs no help: `top + i * H` and
`(y - top) / H` are visibly each other's inverse, and you cannot change one
without changing the other. These lists *differ* — a separator is 9 px where
an item is 28 — so there is no closed form, only a walk. Each file carried
**four** of them, and each of the four spelled out

```rust
match item {
    Separator => SEPARATOR_HEIGHT,
    _ => ITEM_HEIGHT,
}
```

for itself: one summing the heights for the popup's total (`total_height` /
`dropdown_content_height`), one adding them up to place the rows on screen
(the renderer's `let mut current_y` / `let mut cur_y`), one subtracting them
back off to answer a click (`index_at_y` / `item_index_at_y`), and one adding
them up again to decide where a submenu hangs (`y_offset_for_index`, in both).
Eight copies between the two files. Four walks of one list is four chances for
three of them to be right.

**No live fault was found in either** — the eight copies did agree. What they
did not have was any way to *stay* agreeing, and the existing tests could not
have told anyone otherwise: they probed row centres, where a three-pixel drift
is invisible.

The fix is a new toolkit primitive, `gui/toolkit/src/row_strip.rs`. A
`RowStrip` is that walk done once: given the top of the run and each row's
height it reports where every row is (`top`, `height`, `bottom`,
`total_height`) and which row owns a given `y` (`index_at`). A row owns its
top edge and not its bottom one, so adjacent rows never both answer for the
pixel between them; a non-finite height is taken as zero rather than poisoning
every row after it with NaN. It deliberately does not decide whether a row is
*selectable* — a separator has a position and a height like anything else, and
whether clicking one means something is the caller's rule, which is the one
thing `index_at_y` and `item_index_at_y` still add on top.

Both files now build one strip and read it from all four places. Eleven new
tests, all reading the hover rectangle `render()` actually emitted and
sweeping eight points across it plus both its edges:

| test | file | pins |
|---|---|---|
| `every_item_is_selectable_exactly_where_it_was_painted` | menu | eight probes across every painted row, plus its two edges |
| `a_separator_is_drawn_inside_the_run_it_reserves_space_in` | menu | the line lands inside the space the row reserves, and the row is unselectable |
| `a_submenu_hangs_off_the_row_it_belongs_to` | menu | `y_offset_for_index` equals the painted top, for every row |
| `the_menu_is_exactly_as_tall_as_the_rows_it_holds` | menu | the popup reaches one padding past the last row, and `point_in_bounds` agrees |
| `nothing_outside_the_run_selects_an_item` | menu | both paddings, past the bottom, NaN, ±inf |
| `an_empty_menu_is_just_its_padding` | menu | the degenerate case |
| `every_dropdown_row_is_selectable_exactly_where_it_was_painted` | menubar | as above, through the real open-and-hover pointer path |
| `a_dropdown_separator_is_drawn_inside_the_run_it_reserves_space_in` | menubar | as above |
| `a_submenu_hangs_off_the_dropdown_row_it_belongs_to` | menubar | as above |
| `a_dropdown_is_exactly_as_tall_as_the_rows_it_holds` | menubar | the panel reaches one padding past the last row |
| `nothing_outside_the_dropdown_run_selects_a_row` | menubar | padding, past the bottom, NaN, ±inf, and the empty dropdown |

Verified by reintroduction, six defects, all now in
`scripts/reintro-row-hit-tests.py` (21 pinned): drifting either renderer three
pixels fails three tests; giving either hit test its own walk back fails four
and five respectively — the menubar one also takes down the pre-existing
`click_check_item_toggles`; and giving either `y_offset_for_index` its own
walk back fails three. 982 guitk tests pass, rustfmt clean, clippy clean.

Worth recording for the next person who writes such a test: the menu bar's own
bottom border is drawn in the *separator colour*, so filtering emitted `Line`
commands by colour alone finds three separators in a dropdown that has two.
The test keys on the panel's left inset as well.

Still open, and not this fix: neither menu scrolls. A dropdown taller than the
screen is drawn past the bottom edge and the rows below it can be neither seen
nor reached. `RowStrip` is the right shape to build that on — a scroll offset
is just a different `origin` — but no caller needs it yet.

*(Fixed 2026-08-20 — see "Neither menu scrolled" below.)*

### Neither menu scrolled — FIXED

The paragraph directly above, made good. Both toolkit menus now scroll, in
`53cf09c34` (`menu.rs`) and `9e85a16a9` (`menubar.rs`).

The live fault was worst in `ContextMenu::show`, which placed a popup with

```rust
self.y = if y + total_height > 1080.0 { (y - total_height).max(0.0) } else { y };
```

A 120-item menu has `total_height == 3368`, so `y - total_height` is negative,
the `.max(0.0)` clamps it to zero, and the menu is drawn from the top of the
screen running 2288 px off the bottom of it. Every row past the display edge
was painted where no pointer can go. The menu bar's dropdowns had the same
shape with `dropdown_rect` supplying the height.

Three things about the fix are worth carrying to the next scrollable list:

**A scroll offset is just a different origin — provided it is subtracted
exactly once.** Both menus subtract it in one place, the origin handed to
`RowStrip::new`, so `index_at` remains the renderer's placement inverted for
free. A second subtraction anywhere else is a second description of where the
rows are, which is the whole fault family this sweep exists to remove.

**A scrolled list needs two heights and two tests where an unscrolled one
needs one of each.** `content_height` is the rows; `panel_height` is what fits
on screen, and the old code had only the first. Likewise the strip says which
row owns a `y` *in the list*, and a visible-region bound says whether that part
of the list is *on screen*. Without a scroll offset those two coincide, which
is exactly why one of them used to be enough — with one, the list extends into
the panel's own padding and the strip alone cheerfully names a row scrolled out
of sight.

**A non-finite offset must be refused, not clamped.** `NaN` compares false
against both ends of a `clamp`, so a clamp passes it straight through to the
strip's origin and every row's position becomes `NaN` at once — a menu that
answers for no pointer at all. Both `set_scroll` and `clamped_scroll` return
early on `!is_finite()`, and `reintro-row-hit-tests.py` pins it.

Along the way, `menubar.rs`'s five independent descriptions of one panel
rectangle (`dropdown_rect`, `click_in_submenu_chain`, `hover_in_submenu_chain`,
`render_submenu_chain`, and the renderer adding `DROPDOWN_VPAD` back on in a
fifth place) became one `DropdownPanel` value — there was otherwise nowhere to
put a scroll offset that all five would see. `item_index_at_y` and
`y_offset_for_index` were deleted rather than left as dead copies of the
geometry, and their tests rewritten against `DropdownPanel`.

One performance fault fell out of it. `calculate_dropdown_width` widens a panel
to fit its widest label, which means measuring every label, which means shaping
every label through the font. `dropdown_panel` is consulted by the hit test, the
renderer *and* every wheel notch, so a 200-row dropdown was shaping 200 labels
several times per mouse move. It is now computed where the entries change and
cached. The guitk suite went from 92 s to 7.4 s.

1000 → 1015 guitk tests; the sweep grew from 58 defects to 67.

### The tree widget selected its first node for a click above it — FIXED

Found while migrating the menus, by asking which *other* toolkit widget
inverts its own renderer. `gui/toolkit/src/tree.rs` did, twice:

```rust
// handle_click, and again verbatim in handle_context_menu
let row_index = ((y + self.scroll_offset) / self.config.row_height) as usize;
```

`visible.get(row_index)` bounded it above. Nothing bounded it below. **A
negative `f32` cast to `usize` saturates to zero in Rust** — it does not wrap
and it does not trap — so a click a few pixels above the first row selected
the first node, and a right-click there opened the context menu on it. NaN
casts to zero as well, so any coordinate a caller failed to compute did the
same. And `row_height` is a `pub` field on `TreeConfig`: a zero made the
division infinite or NaN, and *both* ends of that cast land on a real row, so
every click in the tree went to row 0.

This is the credmanager fault word for word — the one that decrypted and
displayed the first credential when you clicked the "60 entries" caption —
except in the toolkit, where every tree widget in the OS inherits it rather
than one app.

Both paths now go through one `row_at(y)` that refuses non-finite
coordinates, negative content offsets, non-positive row heights, and anything
past the last row. It stays a plain division rather than a `RowStrip`: these
rows are all one height, and the `row_strip` module doc says plainly that a
uniform list needs no help, because `top + i * H` and `(y - top) / H` are
visibly each other's inverse and you cannot change one without changing the
other. A `RowStrip` here would allocate two `Vec`s per click to express
something a division already expresses honestly.

| test | pins |
|---|---|
| `every_row_answers_exactly_where_it_was_painted` | eight probes across each painted row background, plus both edges |
| `a_point_above_the_first_row_selects_nothing_rather_than_the_first_node` | the saturating cast, through both pointer paths |
| `a_coordinate_that_is_not_a_number_selects_nothing` | NaN and ±inf, through both pointer paths |
| `a_row_height_of_zero_selects_nothing_rather_than_everything` | zero, NaN and negative row heights |
| `nothing_past_the_last_row_answers` | the last row's own edge, past it, and the empty tree |
| `scrolling_moves_the_rows_and_the_answers_together` | the hit test follows a scrolled row to where it is now drawn |

Verified by reintroduction: restoring the unbounded cast fails three of them,
drifting the renderer three pixels fails the other two. Both are now in
`scripts/reintro-row-hit-tests.py` (23 pinned). 988 guitk tests pass, rustfmt
clean, clippy clean.

### `reintro-row-hit-tests.py` credited a compile error as evidence — FIXED

Worth writing down because the script's whole purpose is to be believed.

It decides a defect is pinned by watching the suite go from green to red. Run
against a working tree with a half-finished edit in it, the suite was *already*
red, so every defect "pinned" — including, in the run that exposed this, two
guitk entries whose evidence was `error[E0599]: no method named selected_id`,
a compile failure in an unrelated file. **It printed `all 21 defects pinned`
and exited 0.**

The existing fallback that prints the compiler's own words when no test is
named is what made it visible at all — but that is something a reader has to
notice, not something the script refuses to do. It now runs each crate's suite
once before touching anything and exits 2 with `BASELINE NOT GREEN` if any of
them is red, on the principle that a measurement taken against an unknown
baseline is not a weaker measurement but no measurement.

The general form, for anything else built this way: a test that proves a
property by observing a *transition* must establish the starting state, or it
is only observing the end state and calling it a transition.

### partmanager described one rectangle four times — FIXED

The partition list's geometry was spelled out independently by the renderer,
the click handler and the wheel handler. Each was correct about something and
none agreed with the others:

| what | where rows begin | how tall the region is |
|---|---|---|
| renderer | `top + 18 + PARTITION_ROW_HEIGHT` | clip of `list_height - 20` |
| click | a flat seven-term sum ending `+ PARTITION_ROW_HEIGHT + 18.0` | down to `height - STATUS_BAR - queue_h` |
| wheel | plain `top` — the section heading's own y, 42 px too high | `queue_top - top - 40` |

The consequences, in descending order of how much they matter:

- **The clip ran 22 px below the bottom a click was accepted at.** Rows were
  painted over the queue panel's header where no click could reach them. That
  is invisible *today* only because `render_queue_panel` runs after
  `render_partition_list` and covers the overdraw — which is paint order doing
  a hit test's job, and stops being true the moment either panel moves.
- **The wheel measured the viewport from the heading rather than from the
  first row**, making it 2 px taller than it is. A viewport believed too tall
  gives a maximum scroll that is too small, so the last row's final sliver
  could never be brought into view.
- **The click accepted a `DISK_MAP_PADDING`-wide strip right of the last
  painted pixel; the wheel accepted one left of the first.** A gutter that
  belongs to no list scrolled and selected in one.
- **The row index came from an unguarded `as usize`.** A negative or NaN `f32`
  cast to `usize` saturates to zero in Rust rather than wrapping or trapping,
  so a NaN coordinate named row 0.

`PartitionList` now holds the left, width, panel top, data top and bottom;
`row_at()` is `row_y()` inverted; and the renderer, the hit test and the wheel
all read it. `queue_panel_height()` collapses the four hand-written choices
between `QUEUE_PANEL_HEIGHT` and a literal `28.0`, one of which *is* the
partition list's bottom edge.

| test | pins |
|---|---|
| `every_visible_partition_row_answers_exactly_where_it_was_painted` | eight probes across every row the renderer emitted and did not clip away, plus its last pixel |
| `the_clip_the_renderer_emits_is_the_region_a_click_lands_in` | the clip's own top and bottom edge, read out of the `PushClip` command |
| `nothing_beside_the_painted_list_selects_a_partition` | both horizontal gutters |
| `a_coordinate_that_is_not_a_number_selects_nothing` | NaN, ±inf and negative, through the click path and through `row_at` directly |
| `scrolling_to_the_end_brings_the_last_row_fully_into_view` | the wheel's idea of the viewport against the renderer's |
| `the_wheel_and_the_click_agree_on_where_the_list_is` | the wheel's horizontal extent against the painted one |

All six verified by reintroduction — each defect put back one at a time, each
pinned by the test named for it. partmanager 113 → 119 tests.

**The queue panel, noted here as still open, is now done too** — and it did
have divergences, four of them. See "partmanager's operation queue panel was
described four ways" below.

### Two hit tests let a NaN select the first row — FIXED

Found while sweeping the rest of lane C for the same family. Both had a bounds
guard that looked complete and was not, for the same reason: **a NaN compares
false against every operator, so it passes a `<`/`>=` bounds test by failing
it**, and `NaN as usize` is `0` rather than a trap or a wrap. The two compose
into "a coordinate that is nowhere selects row 0".

- `apps/settings` `DropdownLayout::item_at` — every one of its three guards is
  a `<` or a `>=`, so a NaN reached the divide and chose the popup's first
  visible item.
- `apps/remotedesktop` `handle_sidebar_click` — guarded with `y < 0.0`. Its
  `Connections` arm survived by accident, because it walks rows with `y < next`
  and that is false for a NaN too, so the loop ran off the end. Its
  `ActiveSessions` arm divides, and selected session 0.

Both now reject non-finite coordinates up front, where it is one condition
rather than three double negatives.

Also strengthened while there:
`the_hit_test_names_the_item_that_was_drawn_under_the_pointer` in settings was
probing each dropdown row's **centre**, where a drift of a few pixels is
invisible in a 36 px row — it now sweeps eight points across each row plus the
row's last pixel. A centre probe passes straight through the fault it exists
to catch.

**Cleared in the same sweep, no change needed:** `guitk/textview`
(`hit_test` clamps a caret to line 0 for a click above the text, which is what
every editor does and is not the list-selection rule), `gui/desktop/run_dialog`,
`apps/compass`, `apps/ebook`, `apps/unitconverter`, `apps/jsonviewer`,
`apps/sysinfo`, `apps/devicemanager`, `apps/diskimager`, `apps/benchmark` —
the last four already carry explicit `is_finite` guards from earlier passes.

### A settings test positioned its probes with the function it was testing — FIXED

Found by trying to reintroduce a defect and discovering there was nothing to
reintroduce it *into*. `the_hit_test_names_the_item_that_was_drawn_under_the_pointer`
computed each probe as `layout.row_top(row) + …` and then asked
`layout.item_at(…)` which item that was. But `item_at` is `row_top` inverted,
so the probe and the answer came from the same place and agreed by
construction. What the test could not see is the only thing worth seeing: the
*renderer* drawing somewhere other than `row_top`. Shifting the dropdown
renderer's `iy` three pixels — the exact fault the test is named for — left it
passing.

It now recovers each row's top from the `Text` command the renderer actually
pushed. That anchor rests on the renderer drawing an item's label a fixed
distance below the item's top, so a second test,
`the_selected_rows_highlight_confirms_where_the_rows_are_painted`, checks that
distance against the highlight rectangle the selected row paints at its own top
edge — the one place the popup emits a rectangle aligned to a row. Without it,
moving the baseline would silently move every probe and quietly restore the
original flaw.

Both failure modes are now in `scripts/reintro-row-hit-tests.py`. The general
rule, which is the same one that broke the sweep script itself: **a test must
get its expected values from somewhere other than the code under test.** A
renderer's own arithmetic is not an independent source just because it lives in
a different function.

### The notification pane's per-app settings list described one card four ways — FIXED

The pane's *notification* half was collapsed onto `card_tops` / `card_at` /
`qs_at` in an earlier pass, and the doc comments there still narrate the 76 px
drift that prompted it. The *per-app settings* half — the page you reach
through the pane's "Settings" link — was left as it was, and it had every
symptom the other half was fixed for.

`render_app_settings` walked a running total; `handle_app_settings_click`
divided by a literal. Four disagreements, in descending severity:

| | Renderer paints | Click accepted |
|---|---|---|
| card body | `FillRect { height: 100.0 }`, then `y += 108.0` | the full 108 px pitch |
| enabled pill, x | `enabled_x .. enabled_x + 40` | `rx >= enabled_x`, **no right edge at all** |
| enabled pill, y | `card_top + 10 .. card_top + 32` | `card_local_y < 35.0`, i.e. `card_top .. card_top + 35` |
| bottom of list | clipped to the pane, loop breaks at the edge | **no bottom bound whatsoever** |

So: the eight-pixel gutter between two cards belonged to the card above it;
a click anywhere to the right of the pill — including past the pane's own edge
— switched an app's notifications off; so did a click on the app *name*, which
is drawn in that 0..35 band; and so did a click below the pane entirely, on an
app card that was never on screen, hitting whatever window was behind it. The
last one is the serious one: it is a click the user aimed at something else.

`content_y < 0.0` was also the only guard, and a NaN is not less than zero, so
it passed — and `NaN as usize` is `0`, not a trap. A pointer position that is
nowhere at all named the first app in the list. The same shape as the settings
dropdown and the remotedesktop sidebar above.

**Fixed** by the same collapse: `APP_HEADING_HEIGHT` / `APP_CARD_HEIGHT` /
`APP_CARD_PITCH` / `APP_TOGGLE_TOP` as named constants, `app_card_top(idx)` as
the walk, `app_card_at(local_y)` as that walk inverted and bounded by the same
clip the renderer draws inside, and `app_toggle_rect(card_top)` returning the
one rectangle both the renderer and the hit test use. `card_width()` replaces
three hand-written copies of `PANE_WIDTH - 2.0 * PANE_PADDING`. The link at the
bottom of the list is placed from `app_card_top(len)` rather than from whatever
`y` the loop happened to leave behind.

Seven tests, each reintroduction-verified:

| Test | Catches |
|---|---|
| `every_pixel_of_a_painted_pill_toggles_its_own_app` | the pill's rectangle shrinking, or drifting from the paint |
| `nothing_outside_a_painted_pill_toggles_anything` | the unbounded right edge and the 0..35 band |
| `the_gutter_between_two_app_cards_names_no_card` | the pitch/height confusion |
| `a_card_below_the_panes_bottom_edge_is_not_clickable` | the missing bottom bound |
| `the_pills_are_painted_on_the_cards_the_walk_places` | renderer and hit test drifting *together* |
| `an_app_settings_coordinate_that_is_not_a_number_names_no_card` | the walk going back to a divide-and-cast |
| `the_per_app_heading_is_not_part_of_the_first_card` | the caption being absorbed into card 0 |

Three notes on the tests themselves, each of which cost a failing run to learn:

- **The gutter has to be asked of `app_card_at` directly, not through a
  click.** A click in the gutter is refused twice — once for being on no card,
  and again for being nowhere near the pill — and the second refusal hides the
  first, so the click-level assertion passes whether the gutter is handled
  correctly or not. It is the card *identity* that is wrong, and only the
  identity test sees it.
- **Filtering the render tree by shape alone is not enough here.** The
  quick-settings block above the list paints its toggles at the same size *and
  the same x* as an app card's pill, so a shape filter returned nine
  rectangles for four cards and the first "pill" the test swept was a
  quick-setting 200 px higher up. The helper now slices the command list from
  the "Per-App Settings" caption onwards first.
- **The NaN guard as written is not reintroducible, and pretending otherwise
  would have been the same mistake as the settings dropdown test above.**
  Deleting `!local_y.is_finite()` from `app_card_at` leaves the suite green,
  because a NaN also fails both comparisons *inside* the walk, so the walk
  refuses it on its own. The guard is real defence but it is defence against a
  future shape, not the current one. So the entry in
  `scripts/reintro-row-hit-tests.py` reintroduces what actually reopens the
  hole — the walk collapsing back into `(local_y - heading) / pitch as usize`
  — and that the test does catch. A sweep entry whose defect the suite cannot
  see is worse than no entry: it reports a green that means nothing.

**Also cleared here:** `desktop`'s clippy run was failing outright — not
warning, failing — on a `manual_contains` lint in a pre-existing test, so
`cargo clippy -p desktop` had been red for however long since the toolchain
picked that lint up. Fixed in passing; the crate now emits 77 warnings and no
errors, down from 79 and one.

### partmanager's operation queue panel was described four ways — FIXED

The last site logged as still-open from the earlier partmanager entry, and it
was not the quiet one that note assumed. Four consumers — `render_queue_panel`,
the header click in `handle_left_click`, the row hover in `handle_mouse_move`,
and the wheel in `handle_scroll` — each recomputed the panel's top and its
rows' offsets, and `28.0` (the header's height, which `QUEUE_HEADER_HEIGHT`
already named) was written out five times.

| what | region it accepted | how it placed a row |
|---|---|---|
| renderer | `top + 28` down `queue_h - 28` | `list_top + i * 22 - scroll` |
| header click | `queue_top` to `queue_top + 28`, `x >= SIDEBAR_WIDTH`, no right edge | — |
| hover | `queue_top + 28` to `height - STATUS_BAR`, `x >= SIDEBAR_WIDTH`, no right edge | `((y - queue_top - 28 + scroll) / 22) as usize`, unguarded |
| wheel | `queue_top` to `height - STATUS_BAR` — the header **included** | viewport as `queue_h - 28` |

What that cost, worst first:

- **A notch over a *collapsed* panel scrolled it to an arbitrary place.** The
  wheel accepted the header; a shut panel is nothing *but* its header. Shut,
  `queue_h` is 28, so the wheel's viewport term `queue_h - 28` is zero and its
  maximum scroll became the entire content height. You could park a closed
  panel anywhere — and then find it there, showing blank space, when you
  opened it.
- **`queue_scroll` was clamped by the wheel and by nothing else.**
  `undo_last_operation`, `clear_operations` and `apply_operations` all shorten
  the queue out from under a scrolled panel; none touched the offset. Undo
  twenty operations from a scrolled-to-the-end panel and it stayed parked below
  its own last row.
- **A notch over the words "Pending Operations" scrolled rows the pointer was
  not on**, because the wheel accepted a strip the hover refused.
- **The hover divided and cast with no guard on the offset.** A NaN clears a
  `y >= queue_top + 28.0` bound *by failing it*, and `NaN as usize` is `0`, so
  a pointer that is nowhere at all highlighted row 0.
- **Neither the header click nor the hover had a right-hand bound.** Both
  answered for the whole half-plane right of the sidebar, the region past the
  window's own right edge included.

`QueueList` now holds left, width, panel top, data top and bottom; `row_at()`
is `row_y()` inverted; `contains_header()` and `contains()` are the two regions,
and the second is empty by construction while the panel is collapsed, so no
consumer has to ask separately whether it is open. `queue_panel_height()` is
gone — `PartitionList::of` now takes its bottom edge from
`QueueList::of(app).panel_top` rather than recomputing the same subtraction,
which is the one number the two panels genuinely share.

`QueueList::max_scroll` deliberately takes **no `self`**: it measures against
the viewport the panel has when *open*, not the one it has right now. Using the
current viewport is what let a zero-tall collapsed panel hold a scroll position
the open panel refuses, and it would also snap the list to the top every time
the panel was shut.

| test | pins |
|---|---|
| `every_visible_queue_row_answers_exactly_where_it_was_painted` | eight probes across every row the renderer emitted and did not clip away, plus its last pixel, at a deliberate half-row scroll offset |
| `the_clip_the_queue_panel_emits_is_the_region_the_pointer_lands_in` | the clip's own top and bottom edge, read out of the `PushClip` command |
| `the_queue_panels_header_toggles_it_and_highlights_no_row` | the header is a button and not a row, at both its edges |
| `nothing_right_of_the_window_belongs_to_the_queue_panel` | the missing right-hand bound, through the click and the hover |
| `a_collapsed_queue_panel_neither_scrolls_nor_highlights` | the wheel against a shut panel, and that reopening does not move the list |
| `shortening_the_queue_pulls_the_panel_back_onto_its_rows` | undo and clear, checked as "is there a blank strip below the last painted row" rather than against a recomputed offset |
| `the_queue_row_walk_refuses_a_coordinate_that_is_not_in_the_list` | `row_at` directly: the header, the status bar, a NaN coordinate, a NaN scroll, a negative offset |
| `the_wheel_and_the_hover_agree_on_where_the_queue_is` | forty probes down through the panel, asserting the two consumers answer the same at every one |

Two notes on building those:

- **The wheel's own existing test was probing the header.** `QUEUE_POINT` was
  `(400, 600)`, which in a 750 px window is 28 px above the first row — inside
  the strip only the wheel thought was part of the list. It passed *because* of
  the defect. Moved to `(400, 650)`, and the reason recorded next to it.
- **The blank-strip assertion had to come from the render tree, not from
  `max_scroll`.** Asserting `queue_scroll == QueueList::max_scroll(n)` after an
  undo would have compared the code under test with itself. What the test
  actually asks is whether the bottom-most row the renderer painted still
  reaches the bottom of the clip it painted, which is the thing a user sees.

Verified by reintroduction: all nine new entries in
`scripts/reintro-row-hit-tests.py` put their defect back and go red.
partmanager 119 → 127 tests, rustfmt clean (two pre-existing diffs cleared with
it), clippy down from 19 warnings to 11 — the test module now allows
`unwrap_used`/`expect_used`/`panic`, which CLAUDE.md says belongs there, and
the 11 that remain are all `arithmetic_side_effects` on `f32` in production
code, pre-existing.

**Then still open in this file:** the sidebar's disk list. Now fixed — see the
next section.

### partmanager's disk sidebar was described three ways — FIXED

The last instance of this family in `apps/partmanager/src/main.rs`, and the
quietest: unlike the partition list and the queue panel, **no divergence had
happened yet**. `top + 28.0` — the height of the "Disks" caption — was written
out three times, in `render_sidebar`, in the sidebar click and in the sidebar
hover, and `((y - list_top) / SIDEBAR_DISK_ROW_HEIGHT) as usize` twice, once in
each pointer path, with no shared definition of either. The three agreed.

That is not a reason to leave it. The other two lists in this same file agreed
once too. What "no divergence yet" actually describes is a defect that has not
been *committed* yet, in a shape where committing one takes a single edit to
any of the three copies — and where nothing in the test suite would have
noticed, because the tests probed row centres.

Two smaller things were true of it as well:

- The unguarded divide-and-cast — the fault that let `credmanager` decrypt the
  first credential from a click on a caption, because a negative `f32` cast to
  `usize` **saturates to 0** in Rust rather than wrapping — was closed here
  only *by accident*, by the enclosing `x < SIDEBAR_WIDTH && y >= top &&
  y < bottom` test happening to run first. It is now closed *at the cast*, in
  `DiskSidebar::row_at`, where an edit somewhere else cannot lose it.
- The clip was `bottom - list_top` with nothing stopping it going negative. A
  window short enough that the title bar, toolbar, caption and status bar
  between them eat the column would have pushed a negative-height clip into
  the render tree. `viewport_height()` clamps at 0, and `of()` clamps `bottom`
  to `data_top`, the same way `PartitionList` already did.

#### `DiskSidebar`, and the two things it writes down that were only implied

The fix is the same collapse as `PartitionList` and `QueueList`: one
`DiskSidebar` value with `panel_top` / `data_top` / `bottom` / `left` /
`width`, a `row_y(i)` and a `row_at(y, count)` that is its inverse, and every
consumer — renderer included — routed through it. `render_sidebar` no longer
computes a single coordinate of its own.

Two facts about this list were true of the code but stated nowhere, so the next
edit could have reversed either without contradicting anything:

| | The rule | Where it now lives |
|---|---|---|
| **The inset and the gap are decoration** | A row is *painted* inset 4 px from each side of the column and 2 px short of its own bottom, so the selected row reads as a rounded chip. It *answers* across the column's full width and its full pitch. A click in the gap belongs to the row above it. | `row_paint_rect()`, which only the renderer may call; `ROW_INSET_X` / `ROW_GAP_Y` are named and commented as decoration |
| **The column consumes the pointer where no row answers** | A click on the "Disks" caption, or on the blank space below the last disk, must not fall through to the disk map behind it. | `contains_column()` (the whole column) is a separate and wider question from `contains()` (the rows) |

A third, smaller one came out of the same pass: `render_sidebar` still held
seven literals — three `x: 12.0`, three `max_width: Some(SIDEBAR_WIDTH - 24.0)`
and the health dot's `x: SIDEBAR_WIDTH - 20.0`. Those are single-consumer
decoration and were not part of the fault, but every `SIDEBAR_WIDTH` in that
function is a second spelling of `geom.width`, which is exactly the state the
whole collapse exists to leave. They now go through `text_x()` /
`text_max_width()` / `right()`, and `render_sidebar` contains no occurrence of
`SIDEBAR_WIDTH` at all.

The first of the two rules above is the **opposite** of the one `notif_pane`
needed, where the 8 px
gutter between cards belongs to no card and the hit test had to be taught to
refuse it. Two lists in the same tree, two different answers, both correct —
which is exactly why neither can be left to be inferred from the arithmetic.
Both are now asserted by a test, so a future edit that swaps one rule for the
other fails rather than ships.

#### Tests

Eight, all reading their expected values from the render tree — the clip
`render_sidebar` pushes and the row rectangles it fills — never from a second
copy of the code under test's arithmetic:

| Test | What it pins |
|---|---|
| `every_visible_sidebar_disk_answers_exactly_where_it_was_painted` | 8 probes across each painted row plus its last pixel, for **both** pointer paths. Probing centres cannot see a renderer 3 px out of step with a 48 px row |
| `the_clip_the_sidebar_emits_is_the_region_the_pointer_lands_in` | The clip's first pixel names row 0; the pixel above it and the pixel past its end name nothing |
| `the_inset_and_the_gap_under_a_sidebar_row_still_belong_to_that_row` | The decoration rule above, at four x positions and inside the gap |
| `the_disks_caption_consumes_the_pointer_and_names_no_disk` | `Consumed` with no selection change, for click and hover |
| `nothing_right_of_the_sidebar_belongs_to_it` | The right-hand bound neither pointer path used to have — including a point past the window's own edge |
| `a_sidebar_row_below_the_fold_answers_nowhere` | 24 disks in a column with room for ~13: no row painted entirely below the clip is reachable from any y in the window |
| `the_sidebar_walk_refuses_a_coordinate_that_is_not_in_the_list` | The caption, the status bar, NaN, ±∞, and an empty list |
| `the_click_and_the_hover_agree_on_where_the_sidebar_is` | 60 probes down the whole column: the two must name the same row *and* agree on whether the sidebar consumed the pointer |

127 → 135 tests, all green. rustfmt clean; clippy unchanged at 13 warnings for
the crate with `--all-targets` (measured `git stash`-bracketed, so the number
is a delta and not a snapshot) — all `arithmetic_side_effects` on `f32` in
production code, pre-existing.

#### One case deliberately *not* added to the reintroduction sweep

`DiskSidebar::row_at` checks `!offset.is_finite() || offset < 0.0` before the
cast, and that check is **unreachable today**: the `y >= self.data_top &&
y < self.bottom` test immediately above it already rejects every input that
could make the offset negative or NaN. Removing it therefore leaves the suite
green, and a sweep entry for it would report a green that means nothing — which
is worse than no entry. It stays in the code as a guard at the cast rather than
a guard somewhere else, and it stays out of the sweep, and the doc comment on
`row_at` says both. The sweep instead pins the *pair* of guards: removing both
is caught, which is the edit that would actually reintroduce the fault.

`scripts/reintro-row-hit-tests.py` is now **58 defects** (was 49), nine of them
new here: the row walk answering above its own first row; the renderer drawing
3 px below where rows answer; the renderer pacing rows by the height it paints
rather than their pitch (the classic gap-vs-pitch slip, which accumulates 2 px
per row and so is invisible at the top of the list); the clip stopping a row
short; no right-hand bound on the click; the same on the hover; a caption click
falling through to the disk map; and the gap and the inset each being treated
as a boundary rather than as decoration.

**With this, every list in `apps/partmanager/src/main.rs` — the partition list,
the operation queue and the disk sidebar — has exactly one description of where
its rows are, and each has a reintroduction case proving the suite can see it
being undone.**
