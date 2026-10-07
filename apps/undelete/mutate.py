"""Mutation test for the file recovery tool's signature table.

Breaks one piece of production code at a time and checks that the test which
claims to cover it is the one that fails.  A test that passes against a broken
program is not testing the program.

Covers what changed on 2026-09-28: a file of the ISO base media family (an
`ftyp` box first) is named by its brand.  The bare `ftyp` recovered a phone's
HEIC photographs and every AVIF as `.mp4` videos.

Run it with no arguments to sweep everything, or with substrings of the
mutation names to run only those.
"""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "scripts"))

from mutation_harness import sweep  # noqa: E402  (path set above)

SRC = Path(__file__).parent / "src" / "main.rs"

BRAND = "an_iso_media_file_is_named_by_its_brand"

# (name, old, new, [tests that must fail])
MUTATIONS = [
    (
        "a HEIC photograph is recovered as an MP4",
        '        FileSignature::new(FileSignatureKind::Heic, 4, b"ftyp").with_secondary(8, b"heic"),\n',
        "",
        [BRAND],
    ),
    (
        "an AVIF is recovered as an MP4",
        '        FileSignature::new(FileSignatureKind::Avif, 4, b"ftyp").with_secondary(8, b"avif"),\n',
        "",
        [BRAND],
    ),
    (
        "M4A audio is filed as a video",
        "            Self::Mp3 | Self::Flac | Self::Ogg | Self::Wav | Self::M4a => FileCategory::Audio,\n"
        "            Self::Mp4 | Self::Mov | Self::Avi | Self::Mkv => FileCategory::Video,",
        "            Self::Mp3 | Self::Flac | Self::Ogg | Self::Wav => FileCategory::Audio,\n"
        "            Self::Mp4 | Self::M4a | Self::Mov | Self::Avi | Self::Mkv => FileCategory::Video,",
        [BRAND],
    ),
    (
        'a chord raises the list of keys',
        '        if key.key == Key::F1 && plain {',
        '        if key.key == Key::F1 {',
        ['a_chord_is_neither_an_undelete_key_nor_typing'],
    ),
    (
        'a chorded Escape puts the list of keys away',
        '            if plain && matches!(key.key, Key::Escape | Key::Enter) {',
        '            if matches!(key.key, Key::Escape | Key::Enter) {',
        ['a_chord_is_neither_an_undelete_key_nor_typing'],
    ),
    (
        'AltGr is taken for Ctrl',
        '        if textline::is_ctrl_chord(key.modifiers) {\n            return match key.key {',
        '        if key.modifiers.ctrl {\n            return match key.key {',
        ['a_chord_is_neither_an_undelete_key_nor_typing'],
    ),
    (
        "a chord works the setup screen's keys",
        '            _ if !plain => EventResult::Ignored,\n            UiScreen::ScanSetup',
        '            UiScreen::ScanSetup',
        ['a_chord_is_neither_an_undelete_key_nor_typing'],
    ),
    (
        'Alt+Backspace deletes from the search',
        '            Key::Backspace if !textline::is_alt_or_windows_chord(key.modifiers) => {',
        '            Key::Backspace => {',
        ['a_chord_is_neither_an_undelete_key_nor_typing'],
    ),
    (
        "a chord works the results' keys",
        '            _ if !plain => self.type_into_search(key),\n',
        '',
        ['a_chord_is_neither_an_undelete_key_nor_typing'],
    ),
    (
        "the search types a command's letter",
        '        if !textline::types_into_field(key) {\n            return EventResult::Ignored;\n        }\n        let typed: String = key.typed().collect();',
        '        let typed: String = key.typed().collect();',
        ['a_chord_is_neither_an_undelete_key_nor_typing'],
    ),
    # -- the toolkit's radio buttons and check boxes (c-e-the-toolkit-has-switches-...)
    (
        "the chosen scan mode is not dotted",
        "            self.scan_mode == mode,\n",
        "            false,\n",
        ['the_scan_modes_are_the_toolkits_radio_buttons'],
    ),
    (
        "a scan mode is drawn the same wherever the pointer is",
        "                hovered: self.hover == Some(Control::Mode(mode)),",
        "                hovered: false,",
        ['the_scan_modes_are_the_toolkits_radio_buttons'],
    ),
    (
        "a row's box is drawn the same wherever the pointer is",
        "            hovered: self.hover == Some(Control::File(index)),",
        "            hovered: false,",
        ['a_rows_box_is_the_toolkits_check_box'],
    ),
    (
        "a file to be recovered is not ticked",
        "            if file.selected {\n                CheckState::Checked",
        "            if false {\n                CheckState::Checked",
        ['a_rows_box_is_the_toolkits_check_box'],
    ),
    (
        "the pointer is not followed",
        "                let over = self.control_at(mouse.x, mouse.y);",
        "                let over = None::<Control>;",
        ['the_scan_modes_are_the_toolkits_radio_buttons', 'a_rows_box_is_the_toolkits_check_box'],
    ),
    (
        "the pointer leaving the window leaves its control lit",
        "                if self.hover.take().is_some() {",
        "                if self.hover.is_some() {",
        ['a_rows_box_is_the_toolkits_check_box'],
    ),
]

