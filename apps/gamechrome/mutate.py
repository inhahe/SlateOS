"""Mutation test for gamechrome's suite.

Breaks one piece of production code at a time and checks that the test which
claims to cover it is the one that fails.  A test that passes against a broken
program is not testing the program.

Three tables, one per source file: the chrome and its buttons (`lib.rs`),
the legibility reader a game's tests hold every text to (`legibility.rs`),
and the keys a game's undo history answers (`history.rs`).

Usage:  python -u apps/gamechrome/mutate.py [substring ...]
"""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "scripts"))

from mutation_harness import sweep  # noqa: E402  (path set above)

LIB = Path(__file__).parent / "src" / "lib.rs"
LEGIBILITY = Path(__file__).parent / "src" / "legibility.rs"
HISTORY = Path(__file__).parent / "src" / "history.rs"
HELP = Path(__file__).parent / "src" / "help.rs"

# (name, old, new, [tests that must fail])
LIB_MUTATIONS = [
    (
        "a button's width leaves out the room on one side",
        "    text::measure(label, font_size, FontWeightHint::Bold) + label_pad(h) * 2.0",
        "    text::measure(label, font_size, FontWeightHint::Bold) + label_pad(h)",
        ["a_button_as_wide_as_it_asks_shows_its_label_whole"],
    ),
    (
        "a label taller than its button is drawn over its edges",
        "    if line > h || room <= 0.0 {",
        "    if room <= 0.0 {",
        ["a_label_taller_than_its_button_is_left_out"],
    ),
    (
        "a button with no room still draws its face",
        "    if w <= 0.0 || h <= 0.0 {\n        return;\n    }",
        "    if false {\n        return;\n    }",
        ["a_button_with_no_room_draws_nothing"],
    ),
    (
        "a label is centred on its font size rather than its line",
        "        y: y + (h - line) / 2.0,",
        "        y: y + (h - font_size) / 2.0,",
        ["a_buttons_label_is_centred_on_its_line"],
    ),
    (
        "a label's room is measured from the button's edge, not its start",
        "        max_width: Some(x + w - label_pad(h) - text_x),",
        "        max_width: Some(room),",
        ["a_labels_room_ends_at_the_buttons_padding"],
    ),
    (
        "large text is held to the small floor",
        "            large: to(legibility::LARGE_TEXT_FLOOR),",
        "            large: to(legibility::TEXT_FLOOR),",
        ["an_ink_is_moved_only_as_far_as_it_must_be_to_read"],
    ),
    (
        "an ink is moved toward the pole that does not read",
        "    let toward = if contrast_ratio(ground, black) >= contrast_ratio(ground, white) {",
        "    let toward = if contrast_ratio(ground, black) < contrast_ratio(ground, white) {",
        ["an_ink_is_moved_only_as_far_as_it_must_be_to_read"],
    ),
    (
        "an ink is moved past its floor",
        "    for _ in 0..24 {",
        "    for _ in 0..2 {",
        ["an_ink_is_moved_only_as_far_as_it_must_be_to_read"],
    ),
    (
        "every text takes the large strength",
        "        if legibility::is_large(size, bold) {\n            self.large",
        "        if true {\n            self.large",
        ["an_ink_is_picked_by_the_size_it_is_drawn_at"],
    ),
    (
        "on leaves the secondary grey as the page's",
        "            dim: read(self.dim),",
        "            dim: self.dim,",
        ["the_chrome_on_a_ground_reads_there"],
    ),
    (
        "on moves the inks only as far as large text needs",
        "        let read = |ink: Color| Ink::on(ink, &[ground]).small;",
        "        let read = |ink: Color| Ink::on(ink, &[ground]).large;",
        ["the_chrome_on_a_ground_reads_there"],
    ),
]

