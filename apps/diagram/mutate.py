"""Mutation test for the diagram editor's own file, and asking before a diagram
is lost.

Breaks one piece of production code at a time and checks that the test which
claims to cover it is the one that fails.  A test that passes against a broken
program is not testing the program.

It could export an SVG, or a JSON that kept the shapes and dropped their
colours, borders, fonts, arrowheads, layers and groups, for an importer that
did not exist -- and nothing it wrote could be opened again.  Nothing recorded
unsaved changes, and the window closed over them.  The table covers the file
it has now and the question; the rest of the suite predates them.

The undo history is a tree now (C-Q24): an edit after an undo keeps the undone
diagram as a branch, reached with Alt+Z.  Its rows cover the keys -- and
AltGr, which arrives as Ctrl+Alt, not being taken for either -- the toolbar's
word on it, the cap, and an opened diagram's history starting with it; the
tree itself is `statehistory`'s and the toolkit's to test.

Run it with no arguments to sweep everything, or with substrings of the
mutation names to run only those.
"""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "scripts"))

from mutation_harness import sweep  # noqa: E402  (path set above)

SRC = Path(__file__).parent / "src" / "main.rs"

ROUND = "a_diagram_saved_and_opened_again_is_the_same_diagram"
REFUSED = "a_file_that_is_not_a_diagram_is_refused"
PARTIAL = "what_is_not_understood_is_left_out_and_the_rest_read"
KEYS = "ctrl_s_saves_over_the_diagrams_own_file_once_it_has_one"
CLOSE = "closing_or_opening_over_unsaved_changes_asks"
TREE = "an_edit_after_an_undo_keeps_the_undone_diagram_reachable_with_alt_z"
CTRL_SHIFT_Z = "ctrl_shift_z_redoes"
ALTGR = "altgr_z_does_not_undo"
SUPER = "alt_z_with_the_windows_key_goes_nowhere"
GUARD = "a_key_held_with_altgr_alt_or_the_windows_key_is_no_shortcut"
INDICATOR = "the_toolbar_says_whether_undo_and_redo_can_go"
CAP = "the_history_keeps_the_last_hundred_edits"
OPENED = "an_opened_diagram_cannot_be_undone_into_the_one_before"

