"""Mutation test for the calculator's list of keys, its CE, and its chords.

Breaks one piece of production code at a time and checks that the test which
claims to cover it is the one that fails.  A test that passes against a broken
program is not testing the program.

The table covers what changed on 2026-10-04: F1 did nothing -- the keyboard's
keys for the keypad (Escape is C, Delete is CE, Enter is `=`, `^` is x^y) were
written nowhere -- and now raises a list of them, modal for the keys and the
pointer; CE cleared everything, as C does, and now clears the number being
typed; and a command (Alt+5, Ctrl+Enter) typed or worked the sum out.

Run it with no arguments to sweep everything, or with substrings of the
mutation names to run only those.
"""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "scripts"))

from mutation_harness import sweep  # noqa: E402  (path set above)

SRC = Path(__file__).parent / "src" / "main.rs"

EVERY = "every_advertised_key_does_something"
REACHES = "the_shortcut_list_reaches_the_window"
MODAL = "the_shortcut_list_takes_the_keys_and_a_press"
SAYS = "the_keys_do_what_the_list_says"
CHORD = "a_command_is_not_typing"

ASKS = (
    "        let asks = plain && (key.key == Key::F1 || key.key == Key::Slash && key.modifiers.shift);\n"
)
CLOSE = "            if asks || plain && key.key == Key::Escape {\n"

MUTATIONS = [
    (
        "the list of keys never comes up",
        "        if asks {\n            self.show_help = true;\n            return true;\n        }\n",
        "",
        [EVERY, REACHES],
    ),
    (
        "? raises nothing",
        ASKS,
        "        let asks = plain && key.key == Key::F1;\n",
        [EVERY, REACHES],
    ),
    (
        "Alt+F1 raises the list",
        ASKS,
        "        let asks = key.key == Key::F1 || plain && key.key == Key::Slash && key.modifiers.shift;\n",
        [REACHES],
    ),
    (
        "the list of keys is not drawn",
        "        if self.show_help {\n            guitk::shortcut::render_card(\n",
        "        if false {\n            guitk::shortcut::render_card(\n",
        [REACHES],
    ),
    (
        "Escape leaves the list up",
        CLOSE,
        "            if asks {\n",
        [REACHES, MODAL],
    ),
    (
        "Alt+Escape puts the list away",
        CLOSE,
        "            if asks || key.key == Key::Escape {\n",
        [REACHES],
    ),
    (
        "the list is not modal for the keys",
        "                self.show_help = false;\n            }\n            return true;\n        }\n",
        "                self.show_help = false;\n            }\n        }\n",
        [MODAL],
    ),
    (
        "a press reaches what the list covers",
        "        Event::Mouse(m) if ui.show_help => ui.help_mouse(&m.kind),\n",
        "        Event::Mouse(m) if ui.show_help && !matches!(m.kind, MouseEventKind::Press(_)) => ui.help_mouse(&m.kind),\n",
        [MODAL],
    ),
    (
        "a press leaves the list up",
        "            MouseEventKind::Press(_) | MouseEventKind::DoubleClick(_) => {\n"
        "                self.show_help = false;\n",
        "            MouseEventKind::Press(_) | MouseEventKind::DoubleClick(_) => {\n",
        [MODAL],
    ),
    (
        "the wheel scrolls what the list covers",
        "        Event::Mouse(m) if ui.show_help => ui.help_mouse(&m.kind),\n",
        "        Event::Mouse(m) if ui.show_help && !matches!(m.kind, MouseEventKind::Scroll { .. }) => ui.help_mouse(&m.kind),\n",
        [MODAL],
    ),
    (
        "CE clears everything, as C does",
        "        // A prefix of the string, so its length is on a character boundary.\n"
        "        let kept = self\n",
        "        // A prefix of the string, so its length is on a character boundary.\n"
        "        let kept = 0;\n"
        "        let _ = self\n",
        [SAYS],
    ),
    (
        "CE clears nothing",
        "        self.expression.truncate(kept);\n",
        "        let _ = kept;\n",
        [SAYS],
    ),
    (
        "CE after = leaves the answer",
        "    pub fn clear_entry(&mut self) {\n"
        "        if self.showing_result {\n"
        "            self.clear_all();\n"
        "            return;\n"
        "        }\n",
        "    pub fn clear_entry(&mut self) {\n",
        [SAYS],
    ),
    (
        "C leaves a bracket open",
        "        self.showing_result = false;\n        self.paren_depth = 0;\n    }\n",
        "        self.showing_result = false;\n    }\n",
        [SAYS],
    ),
    (
        "a command types",
        "    if textline::is_command(key.modifiers) {\n        return false;\n    }\n",
        "",
        [CHORD],
    ),
    (
        "AltGr is taken for a command",
        "    if textline::is_command(key.modifiers) {\n",
        "    if key.modifiers.ctrl || key.modifiers.alt || key.modifiers.super_key {\n",
        [CHORD],
    ),
]

if __name__ == "__main__":
    only = sys.argv[1:] or None
    raise SystemExit(sweep(SRC, MUTATIONS, "calculator", timeout=600, only=only))
