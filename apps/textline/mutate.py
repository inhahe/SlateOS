"""Mutation test for the keys a one-line field answers.

Breaks one piece of production code at a time and checks that the test which
claims to cover it is the one that fails.  A test that passes against a broken
program is not testing the program.

The table was seven applications' before it was this crate's; the rows that
swept it in `apps/regextester` and `apps/qrcode` moved here with it.

Run it with no arguments to sweep everything, or with substrings of the
mutation names to run only those.
"""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "scripts"))

from mutation_harness import sweep  # noqa: E402  (path set above)

SRC = Path(__file__).parent / "src" / "lib.rs"

TYPING = "typing_goes_in_at_the_caret_and_replaces_a_selection"
SELECT = "shift_with_an_arrow_selects"
DELETE = "backspace_and_delete_take_one_character_each_way"
CLIP = "copy_and_cut_hand_back_the_selection_and_paste_types_the_clipboard"
PASTE = "a_paste_leaves_line_breaks_out_and_stops_at_the_capacity_in_characters"
NOTHING = "a_key_that_types_nothing_is_not_the_fields_and_keeps_the_selection"
TABLE = "a_chord_a_command_and_typing_by_modifiers"

# (name, old, new, [tests that must fail])
MUTATIONS = [
    (
        "the caret does not move",
        "        Key::Left => input.move_cursor_left(shift, font_size, FontWeightHint::Regular),",
        "        Key::Left => {}",
        [TYPING, SELECT, DELETE],
    ),
    (
        "shift does not select",
        "        Key::Left => input.move_cursor_left(shift, font_size, FontWeightHint::Regular),",
        "        Key::Left => input.move_cursor_left(false, font_size, FontWeightHint::Regular),",
        [SELECT],
    ),
    (
        "Home goes nowhere",
        "        Key::Home => input.move_home(shift),",
        "        Key::Home => {}",
        [SELECT],
    ),
    (
        "Delete deletes backwards",
        "        Key::Delete => input.delete(),",
        "        Key::Delete => input.backspace(),",
        [DELETE],
    ),
    (
        "a copy takes nothing",
        "        Key::C if chord => {\n            if input.has_selection() {\n"
        "                copied = Some(input.selected_text().to_string());",
        "        Key::C if chord => {\n            if input.has_selection() {\n"
        "                copied = None;",
        [CLIP],
    ),
    (
        "a cut leaves the text",
        "                copied = Some(input.selected_text().to_string());\n"
        "                input.delete_selection();\n",
        "                copied = Some(input.selected_text().to_string());\n",
        [CLIP],
    ),
    (
        "a paste types nothing",
        "        Key::V if chord => input.insert_text(clipboard),",
        "        Key::V if chord => {}",
        [CLIP, PASTE],
    ),
    # A command's letter is kept out by the toolkit's own `KeyEvent::typed`,
    # which `types_text` asks; TABLE and
    # a_command_types_nothing_though_it_carries_its_letter check that the two
    # still agree. No row here can break the toolkit's rule.
    (
        "a key that types nothing is the field's",
        "    key.pressed && key.types_text()",
        "    key.pressed",
        [NOTHING, TABLE],
    ),
    (
        "a key's release edits the field",
        "    if !key.pressed {\n        return LineEdit::default();\n    }\n",
        "",
        ["a_release_edits_nothing"],
    ),
    (
        "a release types",
        "    key.pressed && key.types_text()",
        "    key.types_text()",
        [TABLE],
    ),
    (
        "a field without a capacity goes unnoticed",
        "    debug_assert!(\n        input.capacity().is_some(),",
        "    debug_assert!(\n        true || input.capacity().is_some(),",
        ["a_field_without_a_capacity_is_caught"],
    ),
    # The Ctrl-chord, command and plain rules are the toolkit's (`Modifiers`),
    # which textline's functions call: its own table test covers them, and
    # TABLE checks that textline's names still give the same answers.
    (
        "AltGr is Alt's",
        '    (modifiers.alt && !modifiers.ctrl) || modifiers.super_key\n}',
        '    modifiers.alt || modifiers.super_key\n}',
        ['a_chord_a_command_and_typing_by_modifiers'],
    ),
    (
        "Alt alone is not Alt's",
        '    (modifiers.alt && !modifiers.ctrl) || modifiers.super_key\n}',
        '    modifiers.super_key\n}',
        ['a_chord_a_command_and_typing_by_modifiers'],
    ),
    (
        "the Windows key is not the desktop's",
        '    (modifiers.alt && !modifiers.ctrl) || modifiers.super_key\n}',
        '    modifiers.alt && !modifiers.ctrl\n}',
        ['a_chord_a_command_and_typing_by_modifiers'],
    ),
    (
        'a key held with Alt or the Windows key edits the field',
        '    if is_alt_or_windows_chord(key.modifiers) {\n        return LineEdit::default();\n    }\n',
        '',
        ['a_key_held_with_alt_or_the_windows_key_edits_nothing'],
    ),
]