# (name, old, new, [tests that must fail])
MUTATIONS = [
    (
        "a save writes nothing",
        "        match safeio::write_str_atomically(path, &self.diagram_document().to_text()) {",
        "        match Ok::<(), std::io::Error>(()) {",
        [ROUND, KEYS],
    ),
    (
        "the file loses a box's fill",
        '            doc.set_str(&at("fill"), &colour_hex(node.fill_color));\n',
        "",
        [ROUND],
    ),
    (
        "the file loses the arrowheads",
        '            doc.set_str(&at("end"), edge.end_arrow.label());\n',
        "",
        [ROUND],
    ),
    (
        "the file loses the groups",
        "        for (i, group) in self.groups.iter().enumerate() {",
        "        for (i, group) in self.groups.iter().enumerate().take(0) {",
        [ROUND],
    ),
    (
        "a hidden layer comes back visible",
        '        layer.visible = doc.get_bool(&at("visible")).unwrap_or(true);',
        "        layer.visible = true;",
        [ROUND],
    ),
    (
        "a later format is half-read",
        "        Some(v) if v > DIAGRAM_FORMAT => {",
        "        Some(v) if v > DIAGRAM_FORMAT + 100 => {",
        [REFUSED],
    ),
    (
        "clashing ids are taken as they come",
        "    if seen.insert(id) {",
        "    if seen.insert(id) || true {",
        [REFUSED],
    ),
    (
        "a file too big to read whole is read in part",
        "        if read.truncated {",
        "        if false {",
        [REFUSED],
    ),
    (
        "an arrow to a missing box is read",
        "        if !nodes.iter().any(|n| n.id == from) || !nodes.iter().any(|n| n.id == to) {\n"
        "            left_out = left_out.saturating_add(1);\n"
        "            continue;\n"
        "        }\n",
        "",
        [PARTIAL],
    ),
    (
        "what was left out is not said",
        "                if left_out == 0 {",
        "                if true {",
        [PARTIAL],
    ),
    (
        "new boxes take the ids of opened ones",
        "                self.id_gen = IdGen::new(highest.saturating_add(1));",
        "                self.id_gen = IdGen::new(1);",
        ["new_things_do_not_take_the_ids_of_opened_ones"],
    ),
    (
        "Ctrl+S asks where every time",
        "            Some(path) => self.last_save = Some(self.write_native(&path)),",
        "            Some(_) => self.ask_where_to_save(PickerFor::Save),",
        [KEYS],
    ),
    (
        "an export counts as a save",
        "            PickerFor::Export => self.write_diagram(path),",
        "            PickerFor::Export => self.write_native(path),",
        ["an_export_is_not_a_save"],
    ),
    (
        "a change does not mark the diagram",
        "        self.undo.begin(snap);\n        self.dirty = true;\n",
        "        self.undo.begin(snap);\n",
        [KEYS, CLOSE],
    ),
    (
        "a name left as it was counts as a change",
        "        if self.find_node(id).is_some_and(|n| n.label == label) {\n"
        "            return;\n"
        "        }\n",
        "",
        ["a_name_left_as_it_was_is_no_change"],
    ),
    (
        "the window closes over unsaved changes",
        "            }\n        }\n        if !self.dirty {\n            return true;\n        }",
        "            }\n        }\n        if true {\n            return true;\n        }",
        [CLOSE],
    ),
    (
        "the question is drawn into a window the loop has closed",
        "            } else {\n                Response::KeepOpen\n            };",
        "            } else {\n                Response::Redraw\n            };",
        [CLOSE],
    ),
    (
        "keys reach the canvas under the question",
        "        if let Some(question) = self.question.as_mut()\n"
        "            && matches!(event, Event::Key(_) | Event::Mouse(_))",
        "        if let Some(question) = self.question.as_mut()\n"
        "            && false",
        [CLOSE],
    ),
    (
        "Ctrl+O opens over unsaved changes",
        "                self.unless_unsaved(Pending::Open);",
        "                self.go_on(Pending::Open);",
        [CLOSE],
    ),
    (
        "what a save did is drawn nowhere",
        "        if let Some(note) = &self.last_save {",
        "        if let Some(note) = None::<&String> {",
        ["the_status_bar_says_what_the_last_save_did"],
    ),
    (
        "no selection box starts",
        "                        self.rect_select_start = Some((cx, cy));\n                        self.rect_select_end = Some((cx, cy));",
        "",
        ["dragging_across_empty_canvas_selects_the_shapes_the_box_touches"],
    ),
    (
        "the box selects nothing",
        "        self.selection.clear();\n        self.selection.nodes = caught;",
        "        self.selection.clear();\n        let _ = caught;",
        ["dragging_across_empty_canvas_selects_the_shapes_the_box_touches"],
    ),
    (
        "a press on a selected shape drops the others",
        "                        if !self.selection.has_node(id) {\n                            self.selection.select_single_node(id);\n                        }",
        "                        self.selection.select_single_node(id);",
        ["dragging_across_empty_canvas_selects_the_shapes_the_box_touches"],
    ),
    # -- the history: a tree, walked with Alt+Z (C-Q24) -----------------------
    (
        "a diagram the history puts back is not marked",
        "        self.restore_snapshot(snapshot);\n"
        "        // Undoing past a save leaves a diagram the file does not hold.\n"
        "        self.dirty = true;\n",
        "        self.restore_snapshot(snapshot);\n",
        [TREE],
    ),
    (
        "Alt+Z goes nowhere",
        "                } else {\n                    self.earlier()\n                })",
        "                } else {\n                    false\n                })",
        [TREE],
    ),
    (
        "Alt+Shift+Z goes back too",
        "                    self.later()\n                } else {",
        "                    self.earlier()\n                } else {",
        [TREE],
    ),
    (
        "Alt+Z only undoes",
        "        let earlier = self.undo.earlier(current);",
        "        let earlier = self.undo.undo(current);",
        [TREE],
    ),
    (
        "Alt+Shift+Z only redoes",
        "        let later = self.undo.later(current);",
        "        let later = self.undo.redo(current);",
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
        "        let next = self.undo.redo(current);",
        "        let next = self.undo.undo(current);",
        [CTRL_SHIFT_Z, "test_redo_add_node"],
    ),
    (
        "the toolbar says undo never can",
        'if self.undo.can_undo() { "yes" } else { "no" },',
        'if false { "yes" } else { "no" },',
        [INDICATOR],
    ),
    (
        "the toolbar says whether undo can for redo",
        'if self.undo.can_redo() { "yes" } else { "no" }',
        'if self.undo.can_undo() { "yes" } else { "no" }',
        [INDICATOR],
    ),
    (
        "the history keeps more than a hundred edits",
        "            undo: StateHistory::new(UNDO_LIMIT),",
        "            undo: StateHistory::new(UNDO_LIMIT.saturating_add(20)),",
        [CAP],
    ),
    (
        "an opened diagram keeps the history of the one before",
        "                self.undo.clear();\n",
        "",
        [OPENED],
    ),
    # No rows for "a label refuses what AltGr types" or "types a command's
    # letter": since 2026-10-04 a label's typing is textline::apply_key's,
    # which makes both distinctions itself, in its own crate and with its own
    # tests; a_label_takes_altgr_letters_and_no_commands_letter still holds the
    # label to them.
    # -- the shortcut card is modal, for the keys and the pointer
    (
        "the card is modal for nothing",
        "        if self.show_help {\n            match event {\n",
        "        if false && self.show_help {\n            match event {\n",
        ["the_shortcut_card_takes_every_key_and_press_while_it_is_up"],
    ),
    (
        "a key that is not the card's acts behind it",
        "                    if closes {\n"
        "                        self.show_help = false;\n"
        "                    }\n"
        "                    return EventResult::Consumed;\n",
        "                    if closes {\n"
        "                        self.show_help = false;\n"
        "                        return EventResult::Consumed;\n"
        "                    }\n",
        ["the_shortcut_card_takes_every_key_and_press_while_it_is_up"],
    ),
    (
        "? does not put the card away",
        "                        Key::Slash => key.modifiers.shift,\n",
        "                        Key::Slash => false,\n",
        ["the_shortcut_card_takes_every_key_and_press_while_it_is_up"],
    ),
    (
        "Escape does not put the card away",
        "                        Key::F1 | Key::Escape => true,\n",
        "                        Key::F1 => true,\n",
        ["the_shortcut_card_takes_every_key_and_press_while_it_is_up"],
    ),
    (
        "a press goes through the card",
        "                    MouseEventKind::Press(_) | MouseEventKind::DoubleClick(_) => {\n"
        "                        self.show_help = false;\n"
        "                        return EventResult::Consumed;\n"
        "                    }\n",
        "",
        ["the_shortcut_card_takes_every_key_and_press_while_it_is_up"],
    ),
    (
        "only the left button puts the card away",
        "                    MouseEventKind::Press(_) | MouseEventKind::DoubleClick(_) => {\n",
        "                    MouseEventKind::Press(MouseButton::Left) | MouseEventKind::DoubleClick(_) => {\n",
        ["the_shortcut_card_takes_every_key_and_press_while_it_is_up"],
    ),
    (
        "the wheel zooms what the card covers",
        "                    MouseEventKind::Scroll { .. } => return EventResult::Ignored,\n",
        "",
        ["the_shortcut_card_takes_every_key_and_press_while_it_is_up"],
    ),
]

