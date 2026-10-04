"""Mutation test for the mind map's own file, its tabs, and asking before a
map is lost.

Breaks one piece of production code at a time and checks that the test which
claims to cover it is the one that fails.  A test that passes against a broken
program is not testing the program.

Ctrl+S wrote an outline -- the words and the branches, and none of the
colours, shapes, positions or folds -- and the window closed over unsaved maps
without a word.  One undo history served every map, and acted on whichever was
showing.  The tabs and the toolbar were drawn and answered no click.  The table
covers what replaced all of that; the rest of the suite predates it.

Each map's history is a tree now (C-Q24): a change after an undo keeps the
undone one as a branch, reached with Alt+Z.  Its rows cover the keys -- and
AltGr, which arrives as Ctrl+Alt, being taken for no chord and no plain key --
the marking, and the cap; the tree itself is the toolkit's to test.

Run it with no arguments to sweep everything, or with substrings of the
mutation names to run only those.
"""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "scripts"))

from mutation_harness import sweep  # noqa: E402  (path set above)

SRC = Path(__file__).parent / "src" / "main.rs"

ROUND = "a_map_saved_and_opened_again_is_the_same_map"
CARD = "the_shortcut_card_takes_every_key_and_press_while_it_is_up"
NUMBERS = "a_node_added_after_opening_takes_a_number_of_its_own"
CTRL_S = "ctrl_s_saves_over_the_maps_own_file_and_asks_only_when_it_has_none"
MARK = "a_change_marks_the_map_and_the_title_and_tab_say_so"
LAYOUT = "laying_the_map_out_again_marks_it_only_when_it_moves_a_node"
UNDO = "undo_on_one_map_never_touches_another"
CLOSE_MAP = "closing_a_changed_map_asks_and_each_answer_does_what_it_says"
SAVE_FAILS = "a_map_whose_save_fails_stays_open"
WINDOW = "closing_the_window_asks_about_every_unsaved_map"
QUIT_FAILS = "a_failed_save_keeps_the_window_open"
TYPED = "a_name_being_typed_is_asked_about_on_close"
TWICE = "opening_a_file_that_is_open_already_shows_its_tab"
KEEP_SHOWING = "closing_another_map_keeps_this_one_showing"
CONTENT = "a_file_is_opened_as_what_it_holds_not_what_it_is_called"
REFUSED = "a_map_file_that_cannot_be_read_whole_is_refused_and_says_why"
LARGE = "a_map_file_larger_than_an_open_reads_is_refused"
CUT = "an_outline_cut_short_ends_at_a_whole_line_and_says_so"
EXPORT = "exporting_an_outline_is_not_a_save"
STATUS = "the_status_line_says_what_the_last_save_did"
TABS = "the_tabs_answer_clicks"
TOOLBAR = "the_toolbar_buttons_answer_clicks"
CTRL_TAB = "ctrl_tab_goes_round_the_maps"
BESIDE = "a_press_beside_the_canvas_keeps_the_selection"
LAST = "closing_the_last_map_leaves_a_fresh_one"
NAMES = "a_new_map_is_not_named_after_one_still_open"
TREE = "a_change_after_an_undo_keeps_the_undone_map_reachable_with_alt_z"
CTRL_SHIFT_Z = "ctrl_shift_z_redoes"
ALTGR = "altgr_z_does_not_undo"
SUPER = "alt_z_with_the_windows_key_goes_nowhere"
GUARD = "a_key_held_with_altgr_alt_or_the_windows_key_is_no_shortcut"

