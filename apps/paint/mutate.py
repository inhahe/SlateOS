"""Mutation test for paint's record of unsaved changes, its saving, and the
question before a picture is lost.

Breaks one piece of production code at a time and checks that the test which
claims to cover it is the one that fails.  A test that passes against a broken
program is not testing the program.

Nothing recorded whether the picture had changed, so nothing could ask: the
window closed over it, and Ctrl+N and Ctrl+O replaced it, without a word.
Ctrl+S asked where every time, and what a save did was drawn nowhere, so a
failed one looked like one that worked.  The table covers the repair; the rest
of the suite predates it.

The undo history is a tree now (C-Q24): an edit after an undo keeps the undone
picture as a branch, reached with Alt+Z.  Its rows cover the keys -- and
AltGr, which arrives as Ctrl+Alt, not being taken for either -- the status bar
and the cap; the tree itself is `statehistory`'s and the toolkit's to test.

Run it with no arguments to sweep everything, or with substrings of the
mutation names to run only those.
"""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "scripts"))

from mutation_harness import sweep  # noqa: E402  (path set above)

SRC = Path(__file__).parent / "src" / "main.rs"

MARKS = "an_edit_marks_the_picture_and_a_save_clears_it"
CLOSE = "closing_over_unsaved_changes_asks_and_each_answer_is_kept"
REPLACE = "new_and_open_ask_before_replacing_the_picture"

# (name, old, new, [tests that must fail])
JPEG = "a_jpeg_is_never_saved_over"
NAMED = "a_save_is_the_format_its_name_says"
TRANSPARENT = "a_saved_bmp_keeps_its_transparency"
CROPPED = "a_picture_wider_than_any_canvas_is_cropped"
LISTS = "the_open_dialog_lists_pictures"
TREE = "an_edit_after_an_undo_keeps_the_undone_picture_reachable_with_alt_z"
CTRL_SHIFT_Z = "ctrl_shift_z_redoes"
ALTGR = "altgr_z_does_not_undo"
SUPER = "alt_z_with_the_windows_key_goes_nowhere"
STATUS = "the_status_bar_says_whether_undo_and_redo_can_go"
CAP = "the_history_keeps_the_last_fifty_edits"
LAYERS = "every_layer_operation_can_be_undone"

