"""Mutation test for the file search's pointer layer and what it made reachable.

Breaks one piece of production code at a time and checks that the test which
claims to cover it is the one that fails.  A test that passes against a broken
program is not testing the program.

The search drew a query box, three filter strips, four switches, a sortable
table and a pane of action buttons, and handled no pointer event
(`known-issues.md`, `TD-C-TWENTY-ONE-APPLICATIONS-DRAW-A-UI-THAT-CANNOT-BE-CLICKED`).
Asking of each thing it offered "can anything reach this?" found more:

  * Enter, listed on the F1 card as "Open what is selected", re-ran the search,
    and the preview's Open / Open Location / Copy Path / Properties buttons were
    wired to nothing -- the tool could find a file and do nothing with it;
  * the results were cut off at the panel's edge with no scrolling, and the
    keyboard could select rows that were never drawn;
  * the filters panel ran under the status bar;
  * every search was recorded on every keystroke into a history nothing
    showed, and saving a search had no caller;
  * and "now" was the constant 1_779_000_000, so "Today" was one day in May
    2026 for ever.

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
    (
        "a filter chip is drawn and records no hit box",
        "                    f.hit(target, Rect::new(rect.x + 4.0, rect.y, rect.w - 8.0, 22.0));\n",
        "",
        ["every_filter_answers_the_pointer"],
    ),
    (
        "the lit file-type chip does not turn itself off",
        "                self.criteria.category_filter = if self.criteria.category_filter == Some(cat) {",
        "                self.criteria.category_filter = if false && self.criteria.category_filter == Some(cat) {",
        ["every_filter_answers_the_pointer"],
    ),
    (
        "the mode chip is a label again",
        "        f.hit(Target::SearchMode, mode);\n",
        "",
        ["every_filter_answers_the_pointer"],
    ),
    (
        "the filters panel does not scroll",
        "            Some(target) if self.show_filters && target.is_in_filters() => {",
        "            Some(target) if false && self.show_filters && target.is_in_filters() => {",
        [
            "the_filters_panel_scrolls_to_its_last_row",
            "every_filter_answers_the_pointer",
            "a_saved_search_outlives_the_window",
            "a_recent_search_can_be_run_again",
        ],
    ),
    (
        "a column heading does not sort",
        "            f.hit(Target::Header(sort), rect);\n",
        "",
        ["a_column_heading_sorts"],
    ),
    (
        "a result row records no hit box",
        "            });\n            f.hit(target, row);\n\n            ry += RESULT_ROW_H;",
        "            });\n\n            ry += RESULT_ROW_H;",
        ["a_press_selects_the_result_under_it"],
    ),
    (
        "the selection walks off the bottom of the results",
        "            self.results_scroll = selected.saturating_sub(rows.saturating_sub(1));",
        "            let _ = rows;",
        ["results_past_the_panel_can_be_reached"],
    ),
    (
        "the wheel over the results scrolls nothing",
        "                    .saturating_add_signed(rows)",
        "                    .saturating_add_signed(rows * 0)",
        ["results_past_the_panel_can_be_reached"],
    ),
    (
        "a page step past the end refuses to move",
        "                let moved = pos.saturating_add(delta).max(0);",
        "                let moved = pos.saturating_add(delta).max(0);\n"
        "                if usize::try_from(moved).map_or(true, |m| m >= self.results.len()) {\n"
        "                    return EventResult::Ignored;\n"
        "                }",
        ["results_past_the_panel_can_be_reached"],
    ),
    (
        "a new search keeps the old scroll",
        "        self.results_scroll = 0;\n    }",
        "    }",
        ["results_past_the_panel_can_be_reached"],
    ),
    (
        "Enter re-runs the search again",
        "                if self.selected_result.is_some() {",
        "                if false && self.selected_result.is_some() {",
        [
            "enter_and_the_actions_open_what_is_selected",
            "a_search_is_remembered_when_it_is_used_not_as_it_is_typed",
        ],
    ),
    (
        "Open Location opens nothing",
        "            Target::OpenLocation => self.open_location(),",
        "            Target::OpenLocation => EventResult::Consumed,",
        ["enter_and_the_actions_open_what_is_selected"],
    ),
    (
        "a folder result is opened with a file's program",
        "            Some(FILE_MANAGER.to_string())\n        } else {\n            opener_for(&path)\n        };",
        "            opener_for(&path)\n        } else {\n            opener_for(&path)\n        };",
        ["a_folder_result_opens_in_the_file_manager"],
    ),
    (
        "the same search is listed twice",
        "        if let Some(pos) = self\n            .search_history\n            .iter()\n"
        "            .position(|s| s.query == query && s.mode == mode)\n        {\n"
        "            let mut again = self.search_history.remove(pos);",
        "        if let Some(pos) = self\n            .search_history\n            .iter()\n"
        "            .position(|s| false && s.query == query && s.mode == mode)\n        {\n"
        "            let mut again = self.search_history.remove(pos);",
        ["a_search_is_remembered_when_it_is_used_not_as_it_is_typed"],
    ),
    (
        "an unreadable saved search is dropped on the next write",
        "            .chain(self.unreadable_saved.iter().cloned())\n",
        "",
        ["an_unreadable_saved_search_is_written_back_as_it_was"],
    ),
    (
        "now is frozen again",
        "            current_time: unix_now(),",
        "            current_time: 1_779_000_000,",
        ["now_is_read_from_the_clock"],
    ),
    (
        "indexing keeps a stale clock",
        "        self.criteria.current_time = unix_now();",
        "        let _ = unix_now();",
        ["now_is_read_from_the_clock"],
    ),
    # -- content search -------------------------------------------------------
    (
        "content searches run on the window's thread",
        "        if criteria.mode == SearchMode::Content && !criteria.query.is_empty() {",
        "        if false && criteria.mode == SearchMode::Content && !criteria.query.is_empty() {",
        ["content_mode_searches_contents_not_names"],
    ),
    (
        "ext: is searched for as text",
        '            if let Some(ext) = word.strip_prefix("ext:") {',
        '            if let Some(ext) = word.strip_prefix("ext-never:") {',
        ["ext_and_in_words_narrow_the_search_and_leave_the_query"],
    ),
    (
        "the search ignores ext: and in:",
        "        self.content = None;\n        let criteria = self.criteria.effective();",
        "        self.content = None;\n        let criteria = self.criteria.clone();",
        ["a_search_typed_with_ext_and_in_finds_only_those_files"],
    ),
    (
        "a regex loses its ext:",
        "        if self.mode == SearchMode::Regex {\n            return out;\n        }",
        "",
        ["ext_and_in_words_narrow_the_search_and_leave_the_query"],
    ),
    (
        "a search in progress outlives a change of mode",
        "        // Whatever was being searched for, it is not what is being asked now.\n"
        "        self.content = None;\n",
        "",
        ["leaving_content_mode_stops_its_search"],
    ),
    (
        "a file over the limit is read anyway",
        "    if size > limit {",
        "    if false && size > limit {",
        ["a_file_too_large_to_read_is_counted_not_hidden"],
    ),
    (
        "text is compared without folding its case",
        "        return Ok(text.to_lowercase().contains(&query.to_lowercase()));",
        "        return Ok(text.contains(query));",
        [
            "content_folding_is_textual_for_text_and_exact_for_bytes",
            "content_mode_searches_contents_not_names",
        ],
    ),
    (
        "the window never takes in what the worker finds",
        "        self.content\n            .as_ref()\n            .map(|_| Duration::from_millis(CONTENT_POLL_MS))",
        "        None",
        ["the_clock_runs_only_while_contents_are_read"],
    ),
    (
        "the folder button is drawn and records no hit box",
        "        self.draw_button(f, folder, FOLDER_BUTTON_LABEL, Target::ChooseFolder);",
        "        self.draw_button(f, folder, FOLDER_BUTTON_LABEL, Target::SearchBox);",
        ["the_folder_button_asks_for_a_folder"],
    ),
    (
        "an AVIF is not a picture to the search",
        "        \"jpg\" | \"jpeg\" | \"png\" | \"gif\" | \"bmp\" | \"svg\" | \"ico\" | \"webp\" | \"avif\" | \"tif\"",
        "        \"jpg\" | \"jpeg\" | \"png\" | \"gif\" | \"bmp\" | \"svg\" | \"ico\" | \"webp\" | \"tif\"",
        ["test_categorize_image"],
    ),
    (
        'an announced change is not read',
        '            Event::SettingsChanged { group } if group.file_name() == CONFIG_NAME => {',
        '            Event::SettingsChanged { group } if false && group.file_name() == CONFIG_NAME => {',
        ['a_search_saved_in_another_window_reaches_this_one'],
    ),
    (
        "every program's announcement is read as this one's",
        '            Event::SettingsChanged { group } if group.file_name() == CONFIG_NAME => {',
        '            Event::SettingsChanged { group } if !group.file_name().is_empty() => {',
        ['a_search_saved_in_another_window_reaches_this_one'],
    ),
    (
        'a search forgotten elsewhere stays saved here',
        '            if search.is_bookmarked && !kept {',
        '            if false && search.is_bookmarked && !kept {',
        ['a_search_saved_in_another_window_reaches_this_one'],
    ),
    (
        'a search saved elsewhere that this window ran is not saved here',
        '                search.is_bookmarked = true;\n                search.name = Some(query);\n                continue;',
        '                continue;',
        ['a_search_saved_in_another_window_reaches_this_one'],
    ),
    (
        'a search saved elsewhere that this window ran is listed twice',
        '                .find(|s| s.mode == mode && s.query == query)',
        '                .find(|_| false)',
        ['a_search_saved_in_another_window_reaches_this_one'],
    ),
    (
        'an unreadable entry is kept once more at every reading',
        '        self.unreadable_saved.clear();\n',
        '',
        ['a_search_saved_in_another_window_reaches_this_one'],
    ),
    (
        'a re-read says it changed nothing',
        '        before != self.saved_search_items()\n    }',
        '        false\n    }',
        ['a_search_saved_in_another_window_reaches_this_one'],
    ),
    (
        'the filters scroll is left past its end by a re-read',
        '        self.keep_filters_in_reach();\n    }\n\n    /// Read the saved searches again',
        '    }\n\n    /// Read the saved searches again',
        ['the_filters_panel_is_not_left_scrolled_past_its_end'],
    ),
    (
        "the filters scroll is left past its end by forgetting a saved search",
        '            search.name = None;\n        }\n        self.keep_filters_in_reach();\n',
        '            search.name = None;\n        }\n',
        ['the_filters_panel_is_not_left_scrolled_past_its_end'],
    ),
    (
        'the filters scroll is left past its end by a taller window',
        '                self.keep_selection_visible();\n                self.keep_filters_in_reach();\n',
        '                self.keep_selection_visible();\n',
        ['the_filters_panel_is_not_left_scrolled_past_its_end'],
    ),
    (
        'keeping the filters scroll in reach keeps nothing',
        '            .min(self.filters_scroll_limit(l.filters));',
        '            .min(f32::MAX);',
        ['the_filters_panel_is_not_left_scrolled_past_its_end'],
    ),
]

FIELD = 'the_query_box_is_the_toolkits_field'
ALTGR = 'altgr_types_into_the_query_and_a_command_does_not'
CARET = 'the_caret_follows_the_query_in_its_box'

MUTATIONS += [
    # The query's box is the toolkit's field, with a caret, and takes what
    # AltGr types (2026-10-04; lane C, c-e-a-theme-can-shape-the-controls).
    (
        'the box never lights',
        '            hovered: open && self.hover == Some(Target::SearchBox),\n',
        '            hovered: false,\n',
        [FIELD],
    ),
    (
        'the box never has the keyboard',
        '            focused: open,\n',
        '            focused: false,\n',
        [FIELD],
    ),
    (
        'the box shows through the card',
        '        let open = !self.show_help && !self.picker.is_open();\n        field::State {\n',
        '        let open = !self.picker.is_open();\n        field::State {\n',
        [FIELD],
    ),
    (
        'a query that finds nothing is not red',
        '                && self.results.is_empty()\n                && self.content.is_none(),\n',
        '                && false,\n',
        [FIELD],
    ),
    (
        'the focus mark is the toolkit\'s width, not the user\'s',
        '        self.focus_ring_width = settings.focus_ring_width();\n',
        '        let _ = settings;\n',
        [FIELD],
    ),
    (
        'the caret is at the start of the query',
        '                cursor: self.query_caret().0,\n',
        '                cursor: guitk::text::TextCursor::from(0),\n',
        [CARET],
    ),
    (
        'AltGr+S sorts',
        '        let ctrl = textline::is_ctrl_chord(key.modifiers);\n',
        '        let ctrl = key.modifiers.ctrl;\n',
        [ALTGR],
    ),
    # No row for "a command's letter is typed": since 2026-10-04 the query's
    # typing is textline::apply_key's, which tells a command from AltGr itself,
    # in its own crate and with its own tests;
    # altgr_types_into_the_query_and_a_command_does_not still holds the query
    # to it.
]

# The query edits at a caret (2026-10-04,
# known-issues/E-twenty-nine-applications-type-only-at-the-end-of-a-box), and
# the two sorts that shared the clipboard's keys take Shift (design-decisions
# 1233).
EDITS = "the_query_edits_at_a_caret"
SORTS = "the_sorts_that_shared_the_clipboards_keys_take_shift"
BESIDE = "the_results_keep_their_keys_beside_the_query"
SHOWN = "the_query_edits_what_it_shows"
ARROWS = "the_arrows_walk_the_results_and_stop_at_the_ends"

MUTATIONS += [
    (
        "Ctrl+Shift+C does not sort by category",
        "            Key::C if ctrl && key.modifiers.shift => return self.sort_by(SortColumn::Category),\n",
        "",
        [SORTS],
    ),
    (
        "Ctrl+Shift+A does not sort by path",
        "            Key::A if ctrl && key.modifiers.shift => return self.sort_by(SortColumn::Path),\n",
        "",
        [SORTS],
    ),
    (
        "Ctrl+End is the query's",
        "            Key::End if ctrl => return self.select_edge(false),\n",
        "",
        [ARROWS],
    ),
    (
        "Ctrl+Home is the query's",
        "            Key::Home if ctrl => return self.select_edge(true),\n",
        "",
        [ARROWS],
    ),
    (
        "the query answers no key",
        "        if let Some(answered) = self.query_key(key) {\n            return answered;\n        }\n",
        "",
        [EDITS, BESIDE],
    ),
    (
        "a cut or a copy takes nothing to the clipboard",
        "            self.clipboard = copied;\n",
        "            let _ = copied;\n",
        [EDITS],
    ),
    (
        "an edit of the query is not searched for",
        "            self.criteria.query = self.query_editor.text().to_owned();\n"
        "            self.execute_search();\n",
        "            self.criteria.query = self.query_editor.text().to_owned();\n",
        [EDITS],
    ),
    (
        "every key the query answers is a redraw",
        "        Some(if after == before {\n",
        "        Some(if false {\n",
        [BESIDE],
    ),
    (
        "a key finds the editor holding another query",
        "        if self.query_editor.text() != self.criteria.query {\n"
        "            self.query_editor.set_text(&self.criteria.query);",
        "        if false {\n"
        "            self.query_editor.set_text(&self.criteria.query);",
        [SHOWN],
    ),
    (
        "the query's selection is not drawn",
        "                selection_anchor: self.query_caret().1,\n",
        "                selection_anchor: None,\n",
        [EDITS],
    ),
    (
        "a press puts the caret at the start",
        "            x - area.x,\n",
        "            0.0,\n",
        [EDITS],
    ),
    (
        "a press in the query's box does not place the caret",
        "                if target == Target::SearchBox {\n"
        "                    self.press_query(event.x);\n"
        "                    return EventResult::Consumed;\n"
        "                }\n",
        "",
        [EDITS],
    ),
]

if __name__ == "__main__":
    only = sys.argv[1:] or None
    raise SystemExit(sweep(SRC, MUTATIONS, "filesearch", timeout=600, only=only))