FIELD = "a_label_is_typed_into_the_toolkits_field"
FIRST = "a_thing_with_no_label_shows_its_first_label_as_it_is_typed"
CARET = "the_caret_follows_the_typing_and_stays_in_the_box"
EDITS = "a_label_being_typed_edits_at_a_caret"
STARTS = "a_label_starts_from_its_word_the_caret_after_it"

MUTATIONS += [
    # A label is typed into the toolkit's field (2026-10-04; lane C,
    # c-e-a-theme-can-shape-the-controls). There was no box and no caret, and
    # a thing with no label showed none of its first one until Enter.
    (
        "a box's first label is not drawn as it is typed",
        "            Some((LabelTarget::Node(id), buf)) if *id == node.id => {",
        "            Some((LabelTarget::Node(id), buf)) if *id == node.id && !node.label.is_empty() => {",
        [FIRST, FIELD],
    ),
    (
        "a line's first label is not drawn as it is typed",
        "            Some((LabelTarget::Edge(id), buf)) if *id == edge.id => {",
        "            Some((LabelTarget::Edge(id), buf)) if *id == edge.id && !edge.label.is_empty() => {",
        [FIRST, FIELD],
    ),
    (
        "a label is typed into no box",
        "        field::draw(\n"
        "            cmds,\n"
        "            &self.palette,\n"
        "            strip.field(),\n",
        "        let _ = (\n"
        "            cmds.len(),\n"
        "            &self.palette,\n"
        "            strip.field(),\n",
        [FIELD],
    ),
    (
        "the label's box does not have the keyboard",
        "                focused: true,\n                ..field::State::default()",
        "                focused: false,\n                ..field::State::default()",
        [FIELD],
    ),
    (
        "the focus mark is the toolkit's width, not the user's",
        "        self.focus_ring_width = settings.focus_ring_width();\n",
        "        let _ = settings;\n",
        [FIELD],
    ),
    (
        "the caret is at the start of the typing",
        "                cursor: self.label_caret(buf).0,\n",
        "                cursor: guitk::text::TextCursor::from(0),\n",
        [CARET, EDITS],
    ),
]

# A label edits at a caret (2026-10-04,
# known-issues/E-twenty-nine-applications-type-only-at-the-end-of-a-box): it
# took typing at its end and Backspace from it, and nothing else.
MUTATIONS += [
    (
        "a label starts from the last label's caret",
        "        self.label_editor.set_text(&existing);\n",
        "",
        [STARTS],
    ),
    (
        "a cut or a copy takes nothing to the clipboard",
        "                    self.label_clipboard = copied;\n",
        "                    let _ = copied;\n",
        [EDITS],
    ),
    (
        "the selection is not drawn",
        "                selection_anchor: self.label_caret(buf).1,\n",
        "                selection_anchor: None,\n",
        [EDITS],
    ),
    (
        "a press puts the caret at the start",
        "            x - strip.x,\n",
        "            0.0,\n",
        [EDITS],
    ),
    (
        "a press in the label goes to the canvas",
        "            && strip.field().contains(ev.x, ev.y)\n",
        "            && false\n",
        [EDITS],
    ),
    (
        "a label is pressed where it lies on the canvas, not the screen",
        "            x: strip.x + PALETTE_WIDTH + self.pan_x,\n",
        "            x: strip.x,\n",
        [EDITS],
    ),
]

if __name__ == "__main__":
    only = sys.argv[1:] or None
    raise SystemExit(sweep(SRC, MUTATIONS, "diagram", timeout=900, only=only))
