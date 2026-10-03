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
    (
        "a command's letter is typed into the annotation",
        '                    if textline::types_into_field(event) {',
        '                    if event.types_text() {',
        ['a_chord_is_not_a_capture_key_and_altgr_is_not_ctrl'],
    ),
    (
        'Alt+Backspace deletes from the annotation',
        '                        && !textline::is_alt_or_windows_chord(event.modifiers)\n',
        '',
        ['a_chord_is_not_a_capture_key_and_altgr_is_not_ctrl'],
    ),
]

if __name__ == "__main__":
    only = sys.argv[1:] or None
    raise SystemExit(sweep(SRC, MUTATIONS, "screenshot", timeout=600, only=only))
