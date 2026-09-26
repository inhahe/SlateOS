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

Run it with no arguments to sweep everything, or with substrings of the
mutation names to run only those.
"""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "scripts"))

from mutation_harness import sweep  # noqa: E402  (path set above)

SRC = Path(__file__).parent / "src" / "main.rs"

ROUND = "a_map_saved_and_opened_again_is_the_same_map"
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
        "        let map = self.active_map_mut();\n        map.dirty = true;\n        map.redo_stack.clear();",
        "        let map = self.active_map_mut();\n        map.redo_stack.clear();",
        [MARK, CLOSE_MAP],
    ),
    (
        "an undo does not mark the map",
        "            // Undoing past a save leaves a map its file does not hold.\n            map.dirty = true;\n",
        "",
        [MARK],
    ),
    (
        "a layout marks the map whether or not it moved anything",
        "        if moved {",
        "        if true {",
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
    # ---- each map its own history ----
    (
        "one history for every map",
        "        !self.active_map_ref().undo_stack.is_empty()",
        "        self.maps.iter().any(|m| !m.undo_stack.is_empty())",
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
]

if __name__ == "__main__":
    only = sys.argv[1:] or None
    raise SystemExit(sweep(SRC, MUTATIONS, "mindmap", timeout=900, only=only))
