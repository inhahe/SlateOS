"""Mutation test for the JSON viewer's saving, its clickable chrome, and asking
before unsaved work is lost.

Breaks one piece of production code at a time and checks that the test which
claims to cover it is the one that fails.  A test that passes against a broken
program is not testing the program.

It could not save at all: the tree changed values and deleted nodes and marked
the tab modified, and every change went when the window closed -- which it did
on any close request, without asking.  Its toolbar's buttons and its tabs'
close marks were drawn and answered nothing.  The table covers what was added
for that; the rest of the suite predates it.

Run it with no arguments to sweep everything, or with substrings of the
mutation names to run only those.
"""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "scripts"))

from mutation_harness import sweep  # noqa: E402  (path set above)

SRC = Path(__file__).parent / "src" / "main.rs"

CLOSE = "closing_the_window_over_unsaved_work_asks_and_each_answer_is_kept"
LAYOUT = "a_tree_edit_keeps_the_texts_layout"
ENTER = "enter_in_the_raw_view_edits_the_text_itself"
REPAIR = "a_document_that_does_not_parse_is_repaired_where_it_goes_wrong"
NEW_DOC = "a_new_document_can_be_typed_into"
ESCAPE = "escape_stops_editing_and_keeps_the_changes"
TYPED = "a_typed_key_is_the_texts_while_it_is_edited"
PRESS = "a_press_in_the_raw_view_puts_the_caret_where_it_lands"
PRESS_ERROR = "a_press_on_a_parse_error_goes_to_it"
SCROLLED = "a_press_after_scrolling_lands_on_the_line_under_it"
COMMIT = "a_value_being_typed_over_is_committed_before_the_text_is_edited"
LEAVE = "leaving_the_raw_view_stops_editing"
FIND = "the_find_bar_takes_the_keys_from_the_text"
CARET = "the_caret_stays_on_screen"
TAB = "tab_indents_in_the_texts_own_step"
CUT = "cut_and_paste_within_the_text"
CAP = "the_text_stops_where_a_reopened_file_would_be_cut"
SAVE = "what_is_typed_is_what_is_saved"
WHEEL = "a_notch_of_the_wheel_moves_a_view_its_rows"
BYTE_AT = "a_line_and_column_become_a_byte"
TRUNC = "the_truncation_warning_leads_with_the_word_that_matters"