LEGIBILITY_MUTATIONS = [
    (
        "a translation is ignored",
        "            RenderCommand::PushTranslate { dx: tx, dy: ty } => offsets.push((dx + tx, dy + ty)),",
        "            RenderCommand::PushTranslate { dx: tx, dy: ty } => offsets.push((dx + 0.0 * tx, dy + 0.0 * ty)),",
        ["a_translation_moves_fills_and_texts_alike"],
    ),
    (
        "a fill is painted outside the clip in force",
        "                .intersect(clip);\n                painted.push((area, Paint::Fill(*color)));",
        "                ;\n                painted.push((area, Paint::Fill(*color)));",
        ["a_fill_clipped_away_is_not_under_anything"],
    ),
    (
        "a see-through fill hides what it covers",
        "                        Paint::Fill(c) => ground.map(|g| c.over(g)),",
        "                        Paint::Fill(c) => ground.map(|_| *c),",
        ["a_see_through_fill_is_composited_over_what_it_covers"],
    ),
    (
        "an opaque fill over a picture is still unknown",
        "                        Paint::Fill(c) if c.a == u8::MAX => Some(*c),",
        "                        Paint::Fill(c) if false => Some(*c),",
        ["a_text_over_a_picture_is_not_read"],
    ),
    (
        "a picture is read as whatever was under it",
        "                        Paint::Picture => None,",
        "                        Paint::Picture => ground,",
        ["a_text_over_a_picture_is_not_read"],
    ),
    (
        "a text is read at its corner",
        "                let mid_x = x + dx + wide / 2.0;",
        "                let mid_x = x + dx + wide * 0.0;",
        ["a_text_is_read_at_the_middle_of_its_line_not_its_corner"],
    ),
    (
        "a large text read is held to the small text's floor",
        "        if self.is_large() {\n            LARGE_TEXT_FLOOR\n",
        "        if false {\n            LARGE_TEXT_FLOOR\n",
        ["a_faint_text_is_illegible_unless_exempt_and_large_text_has_the_lower_floor"],
    ),
    (
        "an exempt text is held to the floor anyway",
        "        .filter(|r| !exempt(r) && r.ratio() < r.floor())",
        "        .filter(|r| r.ratio() < r.floor())",
        ["a_faint_text_is_illegible_unless_exempt_and_large_text_has_the_lower_floor"],
    ),
    (
        "bold text is large only at the plain size",
        "    size >= 24.0 || (bold && size >= 18.66)",
        "    size >= 24.0 || (bold && size >= 24.0)",
        ["an_ink_is_picked_by_the_size_it_is_drawn_at"],
    ),
    (
        "the looks leave out the soft-text themes",
        "            (\n                \"soft text, light\",\n                soft_text(true, Color::from_hex(0x65_7B_83)),\n            ),\n            (\n                \"soft text, dark\",\n                soft_text(false, Color::from_hex(0x83_94_96)),\n            ),\n",
        "",
        ["the_looks_are_each_palette_in_either_surface_look"],
    ),
    (
        "a soft-text theme keeps the palette's text",
        "    roles.insert(\"text\".to_string(), text);",
        "    let _ = (roles, text);",
        ["a_theme_without_room_leaves_a_raised_ground_none"],
    ),
    (
        "a theme's hues are its own",
        "    p.green = from.green;",
        "",
        ["a_theme_without_room_leaves_a_raised_ground_none"],
    ),
    (
        "every look is bordered",
        "            p.set_surface_style(if cards {\n                SurfaceStyle::Cards\n",
        "            p.set_surface_style(if false {\n                SurfaceStyle::Cards\n",
        ["the_looks_are_each_palette_in_either_surface_look"],
    ),
]

HISTORY_MUTATIONS = [
    (
        "a key held with the Windows key is a history key",
        "        if !key.pressed || m.super_key {",
        "        if !key.pressed {",
        ["nothing_with_the_windows_key"],
    ),
    (
        "a release is a press",
        "        if !key.pressed || m.super_key {",
        "        if m.super_key {",
        ["a_bare_key_or_a_release_is_not_one"],
    ),
    (
        "AltGr+Z undoes",
        "            (Key::Z, true, false, false) => Some(Self::Undo),",
        "            (Key::Z, true, _, false) => Some(Self::Undo),",
        ["altgr_is_neither_ctrl_nor_alt"],
    ),
    (
        "Ctrl+Z redoes",
        "            (Key::Z, true, false, false) => Some(Self::Undo),",
        "            (Key::Z, true, false, false) => Some(Self::Redo),",
        ["the_four_are_read"],
    ),
    (
        "Ctrl+Shift+Z is not a redo",
        "            (Key::Z, true, false, true) | (Key::Y, true, false, false) => Some(Self::Redo),",
        "            (Key::Y, true, false, false) => Some(Self::Redo),",
        ["the_four_are_read"],
    ),
    (
        "Ctrl+Shift+Y redoes",
        "(Key::Y, true, false, false) => Some(Self::Redo),",
        "(Key::Y, true, false, _) => Some(Self::Redo),",
        ["other_keys_are_not_one"],
    ),
    (
        "AltGr+Z goes back",
        "            (Key::Z, false, true, false) => Some(Self::Earlier),",
        "            (Key::Z, _, true, false) => Some(Self::Earlier),",
        ["altgr_is_neither_ctrl_nor_alt"],
    ),
    (
        "a bare Z goes back",
        "            (Key::Z, false, true, false) => Some(Self::Earlier),",
        "            (Key::Z, false, _, false) => Some(Self::Earlier),",
        ["a_bare_key_or_a_release_is_not_one"],
    ),
    (
        "Alt+Shift+Z goes back too",
        "            (Key::Z, false, true, true) => Some(Self::Later),",
        "            (Key::Z, false, true, true) => Some(Self::Earlier),",
        ["the_four_are_read"],
    ),
]

