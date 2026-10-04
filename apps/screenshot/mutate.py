"""Mutation test for the screenshot tool's save path.

Breaks one piece of production code at a time and checks that the test which
claims to cover it is the one that fails.  A test that passes against a broken
program is not testing the program.

This table covers only how a capture is written to disk -- the part that
changed on 2026-09-26, when a new capture stopped choosing a free name and
then writing it, and began claiming the name in the same step as it writes
it (`safeio::write_new_atomically`).  The old way left a moment between the
look and the write in which another program's file could arrive under the
name and be replaced; and once ten thousand names were taken it wrote over the
first on purpose.

One mutation is deliberately absent: making the real look
(`std::fs::symlink_metadata(name).is_ok()`) see nothing.  The claim still
refuses every taken name, so nothing a test can observe changes -- only the
cost, since each refused claim first writes the whole picture to a temporary.
`a_name_seen_taken_is_passed_over_unwritten` pins the look's contract instead:
a name it reports taken is never written.
"""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "scripts"))

from mutation_harness import sweep  # noqa: E402  (path set above)

SRC = Path(__file__).parent / "src" / "main.rs"

RACE = "a_name_taken_after_the_look_is_not_written_over"
FULL = "every_name_taken_fails_rather_than_replace_one"
AT_ONCE = "an_unwritable_folder_fails_at_once_with_its_reason"
SEEN = "a_name_seen_taken_is_passed_over_unwritten"
NAMES = "a_new_capture_tries_its_name_then_numbered_ones"
EXT = "a_disambiguated_name_keeps_its_extension"
RESAVE = "re_saving_one_capture_rewrites_its_own_file"
TWO = "a_second_capture_does_not_overwrite_the_first_ones_file"

# (name, old, new, [tests that must fail])
MUTATIONS = [
    (
        "a new capture replaces a name taken since the look",
        "            match safeio::write_new_atomically(&name, data) {",
        "            match safeio::write_atomically(&name, data) {",
        [RACE, FULL],
    ),
    (
        "a name taken since the look ends the save",
        "                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}",
        "                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => return Err(e),",
        [RACE],
    ),
    (
        "any failure goes on to the next name",
        "                Err(e) => return Err(e),\n            }\n        }\n        last = Some(name);",
        "                Err(_) => {}\n            }\n        }\n        last = Some(name);",
        [AT_ONCE],
    ),
    (
        "a name seen taken is written to find out",
        "        if !taken(&name) {",
        "        if true {",
        [SEEN],
    ),
    (
        "the look is read backwards",
        "        if !taken(&name) {",
        "        if taken(&name) {",
        [RACE, SEEN, AT_ONCE],
    ),
    (
        "the refusal names the first name, not the last",
        "        last = Some(name);",
        "        last = last.or(Some(name));",
        [FULL],
    ),
    (
        "the refusal does not say which names",
        '        Some(last) => format!("every name up to {} is in use", last.shown()),',
        '        Some(_) => String::from("every name is in use"),',
        [FULL],
    ),
    (
        "every name taken is reported as some other failure",
        "    Err(std::io::Error::new(std::io::ErrorKind::AlreadyExists, why))",
        "    Err(std::io::Error::other(why))",
        [FULL],
    ),
    (
        "the numbered names start at three",
        "        .chain((2..SAVE_NAME_TRIES).map(",
        "        .chain((3..SAVE_NAME_TRIES).map(",
        [NAMES, EXT],
    ),
    (
        "one name more is tried",
        "const SAVE_NAME_TRIES: u32 = 10_000;",
        "const SAVE_NAME_TRIES: u32 = 10_001;",
        [NAMES],
    ),
    (
        "the number goes after the extension",
        "        Some((stem, ext)) if !stem.is_empty() => (stem.to_string(), format!(\".{ext}\")),",
        "        Some((_, _)) if !filename.is_empty() => (filename.to_string(), String::new()),",
        [NAMES, EXT],
    ),
    (
        "a name that starts with its dot loses it",
        "        Some((stem, ext)) if !stem.is_empty() => (stem.to_string(), format!(\".{ext}\")),",
        "        Some((stem, ext)) => (stem.to_string(), format!(\".{ext}\")),",
        [NAMES],
    ),
    (
        "a new capture is written over its own name",
        "    Ok(write_new_file(dir, filename, &data)?)",
        "    let path = dir.join(filename);\n    safeio::write_atomically(&path, &data)?;\n    Ok(path)",
        [TWO],
    ),
    (
        "saving again makes a new file",
        "            Some(existing) => {\n                write_png(existing, capture.width, capture.height, &pixels)?;\n                existing.clone()\n            }",
        "            Some(_) => write_new_png(\n                &self.settings.save_directory,\n                &capture.default_filename(),\n                capture.width,\n                capture.height,\n                &pixels,\n            )?,",
        [RESAVE],
    ),
    (
        "the name a new capture took is not remembered",
        "            if let Ok(path) = &outcome {\n                self.current_saved_path = Some(path.clone());\n            }",
        "",
        [TWO],
    ),
]

