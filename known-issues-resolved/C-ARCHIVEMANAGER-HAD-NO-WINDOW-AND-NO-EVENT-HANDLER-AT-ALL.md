## C-ARCHIVEMANAGER-HAD-NO-WINDOW-AND-NO-EVENT-HANDLER-AT-ALL (lane C, 2026-08-26) — ✅ FIXED 2026-08-26

**In short:** the Archive Manager drew a complete-looking window — toolbar, a
folder tree down the left, sortable columns, a file list, a progress strip — and
none of it could be touched. `fn main` was empty, so the program started and
immediately exited without ever opening a window. Every button existed only for
the unit tests to call. It now opens a real window and everything in it responds
to the mouse and the keyboard. What it still cannot do is *read an archive* —
see the entry below this one.

### What changed

1. **`oswindow` dependency and a real `fn main`.** `apps/archivemanager` now
   depends on `gui/window` and `main` ends in `app::launch("archivemanager",
   &mut state)`. `AppState` implements the `App` trait: `title` (which names the
   open archive), `app_id`, `initial_size`, `on_event`, `render`.

2. **One renderer, which records where it put things.** The eight `render_*`
   functions used to append to a `Vec<RenderCommand>`; they now take a `&mut
   Frame`, which is that vector plus a list of `(Target, Rect)` pairs recorded
   as the drawing happens. `AppState::hit_test` answers a click by *running the
   renderer* and reading the list back-to-front. There is no second copy of the
   layout arithmetic for the two to disagree about, which is the failure mode a
   separate `Layout` struct would have had in a 3,700-line running-`y` renderer.

3. **The hit list is clip-aware.** `Frame::push` watches for `PushClip`/`PopClip`
   and keeps a clip stack; `Frame::hit` trims each rect to the clip in force and
   drops it if it falls outside. Without this a row scrolled off the top of the
   list would still take clicks from the column header painted over it.

4. **Rows are addressed by identity, not by position.** `Target::FileRow` carries
   the entry's stable `id` and `Target::TreeRow` is resolved through the
   flattened tree at click time, because both lists reorder under the pointer —
   the file list when a column header is clicked, the tree when a node is
   expanded. `re_sorting_does_not_move_the_selection_to_a_different_file` is the
   test that pins this.

5. **`content_band` is the single copy of the vertical layout.** The height of
   the file list was computed once in the renderer and again wherever a scroll
   limit was needed. Those two copies disagreed as soon as the progress strip
   appeared, which would have let the last rows be scrolled behind it and stay
   there. `build_frame` now `debug_assert!`s its own `y` against `content_band`
   rather than recomputing it.

6. **Pre-existing clippy debt cleared.** The crate had 32 `arithmetic_side_effects`
   / `indexing_slicing` / `slicing` errors that predate this work — unchecked
   `+= 1` on every counter, `&parts[..parts.len() - 1]`, `self.nav_history[i]`.
   All are now `saturating_*`/`checked_*`/`get`/`split_last`. The one remaining
   suppression is on `get_or_create_child`, where every safe `Vec` accessor
   returns an `Option` and there is no honest `TreeNode` to put in the `None`
   arm; the index is established by a `push` two lines above.

### Two things the wiring work found and fixed

- **`navigate_back` and `navigate_forward` indexed `nav_history` directly.** A
  `nav_position` that ever got out of step with the history — which
  `navigate_to`'s truncate-then-push could produce — would have panicked the
  program rather than refusing to navigate. Both now read through `get` and
  return `false` on a miss.
- **Home and End moved the cursor by `isize::MIN / 2`.** "Move very far" is a
  worse way to say "go to the first row", and `-isize::MIN` is an overflow that
  only the `/ 2` was hiding. There is now a `set_cursor(index)` that Home and
  End name a row with directly.

### How the tests were written so they could fail

142 tests, all green. The 40 new interaction tests find their coordinates by
rendering and reading the recorded hit boxes back — `centre_of(&state, pred,
what)` panics rather than skipping if the control it wants was never drawn, so a
test cannot quietly pass against a program that has no such button. Four
deliberate mutations were run against the finished suite to check the tests
actually bite:

| Mutation | Test that caught it |
|---|---|
| `Frame::hit` records the untrimmed rect | `a_row_scrolled_off_the_top_is_not_clickable_where_it_used_to_be` |
| `hit_test` searches front-to-back | `the_tree_arrow_collapses_without_navigating` |
| `on_event` drops the resize scroll re-clamp | `resizing_updates_the_size_and_reclamps_both_scrolls` |
| `render` trusts the stored size instead of its argument | `the_first_frame_is_drawn_at_the_size_the_window_gives_it` |

`scripts/check-window-wiring.py` baseline lowered 124 → 123.