# (name, old, new, [tests that must fail])
MUTATIONS = [
    # ---- the file ----
    (
        "a save writes nothing",
        "        match safeio::write_str_atomically(path, &mindmap_document(map).to_text()) {",
        "        match Ok::<(), std::io::Error>(()) {",
        [ROUND, CTRL_S],
    ),
    (
        "the file loses a node's colour",
        '        doc.set_str(&at("colour"), &colour_hex(node.color));\n',
        "",
        [ROUND],
    ),
    (
        "the file loses the folds",
        '        doc.set_bool(&at("collapsed"), node.collapsed);',
        '        doc.set_bool(&at("collapsed"), false);',
        [ROUND],
    ),
    (
        "the file loses the shapes",
        '        doc.set_str(&at("shape"), node.shape.label());',
        '        doc.set_str(&at("shape"), NodeShape::RoundedRect.label());',
        [ROUND],
    ),
    (
        "the file loses where a node was put",
        '        doc.set_f64(&at("x"), f64::from(node.x));',
        '        doc.set_f64(&at("x"), 0.0);',
        [ROUND],
    ),
    (
        "children come back in the wrong order",
        "        stack.extend(node.children.iter().rev().copied());\n    }\n    for (i, node) in order.iter().enumerate() {",
        "        stack.extend(node.children.iter().copied());\n    }\n    for (i, node) in order.iter().enumerate() {",
        [ROUND],
    ),
    (
        "new numbers start again at one",
        "            next: highest.saturating_add(1),",
        "            next: 1,",
        [NUMBERS],
    ),
    # ---- reading it back, whole or not at all ----
    (
        "a later format is read",
        "        Some(MINDMAP_FORMAT) => {}",
        "        Some(v) if v >= MINDMAP_FORMAT => {}",
        [REFUSED],
    ),
    (
        "a parent that is not there is taken",
        '                if !nodes.contains_key(&p) {\n                    return Err(bad("its parent is not written before it"));',
        '                if false {\n                    return Err(bad("its parent is not written before it"));',
        [REFUSED],
    ),
    (
        "a second root is taken",
        "                if root.is_some() {",
        "                if false {",
        [REFUSED],
    ),
    (
        "two nodes may share a number",
        "        if nodes.contains_key(&id) {",
        "        if false {",
        [REFUSED],
    ),
    (
        "a shape this version does not draw is taken",
        "            .find(|s| s.label() == shape_label)",
        "            .find(|s| s.label() == shape_label || true)",
        [REFUSED],
    ),
    (
        "a colour number past the palette is taken",
        "            .filter(|&n| usize::from(n) < NODE_COLORS.len())\n",
        "",
        [REFUSED],
    ),
    (
        "a node of no size is taken",
        "                if v > 0.0 {",
        "                if v >= 0.0 {",
        [REFUSED],
    ),
    (
        "a fold that says neither is taken as open",
        '            .get_bool(&at("collapsed"))\n            .ok_or_else(|| bad("whether it is folded is missing"))?;',
        '            .get_bool(&at("collapsed"))\n            .unwrap_or(false);',
        [REFUSED],
    ),
    (
        "a map file too large is read in part",
        "        if read.truncated {\n            return format!(\n                \"Could not open {}: at {} bytes",
        "        if false {\n            return format!(\n                \"Could not open {}: at {} bytes",
        [LARGE],
    ),
    (
        "decided by the name, not what it holds",
        '        if !doc.contains(&["slateos-mindmap"]) {',
        '        if path.extension().is_none_or(|e| e != "mindmap") {',
        [CONTENT],
    ),
    (
        "an outline cut mid-line keeps the half line",
        "            return self.import_outline(path, read.to_last_line(), max);",
        "            return self.import_outline(path, read, max);",
        [CUT],
    ),
    (
        "an imported outline takes the outline as its file",
        "        auto_layout(&mut map, self.win_width / 2.0, self.win_height / 2.0);\n        self.push_map(map);\n        format!(",
        "        auto_layout(&mut map, self.win_width / 2.0, self.win_height / 2.0);\n        map.document_path = Some(path.to_path_buf());\n        self.push_map(map);\n        format!(",
        [CONTENT],
    ),
    (
        "one file opens in two tabs",
        "                .is_some_and(|p| same_file(p, path))",
        "                .is_some_and(|p| p == path && false)",
        [TWICE],
    ),
    (
        "another spelling of a path is another file",
        "        (Ok(a), Ok(b)) => a == b,",
        "        (Ok(_), Ok(_)) => false,",
        [TWICE],
    ),
    # ---- the mark ----
    (
        "a save leaves the mark",
        "                map.document_path = Some(path.to_path_buf());\n                map.dirty = false;",
        "                map.document_path = Some(path.to_path_buf());",
        [ROUND, CTRL_S],
    ),
    (
        "a change does not mark the map",
        "        let map = self.active_map_mut();\n        map.dirty = true;\n        map.history.record(action);",
        "        let map = self.active_map_mut();\n        map.history.record(action);",
        [MARK, CLOSE_MAP],
    ),
    (
        "an undo does not mark the map",
        "        // Undoing past a save leaves a map its file does not hold.\n        self.active_map_mut().dirty = true;\n",
        "",
        [MARK],
    ),
    (
        "a layout marks the map whether or not it moved anything",
        "            .any(|(id, n)| before.get(id) != Some(&(n.x, n.y)));\n        if moved {",
        "            .any(|(id, n)| before.get(id) != Some(&(n.x, n.y)));\n        if true {",
        [LAYOUT],
    ),
    (
        "save as keeps the old name",
        "        if let Some(stem) = stem {\n            map.name = stem;\n        }",
        "        drop(stem);",
        [ROUND],
    ),
    (
        "an export clears the mark",
        "            PickerFor::Export => self.write_outline(path),",
        "            PickerFor::Export => {\n                let said = self.write_outline(path);\n                self.active_map_mut().dirty = false;\n                said\n            }",
        [EXPORT],
    ),
    # ---- the history: a tree, walked with Alt+Z (C-Q24) ----
    (
        "a redo does not mark the map",
        "        self.apply_forward(&action);\n        self.active_map_mut().dirty = true;\n        self.refresh_search();\n        true",
        "        self.apply_forward(&action);\n        self.refresh_search();\n        true",
        [MARK],
    ),
    (
        "a journey does not mark the map",
        "        if moved {\n            self.active_map_mut().dirty = true;\n            self.refresh_search();\n        }\n        moved",
        "        if moved {\n            self.refresh_search();\n        }\n        moved",
        [TREE],
    ),
    (
        "a journey takes its steps back the wrong way",
        "                Travel::Undo(action) => self.apply_reverse(&action),",
        "                Travel::Undo(action) => self.apply_forward(&action),",
        [TREE],
    ),
    (
        "Alt+Z goes nowhere",
        "                } else {\n                    self.earlier()\n                })",
        "                } else {\n                    false\n                })",
        [TREE, "every_advertised_key_does_something"],
    ),
    (
        "Alt+Shift+Z goes back too",
        "                    self.later()\n                } else {",
        "                    self.earlier()\n                } else {",
        [TREE],
    ),
    (
        "Alt+Z only undoes",
        "        let steps = self.active_map_mut().history.earlier();",
        "        let steps: Vec<Travel<Action>> =\n"
        "            self.active_map_mut().history.undo().map(Travel::Undo).into_iter().collect();",
        [TREE],
    ),
    (
        "Alt+Shift+Z only redoes",
        "        let steps = self.active_map_mut().history.later();",
        "        let steps: Vec<Travel<Action>> =\n"
        "            self.active_map_mut().history.redo().map(Travel::Redo).into_iter().collect();",
        [TREE],
    ),
    (
        "AltGr+Z goes back",
        "Key::Z if key.modifiers.alt && !key.modifiers.ctrl && !key.modifiers.super_key =>",
        "Key::Z if key.modifiers.alt && !key.modifiers.super_key =>",
        [ALTGR],
    ),
    (
        "Super+Alt+Z goes back",
        "Key::Z if key.modifiers.alt && !key.modifiers.ctrl && !key.modifiers.super_key =>",
        "Key::Z if key.modifiers.alt && !key.modifiers.ctrl =>",
        [SUPER],
    ),
    (
        "a key held with Alt is a shortcut",
        "            _ if key.modifiers.alt || key.modifiers.super_key => EventResult::Ignored,",
        "            _ if key.modifiers.super_key => EventResult::Ignored,",
        [ALTGR, GUARD],
    ),
    (
        "a key held with the Windows key is a shortcut",
        "            _ if key.modifiers.alt || key.modifiers.super_key => EventResult::Ignored,",
        "            _ if key.modifiers.alt => EventResult::Ignored,",
        [GUARD],
    ),
    (
        "Ctrl+Shift+Z undoes",
        "            Key::Z if ctrl && key.modifiers.shift => moved(self.redo()),\n",
        "",
        [CTRL_SHIFT_Z],
    ),
    (
        "redo undoes",
        "        let Some(action) = self.active_map_mut().history.redo() else {",
        "        let Some(action) = self.active_map_mut().history.undo() else {",
        [CTRL_SHIFT_Z, "ctrl_z_reaches_the_undo_stack_the_app_was_already_keeping"],
    ),
    (
        "the history keeps more than its limit",
        "const UNDO_LIMIT: core::num::NonZeroUsize = match core::num::NonZeroUsize::new(MAX_UNDO) {",
        "const UNDO_LIMIT: core::num::NonZeroUsize = match core::num::NonZeroUsize::new(MAX_UNDO + 20) {",
        ["test_app_undo_stack_limit"],
    ),
    # ---- each map its own history ----
    (
        "one history for every map",
        "        self.active_map_ref().history.can_undo()",
        "        self.maps.iter().any(|m| m.history.can_undo())",
        [UNDO],
    ),
    # ---- the question ----
    (
        "a changed map closes without asking",
        "        if map.dirty {\n            let message = unsaved::message_for(&[&map.name]);",
        "        if false {\n            let message = unsaved::message_for(&[&map.name]);",
        [CLOSE_MAP],
    ),
    (
        "Don't save keeps the map",
        "            (CloseScope::Map(id), Choice::Discard) => self.close_map(id),",
        "            (CloseScope::Map(_), Choice::Discard) => {}",
        [CLOSE_MAP],
    ),
    (
        "a map whose save failed is closed anyway",
        '                            Err(said) => format!("{said} -- so the map stays open"),',
        "                            Err(said) => {\n                                self.close_map(id);\n                                said\n                            }",
        [SAVE_FAILS],
    ),
    (
        "saved, and not closed",
        "            PickerFor::SaveThenClose(id) => match self.save_map_as(id, path) {\n                Ok(said) => {\n                    self.close_map(id);",
        "            PickerFor::SaveThenClose(id) => match self.save_map_as(id, path) {\n                Ok(said) => {",
        [CLOSE_MAP],
    ),
    (
        "a key reaches the map under the question",
        "                self.answer_close(scope, choice);\n            }\n            return EventResult::Consumed;",
        "                self.answer_close(scope, choice);\n            }",
        [CLOSE_MAP],
    ),
    (
        "the window closes over unsaved maps",
        "        if names.is_empty() {",
        "        if true {",
        [WINDOW, QUIT_FAILS, TYPED],
    ),
    (
        "a failed save while closing goes on anyway",
        '                self.last_file_action = Some(format!("{said} -- so the window stays open"));\n                return;',
        '                self.last_file_action = Some(format!("{said} -- so the window stays open"));',
        [QUIT_FAILS],
    ),
    (
        "saved, and the window does not go",
        "            None => self.quit = true,\n        }\n    }",
        "            None => {}\n        }\n    }",
        [WINDOW],
    ),
    (
        "the window's answer is not honoured",
        "        let result = self.handle_event(event);\n        if self.quit {",
        "        let result = self.handle_event(event);\n        if false {",
        [QUIT_FAILS],
    ),
    (
        "the question is not drawn",
        "            question.render(&palette, width, height, &mut tree);\n",
        "",
        [WINDOW],
    ),
    (
        "a name being typed is not kept",
        "    fn settle(&mut self) {\n        if self.editing_node.is_some() {",
        "    fn settle(&mut self) {\n        if false {",
        [TYPED],
    ),
    # ---- maps and tabs ----
    (
        "closing the last map leaves the old one",
        "        if self.maps.is_empty() {\n            self.active_map = 0;\n            self.add_map();\n            return;\n        }",
        "",
        [LAST],
    ),
    (
        "a new map is named by counting",
        '            .map(|n| format!("Mind Map {n}"))\n            .find(|name| !self.maps.iter().any(|m| &m.name == name))',
        '            .map(|_| format!("Mind Map {}", self.maps.len().saturating_add(1)))\n            .find(|_| true)',
        [NAMES],
    ),
    (
        "closing an earlier tab changes the map showing",
        "        if index < self.active_map || self.active_map >= self.maps.len() {",
        "        if self.active_map >= self.maps.len() {",
        [KEEP_SHOWING],
    ),
    (
        "closing another map drops the selection",
        "        if was_showing {\n            self.showing_another_map();\n        }",
        "        self.showing_another_map();",
        [KEEP_SHOWING],
    ),
    (
        "a tab does not answer a click",
        "            if inside(self.tab_rect(i), x, y) {",
        "            if false {",
        [TABS],
    ),
    (
        "a tab's close mark does not answer a click",
        "            if inside(self.tab_close_rect(i), x, y) {",
        "            if false {",
        [TABS],
    ),
    (
        "\"+\" does not answer a click",
        "        if inside(self.plus_rect(), x, y) {",
        "        if false {",
        [TABS],
    ),
    (
        "a click above the canvas goes to the map",
        "            if ev.y < self.canvas_y() {\n                return self.click_above_canvas(ev.x, ev.y);",
        "            if false {\n                return self.click_above_canvas(ev.x, ev.y);",
        [TABS, TOOLBAR],
    ),
    (
        "a press beside the canvas pans it",
        "            if ev.x < self.canvas_x() || ev.y >= self.canvas_y() + self.canvas_height() {",
        "            if false {",
        [BESIDE],
    ),
    (
        "the toolbar answers no click",
        "                Some(action) => self.toolbar(action),",
        "                Some(_) => EventResult::Ignored,",
        [TOOLBAR],
    ),
    (
        "Ctrl+Tab adds a child",
        "            Key::Tab if ctrl => {",
        "            Key::Tab if false => {",
        [CTRL_TAB],
    ),
    (
        "Ctrl+Shift+Tab goes forward",
        "                if self.step_map(!key.modifiers.shift) {",
        "                if self.step_map(true) {",
        [CTRL_TAB],
    ),
    (
        "Ctrl+S asks where every time",
        "            Key::S if ctrl => {\n                self.save();",
        "            Key::S if ctrl => {\n                self.ask_where_to_save(PickerFor::Save(self.active_map_ref().id));",
        [CTRL_S],
    ),
    (
        "Ctrl+Shift+S saves over the file",
        "            Key::S if ctrl && key.modifiers.shift => {",
        "            Key::S if false => {",
        [CTRL_S],
    ),
    (
        "what the last open or save did is drawn nowhere",
        '            Some(said) => format!("{said} | {counts}"),',
        "            Some(_) => counts.clone(),",
        [STATUS],
    ),
    (
        "the history keeps ten changes more",
        "const MAX_UNDO: usize = 200;",
        "const MAX_UNDO: usize = 210;",
        ["test_app_undo_stack_limit"],
    ),
    # No rows for "a node's text or the search box refuses what AltGr types"
    # or "takes a command's letter": since 2026-10-04 both boxes' typing is
    # textline::apply_key's, which makes both distinctions itself, in its own
    # crate and with its own tests;
    # a_node_and_the_search_box_take_altgr_letters_and_no_commands_letter
    # still holds the boxes to them.
    # -- the shortcut card is modal, for the keys and the pointer
    (
        "the card is modal for nothing",
        "        if self.show_help {\n            match event {\n",
        "        if false && self.show_help {\n            match event {\n",
        [CARD],
    ),
    (
        "a key that is not the card's acts behind it",
        "                        return EventResult::Consumed;\n"
        "                    }\n"
        "                    return EventResult::Ignored;\n",
        "                        return EventResult::Consumed;\n"
        "                    }\n",
        [CARD],
    ),
    (
        "? does not put the card away",
        "                        || key_ev.key == Key::Slash && key_ev.modifiers.shift;\n",
        "                        || false;\n",
        [CARD],
    ),
    (
        "Escape does not put the card away",
        "                    let closes = matches!(key_ev.key, Key::F1 | Key::Escape)\n",
        "                    let closes = matches!(key_ev.key, Key::F1)\n",
        [CARD],
    ),
    (
        "a press goes through the card",
        "                    MouseEventKind::Press(_) | MouseEventKind::DoubleClick(_) => {\n"
        "                        self.show_help = false;\n"
        "                        return EventResult::Consumed;\n"
        "                    }\n",
        "",
        [CARD],
    ),
    (
        "only the left button puts the card away",
        "                    MouseEventKind::Press(_) | MouseEventKind::DoubleClick(_) => {\n",
        "                    MouseEventKind::Press(MouseButton::Left) | MouseEventKind::DoubleClick(_) => {\n",
        [CARD],
    ),
    (
        "the wheel zooms what the card covers",
        "                    MouseEventKind::Scroll { .. } => return EventResult::Ignored,\n",
        "",
        [CARD],
    ),
]