# A masked field (2026-10-04): one mask for each character, the caret and the
# selection on the same characters, a press mapped back, the arrows stepping a
# character at a time, and nothing copied or cut.
MASKS = "a_masked_field_masks_each_character_and_keeps_its_caret_on_it"
STEPS = "a_masked_field_steps_by_character_and_gives_nothing_to_the_clipboard"

MUTATIONS += [
    (
        "a mask for each byte",
        "    let shown: String = text.chars().map(|_| mask).collect();\n",
        "    let shown: String = text.bytes().map(|_| mask).collect();\n",
        [MASKS],
    ),
    (
        "the caret counted in characters, not the mask's bytes",
        "        chars.saturating_mul(mask.len_utf8())\n",
        "        chars\n",
        [MASKS],
    ),
    (
        "a press counted in the mask's bytes, not its characters",
        "    let nth = at.checked_div(mask.len_utf8()).unwrap_or(0);\n",
        "    let nth = at;\n",
        [MASKS],
    ),
    (
        "a press past the end is the start",
        "        .map_or(text.len(), |(byte, _)| byte)\n",
        "        .map_or(0, |(byte, _)| byte)\n",
        [MASKS],
    ),
    (
        "Alt+Left steps in a masked field",
        "        Key::Left | Key::Right if !is_alt_or_windows_chord(key.modifiers) => {\n",
        "        Key::Left | Key::Right => {\n",
        [STEPS],
    ),
    (
        "a masked field copies and cuts",
        "        Key::C | Key::X if chord => LineEdit {\n",
        "        Key::C | Key::X if false => LineEdit {\n",
        [STEPS],
    ),
    (
        "an unshifted arrow steps out of a selection",
        "    if !shift && input.has_selection() {\n",
        "    if false {\n",
        [STEPS],
    ),
    (
        "Shift does not select in a masked field",
        "    textedit::begin_or_end_selection(shift, input.cursor(), &mut anchor);\n",
        "",
        [STEPS],
    ),
    (
        "a step forward is a byte",
        "            .map(|c| at.saturating_add(c.len_utf8()))\n",
        "            .map(|_| at.saturating_add(1))\n",
        [STEPS],
    ),
    (
        "a step back is a byte",
        "            .map(|c| at.saturating_sub(c.len_utf8()))\n",
        "            .map(|_| at.saturating_sub(1))\n",
        [STEPS],
    ),
]

# The keys on the program's clipboard (2026-10-10), the one a field's
# right-click menu copies to and pastes from.
SHARED = "the_keys_and_the_menu_share_the_programs_clipboard"

