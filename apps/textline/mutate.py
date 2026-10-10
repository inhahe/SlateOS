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
ALTGR = "altgr_types_where_a_ctrl_chord_would_select_copy_cut_or_paste"
COMMAND = "a_command_types_nothing_though_it_carries_its_letter"
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
        "        Key::V if chord => insert_limited(input, clipboard, capacity),",
        "        Key::V if chord => {}",
        [CLIP, PASTE],
    ),
    (
        "a chord nobody bound types its letter",
        "    key.pressed && !is_command(key.modifiers) && key.types_text()",
        "    key.pressed && key.types_text()",
        [NOTHING, COMMAND, TABLE],
    ),
    (
        "a key that types nothing is the field's",
        "    key.pressed && !is_command(key.modifiers) && key.types_text()",
        "    key.pressed && !is_command(key.modifiers)",
        [NOTHING, TABLE],
    ),
    (
        "the field types a command's letter",
        "            if !types_into_field(key) {",
        "            if !key.types_text() {",
        [NOTHING, COMMAND],
    ),
    (
        "a release types",
        "    key.pressed && !is_command(key.modifiers) && key.types_text()",
        "    !is_command(key.modifiers) && key.types_text()",
        [TABLE],
    ),
    (
        "AltGr is a Ctrl chord",
        "    modifiers.ctrl && !modifiers.alt && !modifiers.super_key\n}",
        "    modifiers.ctrl && !modifiers.super_key\n}",
        [ALTGR, TABLE],
    ),
    # This row, "a key held with Alt alone types" and "a key held with the
    # Windows key types" name the modifier table alone: apply_key refuses
    # Alt's and the Windows key's chords before it asks whether a key is a
    # command, so the command test cannot see them.
    (
        "a Ctrl chord held with the Windows key is the program's",
        "    modifiers.ctrl && !modifiers.alt && !modifiers.super_key\n}",
        "    modifiers.ctrl && !modifiers.alt\n}",
        [TABLE],
    ),
    (
        "AltGr is a command",
        "    modifiers.super_key || modifiers.ctrl != modifiers.alt",
        "    modifiers.super_key || modifiers.ctrl || modifiers.alt",
        [ALTGR, TABLE],
    ),
    (
        "a key held with Alt alone types",
        "    modifiers.super_key || modifiers.ctrl != modifiers.alt",
        "    modifiers.super_key || (modifiers.ctrl && !modifiers.alt)",
        [TABLE],
    ),
    (
        "a key held with the Windows key types",
        "    modifiers.super_key || modifiers.ctrl != modifiers.alt",
        "    modifiers.ctrl != modifiers.alt",
        [TABLE],
    ),
    (
        "a paste of nothing eats the selection",
        "    if typed.chars().all(char::is_control) {\n        return;\n    }\n",
        "",
        [NOTHING],
    ),
    (
        "a line break is pasted into one line",
        "        if ch.is_control() {\n            continue;\n        }\n",
        "",
        [PASTE],
    ),
    (
        "the capacity counts bytes",
        "        if input.text().chars().count() >= capacity {",
        "        if input.text().len() >= capacity {",
        [PASTE],
    ),
    (
        "typing does not replace the selection",
        "    if input.has_selection() {\n        input.delete_selection();\n    }\n    for ch in typed.chars() {",
        "    for ch in typed.chars() {",
        [TYPING],
    ),
    (
        'a key held with Ctrl is plain',
        '    !modifiers.ctrl && !modifiers.alt && !modifiers.super_key',
        '    !modifiers.alt && !modifiers.super_key',
        ['a_chord_a_command_and_typing_by_modifiers'],
    ),
    (
        'a key held with Alt is plain',
        '    !modifiers.ctrl && !modifiers.alt && !modifiers.super_key',
        '    !modifiers.ctrl && !modifiers.super_key',
        ['a_chord_a_command_and_typing_by_modifiers'],
    ),
    (
        'a key held with the Windows key is plain',
        '    !modifiers.ctrl && !modifiers.alt && !modifiers.super_key',
        '    !modifiers.ctrl && !modifiers.alt',
        ['a_chord_a_command_and_typing_by_modifiers'],
    ),
    (
        'AltGr is plain',
        '    !modifiers.ctrl && !modifiers.alt && !modifiers.super_key',
        '    modifiers.ctrl == modifiers.alt && !modifiers.super_key',
        ['a_chord_a_command_and_typing_by_modifiers'],
    ),
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

if __name__ == "__main__":
    only = sys.argv[1:] or None
    raise SystemExit(sweep(SRC, MUTATIONS, "textline", timeout=600, only=only))