RENAME = "a_node_is_renamed_in_the_toolkits_field_with_a_caret"
FIND = "the_find_bars_box_is_the_toolkits_field"

MUTATIONS += [
    # The node being renamed and the find bar's query are typed into the
    # toolkit's field (2026-10-04; lane C, c-e-a-theme-can-shape-the-controls).
    (
        "the node being renamed never has the keyboard",
        "            focused: !self.show_help,\n",
        "            focused: false,\n",
        [RENAME],
    ),
    (
        "the node being renamed keeps its mark under the shortcut list",
        "            focused: !self.show_help,\n",
        "            focused: true,\n",
        [RENAME],
    ),
    (
        "the caret is at the start of the name",
        "        let (cursor, selection_anchor) = self.box_caret(TypedBox::Name);\n",
        "        let (cursor, selection_anchor) = (text::TextCursor::from(0), None::<usize>);\n",
        [RENAME],
    ),
    (
        "the find box never has the keyboard",
        "            focused: self.show_search && self.editing_node.is_none() && !self.show_help,\n",
        "            focused: false,\n",
        [FIND],
    ),
    (
        "the find box keeps its mark while a node is renamed",
        "            focused: self.show_search && self.editing_node.is_none() && !self.show_help,\n",
        "            focused: self.show_search && !self.show_help,\n",
        [FIND],
    ),
    (
        "a query that finds nothing is not red",
        "            invalid: !self.search_query.is_empty() && self.search_results.is_empty(),\n",
        "            invalid: false,\n",
        [FIND],
    ),
    (
        "the focus mark is the toolkit's width, not the user's",
        "        self.focus_ring_width = settings.focus_ring_width();\n",
        "        let _ = settings;\n",
        [RENAME, FIND],
    ),
]

