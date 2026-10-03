"""Mutation test for applying one program to a whole group of file types.

Breaks one piece of production code at a time and checks that the test which
claims to cover it is the one that fails.  A test that passes against a broken
program is not testing the program.

Covers the test changed on 2026-09-27 for lane C
(requests/c-e-a-test-that-counts-the-toolkits-audio-types.md): it wrote the
audio group out -- ten types, "5 of 10" -- and now asks the tables the dialog
reads, so a type added to the group changes the numbers and not the verdict.
These rows show it still fails when the group code is broken.

Run it with no arguments to sweep everything, or with substrings of the
mutation names to run only those.
"""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "scripts"))

from mutation_harness import sweep  # noqa: E402  (path set above)

SRC = Path(__file__).parent / "src" / "main.rs"

GROUP = "applying_to_a_group_sets_what_it_can_and_names_what_it_cannot"

MUTATIONS = [
    (
        "the group stops at the first type it cannot take",
        "                Err(e) => outcome.skipped.push((extension.to_string(), e.to_string())),",
        "                Err(_) => break,",
        [GROUP],
    ),
    (
        "what was set is not counted",
        "                Ok(()) => outcome.set.push(extension.to_string()),",
        "                Ok(()) => {}",
        [GROUP],
    ),
    (
        "the total is what was set, not the group",
        "                        let total = category.extensions().count();\n                        self.status = if group.skipped.is_empty() {",
        "                        let total = group.set.len();\n                        self.status = if group.skipped.is_empty() {",
        [GROUP],
    ),
    (
        "the types skipped are not named",
        '                                missed.join(" ")',
        "                                String::new()",
        [GROUP],
    ),
    (
        'a program is registered for no type',
        '                        .any(|m| m.eq_ignore_ascii_case(&ft.mime_type))',
        '                        .any(|_| false)',
        ['the_programs_open_what_their_entries_say', 'the_image_viewer_opens_every_picture_it_shows', 'a_program_installed_here_is_offered_for_what_its_entry_opens'],
    ),
    (
        'a program is registered for every type',
        '                        .any(|m| m.eq_ignore_ascii_case(&ft.mime_type))',
        '                        .any(|_| true)',
        ['the_programs_open_what_their_entries_say'],
    ),
    (
        'a program that runs in a terminal is registered',
        '            if app.terminal {\n                continue;\n            }\n',
        '',
        ['a_program_installed_here_is_offered_for_what_its_entry_opens'],
    ),
    (
        "no type opens with SlateOS's default",
        '                let id = ::programs::default_for(&ft.mime_type)?;',
        '                let id = ::programs::default_for("")?;',
        ['the_programs_open_what_their_entries_say', 'the_image_viewer_opens_every_picture_it_shows'],
    ),
    (
        'a file naming a program as it was named before is not read',
        '                .or_else(|| self.app_id_for_name(&written))\n',
        '',
        ['test_import_config_valid'],
    ),
    (
        "a name is matched against no program's file name",
        '                    .is_some_and(|file| file == name)',
        '                    .is_some_and(|_| false)',
        ['test_import_config_valid'],
    ),
    (
        'a program is recorded by its id, not what it runs',
        '            self.register_app(AppInfo::new(&app.id, &app.name, &exec_path, &opens, 0));',
        '            self.register_app(AppInfo::new(&app.id, &app.name, &app.id, &opens, 0));',
        ['the_programs_open_what_their_entries_say', 'a_program_installed_here_is_offered_for_what_its_entry_opens'],
    ),
    (
        'a chord raises the keys',
        '        if key.key == Key::F1 && plain {',
        '        if key.key == Key::F1 {',
        ['a_chord_is_neither_a_key_of_the_window_nor_typing'],
    ),
    (
        "a command's letter is typed and AltGr's refused",
        '        if textline::types_into_field(key) {',
        '        if !key.text.is_empty() && !key.modifiers.ctrl && !key.modifiers.alt {',
        ['a_chord_is_neither_a_key_of_the_window_nor_typing'],
    ),
    (
        'AltGr is taken for Ctrl',
        '        if textline::is_ctrl_chord(key.modifiers) {\n            return match key.key {\n                Key::E =>',
        '        if key.modifiers.ctrl {\n            return match key.key {\n                Key::E =>',
        ['a_chord_is_neither_a_key_of_the_window_nor_typing'],
    ),
    (
        'a chord works the window',
        '        if !plain {\n            return EventResult::Ignored;\n        }\n',
        '',
        ['a_chord_is_neither_a_key_of_the_window_nor_typing'],
    ),
    (
        'AltGr+Q closes the window',
        '            && textline::is_ctrl_chord(key.modifiers)\n        {\n            return Response::Exit;',
        '            && key.modifiers.ctrl\n        {\n            return Response::Exit;',
        ['a_chord_is_neither_a_key_of_the_window_nor_typing'],
    ),
]

if __name__ == "__main__":
    only = sys.argv[1:] or None
    raise SystemExit(sweep(SRC, MUTATIONS, "fileassoc", timeout=900, only=only))
