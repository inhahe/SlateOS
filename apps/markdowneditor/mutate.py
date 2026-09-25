"""Mutation test for the markdown editor's pointer layer and the operations it
made reachable.

Breaks one piece of production code at a time and checks that the test which
claims to cover it is the one that fails.  A test that passes against a broken
program is not testing the program.

The editor drew a toolbar, a tab bar, a table of contents, a find panel and
three dialogs, and handled no pointer event of any kind -- `known-issues.md`,
`TD-C-TWENTY-ONE-APPLICATIONS-DRAW-A-UI-THAT-CANNOT-BE-CLICKED`.  Asking of
each thing it offered "can anything reach this?" found more than the missing
pointer:

  * the view mode, the table of contents, the templates, Save As and the tabs
    had no key either, so nothing at all could reach them;
  * the HTML export computed its HTML and dropped it (`let _ = html;`);
  * a document's tab could not be closed, and the window closed over unsaved
    work without asking;
  * the file-changed-on-disk check had no caller, so the prompt, the merge
    and the review behind it were never raised -- and a deleted file was
    offered a "Reload from disk" that did nothing;
  * the selection anchor had four writers that cleared it and none that set
    it, so Bold, Italic and Link could never wrap a selection;
  * and undoing a delete that spanned lines put the newlines *inside* one line.

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
    # -- the toolbar ----------------------------------------------------------
    (
        "a toolbar button is drawn and records no hit box",
        "        draw_toolbar_button(f, pal, rect, &button.label, hover == Some(target));\n"
        "        f.hit(target, rect);\n",
        "        draw_toolbar_button(f, pal, rect, &button.label, hover == Some(target));\n",
        ["every_toolbar_button_does_what_its_action_says"],
    ),
    (
        "a separator answers as the New button again",
        "    for sx in &layout.separators {\n",
        "    for sx in &layout.separators {\n"
        "        f.hit(\n"
        "            Target::Toolbar(ToolbarAction::NewFile),\n"
        "            Rect::new(*sx, y, TOOLBAR_SEPARATOR_W, TOOLBAR_HEIGHT),\n"
        "        );\n",
        ["the_gap_between_toolbar_groups_is_not_a_button"],
    ),
    (
        "a narrow toolbar runs off the window instead of offering a chevron",
        "    let right = if natural <= width {",
        "    let right = if true {",
        ["a_narrow_window_offers_its_missing_buttons_through_the_chevron"],
    ),
    (
        "hovering a button says nothing",
        "        if let Some(tip) = tip {\n            return StatusNote::Tip(tip);\n        }\n",
        "",
        ["hovering_a_button_names_it_in_the_status_bar"],
    ),
    # -- tabs and closing -----------------------------------------------------
    (
        "a tab is drawn and records no hit box",
        "        f.hit(Target::Tab(slot.index), rect);\n",
        "",
        ["a_tab_comes_forward_and_its_close_button_closes_it"],
    ),
    (
        "the tab in front can scroll out of the tab bar",
        "    while first < active {",
        "    while false && first < active {",
        ["many_tabs_keep_the_front_one_visible_and_list_the_rest"],
    ),
    (
        "a document with unsaved changes closes without asking",
        "            Some(doc) if doc.modified => self.close_prompt = Some(CloseScope::Tab(idx)),",
        "            Some(doc) if false && doc.modified => {\n"
        "                self.close_prompt = Some(CloseScope::Tab(idx));\n"
        "            }",
        [
            "closing_an_unsaved_document_asks_and_each_answer_is_kept",
            "saving_an_untitled_document_on_close_asks_where_then_closes_it",
        ],
    ),
    (
        "the window closes over unsaved work",
        "                if self.request_quit() {",
        "                if true {",
        ["closing_the_window_with_unsaved_work_asks_first"],
    ),
    (
        "the question is drawn into a window the loop has closed",
        "                } else {\n                    Response::KeepOpen\n                }",
        "                } else {\n                    Response::Redraw\n                }",
        ["closing_the_window_with_unsaved_work_asks_first"],
    ),
    (
        "closing with auto-save on saves nothing on the way out",
        "        if self.autosave_enabled {\n            self.save_every_titled_document();\n        }\n",
        "",
        ["closing_with_auto_save_on_saves_titled_documents_and_asks_about_the_rest"],
    ),
    (
        "a dialog lets a press through to the document behind it",
        "        f.hit(Target::ModalBackdrop, Rect::new(0.0, 0.0, width, height));\n",
        "",
        ["a_press_outside_a_dialog_does_not_reach_the_document"],
    ),
    (
        "Ctrl+Tab goes backwards",
        "        Key::Tab => cycle_tab(app, !shift),",
        "        Key::Tab => cycle_tab(app, shift),",
        ["ctrl_tab_and_ctrl_w_reach_the_tabs"],
    ),
    # -- the contents ---------------------------------------------------------
    (
        "a heading in the contents records no hit box",
        "        f.hit(target, row);\n\n        entry_y += TOC_ROW_H;",
        "\n        entry_y += TOC_ROW_H;",
        ["a_heading_in_the_contents_jumps_to_it"],
    ),
    (
        "the contents ignore their scroll position",
        "    for entry in entries.iter().skip(first) {",
        "    for entry in entries.iter().skip(first.min(0)) {",
        ["a_long_contents_scrolls_and_hides_what_scrolled_away"],
    ),
    # -- the find panel -------------------------------------------------------
    (
        "the replace box does not take the keyboard",
        "            Target::FindReplacement => self.find_state.focus_replacement = true,",
        "            Target::FindReplacement => self.find_state.focus_replacement = false,",
        ["the_find_panel_answers_the_pointer"],
    ),
    (
        "match case is never drawn on",
        "            let lit = target == Target::FindMatchCase && state.case_sensitive;",
        "            let lit = false && target == Target::FindMatchCase && state.case_sensitive;",
        ["the_find_panel_draws_its_focus_and_its_case_switch"],
    ),
    # -- the source pane ------------------------------------------------------
    (
        "a press lands on the boundary after the one under it",
        "            if x - col_x(line, lo, text_x) <= col_x(line, hi, text_x) - x {",
        "            if false && x - col_x(line, lo, text_x) <= col_x(line, hi, text_x) - x {",
        [
            "the_column_under_a_point_is_the_one_drawn_there",
            "a_press_in_the_source_puts_the_caret_under_it",
        ],
    ),
    (
        "Shift is never known to be held",
        "                self.shift_held = key.modifiers.shift;",
        "                self.shift_held = false && key.modifiers.shift;",
        ["a_drag_selects_and_shift_press_extends"],
    ),
    (
        "the wheel over the source scrolls nothing",
        "                doc.scroll_line = doc.scroll_line.saturating_add_signed(rows).min(last);",
        "                doc.scroll_line = doc.scroll_line.saturating_add_signed(rows * 0).min(last);",
        ["the_wheel_scrolls_the_pane_under_it"],
    ),
    (
        "the preview scrolls past its top",
        "                    (doc.preview_scroll + wheel::pixels(dy, LINE_HEIGHT)).clamp(0.0, limit);",
        "                    doc.preview_scroll + wheel::pixels(dy, LINE_HEIGHT);",
        ["the_wheel_scrolls_the_pane_under_it"],
    ),
    (
        "a word is never more than the character pressed",
        "    if !is_word(here) {",
        "    if true || !is_word(here) {",
        ["word_bounds_find_words_and_single_characters"],
    ),
    # -- the status bar and the dialogs ----------------------------------------
    (
        "the auto-save label is not a switch",
        "            Target::Autosave => self.autosave_enabled = !self.autosave_enabled,",
        "            Target::Autosave => {}",
        ["the_status_bar_switches_the_view_and_auto_save"],
    ),
    (
        "the template chooser does not take the keyboard",
        "    if app.template_chooser_open {\n        return handle_template_key(app, key);\n    }\n",
        "",
        ["a_template_is_chosen_with_the_pointer_or_the_keys"],
    ),
    (
        "the shortcut card ignores a press",
        "            f.hit(Target::HelpCard, Rect::new(0.0, 0.0, width, height));\n",
        "",
        ["a_press_puts_the_shortcut_card_away"],
    ),
    (
        "coming back to the window checks nothing",
        "            GEvent::FocusIn => {\n                self.check_external_change();\n",
        "            GEvent::FocusIn => {\n",
        [
            "a_file_changed_elsewhere_is_noticed_on_focus_and_answered",
            "a_deleted_file_offers_keep_or_close",
            "the_merge_review_is_answered_by_pointer_and_keys",
        ],
    ),
    (
        "a deleted file is reloaded from nowhere",
        "    if ExternalChoice::offered(&prompt.change)",
        "    if true || ExternalChoice::offered(&prompt.change)",
        ["a_deleted_file_offers_keep_or_close"],
    ),
    (
        "Accept leaves the review up",
        "            Target::ReviewAccept => self.review_accept(),",
        "            Target::ReviewAccept => {}",
        ["the_merge_review_is_answered_by_pointer_and_keys"],
    ),
    # -- files ------------------------------------------------------------------
    (
        "the export computes its HTML and writes nothing, again",
        "                match safeio::write_str_atomically(path, &html) {",
        "                match Ok::<(), std::io::Error>(()) {",
        ["the_html_export_writes_a_file"],
    ),
    (
        "what a file operation did is never drawn",
        "            Some(FileNote::Done(msg)) => StatusNote::Info(msg),",
        "            Some(FileNote::Done(_)) => StatusNote::Stats,",
        ["the_html_export_writes_a_file"],
    ),
    # -- the document -----------------------------------------------------------
    (
        "a heading of the same level is stacked rather than taken off",
        "    let new_prefix = if is_heading && hashes == level {",
        "    let new_prefix = if false && is_heading && hashes == level {",
        ["ctrl_digits_set_a_heading_level"],
    ),
    (
        "a new heading level is stacked on the old one",
        "    if !old_prefix.is_empty() {",
        "    if false && !old_prefix.is_empty() {",
        ["ctrl_digits_set_a_heading_level", "a_heading_level_changes_in_one_undo_step"],
    ),
    (
        "typing leaves the selection in place",
        "            doc.delete_selection();\n            doc.insert_char(ch);",
        "            doc.insert_char(ch);",
        ["shift_arrows_select_and_typing_replaces_the_selection"],
    ),
    (
        "undo puts a multi-line delete back into one line",
        "        for piece in pieces {\n            at = at.saturating_add(1);",
        "        for piece in pieces.take(0) {\n            at = at.saturating_add(1);",
        ["a_multi_line_delete_undoes_and_redoes_as_lines"],
    ),
    (
        "redo erases a multi-line delete from the first line only",
        "            Some((_, last)) => (line.saturating_add(text.matches('\\n').count()), last.len()),",
        "            Some((_, last)) => (line, last.len()),",
        ["a_multi_line_delete_undoes_and_redoes_as_lines"],
    ),
    (
        "the selection's delete records the column it did not delete at",
        "        let start = self.clamp_position(start);\n        let end = self.clamp_position(end);\n"
        "        let deleted = self.text_between(start, end);\n        self.remove_range(start, end);\n"
        "        self.cursor_line = start.0;\n        self.cursor_col = start.1;\n"
        "        if !deleted.is_empty() {\n            self.push_undo(EditAction::Delete {\n"
        "                line: start.0,\n                col: start.1,",
        "        let raw = start;\n        let start = self.clamp_position(start);\n"
        "        let end = self.clamp_position(end);\n"
        "        let deleted = self.text_between(start, end);\n        self.remove_range(start, end);\n"
        "        self.cursor_line = start.0;\n        self.cursor_col = start.1;\n"
        "        if !deleted.is_empty() {\n            self.push_undo(EditAction::Delete {\n"
        "                line: start.0,\n                col: raw.1,",
        ["a_selection_off_a_boundary_undoes_in_place"],
    ),
]

if __name__ == "__main__":
    only = sys.argv[1:] or None
    raise SystemExit(sweep(SRC, MUTATIONS, "markdowneditor", timeout=600, only=only))
