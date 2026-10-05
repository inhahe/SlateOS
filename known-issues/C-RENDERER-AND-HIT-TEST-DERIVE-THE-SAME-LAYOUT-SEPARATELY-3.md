### C-RENDERER-AND-HIT-TEST-DERIVE-THE-SAME-LAYOUT-SEPARATELY — credmanager struck off, and it was hiding two dead controls — 2026-08-26 — LANE C

`apps/credmanager` is off the list on the same terms as benchmark: one `Layout`
built from the live window size, and a hit box recorded for every control **as
it is drawn**, through `guitk::frame::Frame`. Four hand-written hit tests are
gone — `handle_toolbar_click`, `handle_sidebar_click`, `handle_list_click` and
an empty `handle_detail_click` — replaced by a single `match self.target_at(x,
y)`.

**What the second derivation was actually hiding.** This app is the one that
shows the failure is not theoretical drift, because two of its controls were
not merely at risk of disagreeing — they had already lost the argument:

- **Every folder and every tag in the sidebar was drawn as a selectable row and
  wired to nothing.** `handle_sidebar_click` walked down the pane re-summing
  `y_start + 12.0 + 30.0 + 24.0 + 12.0 + 20.0 + …` and had arms for the
  Categories and Types groups only. The Folders and Tags groups were painted,
  highlighted on selection, and had no counterpart block at all. Clicking one
  did nothing, silently. Nothing failed, because nothing tested a control the
  click handler had never heard of.
- **The lock screen's Unlock button was decoration.** `handle_mouse` returned
  before doing anything while the vault was locked, so the only way into the
  vault was the Enter key. A painted, labelled, inert button is worse than no
  button: it tells the user the pointer is the way in.

Both are fixed by the conversion rather than *by* a fix — a drawn row is a
clickable row by construction now — and both have a test that says so
(`every_sidebar_row_selects_the_item_the_renderer_drew_there`,
`the_unlock_button_unlocks_the_vault`).

**The lesson worth carrying to the rest of the list.** A hand-written hit test
does not just risk drifting from the renderer; it risks never having covered
part of it. Drift is what you get when both sides are written and one changes.
This was the other failure: one side was written and the other was *not
finished*, and no amount of care in the arithmetic would have found it, because
the arithmetic was right about the rows it knew about. What finds it is not
having two sides.

**A measuring pass costs nothing and removes a `&mut`.** `build_render_tree`
took `&mut AppState` for one reason: to write the detail panel's measured
content height back into a cached field. That made the scroll bound zero until
the first frame had gone out — the wheel was dead in a freshly-opened panel —
and it is incompatible with `Probe::draw`, which takes `&self` precisely so a
test cannot be fooled by a renderer that mutates. The field is deleted;
`detail_content_height()` draws into a scratch `Frame` and reads the height
back. `the_detail_panel_cannot_be_scrolled_before_it_has_been_measured` now
passes for the opposite reason it used to: there is no "before".
