"""Mutation test for the QR code generator: its symbols, its barcodes, its
history, its boxes, its pointer layer, and saving.

Breaks one piece of production code at a time and checks that the test which
claims to cover it is the one that fails.  A test that passes against a broken
program is not testing the program.

Nothing answered the pointer (known-issues,
TD-C-TWENTY-ONE-APPLICATIONS-DRAW-A-UI-THAT-CANNOT-BE-CLICKED); the history
kept one entry per keystroke; nothing could be saved; the colours could not
be changed; and the symbols themselves were wrong in ways no test looked at --
version 7-10 QR codes had no version information, version 10's alignment
patterns were two modules off, and the Code128 table was corrupt from value
60, so no barcode with a lowercase letter could be read.

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
    # -- the symbols ---------------------------------------------------------------------------------------
    (
        "versions 7-10 carry no version information",
        "        base_matrix.place_version_info(version);",
        "        let _ = version;",
        ["every_symbol_reads_back_as_what_was_encoded"],
    ),
    (
        "version 10's alignment is two modules off",
        "        10 => vec![6, 28, 50],",
        "        10 => vec![6, 28, 52],",
        ["every_symbol_reads_back_as_what_was_encoded"],
    ),
    (
        "the second format copy loses bit 7",
        "    for i in 0..8 {\n"
        "        matrix.set(8, size - 1 - i, module(i));",
        "    for i in 0..7 {\n"
        "        matrix.set(8, size - 1 - i, module(i));",
        ["every_symbol_reads_back_as_what_was_encoded"],
    ),
    (
        "the version information has no check",
        "        rem = (rem << 1) ^ ((rem >> 11) * 0x1F25);",
        "        rem <<= 1;",
        ["version_information_is_the_standards"],
    ),
    (
        "a Code128 letter is the wrong bars",
        "    [1, 2, 1, 1, 2, 4], // 65: a",
        "    [1, 2, 1, 1, 4, 2], // 65: a",
        ["every_code128_pattern_is_well_formed", "the_code128_table_is_the_standards"],
    ),
    (
        "a barcode drops what it cannot hold",
        "            if !(32..=127).contains(&ascii_val) {\n"
        "                return None;",
        "            if !(32..=127).contains(&ascii_val) {\n"
        "                continue;",
        ["a_barcode_refuses_what_it_cannot_hold"],
    ),
    (
        "the barcode refusal is never asked",
        "                if let Some(why) = code128_refusal(&data) {",
        "                if let Some(why) = None::<String> {",
        ["a_barcode_refuses_what_it_cannot_hold"],
    ),
    (
        "too long names the wrong limit",
        "                        get_version_info(10, self.ec_level).map_or(0, |v| v.byte_mode_capacity());",
        "                        get_version_info(1, self.ec_level).map_or(0, |v| v.byte_mode_capacity());",
        ["too_long_says_how_much_fits"],
    ),
    # -- nothing to encode -----------------------------------------------------------------------------------------
    (
        "an empty box is encoded as its prefix",
        "                self.input_text.is_empty().then_some(NOTHING_TYPED)",
        "                false.then_some(NOTHING_TYPED)",
        ["an_empty_box_is_no_code_in_any_kind", "test_app_generate_empty"],
    ),
    (
        "a WiFi code is made with no network",
        "                .then_some(\"A WiFi code needs the network's name (SSID)\"),",
        "                .then_some(\"A WiFi code needs the network's name (SSID)\")\n"
        "                .filter(|_| false),",
        ["an_empty_box_is_no_code_in_any_kind"],
    ),
    (
        "an emptied box keeps its code on screen",
        "        if self.waiting_for.is_some() {\n"
        "            self.current_qr = None;\n"
        "            self.current_barcode = None;",
        "        if self.waiting_for.is_some() {",
        ["emptying_the_box_takes_the_code_away"],
    ),
    # -- the history -----------------------------------------------------------------------------------------------
    (
        "every keystroke is an entry",
        "            Some(last) if self.history_open => *last = entry,",
        "            Some(last) if false => *last = entry,",
        ["typing_one_thing_leaves_one_history_entry"],
    ),
    (
        "an emptied box does not finish its entry",
        "            // The code being typed is finished with; what is typed next is\n"
        "            // a new one.\n"
        "            self.history_open = false;",
        "",
        ["a_new_code_starts_a_new_entry"],
    ),
    (
        "another kind continues the old entry",
        "        // The same text as a URL is a different code from it as text.\n"
        "        self.history_open = false;",
        "",
        ["another_kind_of_the_same_words_is_a_new_entry"],
    ),
    (
        "the same code twice is two entries",
        "            self.history.remove(older);",
        "            let _ = older;",
        ["the_same_code_twice_is_one_entry"],
    ),
    (
        "a history row does nothing",
        "            Target::HistoryRow(index) => self.restore(index),",
        "            Target::HistoryRow(_) => {}",
        ["a_history_row_brings_its_code_back", "the_history_scrolls"],
    ),
    (
        "a brought-back code is not the newest",
        "        self.focused_field = 0;\n"
        "        self.history.push(entry);",
        "        self.focused_field = 0;\n"
        "        self.history.insert(index, entry);",
        ["a_history_row_brings_its_code_back"],
    ),
    (
        "the history does not scroll",
        "        self.history_scroll = next;",
        "        let _ = next;",
        ["the_history_scrolls"],
    ),
    (
        "the history scrolls past its end",
        "            now.saturating_add(rows.unsigned_abs())\n"
        "        }\n"
        "        .min(last);",
        "            now.saturating_add(rows.unsigned_abs())\n"
        "        };\n"
        "        let _ = last;",
        ["the_history_scrolls"],
    ),
    (
        "Forget is offered with nothing to forget",
        "            !self.history.is_empty(),",
        "            true,",
        ["every_setting_answers_the_pointer"],
    ),
    # -- the boxes -------------------------------------------------------------------------------------------------------
    (
        "the field is given no keys",
        "        let done = textline::apply_key(&mut self.editor, key, MAX_FIELD_CHARS, &clipboard, 12.0);",
        "        let done = textline::LineEdit::default();\n        let _ = &clipboard;",
        ["the_caret_moves_and_typing_goes_where_it_is", "every_advertised_key_does_something"],
    ),
    (
        "a box changed from outside is typed over",
        "        if self.editor.text() != self.field_text(field) {",
        "        if false {",
        ["a_box_changed_from_outside_is_typed_onto"],
    ),
    (
        "Escape empties nothing",
        "            Key::Escape => self.clear_box(),",
        "            Key::Escape => EventResult::Ignored,",
        ["escape_clears_the_field_and_does_nothing_when_it_is_clear", "a_new_code_starts_a_new_entry"],
    ),
    (
        "the list of keys lets keys through",
        "        // The list of keys is modal while it is up.\n"
        "        if self.show_help {",
        "        // The list of keys is modal while it is up.\n"
        "        if false {",
        ["the_list_of_keys_is_modal"],
    ),
    # -- the pointer -------------------------------------------------------------------------------------------------------
    (
        "a press in a box leaves the keys where they were",
        "                self.focused_field = at;",
        "                let _ = at;",
        ["a_press_in_a_box_puts_the_keys_there"],
    ),
    (
        "a press on a kind does nothing",
        "                self.set_input_mode(mode);",
        "                let _ = mode;",
        ["a_press_in_a_box_puts_the_keys_there", "every_setting_answers_the_pointer"],
    ),
    (
        "the security button does nothing",
        "            Target::Encryption => return self.step_encryption(),",
        "            Target::Encryption => return EventResult::Ignored,",
        ["every_setting_answers_the_pointer"],
    ),
    (
        "a press goes through the list of keys",
        "            f.hit(Target::HelpCard, Rect::new(0.0, 0.0, width, height));",
        "",
        ["the_list_of_keys_is_modal", "every_setting_answers_the_pointer"],
    ),
    # -- saving ----------------------------------------------------------------------------------------------------------------
    (
        "Save is offered with nothing to save",
        "        self.button(f, save, \"Save SVG\\u{2026}\", Target::Save, has_code);",
        "        self.button(f, save, \"Save SVG\\u{2026}\", Target::Save, true);",
        ["saving_is_offered_only_with_a_code"],
    ),
    (
        "Ctrl+S saves nothing",
        "            Key::S if ctrl => self.ask_where_to_save(),",
        "            Key::S if ctrl => EventResult::Ignored,",
        ["every_advertised_key_does_something"],
    ),
    (
        "the picture has no margin",
        "            let quiet = 4;",
        "            let quiet = 0;",
        ["a_saved_picture_is_the_code"],
    ),
    (
        "the picture ignores the module size",
        "        let scale = self.module_size.whole_pixels();",
        "        let scale = 1;",
        ["a_saved_picture_is_the_code"],
    ),
    (
        "a run is drawn a square short",
        "                    let run = col - start;",
        "                    let run = col - start - 1;",
        ["a_saved_picture_is_the_code"],
    ),
    (
        "a barcode's label is not escaped",
        "            label = xml_escape(&bc.data),",
        "            label = bc.data.clone(),",
        ["a_saved_barcode_keeps_its_label_as_text"],
    ),
    # -- the colours ------------------------------------------------------------------------------------------------------------
    (
        "a chosen colour is not used",
        "                    Swatch::Foreground => self.fg_color = color,",
        "                    Swatch::Foreground => {}",
        ["the_colours_are_chosen_in_the_colour_dialog"],
    ),
    (
        "a cancelled colour dialog stays up",
        "            Some(ColorPickerEvent::Cancelled) => self.color_dialog = None,",
        "            Some(ColorPickerEvent::Cancelled) => {}",
        ["the_colours_are_chosen_in_the_colour_dialog"],
    ),
    (
        "Black on white is offered on black and white",
        "            (self.fg_color, self.bg_color) != (Color::BLACK, Color::WHITE),",
        "            true,",
        ["the_colours_are_chosen_in_the_colour_dialog"],
    ),
    (
        "an inverted code is not warned about",
        "        dark < light && (light + 0.05) / (dark + 0.05) >= 3.0",
        "        (light + 0.05) / (dark + 0.05) >= 3.0 || dark > light",
        ["pale_squares_are_warned_about"],
    ),
    (
        "pale grey is not warned about",
        "        dark < light && (light + 0.05) / (dark + 0.05) >= 3.0",
        "        dark < light && (light + 0.05) / (dark + 0.05) >= 1.0",
        ["pale_squares_are_warned_about"],
    ),
    (
        "a big code runs out of its panel",
        "        let module_px = self.module_size.pixels().min(fit);",
        "        let module_px = self.module_size.pixels();",
        ["a_big_code_is_shown_smaller_to_fit"],
    ),
    (
        "a barcode's label is black whatever the ground",
        "            text: barcode.data.clone(),\n"
        "            color: self.fg_color,",
        "            text: barcode.data.clone(),\n"
        "            color: Color::BLACK,",
        ["the_barcode_label_is_in_the_codes_ink"],
    ),
]

if __name__ == "__main__":
    only = sys.argv[1:] or None
    raise SystemExit(sweep(SRC, MUTATIONS, "qrcode", timeout=900, only=only))