HELP_MUTATIONS = [
    (
        "F1 does not raise the list",
        "    key.key == Key::F1 || (key.key == Key::Slash && m.shift) || key.single_char() == Some('?')",
        "    (key.key == Key::Slash && m.shift) || key.single_char() == Some('?')",
        ["f1_and_a_question_mark_raise_the_list"],
    ),
    (
        "the slash key with Shift does not raise it",
        "    key.key == Key::F1 || (key.key == Key::Slash && m.shift) || key.single_char() == Some('?')",
        "    key.key == Key::F1 || key.single_char() == Some('?')",
        ["f1_and_a_question_mark_raise_the_list"],
    ),
    (
        "the slash key alone raises it",
        "    key.key == Key::F1 || (key.key == Key::Slash && m.shift) || key.single_char() == Some('?')",
        "    key.key == Key::F1 || key.key == Key::Slash || key.single_char() == Some('?')",
        ["f1_and_a_question_mark_raise_the_list"],
    ),
    (
        "a ? from another key does not raise it",
        "    key.key == Key::F1 || (key.key == Key::Slash && m.shift) || key.single_char() == Some('?')",
        "    key.key == Key::F1 || (key.key == Key::Slash && m.shift)",
        ["f1_and_a_question_mark_raise_the_list"],
    ),
    (
        "a key held with Ctrl, Alt or the Windows key raises it",
        "    if !key.pressed || m.ctrl || m.alt || m.super_key {",
        "    if !key.pressed {",
        ["nothing_held_with_ctrl_alt_or_the_windows_key_raises_it"],
    ),
    (
        "a release raises it",
        "    if !key.pressed || m.ctrl || m.alt || m.super_key {",
        "    if m.ctrl || m.alt || m.super_key {",
        ["nothing_held_with_ctrl_alt_or_the_windows_key_raises_it"],
    ),
    (
        "Escape and Enter do not put it away",
        "    raises(key) || (key.pressed && matches!(key.key, Key::Escape | Key::Enter))",
        "    raises(key)",
        ["escape_enter_and_what_raised_it_put_it_away"],
    ),
    (
        "any key puts it away",
        "    raises(key) || (key.pressed && matches!(key.key, Key::Escape | Key::Enter))",
        "    key.pressed",
        ["escape_enter_and_what_raised_it_put_it_away"],
    ),
    (
        "what raised it does not put it away",
        "    raises(key) || (key.pressed && matches!(key.key, Key::Escape | Key::Enter))",
        "    key.pressed && matches!(key.key, Key::Escape | Key::Enter)",
        ["escape_enter_and_what_raised_it_put_it_away"],
    ),
    (
        "a released Escape puts it away",
        "    raises(key) || (key.pressed && matches!(key.key, Key::Escape | Key::Enter))",
        "    raises(key) || matches!(key.key, Key::Escape | Key::Enter)",
        ["escape_enter_and_what_raised_it_put_it_away"],
    ),
]


WHY = Path(__file__).parent / "src" / "why.rs"