MUTATIONS += [
    (
        "a copy by key is not put on the program's clipboard",
        "    if let Some(copied) = &edit.copied {\n        guitk::clipboard::set_text(copied);\n    }\n",
        "",
        [SHARED],
    ),
    (
        "a paste by key reads no program's clipboard",
        "        guitk::clipboard::text()\n    } else {\n",
        "        String::new()\n    } else {\n",
        [SHARED],
    ),
    (
        "pastes names another key than the paste",
        "    key.pressed && key.key == Key::V && is_ctrl_chord(key.modifiers)\n",
        "    key.pressed && key.key == Key::C && is_ctrl_chord(key.modifiers)\n",
        ["pastes_is_the_key_apply_key_pastes_on"],
    ),
    (
        "pastes counts a release",
        "    key.pressed && key.key == Key::V && is_ctrl_chord(key.modifiers)\n",
        "    key.key == Key::V && is_ctrl_chord(key.modifiers)\n",
        ["pastes_is_the_key_apply_key_pastes_on"],
    ),
    (
        "a field holding a capacity holds none",
        "    input.set_text(text);\n"
        "    input.set_capacity(Some(capacity.min(MOST_A_FIELD_KEEPS)));\n"
        "    input\n}\n",
        "    input.set_text(text);\n    input\n}\n",
        ["a_field_holding_a_capacity_fills_to_it"],
    ),
    (
        "an enormous capacity is read as none",
        "    input.set_capacity(Some(capacity.min(MOST_A_FIELD_KEEPS)));\n",
        "    input.set_capacity(Some(capacity));\n",
        ["a_field_holding_a_capacity_fills_to_it"],
    ),
    (
        "a field built without a capacity is given none",
        "        input.set_capacity(Some(UNBUILT_FIELD_CAPACITY));\n",
        "",
        ["a_field_built_without_a_capacity_is_given_one"],
    ),
]

LINE_SRC = Path(__file__).parent / "src" / "linebox.rs"

S_TYPING = "typing_edits_at_the_caret_and_says_the_text_changed"
S_WINDOWS = "a_key_the_box_does_not_answer_is_the_windows"
S_RELEASE = "a_release_does_nothing"
S_CAPACITY = "typing_stops_at_the_capacity"
S_SAME = "setting_the_same_text_keeps_the_caret"
S_CLEAR = "clear_says_whether_there_was_text"
S_PRESS = "a_press_puts_the_caret_where_it_landed"
S_DRAWN = "a_press_is_measured_against_the_text_as_it_was_drawn"
S_MENU = "the_menu_edits_the_text"
S_FOCUSED = "a_box_with_the_keyboard_draws_its_caret_on_the_line"
S_UNFOCUSED = "a_box_without_the_keyboard_draws_its_text_or_its_placeholder"
S_EMPTY_FOCUSED = "an_empty_box_with_the_keyboard_says_what_it_is_for_under_its_caret"
S_SELECT_ALL = "select_all_types_over_the_text"
S_TO_END = "caret_to_end_types_after_the_text_and_over_no_selection"

INSERT = "a_box_types_what_it_is_given_at_its_caret"
# A box read from its end (diskanalyzer's path): cut at its front without
# the keyboard, and pressed where that was drawn.
S_FROM_END = "a_box_read_from_its_end_is_cut_at_the_front_without_the_keyboard"
S_TAIL_PRESS = "a_press_in_a_box_read_from_its_end_lands_on_the_tail_it_showed"

