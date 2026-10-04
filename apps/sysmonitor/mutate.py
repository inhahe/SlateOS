"""Mutation test for the system monitor's reading of the machine's totals.

Breaks one piece of production code at a time and checks that the test which
claims to cover it is the one that fails.  A test that passes against a broken
program is not testing the program.

The table covers what changed on 2026-09-27: `read_system`'s doc said a figure
`/proc/meminfo` does not carry keeps the value last read, and the code zeroed
it -- every field went through `unwrap_or(0)`, so a figure missing from one
read was drawn as none of that memory at all.

Run it with no arguments to sweep everything, or with substrings of the
mutation names to run only those.
"""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "scripts"))

from mutation_harness import sweep  # noqa: E402  (path set above)

SRC = Path(__file__).parent / "src" / "main.rs"

READ = "the_system_figures_are_read_and_not_invented"
KEEP = "a_figure_the_file_stops_carrying_keeps_its_last_value"

MUTATIONS = [
    (
        "a figure the file does not carry is zeroed",
        "    if let Some(kib) = kib {\n        *field = kib.saturating_mul(1024);\n    }",
        "    *field = kib.unwrap_or(0).saturating_mul(1024);",
        [KEEP],
    ),
    (
        "a figure in KiB is taken for bytes",
        "        *field = kib.saturating_mul(1024);",
        "        *field = kib;",
        [READ, KEEP],
    ),
    (
        "the cache is read from the free figure",
        "            set_kib(&mut info.cached_memory, mem.cached_kib);",
        "            set_kib(&mut info.cached_memory, mem.free_kib);",
        [READ, KEEP],
    ),
    (
        "swap in use is not read",
        "            set_kib(&mut info.swap_used, mem.swap_used_kib());\n",
        "",
        [READ, KEEP],
    ),
    (
        'AltGr is taken for Ctrl in the monitor',
        '        if textline::is_ctrl_chord(key.modifiers) {\n            return match key.key {',
        '        if key.modifiers.ctrl {\n            return match key.key {',
        ['a_chord_is_neither_a_monitor_key_nor_typing'],
    ),
    (
        "a chord works the monitor's keys",
        '        if !textline::is_plain(key.modifiers) {\n            return EventResult::Ignored;\n        }\n',
        '',
        ['a_chord_is_neither_a_monitor_key_nor_typing'],
    ),
    (
        'a chorded Escape or Enter leaves the filter',
        '        if matches!(key.key, Key::Escape | Key::Enter) && textline::is_plain(key.modifiers) {\n',
        '        if matches!(key.key, Key::Escape | Key::Enter) {\n',
        ['a_chord_is_neither_a_monitor_key_nor_typing'],
    ),
    # The filter's other keys are textline's editor's since 2026-10-04, which
    # refuses Alt's and the Windows key's chords and types only what a key
    # typed: the rule is textline's, and its own table covers it. What is this
    # program's is handing the editor the key as it came -- chord and all.
    (
        "the filter's editor is given the key without its chord",
        '            &mut self.filter_editor,\n            key,\n',
        '            &mut self.filter_editor,\n'
        '            &KeyEvent {\n'
        '                modifiers: Modifiers::NONE,\n'
        '                ..key.clone()\n'
        '            },\n',
        ['a_chord_is_neither_a_monitor_key_nor_typing'],
    ),
]

# The filter box is the toolkit's field, edited by textline's editor, and it
# takes any letter (2026-10-04; lane C, c-e-a-theme-can-shape-the-controls).
FIELD = "the_filter_box_is_the_toolkits_field"
EDITS = "the_filter_edits_like_a_field_and_takes_any_letter"
CARET = "the_filter_caret_follows_the_glyphs"