# Why a greyed button is greyed, said while the pointer rests on it
# (2026-10-10; requests/c-e-say-why-a-control-is-disabled.md): the
# greyed boxes picked from a frame's, the pointer and the clock followed,
# the sooner tick asked for, the reason drawn, and a copy whole.
WHY_MUTATIONS = [
    (
        'the greyed boxes are every box the frame recorded',
        '        .filter_map(|(target, r)| why(target).map(|why| ((r.x, r.y, r.w, r.h), why.to_owned())))',
        '        .map(|(target, r)| ((r.x, r.y, r.w, r.h), why(target).unwrap_or("").to_owned()))',
        ['the_greyed_boxes_are_those_the_game_gives_a_reason_for'],
    ),
    (
        "a frame's greyed buttons are not kept",
        '        self.drawn = greyed;',
        '        let _ = greyed;',
        ['a_button_greyed_under_a_resting_pointer_explains_itself', 'a_greyed_button_says_why_after_the_delay_and_asks_for_the_tick', 'the_reason_goes_with_the_pointer_and_with_the_button'],
    ),
    (
        'a frame drawn does not ask about the pointer again',
        '        self.screen = screen;\n        self.settle();',
        '        self.screen = screen;',
        ['a_button_greyed_under_a_resting_pointer_explains_itself', 'the_reason_goes_with_the_pointer_and_with_the_button'],
    ),
    (
        'leaving is not the pointer gone',
        '                MouseEventKind::Leave => self.pointer_left(),',
        '                MouseEventKind::Leave => false,',
        ['the_window_s_events_move_the_pointer_and_the_clock'],
    ),
    (
        'a pointer event does not move the pointer',
        '                _ => self.pointer_at(mouse.x, mouse.y),',
        '                _ => false,',
        ['the_window_s_events_move_the_pointer_and_the_clock'],
    ),
    (
        'a tick event is not time',
        '            Event::Tick { elapsed_ms } => self.tick(*elapsed_ms),',
        '            Event::Tick { .. } => false,',
        ['the_window_s_events_move_the_pointer_and_the_clock'],
    ),
    (
        'a pointer that left is still followed',
        '        self.pointer = None;\n        self.why.pointer_left()',
        '        self.why.pointer_left()',
        ['a_pointer_that_has_left_starts_no_reason_when_a_frame_is_drawn'],
    ),
    (
        'the clock does not move',
        '        self.clock_ms = self.clock_ms.saturating_add(elapsed_ms);',
        '        let _ = elapsed_ms;',
        ['a_button_greyed_under_a_resting_pointer_explains_itself', 'a_copy_keeps_the_buttons_and_the_pointer_and_waits_again', 'a_greyed_button_says_why_after_the_delay_and_asks_for_the_tick', 'the_reason_goes_with_the_pointer_and_with_the_button', 'the_window_s_events_move_the_pointer_and_the_clock'],
    ),
    (
        'the later tick is asked for',
        '            (Some(own), Some(reason)) => Some(own.min(reason)),',
        '            (Some(own), Some(reason)) => Some(own.max(reason)),',
        ['the_window_ticks_for_the_sooner_of_its_own_and_a_reason'],
    ),
    (
        "a reason's tick is dropped for no tick of the game's",
        '            (own, reason) => own.or(reason),',
        '            (own, _) => own,',
        ['the_window_ticks_for_the_sooner_of_its_own_and_a_reason'],
    ),
    (
        'nothing is drawn',
        '        self.why.render(palette)',
        '        let _ = palette;\n        Vec::new()',
        ['a_greyed_button_says_why_after_the_delay_and_asks_for_the_tick'],
    ),
    (
        'a copy forgets the pointer',
        '            pointer: self.pointer,',
        '            pointer: None,',
        ['a_copy_keeps_the_buttons_and_the_pointer_and_waits_again'],
    ),
    (
        'a copy does not ask about the pointer',
        '        copy.settle();\n        copy',
        '        copy',
        ['a_copy_keeps_the_buttons_and_the_pointer_and_waits_again'],
    ),
]

if __name__ == "__main__":
    # A filter goes to the tables it names a row of, and only those: the
    # harness refuses a filter that selects nothing.
    only = sys.argv[1:]
    tables = [
        (LIB, LIB_MUTATIONS),
        (LEGIBILITY, LEGIBILITY_MUTATIONS),
        (HISTORY, HISTORY_MUTATIONS),
        (HELP, HELP_MUTATIONS),
        (WHY, WHY_MUTATIONS),
    ]
    names = [name for _, rows in tables for name, *_ in rows]
    unmatched = [o for o in only if not any(o in n for n in names)]
    if unmatched:
        print(f"{len(unmatched)} filter(s) name no row in any table:")
        for o in unmatched:
            print(f"  {o!r}")
        raise SystemExit(2)
    results = [0]
    for src, rows in tables:
        mine = [o for o in only if any(o in name for name, *_ in rows)]
        if only and not mine:
            continue
        results.append(sweep(src, rows, "gamechrome", timeout=600, only=mine or None))
    raise SystemExit(max(results))