# The boxes edit at a caret, and the search finds what is there (2026-10-04,
# known-issues/E-twenty-nine-applications-type-only-at-the-end-of-a-box): a
# node's text and the query took typing at their end and Backspace from it,
# and nothing else; the results came in hash order, were kept through changes
# to the map, and the first Enter skipped the first.
EDITS = "a_nodes_text_edits_at_a_caret"
STARTS = "a_nodes_text_starts_with_the_caret_after_it"
REDRAW = "a_key_that_changes_nothing_in_a_box_is_not_a_redraw"
QUERY = "the_query_edits_at_a_caret"
BAR = "a_press_on_the_find_bar_is_the_bars"
SHOWN = "a_box_edits_the_text_it_shows"
ORDER = "search_results_come_in_the_order_the_outline_reads"
AGAIN = "a_change_with_the_find_bar_up_is_searched_again"
NEXT = "test_app_search_next_prev"

MUTATIONS += [
    # -- the search
    (
        "the outline is searched branch by branch from the last",
        "            stack.extend(node.children.iter().rev());\n",
        "            stack.extend(node.children.iter());\n",
        [ORDER],
    ),
    (
        "the typing goes to no result",
        "        if let Some(&found) = self.search_results.get(self.search_index) {\n"
        "            self.selected_node = Some(found);\n"
        "        }\n",
        "",
        [NEXT],
    ),
    (
        "the typing jumps away from a node it still finds",
        "            .selected_node\n            .and_then(",
        "            .selected_node\n            .filter(|_| false)\n            .and_then(",
        [NEXT],
    ),
    (
        "a change is not searched again",
        "        map.history.record(action);\n        self.refresh_search();\n",
        "        map.history.record(action);\n",
        [AGAIN],
    ),
    (
        "an undo is not searched again",
        "        self.active_map_mut().dirty = true;\n        self.refresh_search();\n        true\n    }\n\n    /// Redo",
        "        self.active_map_mut().dirty = true;\n        true\n    }\n\n    /// Redo",
        [AGAIN],
    ),
    (
        "a redo is not searched again",
        "        self.apply_forward(&action);\n        self.active_map_mut().dirty = true;\n        self.refresh_search();\n",
        "        self.apply_forward(&action);\n        self.active_map_mut().dirty = true;\n",
        [AGAIN],
    ),
    (
        "Alt+Z is not searched again",
        "            self.active_map_mut().dirty = true;\n            self.refresh_search();\n        }\n        moved\n",
        "            self.active_map_mut().dirty = true;\n        }\n        moved\n",
        [AGAIN],
    ),
    (
        "a change lets the current result go",
        "        self.search_index = current\n            .and_then(",
        "        self.search_index = current\n            .filter(|_| false)\n            .and_then(",
        [AGAIN],
    ),
    # -- the keys
    (
        "a Ctrl chord with the bar up is nobody's",
        "            None if textline::is_ctrl_chord(key.modifiers) => None,\n",
        "",
        [AGAIN],
    ),
    (
        "a plain key the query does not answer reaches the map's",
        "            None => Some(EventResult::Ignored),\n",
        "            None => None,\n",
        [QUERY],
    ),
    (
        "Ctrl+F with the bar up puts it away",
        "            Key::F if textline::is_ctrl_chord(key.modifiers) => {\n",
        "            Key::F if false => {\n",
        [QUERY, REDRAW],
    ),
    (
        "Ctrl+F with the bar up does not select the query",
        "                self.editor.select_all();\n",
        "",
        [QUERY, REDRAW],
    ),
    (
        "every key a box answers is a redraw",
        "        Some(self.editor_state() != before)\n",
        "        Some(true)\n",
        [REDRAW],
    ),
    (
        "a cut or a copy takes nothing to the clipboard",
        "            self.clipboard = copied;\n",
        "            let _ = copied;\n",
        [EDITS],
    ),
    (
        "the editor is kept for the box the keyboard moved to",
        "        if self.editor_for != Some(which) || self.editor.text() != self.box_text(which) {\n",
        "        if self.editor.text() != self.box_text(which) {\n",
        [SHOWN],
    ),
    (
        "a key finds the editor holding another text",
        "        if self.editor_for != Some(which) || self.editor.text() != self.box_text(which) {\n",
        "        if self.editor_for != Some(which) {\n",
        [SHOWN],
    ),
    (
        "a node's text starts from the last node's caret",
        "            self.editor.set_text(&self.edit_buffer);\n"
        "            self.editor_for = Some(TypedBox::Name);\n",
        "",
        [STARTS],
    ),
    # -- the pointer
    (
        "a press puts the caret at the start",
        "            x - area.x,\n",
        "            0.0,\n",
        [EDITS, QUERY],
    ),
    (
        "a press in the node being renamed goes to the map",
        "                && field.contains(ev.x, ev.y)\n",
        "                && false\n",
        [EDITS],
    ),
    (
        "a press in the query goes past it",
        "                if self.editing_node.is_none() && search.contains(ev.x, ev.y) {\n",
        "                if false {\n",
        [QUERY],
    ),
    (
        "a press on the find bar goes to the map under it",
        "            if self.show_search && bar.contains(ev.x, ev.y) {\n",
        "            if false {\n",
        [BAR, QUERY],
    ),
    (
        "a press on the bar off its box goes to the map",
        "                    return EventResult::Consumed;\n"
        "                }\n"
        "                return EventResult::Ignored;\n"
        "            }\n",
        "                    return EventResult::Consumed;\n"
        "                }\n"
        "            }\n",
        [BAR],
    ),
    (
        "a press where the bar would be is the bar's while it is put away",
        "            if self.show_search && bar.contains(ev.x, ev.y) {\n",
        "            if bar.contains(ev.x, ev.y) {\n",
        [BAR],
    ),
    # -- the drawing
    (
        "the name's caret is drawn at its end and its selection not at all",
        "        let (cursor, selection_anchor) = self.box_caret(TypedBox::Name);\n",
        "        let (cursor, selection_anchor) = (text::TextCursor::from(self.edit_buffer.len()), None::<usize>);\n",
        [EDITS],
    ),
    (
        "the query's caret is drawn at its end",
        "                cursor: self.box_caret(TypedBox::Query).0,\n",
        "                cursor: text::TextCursor::from(self.search_query.len()),\n",
        [QUERY],
    ),
    (
        "the query's selection is not drawn",
        "                selection_anchor: self.box_caret(TypedBox::Query).1,\n",
        "                selection_anchor: None,\n",
        [QUERY],
    ),
    (
        "the root's text is typed at a node's size",
        "        (ROOT_FONT_SIZE, FontWeightHint::Bold)\n",
        "        (NODE_FONT_SIZE, FontWeightHint::Bold)\n",
        [RENAME],
    ),
    (
        "a node's text is typed at the field's edge",
        "            x: r.x + NAME_TEXT_INSET,\n",
        "            x: r.x,\n",
        [RENAME],
    ),
]

if __name__ == "__main__":
    only = sys.argv[1:] or None
    raise SystemExit(sweep(SRC, MUTATIONS, "mindmap", timeout=900, only=only))