MUTATIONS += [
    # -- PNG, the folder, the clock (2026-09-26) ------------------------------
    (
        "a new capture's folder is not made",
        "    std::fs::create_dir_all(dir)?;\n",
        "",
        ["a_new_capture_makes_its_folder"],
    ),
    (
        "the default folder is a literal ~",
        "            save_directory: default_save_directory(),",
        "            save_directory: PathBuf::from(\"~/Pictures/Screenshots/\"),",
        ["the_default_folder_is_under_home"],
    ),
    (
        "every capture is stamped with one fixed date",
        "            timestamp: now_utc(),",
        "            timestamp: (2026, 1, 1, 0, 0, 0),",
        ["a_new_capture_is_stamped_with_the_clock"],
    ),
    (
        "the clock's hours are its minutes",
        "        part(into_day / 3_600),",
        "        part(into_day % 3_600 / 60),",
        ["a_new_capture_is_stamped_with_the_clock"],
    ),
]

# `--save`: Ctrl+Print Screen asks where the capture goes
# (requests/c-e-print-screen-can-save-to-a-file.md).
ASK = "the_save_word_asks_where_the_capture_goes"
MUTATIONS += [
    (
        "--save is read and ignored",
        '            Some(word) if word == "--save" => true,',
        '            Some(word) if word == "--save" => false,',
        [ASK],
    ),
    (
        "a capture asked about takes the default action",
        "        if std::mem::take(&mut self.ask_where) {",
        "        if false {",
        [ASK],
    ),
    (
        "every capture after asks too",
        "        if std::mem::take(&mut self.ask_where) {",
        "        if self.ask_where {",
        [ASK],
    ),
    (
        "the place chosen is not written",
        "            Picked::Chose(path) => {\n                self.save_where_chosen(&path);",
        "            Picked::Chose(path) => {\n                let _ = path;",
        [ASK],
    ),
    (
        "the Save dialog is not drawn",
        "        tree.commands.extend(self.picker.render(",
        "        drop(self.picker.render(",
        [ASK],
    ),
    (
        "a cancelled capture is kept",
        "        if self.awaiting.take().is_some() {",
        "        if self.awaiting.is_some() {",
        ["cancelling_the_save_dialog_discards_the_capture"],
    ),
    (
        "a third word is taken",
        "        if let Some(extra) = words.next() {\n            return Err(format!(\n"
        "                \"{} is one argument more",
        "        if let Some(extra) = None::<&OsString> {\n            return Err(format!(\n"
        "                \"{} is one argument more",
        ["the_save_word_is_taken_only_after_a_mode"],
    ),
    (
        "a mistyped mode opens the menu without a word",
        "            _ => {\n                return Err(format!(\n"
        "                    \"{} is not a capture mode;",
        "            _ => {\n                return Ok(());\n                #[allow(unreachable_code)]\n"
        "                return Err(format!(\n                    \"{} is not a capture mode;",
        ["a_mistyped_flag_captures_nothing"],
    ),
    (
        'a chord raises the list of keys',
        '        if plain && (event.key == Key::F1 || (event.key == Key::Slash && event.modifiers.shift)) {',
        '        if event.key == Key::F1 || (event.key == Key::Slash && event.modifiers.shift) {',
        ['a_chord_is_not_a_capture_key_and_altgr_is_not_ctrl'],
    ),
    (
        'a chorded Escape puts the list of keys away',
        '            if plain && matches!(event.key, Key::Escape | Key::Enter) {',
        '            if matches!(event.key, Key::Escape | Key::Enter) {',
        ['a_chord_is_not_a_capture_key_and_altgr_is_not_ctrl'],
    ),
    (
        "Windows+PrintScreen is the capture tool's",
        '        if event.key == Key::PrintScreen && event.modifiers.super_key {\n            return false;\n        }\n',
        '',
        ['a_chord_is_not_a_capture_key_and_altgr_is_not_ctrl'],
    ),
    (
        "a chord works the menu's keys",
        '        if event.key != Key::PrintScreen && !textline::is_plain(event.modifiers) {',
        '        if event.key != Key::PrintScreen && event.modifiers.shift && !event.modifiers.shift {',
        ['a_chord_is_not_a_capture_key_and_altgr_is_not_ctrl'],
    ),
    (
        'a chorded Escape cancels the region',
        '        if event.key == Key::Escape && textline::is_plain(event.modifiers) {\n            self.region_selector.cancel();',
        '        if event.key == Key::Escape {\n            self.region_selector.cancel();',
        ['a_chord_is_not_a_capture_key_and_altgr_is_not_ctrl'],
    ),
    (
        'a chorded Escape stops the countdown',
        '        if event.key == Key::Escape && textline::is_plain(event.modifiers) {\n            self.countdown_remaining = 0;',
        '        if event.key == Key::Escape {\n            self.countdown_remaining = 0;',
        ['a_chord_is_not_a_capture_key_and_altgr_is_not_ctrl'],
    ),
    (
        'AltGr is taken for Ctrl on a picture',
        '        if textline::is_ctrl_chord(event.modifiers) {\n            return match event.key {',
        '        if event.modifiers.ctrl {\n            return match event.key {',
        ['a_chord_is_not_a_capture_key_and_altgr_is_not_ctrl'],
    ),
    (
        'a chorded Escape throws the picture away',
        '            Key::Escape if plain => {\n                self.discard_current();',
        '            Key::Escape => {\n                self.discard_current();',
        ['a_chord_is_not_a_capture_key_and_altgr_is_not_ctrl'],
    ),
    (
        'a chord picks an annotation tool',
        '            Key::Num1 if plain => {',
        '            Key::Num1 => {',
        ['a_chord_is_not_a_capture_key_and_altgr_is_not_ctrl'],
    ),
    # The annotation's text is textline's editor's since 2026-10-04, which
    # refuses Alt's and the Windows key's chords and types only what a key
    # typed: the rule is textline's, and its own table covers it. What is this
    # program's is handing the editor the key as it came -- chord and all.
    (
        "the text box's editor is given the key without its chord",
        '            &mut self.annotation_text_editor,\n            event,\n',
        '            &mut self.annotation_text_editor,\n'
        '            &KeyEvent {\n'
        '                modifiers: guitk::event::Modifiers::NONE,\n'
        '                ..event.clone()\n'
        '            },\n',
        ['a_chord_is_not_a_capture_key_and_altgr_is_not_ctrl'],
    ),
]

