"""Mutation test for the database browser's suite.

Breaks one piece of production code at a time and checks that the test which
claims to cover it is the one that fails.  A test that passes against a broken
program is not testing the program.

DB Viewer is the forty-ninth application in this campaign.  `main` was:

    fn main() {
        let app = DbViewerApp::new();
        let cmds = app.render(1200.0, 800.0);
        // In the real OS, these commands would be submitted to the compositor.
        // For now, just verify it produces output.
        assert!(!cmds.is_empty(), "Render should produce commands");
    }

It built a sample database, drew one frame at a size nobody had asked for,
checked that the `Vec` was not empty, dropped it and exited zero.  Nobody saw
the picture and nothing could be pressed.

What the wiring exposed, in rough order of how badly it would have shown:

  * **Every control in the program was a painted rectangle that answered
    nothing.**  The pass recorded no hit boxes at all, so Execute, New Tab, the
    three exports, Import, Filters, the database tabs and their close boxes, the
    object tree, the filter builder, the column headers that sort, the per-row
    delete boxes, the pagination arrows, the four bottom-panel tabs and the
    query history were all pictures of controls.
  * **The layout was arithmetic on constants with nothing between it and a
    small window.**  `content_height`, `grid_height` and `main_width` all went
    negative; `SIDEBAR_WIDTH` was the only width the sidebar was ever drawn at,
    so a 200-point window handed the data grid a width of -20.
  * **The toolbar started at a hard-coded `x = 130`** and walked right without
    asking how wide the window was, so at 400 points across Import was painted
    past the edge.  The database tab strip did the same.
  * **Three cursors tested the wrong edge.**  The sidebar tree, the filter list
    and the grid's rows each asked `ny > y + height` at the *top* of an item, so
    an item straddling the bottom edge was drawn whole -- which in the grid
    meant a 26-point band of row colour painted over the pagination bar, and in
    the sidebar meant the only way to remove a filter pushed off the window.
  * **Grid columns were drawn at a fixed 140 points** and swallowed by the clip:
    a table that was showing three of its five columns looked complete.
  * **`delete_row` indexed the table** while the screen showed filtered and
    sorted rows, so deleting the third row on screen removed whatever was third
    in insertion order -- silently, with a success message.
  * **The schema pane's three columns were 180, 100 and 200 points wide**
    whatever the pane measured, so `Constraints` was routinely drawn outside it
    and the clip made the table look unconstrained.
  * **The diagram laid its table boxes in one unbounded row** at `ti * 200`, so
    the third table in a 400-point pane was drawn entirely outside it.
  * **The status bar placed its three readings at `x + 10`, `x + 200` and
    `x + width - 150`** whatever the window measured, so in a narrow window all
    three were drawn on top of each other.
  * **Nothing outside the tests reached** `toggle_sort`, `select_table`,
    `delete_row`, `remove_filter`, `add_filter`, `toggle_favorite`,
    `export_json`, `export_sql_inserts`, `import_csv_data`, `filter_column_idx`,
    `filter_op_idx`, `filter_value`, `show_filter_builder` or `bottom_panel`.
    `export_current_table` returned a `String` the program had nowhere to put.
  * **Twelve crate-level `#![allow]`s** covered the file; not one silenced
    anything the crate still trips.

One more was found by the wiring tests after that, and it was the worst of
them, because the program looked entirely correct:

  * **The query history was a title over an empty strip, in every window there
    has ever been.**  `draw_sql_editor` asked for an 80-point editor box.  The
    editor draws a single run of tokens -- no wrapping, no second line, no way
    to put a caret on one -- so 64 of those points were empty box, and in the
    114-point pane the layout actually gives it they were enough to push every
    history row past the bottom edge.  `HISTORY (n queries)` was painted over
    nothing; and because `Frame::hit` drops a box the clip trims away,
    `HistoryEntry` and `FavoriteEntry` were controls that did not exist.  No
    query could be recalled and none could be starred.  The heading now says
    `HISTORY (3 of 20 shown)` when it is holding queries back, the same honesty
    the grid's column caption and the diagram's heading already keep.

Run it with no arguments to sweep everything, or with substrings of the
mutation names to run only those.
"""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "scripts"))