MUTATIONS += [
    (
        "the filter box never has the keyboard's mark",
        "            focused: self.filter_focused && !self.show_help,\n",
        "            focused: false,\n",
        [FIELD],
    ),
    (
        "the filter box keeps its mark under the list of keys",
        "            focused: self.filter_focused && !self.show_help,\n",
        "            focused: self.filter_focused,\n",
        [FIELD],
    ),
    (
        "a filter that matches nothing is not red",
        "            invalid: !self.filter_text.is_empty() && self.visible_indices.is_empty(),\n",
        "            invalid: false,\n",
        [FIELD],
    ),
    (
        "a press on the filter box does nothing",
        "                    self.press_filter(mx);\n",
        "                    let _ = mx;\n",
        [FIELD],
    ),
    (
        "a press elsewhere leaves the filter the keyboard",
        "                self.filter_focused = false;\n\n                // Tab bar click",
        "\n                // Tab bar click",
        [FIELD],
    ),
    (
        "a press leaves the caret where it was",
        "        self.filter_editor.set_cursor(cursor);\n",
        "        let _ = cursor;\n",
        [FIELD],
    ),
    (
        "the filter's caret is at its start",
        "                cursor: self.filter_cursor(),\n",
        "                cursor: TextCursor::default(),\n",
        [CARET],
    ),
    (
        "the filter's editor is not reloaded",
        "        if self.filter_editor.text() != self.filter_text {\n"
        "            self.filter_editor.set_text(&self.filter_text);\n"
        "        }\n"
        "        let edit = textline::apply_key(\n",
        "        let edit = textline::apply_key(\n",
        [EDITS],
    ),
    (
        "the filter holds any length",
        "            FILTER_CAPACITY,\n            &self.filter_clipboard,\n",
        "            usize::MAX,\n            &self.filter_clipboard,\n",
        [EDITS],
    ),
    (
        "a cut takes nothing to the clipboard",
        "            self.filter_clipboard = copied;\n",
        "            let _ = copied;\n",
        [EDITS],
    ),
    (
        "an edit does not filter again",
        "            self.filter_text = self.filter_editor.text().to_owned();\n"
        "            self.rebuild_visible_list();\n",
        "            self.filter_text = self.filter_editor.text().to_owned();\n",
        [FIELD],
    ),
    (
        "Ctrl+F leaves the editor with the last text",
        "        self.filter_focused = true;\n        self.filter_editor.set_text(&self.filter_text);\n",
        "        self.filter_focused = true;\n",
        [EDITS],
    ),
    (
        "the focus mark is the toolkit's width, not the user's",
        "        self.focus_ring_width = settings.focus_ring_width();\n",
        "        let _ = settings;\n",
        [FIELD],
    ),
]

ADVERTISED = "every_advertised_key_does_something"
REACHES = "the_shortcut_list_reaches_the_window"
FILTER_Q = "a_question_mark_is_typed_into_the_filter_and_f1_still_raises_the_list"
MODAL = "the_shortcut_list_takes_the_keys_and_a_press"

MUTATIONS += [
    # The list of keys, which the window did not have (2026-10-04).
    (
        "the list of keys is never drawn",
        "        if self.show_help {\n"
        "            guitk::shortcut::render_card(\n",
        "        if false {\n"
        "            guitk::shortcut::render_card(\n",
        [REACHES],
    ),
    (
        "F1 raises nothing",
        "        if plain && (key.key == Key::F1 || question && !self.filter_focused) {\n",
        "        if plain && question && !self.filter_focused {\n",
        [ADVERTISED, REACHES, FILTER_Q, MODAL],
    ),
    (
        "? raises nothing",
        "        if plain && (key.key == Key::F1 || question && !self.filter_focused) {\n",
        "        if plain && key.key == Key::F1 {\n",
        [ADVERTISED, REACHES],
    ),
    (
        "? is taken from the filter box",
        "        if plain && (key.key == Key::F1 || question && !self.filter_focused) {\n",
        "        if plain && (key.key == Key::F1 || question) {\n",
        [FILTER_Q],
    ),
    (
        "a chord raises the list of keys",
        "        if plain && (key.key == Key::F1 || question && !self.filter_focused) {\n",
        "        if key.key == Key::F1 || question && !self.filter_focused {\n",
        [REACHES],
    ),
    (
        "a key reaches what the list of keys covers",
        "        if self.show_help {\n"
        "            if plain && (matches!(key.key, Key::F1 | Key::Escape) || question) {\n"
        "                self.show_help = false;\n"
        "            }\n"
        "            return EventResult::Consumed;\n"
        "        }\n",
        "",
        [MODAL, REACHES],
    ),
    (
        "a chorded Escape puts the list of keys away",
        "            if plain && (matches!(key.key, Key::F1 | Key::Escape) || question) {\n",
        "            if matches!(key.key, Key::F1 | Key::Escape) || question {\n",
        [REACHES],
    ),
    (
        "a press goes through the list of keys",
        "                MouseEventKind::Press(_) | MouseEventKind::DoubleClick(_) => {\n"
        "                    self.show_help = false;\n"
        "                    return EventResult::Consumed;\n"
        "                }\n",
        "",
        [MODAL],
    ),
    (
        "only the left button puts the list of keys away",
        "                MouseEventKind::Press(_) | MouseEventKind::DoubleClick(_) => {\n",
        "                MouseEventKind::Press(MouseButton::Left) | MouseEventKind::DoubleClick(_) => {\n",
        [MODAL],
    ),
    (
        "the wheel scrolls what the list of keys covers",
        "                MouseEventKind::Scroll { .. } => return EventResult::Ignored,\n",
        "",
        [MODAL],
    ),
]

if __name__ == "__main__":
    only = sys.argv[1:] or None
    raise SystemExit(sweep(SRC, MUTATIONS, "sysmonitor", timeout=600, only=only))