CARD = "the_shortcut_card_takes_a_press_rather_than_passing_it_on"
TOUCHPAD = "a_touchpads_small_turns_add_up_to_rows"
FRACTION = "a_fraction_of_a_notch_does_not_outlive_the_list"

MUTATIONS += [
    # The results' wheel adds a touchpad's small turns up (2026-10-04).
    (
        "a fraction of a notch is truncated to nothing",
        "                let rows = self.results_wheel.rows(dy);\n",
        "                let rows = guitk::wheel::rows_f(dy) as isize;\n",
        [TOUCHPAD],
    ),
    (
        "a turn that moves nothing says it moved",
        "                if self.scroll_offset == before {\n",
        "                if false {\n",
        [TOUCHPAD],
    ),
    (
        "a fraction of a notch outlives the list",
        "        self.results_wheel.reset();\n",
        "",
        [FRACTION],
    ),
]

MUTATIONS += [
    # The list of keys takes the pointer (2026-10-04; known-issues
    # E-a-press-goes-through-the-shortcut-card-to-the-control-drawn-under-it).
    (
        "a press goes through the list of keys",
        "                MouseEventKind::Press(_) | MouseEventKind::DoubleClick(_) => {\n"
        "                    self.show_help = false;\n"
        "                    return EventResult::Consumed;\n"
        "                }\n",
        "",
        [CARD],
    ),
    (
        "only the left button puts the list of keys away",
        "                MouseEventKind::Press(_) | MouseEventKind::DoubleClick(_) => {\n",
        "                MouseEventKind::Press(MouseButton::Left) | MouseEventKind::DoubleClick(_) => {\n",
        [CARD],
    ),
    (
        "a press under the list of keys leaves it up",
        "                    self.show_help = false;\n"
        "                    return EventResult::Consumed;\n",
        "                    return EventResult::Consumed;\n",
        [CARD],
    ),
    (
        "the wheel scrolls what the list of keys covers",
        "                MouseEventKind::Scroll { .. } => return EventResult::Ignored,\n",
        "",
        [CARD],
    ),
]

# The search is drawn, in the toolkit's field, and keeps the case it was
# typed in (2026-10-04; lane C, c-e-a-theme-can-shape-the-controls).
SEARCH = "the_search_is_drawn_in_the_toolkits_field"

MUTATIONS += [
    (
        "the search is typed blind",
        "        self.render_search_box(cmds);\n",
        "",
        [SEARCH],
    ),
    (
        "the search box never has the keyboard's mark",
        "            focused: self.screen == UiScreen::Results && !self.show_help,\n",
        "            focused: false,\n",
        [SEARCH],
    ),
    (
        "the search box keeps its mark under the list of keys",
        "            focused: self.screen == UiScreen::Results && !self.show_help,\n",
        "            focused: self.screen == UiScreen::Results,\n",
        [SEARCH],
    ),
    (
        "a search that matches nothing is not red",
        "            invalid: !self.filter.filename_search.is_empty() && self.visible_files().is_empty(),\n",
        "            invalid: false,\n",
        [SEARCH],
    ),
    (
        "the search box shows away from the results",
        "        (self.screen == UiScreen::Results).then(|| {\n",
        "        true.then(|| {\n",
        [SEARCH],
    ),
    (
        "the search's caret is at its start",
        "                    cursor: TextCursor::from(query.len()),\n",
        "                    cursor: TextCursor::default(),\n",
        [SEARCH],
    ),
    (
        "an empty search box with the keyboard has no caret",
        "                textedit::push_caret(\n"
        "                    &mut tree,\n"
        "                    x,\n"
        "                    y,\n"
        "                    line,\n"
        "                    self.palette.text,\n"
        "                    textedit::CARET_WIDTH,\n"
        "                );\n",
        "                let _ = (x, y, line);\n",
        [SEARCH],
    ),
    (
        "the search is kept in lower case",
        "        self.filename_search = term.to_owned();\n",
        "        self.filename_search = term.to_lowercase();\n",
        [SEARCH],
    ),
    (
        "a search in capitals finds no name in small letters",
        "                .contains(&self.filename_search.to_lowercase())\n",
        "                .contains(&self.filename_search)\n",
        [SEARCH],
    ),
]

if __name__ == "__main__":
    only = sys.argv[1:] or None
    raise SystemExit(sweep(SRC, MUTATIONS, "undelete", timeout=900, only=only))