MUTATIONS = [
    (
        "an edit does not mark the picture",
        "        self.history.begin(snapshot);\n        self.dirty = true;\n",
        "        self.history.begin(snapshot);\n",
        [MARKS, CLOSE],
    ),
    (
        "a picture the history puts back is not marked",
        "        self.active_layer = snapshot.active_layer;\n        self.dirty = true;\n",
        "        self.active_layer = snapshot.active_layer;\n",
        [MARKS, TREE],
    ),
    (
        "a save leaves the picture marked",
        "                self.document_path = Some(path.to_path_buf());\n"
        "                self.dirty = false;\n"
        '                format!("Saved {}", path.shown())',
        "                self.document_path = Some(path.to_path_buf());\n"
        '                format!("Saved {}", path.shown())',
        [MARKS],
    ),
    (
        "Ctrl+S asks where every time",
        "                self.file_status = Some(self.save_file(&path));\n            }",
        "                let _ = &path;\n                self.ask_where_to_save(PickerFor::Save);\n            }",
        [MARKS],
    ),
    (
        "the window closes over unsaved changes",
        "            if !self.dirty {\n                return Response::Exit;\n            }",
        "            if true {\n                return Response::Exit;\n            }",
        [CLOSE],
    ),
    (
        "the question is drawn into a window the loop has closed",
        "            self.unless_unsaved(Pending::Close);\n            return Response::KeepOpen;",
        "            self.unless_unsaved(Pending::Close);\n            return Response::Redraw;",
        [CLOSE],
    ),
    (
        "keys reach the picture under the question",
        "        if let Some(question) = self.question.as_mut()\n"
        "            && matches!(event, Event::Key(_) | Event::Mouse(_))",
        "        if let Some(question) = self.question.as_mut()\n"
        "            && false",
        [CLOSE],
    ),
    (
        "saving on close does not close",
        "                    self.file_status = Some(self.save_file(&path));\n"
        "                    if !self.dirty {\n"
        "                        self.go_on(pending);\n"
        "                    }",
        "                    self.file_status = Some(self.save_file(&path));",
        [CLOSE],
    ),
    (
        "a picture saved where the picker said does not go on",
        "                let said = self.save_file(path);\n"
        "                if !self.dirty {\n"
        "                    self.go_on(pending);\n"
        "                }",
        "                let said = self.save_file(path);",
        ["an_untitled_picture_is_saved_where_the_picker_says_before_closing"],
    ),
    (
        "Ctrl+N replaces the picture without asking",
        "                    self.unless_unsaved(Pending::New);",
        "                    self.go_on(Pending::New);",
        [REPLACE],
    ),
    (
        "Ctrl+O opens over unsaved changes",
        "                    self.unless_unsaved(Pending::Open);",
        "                    self.go_on(Pending::Open);",
        [REPLACE],
    ),
    (
        "a blank canvas counts as unsaved",
        "                // is still one undo away, and undoing marks it again.\n"
        "                self.dirty = false;\n",
        "                // is still one undo away, and undoing marks it again.\n",
        [REPLACE],
    ),
    # -- the history: a tree, walked with Alt+Z (C-Q24) -----------------------
    (
        "Alt+Z goes nowhere",
        "            } else {\n                self.earlier();\n            }\n            return true;",
        "            }\n            return true;",
        [TREE],
    ),
    (
        "Alt+Shift+Z goes back too",
        "                self.later();\n            } else {",
        "                self.earlier();\n            } else {",
        [TREE],
    ),
    (
        "Alt+Z only undoes",
        "        let earlier = self.history.earlier(current);",
        "        let earlier = self.history.undo(current);",
        [TREE],
    ),
    (
        "Alt+Shift+Z only redoes",
        "        let later = self.history.later(current);",
        "        let later = self.history.redo(current);",
        [TREE],
    ),
    (
        "AltGr+Z goes back",
        "key.key == Key::Z && key.modifiers.alt && !key.modifiers.ctrl && !key.modifiers.super_key",
        "key.key == Key::Z && key.modifiers.alt && !key.modifiers.super_key",
        [ALTGR],
    ),
    (
        "Super+Alt+Z goes back",
        "key.key == Key::Z && key.modifiers.alt && !key.modifiers.ctrl && !key.modifiers.super_key",
        "key.key == Key::Z && key.modifiers.alt && !key.modifiers.ctrl",
        [SUPER],
    ),
    (
        "AltGr is taken for Ctrl",
        "            textline::is_ctrl_chord(key.modifiers),\n",
        "            key.modifiers.ctrl,\n",
        [ALTGR],
    ),
    (
        "Ctrl+Shift+Z undoes",
        "                'z' | 'Z' if shift => {\n"
        "                    self.redo();\n"
        "                    return true;\n"
        "                }\n",
        "",
        [CTRL_SHIFT_Z],
    ),
    (
        "redo undoes",
        "        let next = self.history.redo(current);",
        "        let next = self.history.undo(current);",
        [CTRL_SHIFT_Z],
    ),
    (
        "the status bar says undo never can",
        'if self.history.can_undo() { "yes" } else { "no" },',
        'if false { "yes" } else { "no" },',
        [STATUS],
    ),
    (
        "the status bar says whether undo can for redo",
        'if self.history.can_redo() { "yes" } else { "no" }',
        'if self.history.can_undo() { "yes" } else { "no" }',
        [STATUS],
    ),
    (
        "the history keeps more than fifty edits",
        "            history: StateHistory::new(UNDO_LIMIT),",
        "            history: StateHistory::new(UNDO_LIMIT.saturating_add(20)),",
        [CAP],
    ),
    # -- the layer operations, edits the history keeps ------------------------
    (
        "adding a layer is not recorded",
        '        self.push_history("add layer");\n',
        "        self.dirty = true;\n",
        [LAYERS],
    ),
    (
        "deleting a layer is not recorded",
        '        self.push_history("delete layer");\n',
        "        self.dirty = true;\n",
        [LAYERS],
    ),
    (
        "moving a layer up is not recorded",
        '            self.push_history("move layer up");\n',
        "            self.dirty = true;\n",
        [LAYERS],
    ),
    (
        "moving a layer down is not recorded",
        '        self.push_history("move layer down");\n',
        "        self.dirty = true;\n",
        [LAYERS],
    ),
    (
        "merging a layer down is not recorded",
        '        self.push_history("merge layer down");\n',
        "        self.dirty = true;\n",
        [LAYERS],
    ),
    (
        "a refused deletion is recorded",
        "        if self.layers.len() <= 1 || self.active_layer >= self.layers.len() {\n"
        "            return false;\n"
        "        }\n"
        '        self.push_history("delete layer");\n',
        '        self.push_history("delete layer");\n'
        "        if self.layers.len() <= 1 || self.active_layer >= self.layers.len() {\n"
        "            return false;\n"
        "        }\n",
        ["deleting_a_layer_with_a_stale_active_index_declines_rather_than_panicking"],
    ),
    (
        "a refused move down is recorded",
        "        if self.active_layer >= self.layers.len() {\n"
        "            return false;\n"
        "        }\n"
        '        self.push_history("move layer down");\n',
        '        self.push_history("move layer down");\n'
        "        if self.active_layer >= self.layers.len() {\n"
        "            return false;\n"
        "        }\n",
        ["moving_down_with_a_stale_active_index_declines_rather_than_panicking"],
    ),
    (
        "a move down with a stale index is not refused",
        "        // a public field: checked here, as `delete_layer` checks it.\n"
        "        if self.active_layer >= self.layers.len() {\n"
        "            return false;\n"
        "        }\n",
        "        // a public field: checked here, as `delete_layer` checks it.\n",
        ["moving_down_with_a_stale_active_index_declines_rather_than_panicking"],
    ),
    (
        "a refused merge is recorded",
        "        if self.active_layer >= self.layers.len() {\n"
        "            return false;\n"
        "        }\n"
        '        self.push_history("merge layer down");\n',
        '        self.push_history("merge layer down");\n'
        "        if self.active_layer >= self.layers.len() {\n"
        "            return false;\n"
        "        }\n",
        ["merging_down_with_a_stale_active_index_declines_rather_than_panicking"],
    ),
    # -- pictures of every format --------------------------------------------
    (
        "a picture Paint cannot write is saved over",
        "            Some(path) if SaveAs::of(&path).is_some() => {",
        "            Some(path) => {",
        [JPEG],
    ),
    (
        "the offered name keeps a format Paint cannot write",
        "                    file.with_extension(\"png\").into_os_string()",
        "                    file.as_os_str().to_owned()",
        [JPEG],
    ),
    (
        "a name with no extension is refused",
        "        let path = if path.extension().is_none() {",
        "        let path = if false {",
        [NAMED],
    ),
    (
        "any extension is written",
        "        let Some(format) = SaveAs::of(&path) else {\n            return String::from(\"Paint saves PNG and BMP files: name it .png or .bmp\");\n        };",
        "        let format = SaveAs::of(&path).unwrap_or(SaveAs::Png);",
        [NAMED],
    ),
    (
        "a BMP is written as a PNG",
        "            SaveAs::Bmp => encode_bmp(&flat),",
        "            SaveAs::Bmp => pngwrite::encode(flat.width(), flat.height(), &argb_of(&flat))\n                .unwrap_or_default(),",
        [NAMED],
    ),
    (
        "a BMP's alpha mask is empty",
        "    for mask in [0x00FF_0000_u32, 0x0000_FF00, 0x0000_00FF, 0xFF00_0000] {",
        "    for mask in [0x00FF_0000_u32, 0x0000_FF00, 0x0000_00FF, 0] {",
        [TRANSPARENT],
    ),
    (
        "a BMP's colours are written red first",
        "                out.extend_from_slice(&[c.b, c.g, c.r, c.a]);",
        "                out.extend_from_slice(&[c.r, c.g, c.b, c.a]);",
        [TRANSPARENT, "test_bmp_encode_decode_roundtrip"],
    ),
    (
        "a picture wider than any canvas is not cropped",
        "            full.copy_region(0, 0, w, h)",
        "            full",
        [CROPPED],
    ),
    (
        "the open dialog lists every file",
        "                        .with_initial_path(guitk::dialog::FilePicker::default_start())\n                        .with_filter(\"Pictures\", &patterns),",
        "                        .with_initial_path(guitk::dialog::FilePicker::default_start()),",
        [LISTS],
    ),
    (
        "what a save did is drawn nowhere",
        "        if let Some(status) = &self.file_status {",
        "        if let Some(status) = None::<&String> {",
        ["the_status_bar_says_what_the_last_save_did"],
    ),
    (
        "a palette click chooses nothing",
        "                    self.fg_color = colour;",
        "                    let _ = colour;",
        ["a_palette_colour_is_chosen_by_clicking_it"],
    ),
    (
        "a right click chooses the foreground",
        "                if right {\n                    self.bg_color = colour;",
        "                if false {\n                    self.bg_color = colour;",
        ["a_palette_colour_is_chosen_by_clicking_it"],
    ),
    (
        "the tool buttons are pictures",
        "            self.current_tool = tool;\n            return true;",
        "            let _ = tool;\n            return true;",
        ["a_tool_button_chooses_its_tool"],
    ),
    (
        "the foreground swatch opens nothing",
        "        if layout.fg.contains(x, y) {\n            self.open_color_dialog(true);",
        "        if layout.fg.contains(x, y) {",
        ["the_swatches_and_c_open_the_colour_dialog_and_enter_keeps_the_colour"],
    ),
    (
        "Enter leaves the colour",
        "                    self.color_picker.apply_hex_input();\n                }\n                self.apply_color_dialog();",
        "                    self.color_picker.apply_hex_input();\n                }\n                self.color_picker.close();",
        [
            "the_swatches_and_c_open_the_colour_dialog_and_enter_keeps_the_colour",
            "a_typed_hex_colour_is_kept_and_letters_are_not_tools",
        ],
    ),
    (
        "a drag does not move the slider",
        "                let Some(index) = self.color_picker.active_slider else {\n                    return false;\n                };",
        "                let Some(index) = None::<u8> else {\n                    return false;\n                };",
        ["the_swatches_and_c_open_the_colour_dialog_and_enter_keeps_the_colour"],
    ),
    (
        "hex letters reach the tools",
        "        if self.color_picker.is_open && key.key != Key::F1 {\n            return self.picker_key(key);\n        }",
        "",
        ["a_typed_hex_colour_is_kept_and_letters_are_not_tools"],
    ),
    (
        "Tab chooses no other slider",
        "        self.color_picker.focus = self.color_picker.focus.wrapping_add(by) % 4;",
        "        let _ = by;",
        ["tab_and_the_arrows_move_the_chosen_slider"],
    ),
    (
        "a slider runs past its ends",
        ".saturating_add(delta).clamp(0, 255))",
        ".saturating_add(delta))",
        ["tab_and_the_arrows_move_the_chosen_slider"],
    ),
    (
        "C opens nothing",
        "                self.open_color_dialog(!shift);",
        "                let _ = shift;",
        ["the_swatches_and_c_open_the_colour_dialog_and_enter_keeps_the_colour"],
    ),
    (
        'a key held with Alt is a tool',
        '        if textline::is_alt_or_windows_chord(key.modifiers) {',
        '        if key.modifiers.super_key {',
        ['a_key_held_with_alt_or_the_windows_key_is_not_a_tool'],
    ),
    (
        'a key held with the Windows key is a tool',
        '        if textline::is_alt_or_windows_chord(key.modifiers) {',
        '        if key.modifiers.alt && !key.modifiers.ctrl {',
        ['a_key_held_with_alt_or_the_windows_key_is_not_a_tool'],
    ),
    (
        'AltGr typing nothing is the letter under it',
        'typed.or(from_key.filter(|_| !altgr))',
        'typed.or(from_key)',
        ['a_key_held_with_alt_or_the_windows_key_is_not_a_tool'],
    ),
    (
        'a chord raises the list of keys',
        '        if key.key == Key::F1 && plain {',
        '        if key.key == Key::F1 {',
        ['a_key_held_with_alt_or_the_windows_key_is_not_a_tool'],
    ),
    (
        'a chorded Escape puts the list of keys away',
        '        if plain && self.show_help && key.key == Key::Escape {',
        '        if self.show_help && key.key == Key::Escape {',
        ['a_key_held_with_alt_or_the_windows_key_is_not_a_tool'],
    ),
    # -- the shortcut card is modal, for the keys and the pointer
    (
        "a key that is not the card's acts behind it",
        '        if self.show_help {\n'
        '            // Modal: every other key is the card\'s while it is up. It was\n',
        '        if false {\n'
        '            // Modal: every other key is the card\'s while it is up. It was\n',
        ['the_shortcut_card_takes_every_key_and_press_while_it_is_up'],
    ),
    (
        "a press goes through the card",
        '        if self.show_help\n'
        '            && matches!(\n'
        '                mouse.kind,\n',
        '        if false\n'
        '            && matches!(\n'
        '                mouse.kind,\n',
        ['the_shortcut_card_takes_every_key_and_press_while_it_is_up'],
    ),
    (
        "only the left button puts the card away",
        '                MouseEventKind::Press(_) | MouseEventKind::DoubleClick(_)\n'
        '            )\n'
        '        {\n'
        '            self.show_help = false;\n',
        '                MouseEventKind::Press(MouseButton::Left) | MouseEventKind::DoubleClick(_)\n'
        '            )\n'
        '        {\n'
        '            self.show_help = false;\n',
        ['the_shortcut_card_takes_every_key_and_press_while_it_is_up'],
    ),
]

if __name__ == "__main__":
    only = sys.argv[1:] or None
    raise SystemExit(sweep(SRC, MUTATIONS, "paint", timeout=900, only=only))