CARD = "the_shortcut_card_takes_a_press_rather_than_passing_it_on"

MUTATIONS += [
    # -- The list of keys takes the pointer (2026-10-04; known-issues
    # E-a-press-goes-through-the-shortcut-card-to-the-control-drawn-under-it).
    (
        "a press goes through the list of keys",
        "        if self.show_help\n"
        "            && matches!(\n"
        "                event.kind,\n"
        "                MouseEventKind::Press(_) | MouseEventKind::DoubleClick(_)\n"
        "            )\n"
        "        {\n"
        "            self.show_help = false;\n"
        "            return true;\n"
        "        }\n",
        "",
        [CARD],
    ),
    (
        "only the left button puts the list of keys away",
        "                MouseEventKind::Press(_) | MouseEventKind::DoubleClick(_)\n"
        "            )\n"
        "        {\n"
        "            self.show_help = false;\n",
        "                MouseEventKind::Press(MouseButton::Left) | MouseEventKind::DoubleClick(_)\n"
        "            )\n"
        "        {\n"
        "            self.show_help = false;\n",
        [CARD],
    ),
    (
        "a press under the list of keys leaves it up",
        "        {\n"
        "            self.show_help = false;\n"
        "            return true;\n"
        "        }\n"
        "        match self.view {\n",
        "        {\n"
        "            return true;\n"
        "        }\n"
        "        match self.view {\n",
        [CARD],
    ),
]

# The text annotation is typed into the toolkit's field, which has the
# keyboard from choosing the text tool: a digit types, and Escape abandons the
# text, not the picture (2026-10-04; known-issues
# E-the-screenshot-tools-text-annotation-cannot-take-a-digit-and-escape-throws-the-picture-away).
DIGITS = "a_text_annotation_takes_digits_and_escape_abandons_only_the_text"
TEXT_BOX = "the_text_box_is_the_toolkits_field"