LINE_MUTATIONS = [
    (
        "the box holds no capacity",
        '            input: crate::field_holding(text, capacity),\n',
        '            input: crate::field_holding(text, usize::MAX),\n',
        [S_CAPACITY, "a_box_made_holding_text_types_after_it"],
    ),
    (
        "an edit the check refuses is kept",
        "            self.input = kept;\n",
        "",
        ["an_edit_the_check_refuses_is_undone"],
    ),
    (
        "every key is checked, a caret's too",
        "        if edit.changed && !check(self.input.text()) {\n",
        "        if !check(self.input.text()) {\n",
        ["an_edit_the_check_refuses_is_undone"],
    ),
    (
        "a refused edit says it changed the text",
        "                changed: false,\n                moved: false,\n                copied: edit.copied,\n",
        "                changed: true,\n                moved: false,\n                copied: edit.copied,\n",
        ["an_edit_the_check_refuses_is_undone"],
    ),
    (
        "what a box is given to type is not typed",
        "            input.insert_text(text);\n",
        "",
        [INSERT],
    ),
    (
        "typing what a box is given is not the box's",
        "            crate::LineEdit {\n                handled: true,\n",
        "            crate::LineEdit {\n                handled: false,\n",
        [INSERT],
    ),
    (
        "a box made holding text holds none",
        '            input: crate::field_holding(text, capacity),\n',
        '            input: crate::field_holding("", capacity),\n',
        ["a_box_made_holding_text_types_after_it"],
    ),
    (
        "setting the same text counts as a change",
        "        if self.input.text() == text {\n            return false;\n        }\n",
        "",
        [S_SAME, S_CLEAR],
    ),
    (
        "a key that changes nothing says the text changed",
        "            changed: self.input.text() != before,\n",
        "            changed: true,\n",
        [S_TYPING, S_WINDOWS, S_RELEASE, S_CAPACITY, INSERT],
    ),
    (
        "a key never says the caret moved",
        "            moved: caret(&self.input) != caret_before,\n",
        "            moved: false,\n",
        [S_TYPING, "a_copy_or_a_cut_says_it_took_the_selection", INSERT],
    ),
    (
        "a key that leaves the caret says it moved",
        "            moved: caret(&self.input) != caret_before,\n",
        "            moved: true,\n",
        [S_TYPING, "a_copy_or_a_cut_says_it_took_the_selection", S_WINDOWS, S_RELEASE, S_CAPACITY, INSERT],
    ),
    (
        "the box keeps a clipboard of its own",
        "        self.edited(|input| crate::apply_key_on_clipboard(input, key, font_size))\n",
        '        self.edited(|input| crate::apply_key(input, key, "", font_size))\n',
        ["a_copy_or_a_cut_says_it_took_the_selection"],
    ),
    (
        "a copy does not say it took the selection",
        "            copied: edit.copied.is_some(),\n",
        "            copied: false,\n",
        ["a_copy_or_a_cut_says_it_took_the_selection"],
    ),
    (
        "a key that copies nothing says it copied",
        "            copied: edit.copied.is_some(),\n",
        "            copied: edit.handled,\n",
        ["a_copy_or_a_cut_says_it_took_the_selection", S_TYPING, S_CAPACITY, INSERT],
    ),
    (
        "a key the box does not answer is taken from the window",
        "            handled: edit.handled,\n",
        "            handled: true,\n",
        [S_WINDOWS, S_RELEASE],
    ),
    (
        "a press keeps the selection",
        "        self.input.set_selection_anchor(None);\n        self.input.set_cursor(cursor);\n",
        "        self.input.set_cursor(cursor);\n",
        [S_PRESS],
    ),
    (
        "a press without the keyboard is measured as though scrolled",
        "        let drawn = if focused {\n",
        "        let drawn = if true {\n",
        [S_DRAWN],
    ),
    (
        "a box given a new size keeps its old one",
        "        self.font_size = font_size;\n",
        "        let _ = font_size;\n",
        ["a_resized_box_measures_and_draws_at_its_new_size"],
    ),
    (
        "a press with the keyboard is measured from the start",
        "        let drawn = if focused {\n",
        "        let drawn = if false {\n",
        [S_DRAWN],
    ),
    (
        "a menu row that changed nothing says the text changed",
        "        self.input.edit_command(id) == KeyEdit::Changed\n",
        "        self.input.edit_command(id) != KeyEdit::Unhandled\n",
        [S_MENU],
    ),
    (
        "the box with the keyboard draws no caret",
        "                    focused: true,\n                    x: look.x,\n",
        "                    focused: false,\n                    x: look.x,\n",
        [S_FOCUSED],
    ),
    (
        "a box without the keyboard draws a caret",
        "        if look.focused {\n            textedit::draw(\n",
        "        if true {\n            textedit::draw(\n",
        [S_UNFOCUSED],
    ),
    (
        "an empty box draws no placeholder",
        "            if !look.placeholder.is_empty() {\n",
        "            if false {\n",
        [S_UNFOCUSED, S_EMPTY_FOCUSED],
    ),
    (
        "an empty placeholder is drawn",
        "            if !look.placeholder.is_empty() {\n",
        "            if true {\n",
        [S_EMPTY_FOCUSED],
    ),
    (
        "the placeholder is drawn over the text",
        "        if self.is_empty() {\n            // The placeholder",
        "        if true {\n            // The placeholder",
        [S_UNFOCUSED, S_FOCUSED],
    ),
    (
        "an empty box with the keyboard draws no caret",
        "            if look.focused {\n                textedit::push_caret(\n",
        "            if false {\n                textedit::push_caret(\n",
        [S_EMPTY_FOCUSED],
    ),
    (
        "an empty box draws a line of text of its own",
        "            }\n            return;\n        }\n        if look.focused {\n",
        "            }\n        }\n        if look.focused {\n",
        [S_EMPTY_FOCUSED],
    ),
    (
        "the placeholder is drawn in the text's colour",
        "                look.placeholder_color,\n",
        "                look.color,\n",
        [S_UNFOCUSED, S_EMPTY_FOCUSED],
    ),
    (
        "text without the keyboard is not drawn",
        "            push_line(tree, look, self.input.text(), look.color, self.font_size);\n",
        "",
        [S_UNFOCUSED],
    ),
    (
        "the text drops a pixel when the box has not the keyboard",
        "        y: look.y,\n        text: text.to_string(),\n",
        "        y: look.y + 1.0,\n        text: text.to_string(),\n",
        [S_UNFOCUSED],
    ),
    (
        "long text runs out of the box",
        "        max_width: Some(look.width.max(0.0)),\n",
        "        max_width: None,\n",
        [S_UNFOCUSED],
    ),
    (
        "long text is cut without saying so",
        "        overflow: TextOverflow::Ellipsis,\n",
        "        overflow: TextOverflow::Clip,\n",
        [S_UNFOCUSED],
    ),
    (
        "caret_to_end leaves the caret where it was",
        "        self.input\n            .set_cursor(TextCursor::from(self.input.text().len()));\n",
        "",
        [S_TO_END],
    ),
    (
        "caret_to_end keeps the selection",
        "    pub fn caret_to_end(&mut self) {\n        self.input.set_selection_anchor(None);\n",
        "    pub fn caret_to_end(&mut self) {\n",
        [S_TO_END],
    ),
    (
        "select_all selects nothing",
        "        self.input.select_all();\n",
        "",
        [S_SELECT_ALL],
    ),
    (
        "a box read from its end is read from its start",
        "        self.from_end = true;\n",
        "",
        [S_FROM_END, S_TAIL_PRESS],
    ),
    (
        "a box read from its end is drawn as any other",
        "        } else if self.from_end {\n",
        "        } else if false {\n",
        [S_FROM_END],
    ),
    (
        "a box read from its end is cut at its end",
        "                text: guitk::text::elide_start(\n",
        "                text: guitk::text::elide(\n",
        [S_FROM_END],
    ),
    (
        "a box read from its end runs out of the box",
        "                max_width: Some(width),\n",
        "                max_width: None,\n",
        [S_FROM_END],
    ),
    (
        "a press in a box read from its end is measured from its start",
        "        let cursor = if !focused && self.from_end {\n",
        "        let cursor = if false {\n",
        [S_TAIL_PRESS],
    ),
    (
        "a press in a box read from its end with the keyboard is measured against its tail",
        "        let cursor = if !focused && self.from_end {\n",
        "        let cursor = if self.from_end {\n",
        [S_TAIL_PRESS],
    ),
    (
        "text that fits is pressed as though cut",
        "        if guitk::text::measure(text, self.font_size, weight) <= width {\n",
        "        if false {\n",
        [S_TAIL_PRESS],
    ),
    (
        "a press where nothing was drawn puts the caret at the start",
        "            return TextCursor::from(text.len());\n",
        "            return TextCursor::default();\n",
        [S_TAIL_PRESS],
    ),
    (
        "a press on the tail is measured from the box's edge, not after the ellipsis",
        "        let within = guitk::text::cursor_at(tail, dx - lead, self.font_size, weight);\n",
        "        let within = guitk::text::cursor_at(tail, dx, self.font_size, weight);\n",
        [S_TAIL_PRESS],
    ),
    (
        "a press on the tail is counted from the tail's start",
        "        TextCursor::from(start.saturating_add(within.byte()))\n",
        "        within\n",
        [S_TAIL_PRESS],
    ),
]

if __name__ == "__main__":
    only = sys.argv[1:] or None
    worst = 0
    for src, rows in ((SRC, MUTATIONS), (LINE_SRC, LINE_MUTATIONS)):
        mine = [o for o in only if any(o in r[0] for r in rows)] if only else None
        if only and not mine:
            continue
        print(f"\n######## textline/{src.name} ########")
        worst = max(worst, sweep(src, rows, "textline", timeout=600, only=mine))
    raise SystemExit(worst)
