"""Mutation test for the launcher: the installed programs it reads.

Breaks one piece of production code at a time and checks that the test which
claims to cover it is the one that fails.  A test that passes against a broken
program is not testing the program.

The launcher kept its own list of what might be installed, so a program
installed later could not be launched from it at all.  It reads the installed
programs' desktop entries now, as the start menu does (C-Q17,
design-decisions §1423): an entry takes its program's row in place, the rows
that open a program's pages stay, a terminal program starts inside the
terminal, and an entry's working directory is where its program starts.

Run it with no arguments to sweep everything, or with substrings of the
mutation names to run only those.
"""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "scripts"))

from mutation_harness import sweep  # noqa: E402  (path set above)

SRC = Path(__file__).parent / "src" / "main.rs"

OFFERED = "an_installed_program_is_offered_and_started_by_its_entry"
IN_PLACE = "an_installed_entry_takes_its_programs_row_in_place"
PAGES = "the_rows_that_open_a_programs_pages_stay_when_it_is_installed"
HIDDEN = "what_a_menu_would_not_show_is_not_offered"
TERMINAL = "a_terminal_program_is_started_in_the_terminal"
WORKDIR = "an_entrys_working_directory_is_where_it_starts"
STARTED_IN = "a_program_is_started_in_its_entrys_directory"
BY_NAME = "the_programs_its_list_does_not_name_follow_it_by_name"

# (name, old, new, [tests that must fail])
MUTATIONS = [
    (
        "the launcher does not read the installed programs",
        "        let installed = installed_programs(dirs, locale, path_var);",
        "        let installed = Vec::new();",
        [OFFERED, IN_PLACE],
    ),
    (
        "an entry's arguments are dropped",
        "            (program, line.collect())",
        "            (program, Vec::new())",
        [OFFERED],
    ),
    (
        "a comment is not the second line",
        "            description: app\n                .comment\n                .clone()",
        "            description: app\n                .generic_name\n                .clone()",
        [OFFERED],
    ),
    (
        "an entry's keywords are not searched",
        "            keywords: app.keywords.clone(),",
        "            keywords: Vec::new(),",
        [OFFERED],
    ),
    (
        "a settings program is an application",
        "                Folder::Settings => Category::Setting,",
        "                Folder::Settings => Category::Application,",
        [PAGES],
    ),
    (
        "a system program is an application",
        "                Folder::System => Category::System,",
        "                Folder::System => Category::Application,",
        [TERMINAL],
    ),
    (
        "a terminal program is started without the terminal",
        "        let (executable_path, args) = if app.terminal {",
        "        let (executable_path, args) = if false {",
        [TERMINAL],
    ),
    (
        "a terminal program takes the terminal's row",
        "        if self.executable_path == TERMINAL",
        "        if false && self.executable_path == TERMINAL",
        [TERMINAL],
    ),
    (
        "an entry's working directory is dropped",
        "            dir: app.path.as_ref().map(PathBuf::from),",
        "            dir: None,",
        [WORKDIR],
    ),
    (
        "a program is started wherever the launcher is",
        "        start.current_dir(dir);",
        "        let _ = dir;",
        [STARTED_IN],
    ),
    (
        "what a menu hides is offered",
        "        .filter(|app| desktopentry::menu::shows_in_menu(app, &[desktopentry::menu::DESKTOP_NAME]))\n",
        "",
        [HIDDEN],
    ),
    (
        "TryExec is not checked",
        "                .is_none_or(|program| desktopentry::scan::program_exists(program, path_var))",
        "                .is_none_or(|_| true)",
        [HIDDEN],
    ),
    (
        "TryExec is not looked for on PATH",
        "                .is_none_or(|program| desktopentry::scan::program_exists(program, path_var))",
        "                .is_none_or(|program| desktopentry::scan::program_exists(program, None))",
        [HIDDEN],
    ),
    (
        "an installed entry is listed beside its program's row",
        "            Some(at) => list.push(installed.remove(at)),",
        "            Some(_) => list.push(own),",
        [IN_PLACE],
    ),
    (
        "a program's pages are replaced and its own row kept",
        "        let installed_as = if own.args.is_empty() {",
        "        let installed_as = if !own.args.is_empty() {",
        [PAGES],
    ),
    (
        "the programs are listed in the order they were found",
        "        .cmp(&b.name.to_lowercase())",
        "        .cmp(&a.name.to_lowercase())",
        [BY_NAME],
    ),
    (
        "the programs are sorted with regard to case",
        "    a.name\n        .to_lowercase()\n        .cmp(&b.name.to_lowercase())",
        "    a.name\n        .clone()\n        .cmp(&b.name.clone())",
        [BY_NAME],
    ),
    (
        "the launcher offers none of the one list's programs",
        '    let mut list = built_in_programs();\n    list.extend(commands());',
        '    let mut list = Vec::new();\n    list.extend(commands());',
        ['the_launcher_offers_the_one_list_of_programs'],
    ),
    (
        'the launcher offers its commands and nothing else of its own',
        '    let mut list = built_in_programs();\n    list.extend(commands());',
        '    let mut list = built_in_programs();',
        ['the_launcher_offers_the_one_list_of_programs'],
    ),
    (
        "the built-in programs are listed in the library's order, not by name",
        '    list.sort_by(by_name);\n    list\n}',
        '    list.sort_by(by_name);\n    list.reverse();\n    list\n}',
        ['the_launcher_offers_the_one_list_of_programs'],
    ),
    (
        'AltGr is taken for Ctrl',
        '        if textline::is_ctrl_chord(event.modifiers) {',
        '        if event.modifiers.ctrl {',
        ['a_chord_is_neither_a_launcher_key_nor_typing'],
    ),
    (
        "a command's letter is typed into the query",
        '        if textline::types_into_field(event) {',
        '        if event.types_text() {',
        ['a_chord_is_neither_a_launcher_key_nor_typing'],
    ),
    (
        "a chord works the launcher's own keys",
        '        if !textline::is_plain(event.modifiers) {\n            return LauncherAction::None;\n        }\n',
        '',
        ['a_chord_is_neither_a_launcher_key_nor_typing'],
    ),
]

