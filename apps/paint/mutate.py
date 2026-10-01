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

MUTATIONS = [
    (
        "an edit does not mark the picture",
        "        self.history.push(snapshot);\n        self.dirty = true;\n",
        "        self.history.push(snapshot);\n",
        [MARKS, CLOSE],
    ),
    (
        "an undo does not mark the picture",
        "            self.active_layer = prev.active_layer;\n            self.dirty = true;\n",
        "            self.active_layer = prev.active_layer;\n",
        [MARKS],
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
]

if __name__ == "__main__":
    only = sys.argv[1:] or None
    raise SystemExit(sweep(SRC, MUTATIONS, "paint", timeout=900, only=only))