MUTATIONS += [
    (
        "a digit chooses a tool while a text is typed",
        "        if self.annotation_text_focused && self.annotation_tool == AnnotationTool::Text {\n",
        "        if false {\n",
        [DIGITS, TEXT_BOX],
    ),
    (
        "Escape throws the picture away from the text",
        "            if event.key == Key::Escape && textline::is_plain(event.modifiers) {\n"
        "                if self.annotation_text_input.is_empty() {\n",
        "            if false {\n"
        "                if self.annotation_text_input.is_empty() {\n",
        [DIGITS],
    ),
    (
        "Escape on an empty box keeps the keyboard",
        "                    self.annotation_text_focused = false;\n"
        "                } else {\n"
        "                    self.annotation_text_input.clear();\n",
        "                } else {\n"
        "                    self.annotation_text_input.clear();\n",
        [DIGITS, TEXT_BOX],
    ),
    (
        "Escape on a text keeps it",
        "                } else {\n"
        "                    self.annotation_text_input.clear();\n"
        "                }\n",
        "                } else {\n"
        "                }\n",
        [DIGITS],
    ),
    (
        "the text tool does not give its box the keyboard",
        "        self.annotation_text_focused = tool == AnnotationTool::Text;\n",
        "        self.annotation_text_focused = false;\n",
        [DIGITS, TEXT_BOX],
    ),
    (
        "the text tool's key chooses no tool",
        "            Key::Num3 if plain => {\n"
        "                self.choose_tool(AnnotationTool::Text);\n",
        "            Key::Num3 if plain => {\n"
        "                self.annotation_tool = AnnotationTool::Text;\n",
        [DIGITS, TEXT_BOX],
    ),
    (
        "the clipboard keys are not the box's",
        "                _ => self.annotation_text_focused && self.edit_annotation_text(event),\n",
        "                _ => false,\n",
        [TEXT_BOX],
    ),
    (
        "a press on the text box draws on the picture",
        "                if let Some(rect) = self.text_box_rect()\n"
        "                    && rect.contains(event.x, event.y)\n"
        "                {\n"
        "                    self.press_text_box(rect, event.x);\n"
        "                    return true;\n"
        "                }\n",
        "",
        [TEXT_BOX],
    ),
    (
        "a press leaves the caret where it was",
        "        self.annotation_text_editor.set_cursor(cursor);\n",
        "        let _ = cursor;\n",
        [TEXT_BOX],
    ),
    (
        "a press on the box does not take the keyboard back",
        "        self.annotation_text_focused = true;\n"
        "        if self.annotation_text_editor.text() != self.annotation_text_input {\n",
        "        if self.annotation_text_editor.text() != self.annotation_text_input {\n",
        [TEXT_BOX],
    ),
    (
        "a cut takes nothing to the clipboard",
        "            self.annotation_text_clipboard = copied;\n",
        "            let _ = copied;\n",
        [TEXT_BOX],
    ),
    (
        "the text box never has the keyboard's mark",
        "                focused: self.annotation_text_focused && !self.show_help && !self.picker.is_open(),\n",
        "                focused: false,\n",
        [TEXT_BOX],
    ),
    (
        "the text box keeps its mark under the list of keys",
        "                focused: self.annotation_text_focused && !self.show_help && !self.picker.is_open(),\n",
        "                focused: self.annotation_text_focused && !self.picker.is_open(),\n",
        [TEXT_BOX],
    ),
    (
        "the text box shows without the text tool",
        "        if self.view != AppView::Preview || self.annotation_tool != AnnotationTool::Text {\n",
        "        if self.view != AppView::Preview {\n",
        [TEXT_BOX],
    ),
    (
        "the text box's caret is at its start",
        "                cursor: if focused {\n                    self.text_box_cursor()\n",
        "                cursor: if false {\n                    self.text_box_cursor()\n",
        [TEXT_BOX],
    ),
    (
        "the focus mark is the toolkit's width, not the user's",
        "        self.focus_ring_width = settings.focus_ring_width();\n",
        "        let _ = settings;\n",
        [TEXT_BOX],
    ),
]

if __name__ == "__main__":
    only = sys.argv[1:] or None
    raise SystemExit(sweep(SRC, MUTATIONS, "screenshot", timeout=600, only=only))