FIELD = "the_search_box_is_the_toolkits_field"

MUTATIONS += [
    # The search box is the toolkit's field (2026-10-04; lane C,
    # c-e-a-theme-can-shape-the-controls).
    (
        "the search box never has the keyboard",
        "            focused: true,\n            disabled: false,\n            invalid: !self.query.trim().is_empty() && self.results.is_empty(),\n",
        "            focused: false,\n            disabled: false,\n            invalid: !self.query.trim().is_empty() && self.results.is_empty(),\n",
        [FIELD, "the_caret_sits_where_the_query_text_ends"],
    ),
    (
        "a query that finds nothing is not red",
        "            invalid: !self.query.trim().is_empty() && self.results.is_empty(),\n",
        "            invalid: false,\n",
        [FIELD],
    ),
    (
        "the focus mark is the toolkit's width, not the user's",
        "        self.focus_ring_width = settings.focus_ring_width();\n",
        "        let _ = settings;\n",
        [FIELD],
    ),
    (
        "the caret is the toolkit's width, not the user's",
        "                caret_width: self.caret_width,\n",
        "                caret_width: textedit::CARET_WIDTH,\n",
        ["a_wider_caret_setting_draws_a_wider_caret"],
    ),
    (
        "the caret is at the start of the query",
        "        let cursor = if self.query.is_char_boundary(self.cursor) {\n            self.cursor\n",
        "        let cursor = if self.query.is_char_boundary(self.cursor) {\n            0\n",
        ["the_caret_sits_where_the_query_text_ends"],
    ),
]

# The list of keys (2026-10-04): F1 did nothing, and Ctrl+1 to 8 and Tab
# could be found only by pressing them.
EVERY = "every_advertised_key_does_something"
REACHES = "the_shortcut_list_reaches_the_window"
MODAL = "the_shortcut_list_takes_the_keys_and_a_press"

MUTATIONS += [
    (
        "the list of keys never comes up",
        "            self.show_help = true;\n            return LauncherAction::None;\n",
        "            return LauncherAction::None;\n",
        [EVERY, REACHES],
    ),
    (
        "Alt+F1 raises the list",
        "        if plain && event.key == Key::F1 {\n",
        "        if event.key == Key::F1 {\n",
        [REACHES],
    ),
    (
        "the list is not modal for the keys",
        "                self.show_help = false;\n"
        "            }\n"
        "            return LauncherAction::None;\n"
        "        }\n"
        "        if plain && event.key == Key::F1 {\n",
        "                self.show_help = false;\n"
        "            }\n"
        "        }\n"
        "        if plain && event.key == Key::F1 {\n",
        [MODAL],
    ),
    (
        "Escape leaves the list up",
        "            if plain && matches!(event.key, Key::F1 | Key::Escape) {\n",
        "            if plain && matches!(event.key, Key::F1) {\n",
        [REACHES],
    ),
    (
        "Alt+Escape puts the list away",
        "            if plain && matches!(event.key, Key::F1 | Key::Escape) {\n",
        "            if matches!(event.key, Key::F1 | Key::Escape) {\n",
        [REACHES],
    ),
    (
        "the list of keys is not drawn",
        "        if self.show_help {\n            guitk::shortcut::render_card(\n",
        "        if false {\n            guitk::shortcut::render_card(\n",
        [REACHES],
    ),
    (
        "the list is not modal for the pointer",
        "                self.show_help = false;\n"
        "            }\n"
        "            return LauncherAction::None;\n"
        "        }\n"
        "        let layout = Layout::of(self);\n",
        "                self.show_help = false;\n"
        "            }\n"
        "        }\n"
        "        let layout = Layout::of(self);\n",
        [MODAL],
    ),
    (
        "a press leaves the list up",
        "            ) {\n                self.show_help = false;\n            }\n",
        "            ) {\n            }\n",
        [MODAL],
    ),
    (
        "opening and closing the list draws nothing new",
        "            help: self.show_help,\n",
        "            help: false,\n",
        [EVERY],
    ),
]

if __name__ == "__main__":
    only = sys.argv[1:] or None
    raise SystemExit(sweep(SRC, MUTATIONS, "launcher", timeout=900, only=only))