# (name, old, new, [tests that must fail])
MUTATIONS = [
    (
        "saving writes nothing",
        "        match safeio::write_atomically(&path, doc.input.as_bytes()) {",
        "        match Ok::<(), std::io::Error>(()) {",
        ["an_edited_file_is_saved_back_to_itself"],
    ),
    (
        "a file read in part is saved over, and cut short",
        "        if let Some(whole) = doc.whole_len {",
        "        if let Some(whole) = None::<usize> {",
        ["a_file_read_only_in_part_is_never_saved_over"],
    ),
    (
        "a file saved in full still counts as read in part",
        "                doc.whole_len = None;\n",
        "",
        ["a_file_read_only_in_part_is_never_saved_over"],
    ),
    (
        "a tree edit pretty-prints a one-line file",
        r"        let one_line = !self.input.trim().contains('\n');",
        "        let one_line = false;",
        ["a_one_line_file_is_saved_on_one_line", LAYOUT],
    ),
    (
        "a tree edit re-indents the file at two spaces",
        "                IndentStyle::detect(&self.input).unwrap_or(IndentStyle::Spaces2),",
        "                IndentStyle::Spaces2,",
        ["an_edited_file_is_saved_back_to_itself", LAYOUT],
    ),
    (
        "a tree edit drops the final newline",
        r"            (true, false) => text.push('\n'),",
        "            (true, false) => {}",
        [LAYOUT],
    ),
    (
        "a tree edit adds a final newline the text lacked",
        "                text.pop();\n",
        "",
        [LAYOUT],
    ),
    (
        "Ctrl+W closes a modified tab without asking",
        "                    self.request_close_tab(self.active_tab);",
        "                    self.close_tab(self.active_tab);",
        ["a_tab_with_unsaved_changes_asks_before_it_closes"],
    ),
    (
        "the window closes over unsaved work",
        "        if self.documents.iter().any(|d| d.dirty) {",
        "        if false {",
        [CLOSE],
    ),
    (
        "the question is drawn into a window the loop has closed",
        "            } else {\n                Response::KeepOpen\n            };",
        "            } else {\n                Response::Redraw\n            };",
        [CLOSE],
    ),
    (
        "keys and clicks reach the document under the question",
        "        if let Some(question) = self.question.as_mut()\n"
        "            && matches!(event, Event::Key(_) | Event::Mouse(_))",
        "        if let Some(question) = self.question.as_mut()\n"
        "            && false",
        [CLOSE, "a_tab_with_unsaved_changes_asks_before_it_closes"],
    ),
    (
        "saving the untitled document does not carry on closing",
        "                Ok(_) => self.continue_quitting(),",
        "                Ok(_) => {}",
        ["saving_on_close_writes_the_files_and_asks_where_for_the_untitled"],
    ),
    (
        "a value being typed is dropped when the window closes",
        "    fn request_quit(&mut self) -> bool {\n"
        "        if self.editing_path.is_some() {\n"
        "            self.commit_edit();\n"
        "        }\n",
        "    fn request_quit(&mut self) -> bool {\n",
        ["a_value_being_typed_when_the_window_closes_is_asked_about"],
    ),
    (
        "Ctrl+S saves without the value being typed",
        "        // A value half typed is part of what the user means to save.\n"
        "        if self.editing_path.is_some() {\n"
        "            self.commit_edit();\n"
        "        }\n",
        "",
        ["saving_keeps_the_value_being_typed"],
    ),
    (
        "Save As starts in $HOME",
        "            .map_or_else(FilePicker::default_start, Path::to_path_buf);",
        "            .map_or_else(FilePicker::default_start, |_| FilePicker::default_start());",
        ["save_as_starts_beside_the_documents_own_file"],
    ),
    (
        "the toolbar answers nothing",
        "                self.toolbar(action);",
        "                let _ = action;",
        ["every_toolbar_button_does_what_it_says"],
    ),
    (
        "a tab's close mark only selects the tab",
        "                self.request_close_tab(i);\n            } else {",
        "                self.switch_to(i);\n            } else {",
        ["a_tabs_close_mark_closes_that_tab_and_the_active_one_stays"],
    ),
    (
        "closing a tab before the active one moves the selection",
        "        if index < self.active_tab {\n            self.active_tab -= 1;",
        "        if false {\n            self.active_tab -= 1;",
        ["a_tabs_close_mark_closes_that_tab_and_the_active_one_stays"],
    ),
    (
        "a click on the find bar goes through to the tree",
        "        if self.search_visible\n            && (content_y..content_y + SEARCH_BAR_HEIGHT).contains(&y)",
        "        if false\n            && (content_y..content_y + SEARCH_BAR_HEIGHT).contains(&y)",
        ["the_find_bar_takes_its_own_clicks"],
    ),
    (
        "a value being typed follows the user into the next tab",
        "        if self.editing_path.is_some() {\n"
        "            self.commit_edit();\n"
        "        }\n"
        "        self.active_tab = index;",
        "        self.active_tab = index;",
        ["a_value_being_typed_stays_with_its_own_tab"],
    ),
    (
        "an empty tab shows the last tab's matches",
        "            _ => {\n"
        "                self.search_results.clear();\n"
        "                self.search_index = 0;",
        "            _ => {\n"
        "                self.search_index = 0;",
        ["the_matches_belong_to_the_document_on_screen"],
    ),
    (
        "the redraw test cannot see a tab close",
        "            tabs: self\n"
        "                .documents\n"
        "                .iter()\n"
        "                .map(|d| (d.title.clone(), d.dirty))\n"
        "                .collect(),",
        "            tabs: Vec::new(),",
        ["closing_a_tab_is_a_redraw_even_when_the_next_looks_the_same"],
    ),
    (
        "the redraw test cannot see the status line",
        "            note: self.note.clone(),",
        "            note: None,",
        ["an_edited_file_is_saved_back_to_itself"],
    ),
    # ---- the text itself, edited in the raw view (2026-09-26) ----
    (
        "Enter in the raw view does nothing",
        "            && (doc.view_mode == ViewMode::Raw\n                || (doc.view_mode == ViewMode::Tree && doc.parsed.is_none()))",
        "            && (doc.view_mode == ViewMode::Tree && doc.parsed.is_none())",
        [ENTER, ESCAPE, TYPED, TAB, CUT, SAVE],
    ),
    (
        "Enter in an empty or broken tree does nothing",
        "            && (doc.view_mode == ViewMode::Raw\n                || (doc.view_mode == ViewMode::Tree && doc.parsed.is_none()))",
        "            && doc.view_mode == ViewMode::Raw",
        [REPAIR, NEW_DOC],
    ),
    (
        "the caret starts at the start, not where the parse failed",
        "                .map_or(0, |e| byte_at(&doc.input, e.line, e.column));",
        "                .map_or(0, |_| 0);",
        [REPAIR],
    ),
    (
        "a typed ? raises the shortcut list while the text is edited",
        "        let typing = self.search_visible || self.editing_path.is_some() || editing_source;",
        "        let typing = self.search_visible || self.editing_path.is_some();",
        [TYPED],
    ),
    (
        "the text's keys go to the view",
        "        if editing_source {\n            self.source_key(ev);\n            return;\n        }",
        "        if false {\n            self.source_key(ev);\n            return;\n        }",
        [ENTER, TYPED],
    ),
    (
        "Escape does not stop editing",
        "        if ev.key == Key::Escape {\n            doc.source = None;\n            return;\n        }",
        "        if ev.key == Key::Escape {\n            return;\n        }",
        [ESCAPE],
    ),
    (
        "Tab is left to the text area, which ignores it",
        "        let edited = if ev.key == Key::Tab && !ev.modifiers.ctrl && !ev.modifiers.shift {",
        "        let edited = if false {",
        [TAB],
    ),
    (
        "Tab puts in two spaces whatever the text is written with",
        "            let step = IndentStyle::detect(source.area.text())",
        "            let step = Some(IndentStyle::Spaces2)",
        [TAB],
    ),
    (
        "a cut is not kept for a paste",
        "        if let Some(copied) = edited.copied {\n            self.clipboard = copied;\n        }",
        "        drop(edited.copied);",
        [CUT],
    ),
    (
        "the text grows past what a reopen would read",
        "        if source.area.text().len() > MAX_OPEN_BYTES {",
        "        if source.area.text().len() > usize::MAX - 1 {",
        [CAP],
    ),
    (
        "a refused key is kept anyway",
        "            source.area = before;\n",
        "            drop(before);\n",
        [CAP],
    ),
    (
        "a refused key says nothing",
        "            self.note = Some(format!(\n                \"Not added:",
        "            let _ = Some(format!(\n                \"Not added:",
        [CAP],
    ),
    (
        "an edit is not written back to the document",
        "            source.area.text().clone_into(&mut doc.input);",
        "",
        [ENTER, SAVE],
    ),
    (
        "an edit is not marked unsaved",
        "            source.area.text().clone_into(&mut doc.input);\n            doc.dirty = true;",
        "            source.area.text().clone_into(&mut doc.input);",
        [ENTER],
    ),
    (
        "an edit is not parsed again",
        "            doc.dirty = true;\n            doc.reparse();\n            doc.invalidate_caches();\n        }\n    }",
        "            doc.dirty = true;\n            doc.invalidate_caches();\n        }\n    }",
        [ENTER, REPAIR],
    ),
    (
        "an edit is not counted as a change",
        "            doc.reparse();\n            doc.invalidate_caches();\n        }\n    }",
        "            doc.reparse();\n        }\n    }",
        [ESCAPE],
    ),
    (
        "a key does not keep the caret in view",
        "        source.keep_caret_in_view(rows, width);\n        if edited.changed {",
        "        if edited.changed {",
        [CARET],
    ),
    (
        "a caret below the view is not scrolled to",
        "            self.scroll = line.saturating_add(1).saturating_sub(rows);",
        "            self.scroll = self.scroll;",
        [CARET],
    ),
    (
        "a caret above the view is not scrolled to",
        "        if line < self.scroll {\n            self.scroll = line;",
        "        if false {\n            self.scroll = line;",
        [CARET],
    ),
    (
        "a long line does not scroll sideways",
        "        } else if x + guitk::textedit::CARET_WIDTH > self.hscroll + width {",
        "        } else if false {",
        [CARET],
    ),
    (
        "sideways scrolling does not come back",
        "        if x < self.hscroll {\n            self.hscroll = (x - margin).max(0.0);",
        "        if false {\n            self.hscroll = (x - margin).max(0.0);",
        [CARET],
    ),
    (
        "the whole of a long line is sent to be drawn",
        "        if let Some(piece) = shown.get(from..to)",
        "        if let Some(piece) = shown.get(..)",
        [CARET],
    ),
    (
        "the line that fails is not marked",
        "                let bad = bad_line == Some(i);",
        "                let bad = bad_line == Some(usize::MAX);",
        [REPAIR],
    ),
    (
        "the parse's verdict is not said",
        '                format!("{error} -- Esc shows the formatted view"),',
        '                String::from("Esc shows the formatted view"),',
        [REPAIR],
    ),
    (
        "a press in the raw view does nothing",
        "                .is_some_and(|d| d.view_mode == ViewMode::Raw)\n        {\n            self.source_click(x, y - content_y);",
        "                .is_some_and(|d| d.view_mode == ViewMode::Diff)\n        {\n            self.source_click(x, y - content_y);",
        [PRESS, PRESS_ERROR, SCROLLED, COMMIT],
    ),
    (
        "a press puts the caret at the start",
        "        source\n            .area\n            .click(line, x - SOURCE_TEXT_X + source.hscroll, false);",
        "        source.area.move_to(0, false);",
        [PRESS, SCROLLED],
    ),
    (
        "a press lands as if the view were not scrolled",
        "        let line = source.scroll.saturating_add(row);",
        "        let line = row;",
        [SCROLLED],
    ),
    (
        "a press opens the text at its top, not where the view was",
        "            let at = byte_at(&doc.input, first.saturating_add(1), 1);",
        "            let at = 0;",
        [SCROLLED],
    ),
    (
        "a press on a parse error goes to the start",
        "                let at = byte_at(&doc.input, error.line, error.column);",
        "                let at = error.line.min(0);",
        [PRESS_ERROR],
    ),
    (
        "a value being typed over is left pending",
        "        if self.editing_path.is_some() {\n            self.commit_edit();\n        }\n        self.search_visible = false;",
        "        self.search_visible = false;",
        [COMMIT],
    ),
    (
        "editing the text leaves the find bar open",
        "        }\n        self.search_visible = false;\n        let (rows, width) = (self.source_rows(), self.source_width());",
        "        }\n        let (rows, width) = (self.source_rows(), self.source_width());",
        [FIND],
    ),
    (
        "the find bar opens over the text",
        "                    self.close_source();\n                    self.search_visible = !self.search_visible;",
        "                    self.search_visible = !self.search_visible;",
        [FIND],
    ),
    (
        "leaving the raw view keeps editing",
        "                    if *mode != ViewMode::Raw {\n                        doc.source = None;\n                    }",
        "",
        [LEAVE],
    ),
    (
        "a notch of the wheel is three pixels again",
        "            let step = wheel::pixels(dy, LINE_HEIGHT);",
        "            let step = -dy * 3.0;",
        [WHEEL],
    ),
    (
        "the wheel scrolls the text past its end",
        "                        source.scroll = source.scroll.saturating_add_signed(rows).min(last);",
        "                        source.scroll = source.scroll.saturating_add_signed(rows).max(last.min(0));",
        [WHEEL],
    ),
    (
        "the wheel does not move the text",
        "                ViewMode::Raw if doc.source.is_some() => {",
        "                ViewMode::Raw if doc.source.is_some() && false => {",
        [WHEEL],
    ),
    (
        "a line and column land a line late",
        "    for _ in 1..line {",
        "    for _ in 0..line {",
        [BYTE_AT, REPAIR],
    ),
    (
        "a column counts bytes",
        "    start.saturating_add(char_to_byte_pos(within, column.saturating_sub(1)))",
        "    start.saturating_add(column.saturating_sub(1).min(within.len()))",
        [BYTE_AT],
    ),
    (
        "past the last line is its start",
        "            None => return text.len(),",
        "            None => return start,",
        [BYTE_AT],
    ),
    (
        "opening reads the whole file whatever its size",
        "        let read = match safeio::read_to_string_capped(path, MAX_OPEN_BYTES) {",
        "        let read = match safeio::read_to_string_capped(path, usize::MAX / 2) {",
        [TRUNC],
    ),
]

if __name__ == "__main__":
    only = sys.argv[1:] or None
    raise SystemExit(sweep(SRC, MUTATIONS, "jsonviewer", timeout=900, only=only))
