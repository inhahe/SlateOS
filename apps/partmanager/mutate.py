"""Mutation test for the partition manager.

Breaks one piece of production code at a time and checks that the tests
which claim to cover it are the ones that fail.  A test that passes against
a broken program is not testing the program.

The rows cover the shortcut card's hold on the pointer: a press with the
card up puts it away and reaches nothing under it, and the wheel scrolls
nothing it covers (known-issues
E-a-press-goes-through-the-shortcut-card-to-the-control-drawn-under-it).

Run it with no arguments to sweep everything, or with substrings of the
mutation names to run only those.
"""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "scripts"))

from mutation_harness import sweep  # noqa: E402  (path set above)

SRC = Path(__file__).parent / "src" / "main.rs"

CARD = "the_shortcut_card_takes_a_press_rather_than_passing_it_on"

# (name, old, new, [tests that must fail])
MUTATIONS = [
    (
        "a press goes through the shortcut card",
        "            MouseEventKind::Press(_) | MouseEventKind::DoubleClick(_) => {\n"
        "                app.show_help = false;\n"
        "                return EventResult::Consumed;\n"
        "            }\n",
        "",
        [CARD],
    ),
    (
        "only the left button puts the card away",
        "            MouseEventKind::Press(_) | MouseEventKind::DoubleClick(_) => {\n",
        "            MouseEventKind::Press(MouseButton::Left) | MouseEventKind::DoubleClick(_) => {\n",
        [CARD],
    ),
    (
        "a press under the card leaves it up",
        "                app.show_help = false;\n"
        "                return EventResult::Consumed;\n",
        "                return EventResult::Consumed;\n",
        [CARD],
    ),
    (
        "the wheel scrolls what the card covers",
        "            MouseEventKind::Scroll { .. } => return EventResult::Ignored,\n",
        "",
        [CARD],
    ),
]

# A key bound to itself is taken plain, a Ctrl chord is not AltGr, and the
# label box is the toolkit's field (2026-10-04; lane C,
# c-e-a-theme-can-shape-the-controls).
CHORDS = "a_chord_is_none_of_the_partition_managers_keys"
LABEL = "the_label_box_is_the_toolkits_field_and_takes_no_command"

MUTATIONS += [
    (
        "a chord raises the list of keys",
        "    if key_ev.key == Key::F1 && plain {\n",
        "    if key_ev.key == Key::F1 {\n",
        [CHORDS],
    ),
    (
        "Alt+Delete queues a removal",
        "        Key::Delete if plain => {\n",
        "        Key::Delete => {\n",
        [CHORDS],
    ),
    (
        "AltGr+N opens New Partition",
        "        Key::N if ctrl => {\n",
        "        Key::N if key_ev.modifiers.ctrl => {\n",
        [CHORDS],
    ),
    (
        "AltGr+Enter applies the queue",
        "        Key::Enter if ctrl && app.has_pending_operations() => {\n",
        "        Key::Enter if key_ev.modifiers.ctrl && app.has_pending_operations() => {\n",
        [CHORDS],
    ),
    (
        "AltGr+Z undoes",
        "        Key::Z if ctrl => {\n",
        "        Key::Z if key_ev.modifiers.ctrl => {\n",
        [CHORDS],
    ),
    (
        "a command types its letter into the label",
        "            _ if textline::types_into_field(key_ev) => {\n",
        "            _ if key_ev.types_text() => {\n",
        [LABEL],
    ),
    (
        "the label box never has the keyboard",
        "        focused: matches!(app.dialog, ActiveDialog::CreatePartition(_)) && !app.show_help,\n",
        "        focused: false,\n",
        [LABEL],
    ),
    (
        "the label box keeps its mark under the list of keys",
        "        focused: matches!(app.dialog, ActiveDialog::CreatePartition(_)) && !app.show_help,\n",
        "        focused: matches!(app.dialog, ActiveDialog::CreatePartition(_)),\n",
        [LABEL],
    ),
    (
        "the caret is at the start of the label",
        "                cursor: text::TextCursor::from(dialog.label.len()),\n",
        "                cursor: text::TextCursor::from(0),\n",
        [LABEL],
    ),
    (
        "the focus mark is the toolkit's width, not the user's",
        "        self.focus_ring_width = settings.focus_ring_width();\n",
        "        let _ = settings;\n",
        [LABEL],
    ),
]

if __name__ == "__main__":
    only = sys.argv[1:] or None
    raise SystemExit(sweep(SRC, MUTATIONS, "partmanager", timeout=900, only=only))