from mutation_harness import sweep  # noqa: E402  (path set above)

SRC = Path(__file__).parent / "src" / "main.rs"

# (name, old, new, [tests that must fail])
MUTATIONS = [
    # -- the layout ----------------------------------------------------------
    (
        "a window size that is not a number is laid out rather than zeroed",
        "        let w = if w.is_finite() { w.max(0.0) } else { 0.0 };\n"
        "        let h = if h.is_finite() { h.max(0.0) } else { 0.0 };",
        "        let w = w.max(0.0);\n        let h = h.max(0.0);",
        ["a_window_of_no_size_and_a_window_of_nonsense_size_draw_without_panicking"],
    ),
    (
        "the toolbar is given its full height before the status line is placed",
        "        let status_h = STATUS_BAR_HEIGHT.min(h);\n"
        "        let status = Rect::new(0.0, (h - status_h).max(0.0), w, status_h);\n"
        "        let bottom = status.y;\n\n"
        "        let toolbar_h = TOOLBAR_HEIGHT.min(bottom);",
        "        let toolbar_h = TOOLBAR_HEIGHT.min(h);\n"
        "        let status_h = STATUS_BAR_HEIGHT.min(h);\n"
        "        let status = Rect::new(0.0, (h - status_h).max(toolbar_h), w, status_h);\n"
        "        let bottom = status.y;",
        ["every_run_of_text_is_bounded_and_inside_the_window"],
    ),
    (
        "the tab strip is taken without leaving the grid a row",
        "        let tabs_h = if toolbar_h + TAB_HEIGHT + MIN_GRID_HEIGHT <= bottom {",
        "        let tabs_h = if toolbar_h + TAB_HEIGHT <= bottom {",
        ["the_bottom_panel_and_the_tab_strip_are_given_up_before_the_grid_is"],
    ),
    (
        "the bottom panel is taken without leaving the grid a row",
        "        let panel_h = if content_h - EDITOR_HEIGHT >= MIN_GRID_HEIGHT {",
        "        let panel_h = if content_h > 0.0 {",
        ["the_bottom_panel_and_the_tab_strip_are_given_up_before_the_grid_is"],
    ),
    (
        "the sidebar is taken without leaving the grid room for a table",
        "        let side_w = if wanted_side >= MIN_SIDEBAR_WIDTH && w - wanted_side >= MIN_GRID_WIDTH {",
        "        let side_w = if wanted_side > 0.0 {",
        ["the_sidebar_is_given_up_before_the_grid_falls_below_a_table"],
    ),
    (
        "the bottom panel is run down over the status strip",
        "        let panel = Rect::new(main_x, content_y + grid_h, main_w, panel_h);",
        "        let panel = Rect::new(main_x, content_y + grid_h, main_w, panel_h + 20.0);",
        ["nothing_is_drawn_over_the_status_line"],
    ),
    (
        "the pagination bar is taken off the bottom of the grid unconditionally",
        "        let h = PAGE_BAR_HEIGHT.min((self.grid.h - HEADER_HEIGHT).max(0.0));",
        "        let h = PAGE_BAR_HEIGHT.min(self.grid.h);",
        ["the_pagination_bar_never_rises_above_its_own_header"],
    ),
    # -- what is painted -----------------------------------------------------
    (
        "only the toolbar is filled, leaving the rest of the window bare",
        "        f.push(fill(l.window, BASE, 0.0));",
        "        f.push(fill(l.toolbar, BASE, 0.0));",
        ["the_window_is_painted_edge_to_edge_at_every_size"],
    ),
    (
        "text is drawn unbounded, so a long table name walks over the column beside it",
        "        max_width: Some(ink.w),",
        "        max_width: None,",
        ["every_run_of_text_is_bounded_and_inside_the_window"],
    ),
    (
        "a run taller than its box is centred in it anyway",
        "fn ink_box(r: Rect, size: f32) -> Rect {\n    let size = size.min(r.h);",
        "fn ink_box(r: Rect, size: f32) -> Rect {",
        ["a_run_of_text_is_never_inked_outside_the_box_it_was_given"],
    ),
    (
        "text is emitted wherever the caller asks, clip or no clip",
        "    if ink.is_empty() || s.is_empty() || !f.is_visible(ink) {",
        "    if false {",
        ["every_run_of_text_is_bounded_and_inside_the_window"],
    ),
    (
        "the toolbar title is given a hundred points whatever the window measures",
        "        let title_w = (area.right() - bx).clamp(0.0, 100.0);",
        "        let title_w = 100.0_f32;",
        ["every_run_of_text_is_bounded_and_inside_the_window"],
    ),
    (
        "the toolbar keeps drawing buttons past the right-hand edge",
        "            if btn.is_empty() || btn.right() > area.right() {\n                break;",
        "            if btn.is_empty() {\n                break;",
        ["no_pass_paints_outside_the_box_it_was_given"],
    ),
    (
        "the tab strip keeps drawing tabs past the right-hand edge",
        "            if cell.is_empty() || cell.right() > area.right() {",
        "            if cell.is_empty() {",
        ["no_pass_paints_outside_the_box_it_was_given"],
    ),
    (
        "the object tree draws a row that straddles the bottom edge whole",
        "                TREE_ROW_HEIGHT - 2.0,\n            );\n"
        "            if row.is_empty() || row.bottom() > area.bottom() {",
        "                TREE_ROW_HEIGHT - 2.0,\n            );\n"
        "            if row.is_empty() || row.y > area.bottom() {",
        ["no_pass_paints_outside_the_box_it_was_given"],
    ),
    (
        "the filter builder draws a row that straddles the bottom edge whole",
        "                TREE_ROW_HEIGHT - 4.0,\n            );\n"
        "            if row.is_empty() || row.bottom() > area.bottom() {",
        "                TREE_ROW_HEIGHT - 4.0,\n            );\n"
        "            if row.is_empty() || row.y > area.bottom() {",
        ["no_pass_paints_outside_the_box_it_was_given"],
    ),
    (
        "the schema pane heads a table it has no room to draw",
        "        let head = Rect::new(area.x + 8.0, cy, (area.w - 16.0).max(0.0), 18.0);\n"
        "        if head.bottom() > area.bottom() {",
        "        let head = Rect::new(area.x + 8.0, cy, (area.w - 16.0).max(0.0), 18.0);\n"
        "        if head.y > area.bottom() {",
        ["no_pass_paints_outside_the_box_it_was_given"],
    ),
    (
        "the grid keeps drawing rows after the cursor has run past the bottom",
        "            let line = Rect::new(rows_area.x, ry, area.w, ROW_HEIGHT);\n"
        "            if line.bottom() > rows_area.bottom() {\n                break;\n            }",
        "            let line = Rect::new(rows_area.x, ry, area.w, ROW_HEIGHT);",
        # Not a text or control test: `put_text` refuses to emit a run the clip
        # cannot show and `Frame::hit` drops a hit box the clip trims to
        # nothing, so a row drawn past the bottom of the grid is invisible
        # *and* unclickable. What survives is the paint -- the row's own fill,
        # pushed unconditionally and landing wholly outside the clip.
        ["nothing_is_painted_entirely_outside_the_clip_in_force"],
    ),
    (
        "the grid's row clip is opened and never closed",
        "        f.unclip();\n\n        // --- pagination bar ---",
        "\n        // --- pagination bar ---",
        ["the_frame_is_balanced_at_every_size_and_state"],
    ),
    (
        "the bottom panel's clip is pushed a row deeper than the panel",
        "        f.clip(body);",
        "        f.clip(Rect::new(body.x, body.y, body.w, body.h + 60.0));",
        ["every_clip_lies_inside_the_window"],
    ),
    (
        "the editor box asks for the eighty points it used to, and buries the history",
        "        let box_h = (line_h + 8.0).min((area.h - 24.0).max(0.0));",
        "        let box_h = 80.0_f32.min((area.h - 24.0).max(0.0));",
        ["the_history_heading_is_never_a_title_over_an_empty_strip"],
    ),
    # -- what the picture says about itself ----------------------------------
    (
        "the grid's caption does not say it is hiding a column",
        "        if hidden > 0 {",
        "        if false {",
        ["the_grid_says_how_many_columns_it_is_showing_when_it_cannot_show_them_all"],
    ),
    (
        "the diagram's heading does not say it is hiding a table",
        "        if placed.len() < total {",
        "        if false {",
        ["the_diagram_wraps_its_boxes_and_says_how_many_it_is_showing"],
    ),
    (
        "the history heading counts the queries rather than the rows it drew",
        '            &if placed.len() == total {\n'
        '                format!("HISTORY ({total} queries)")\n'
        '            } else {\n'
        '                format!("HISTORY ({} of {total} shown)", placed.len())\n'
        "            },",
        '            &format!("HISTORY ({total} queries)"),',
        ["the_history_heading_is_never_a_title_over_an_empty_strip"],
    ),
    # -- where a press goes --------------------------------------------------
    (
        "every toolbar button executes the query",
        "            f.hit(*target, btn);",
        "            f.hit(Target::Execute, btn);",
        [
            "the_filters_button_shows_and_hides_the_builder",
            "export_puts_the_table_in_the_editor_and_import_reads_it_back",
        ],
    ),
    (
        "the close box is recorded before the tab, so the tab swallows it",
        "            f.hit(Target::SelectTab(i), cell);\n"
        "            f.hit(Target::CloseTab(i), close);",
        "            f.hit(Target::CloseTab(i), close);\n"
        "            f.hit(Target::SelectTab(i), cell);",
        ["closing_a_tab_closes_the_one_pointed_at_not_the_active_one"],
    ),
    (
        "every tab in the strip selects the first database",
        "            f.hit(Target::SelectTab(i), cell);",
        "            f.hit(Target::SelectTab(0), cell);",
        ["the_plus_opens_a_database_and_the_strip_selects_between_them"],
    ),
    (
        "a category heading answers presses like the rows under it",
        "            if !is_header {\n                f.hit(Target::TreeNode(i), row);",
        "            if true {\n                f.hit(Target::TreeNode(i), row);",
        ["a_heading_names_a_category_and_is_not_a_control"],
    ),
    (
        "every row of the object tree selects the first node",
        "                f.hit(Target::TreeNode(i), row);",
        "                f.hit(Target::TreeNode(0), row);",
        ["a_tree_row_selects_the_table_it_names"],
    ),
    (
        "every cell of the filter builder takes the value",
        "            f.hit(target, row);\n            y += TREE_ROW_HEIGHT;",
        "            f.hit(Target::FilterValue, row);\n            y += TREE_ROW_HEIGHT;",
        ["the_builder_steps_its_column_and_its_comparison"],
    ),
    (
        "the x beside a filter adds one instead of removing it",
        "            f.hit(Target::RemoveFilter(fi), remove);",
        "            f.hit(Target::AddFilter, remove);",
        ["the_builder_takes_a_value_adds_a_filter_and_the_x_takes_it_away"],
    ),
    (
        "every column header sorts the first column",
        "                f.hit(Target::SortColumn(ci), cell);",
        "                f.hit(Target::SortColumn(0), cell);",
        ["a_column_header_sorts_then_reverses_then_the_arrow_says_which"],
    ),
    (
        "the delete box is named by the row's place on the screen, not in the table",
        "            f.hit(Target::DeleteRow(*source_idx), del);",
        "            f.hit(Target::DeleteRow(ri), del);",
        ["deleting_a_row_with_a_sort_in_force_removes_the_row_pointed_at"],
    ),
    (
        "both pagination arrows turn the page forward",
        "            f.hit(target, btn);\n            bx = btn.x;",
        "            f.hit(Target::NextPage, btn);\n            bx = btn.x;",
        ["the_pagination_bar_turns_pages_and_stops_at_both_ends"],
    ),
    (
        "every panel tab shows the results panel",
        "            f.hit(Target::ShowPanel(*panel), cell);",
        "            f.hit(Target::ShowPanel(BottomPanel::Results), cell);",
        ["the_four_panel_tabs_each_show_their_own_panel"],
    ),
    (
        "the editor box is not a control",
        "        f.hit(Target::SqlEditor, editor);",
        "        f.hit(Target::Execute, editor);",
        ["the_editor_takes_typing_and_enter_runs_what_was_typed"],
    ),
    (
        "every history row recalls the newest query",
        "            f.hit(Target::HistoryEntry(i), row);",
        "            f.hit(Target::HistoryEntry(0), row);",
        ["the_history_puts_a_query_back_and_the_star_is_its_own_control"],
    ),
    (
        "the star is recorded before the row, so the row swallows it",
        "            f.hit(Target::HistoryEntry(i), row);\n"
        "            f.hit(Target::FavoriteEntry(i), star);",
        "            f.hit(Target::FavoriteEntry(i), star);\n"
        "            f.hit(Target::HistoryEntry(i), row);",
        ["the_history_puts_a_query_back_and_the_star_is_its_own_control"],
    ),
    # -- what the controls then do -------------------------------------------
    (
        "the last database tab is closed like any other",
        "            Target::CloseTab(i) => {\n                if self.tabs.len() <= 1 {",
        "            Target::CloseTab(i) => {\n                if false {",
        ["the_last_database_tab_stays_open"],
    ),
    (
        "the next-page arrow runs off the end of the table",
        "            let max_page = table.row_count().saturating_sub(1) / PAGE_SIZE;\n"
        "            if tab.page < max_page {\n"
        "                tab.page = tab.page.saturating_add(1);\n            }",
        "            tab.page = tab.page.saturating_add(1);",
        ["the_pagination_bar_turns_pages_and_stops_at_both_ends"],
    ),
    (
        "pressing a column header a second time sorts it the same way again",
        "                Some(s) if s.column_idx == col_idx => SortState {\n"
        "                    column_idx: col_idx,\n"
        "                    direction: match s.direction {\n"
        "                        SortDir::Ascending => SortDir::Descending,\n"
        "                        SortDir::Descending => SortDir::Ascending,\n"
        "                    },\n                },",
        "                Some(s) if s.column_idx == col_idx => SortState {\n"
        "                    column_idx: col_idx,\n"
        "                    direction: s.direction,\n                },",
        ["a_column_header_sorts_then_reverses_then_the_arrow_says_which"],
    ),
    (
        "an import reads the file and leaves the old table on the screen",
        "                        self.select_table(&name);\n"
        '                        format!("Imported {name}")',
        '                        format!("Imported {name}")',
        ["export_puts_the_table_in_the_editor_and_import_reads_it_back"],
    ),
    (
        "an export is thrown away rather than put where it can be seen",
        "        Target::Export(format) => match self.export_current_table(format) {",
        "        Target::Export(_) => match None::<String> {",
        ["export_puts_the_table_in_the_editor_and_import_reads_it_back"],
    ),
    # -- the entry points the platform calls ---------------------------------
    (
        "the picture is drawn at the size it was launched with rather than the given one",
        "        self.frame(width, height).into_tree()",
        "        self.frame(WINDOW_WIDTH, WINDOW_HEIGHT).into_tree()",
        ["the_picture_is_drawn_at_the_size_render_is_given"],
    ),
    (
        "the size the frame was drawn at is not remembered, so a press is answered blind",
        "    fn render(&mut self, width: f32, height: f32) -> RenderTree {\n"
        "        self.window_width = width;\n        self.window_height = height;",
        "    fn render(&mut self, width: f32, height: f32) -> RenderTree {",
        ["render_remembers_the_size_it_was_asked_for"],
    ),
    (
        "a resize is not remembered, so a press before the next frame is answered blind",
        "                self.window_width = *width as f32;\n"
        "                self.window_height = *height as f32;",
        "                let _ = (width, height);",
        ["a_press_is_answered_against_the_size_the_last_frame_was_drawn_at"],
    ),
    (
        "the close button is answered with a redraw",
        "            Event::CloseRequested => Response::Exit,",
        "            Event::CloseRequested => Response::Redraw,",
        ["the_close_button_closes_the_window_and_nothing_else_does"],
    ),
    (
        "the window asks for a redraw on a timer it has no use for",
        "        // A database browser changes when someone asks it something, and at no\n"
        "        // other time. There is no clock on the screen to keep.\n        None",
        "        Some(Duration::from_millis(16))",
        ["the_window_says_what_it_is"],
    ),
]

if __name__ == "__main__":
    only = sys.argv[1:] or None
    raise SystemExit(sweep(SRC, MUTATIONS, "dbviewer", timeout=420, only=only))
