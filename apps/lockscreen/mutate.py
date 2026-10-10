"""Mutation test for the lock screen.

Breaks one piece of production code at a time and checks that the test which
claims to cover it is the one that fails.  A test that passes against a broken
program is not testing the program.

Covers what changed on 2026-09-28: the clock's settings, `lockscreen.yaml`,
are read again when the desktop announces the file changed (C-Q26,
`design-decisions.md` §1418; the announcement is §1434).

Run it with no arguments to sweep everything, or with substrings of the
mutation names to run only those.
"""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "scripts"))

from mutation_harness import sweep  # noqa: E402  (path set above)

SRC = Path(__file__).parent / "src" / "main.rs"

REREAD = "a_changed_settings_file_is_read_again_when_announced"

MUTATIONS = [
    (
        "an announced change is not read",
        "            Event::SettingsChanged { group } if group.file_name() == CONFIG_NAME => {",
        "            Event::SettingsChanged { group } if false && group.file_name() == CONFIG_NAME => {",
        [REREAD],
    ),
    (
        "every program's announcement is read as this screen's",
        "            Event::SettingsChanged { group } if group.file_name() == CONFIG_NAME => {",
        "            Event::SettingsChanged { group } if !group.file_name().is_empty() => {",
        [REREAD],
    ),
    (
        "a deleted file keeps the seconds it had",
        "        self.show_clock_seconds = fresh.show_clock_seconds;",
        "        self.show_clock_seconds |= fresh.show_clock_seconds;",
        [REREAD],
    ),
]

FIELD = "the_password_box_is_the_toolkits_field"
CHORD = "a_chord_is_not_typed_into_the_password_and_altgr_is"
DOTS = "one_dot_per_character_and_the_caret_after_them"
LIGHT = "a_change_in_the_submit_buttons_light_is_a_repaint"

MUTATIONS += [
    # The password box is the toolkit's field; chords stay out of it; a dot
    # per character and a caret; the submit light repaints (2026-10-04;
    # lane C, c-e-a-theme-can-shape-the-controls).
    (
        "the password box never has the keyboard",
        "            focused: self.password_focused,\n",
        "            focused: false,\n",
        [FIELD, DOTS],
    ),
    (
        "the box is not switched off by a lockout",
        "            disabled: self.lockout.is_active(),\n",
        "            disabled: false,\n",
        [FIELD],
    ),
    (
        "a refused password is not red",
        "            invalid: self.show_error,\n",
        "            invalid: false,\n",
        [FIELD],
    ),
    (
        "the focus mark is the toolkit's width, not the user's",
        "        self.focus_ring_width = settings.focus_ring_width();\n",
        "        let _ = settings;\n",
        [FIELD],
    ),
    # No row for "a chord's letter goes into the password": since 2026-10-04
    # the password's keys are textline::apply_masked_key's, which tells a
    # command from AltGr itself, in its own crate and with its own tests;
    # a_chord_is_not_typed_into_the_password_and_altgr_is still holds the box
    # to it.
    (
        "a chord from the clock types its letter",
        "                        if textline::types_into_field(key) {\n",
        "                        if key.types_text() {\n",
        [CHORD],
    ),
    (
        "a dot per byte",
        "            let dot_count = self.password_buffer.chars().count();\n",
        "            let dot_count = self.password_buffer.len();\n",
        [DOTS],
    ),
    (
        "there is no caret",
        "        if state.focused && !state.disabled {\n",
        "        if false {\n",
        [DOTS],
    ),
    (
        "the submit light asks for no repaint",
        "                        if over == self.submit_hovered {\n",
        "                        if true {\n",
        [LIGHT],
    ),
    (
        "the light stays after the pointer leaves",
        "                    MouseEventKind::Leave if self.submit_hovered => {\n",
        "                    MouseEventKind::Leave if false => {\n",
        [LIGHT],
    ),
]

# The password edits at a caret (2026-10-04,
# known-issues/E-twenty-nine-applications-type-only-at-the-end-of-a-box): it
# took typing at its end and Backspace from it, and Delete threw the whole
# password away.
EDITS = "the_password_edits_at_a_caret"
PRESS = "a_press_between_the_dots_puts_the_caret_there"
LOCKOUT = "a_lockout_refuses_the_passwords_keys"
HOLDS = "the_password_edits_what_it_holds"

MUTATIONS += [
    (
        "a lockout lets the password's keys through",
        "        if self.lockout.is_active() {\n            return EventResult::Ignored;\n        }\n        self.load_password();\n",
        "        self.load_password();\n",
        [LOCKOUT],
    ),
    (
        "a key finds the editor holding another password",
        "        if self.password_editor.text() != self.password_buffer {\n"
        "            self.password_editor.set_text(&self.password_buffer);",
        "        if false {\n"
        "            self.password_editor.set_text(&self.password_buffer);",
        [HOLDS],
    ),
    (
        "every key the box answers is a redraw",
        "        if after == before {\n",
        "        if false {\n",
        [PRESS],
    ),
    (
        "the caret is drawn after the last dot",
        "                x + PASSWORD_DOTS_INSET + self.caret_dots().0 as f32 * PASSWORD_DOT_SPACING\n",
        "                x + PASSWORD_DOTS_INSET + self.password_buffer.chars().count() as f32 * PASSWORD_DOT_SPACING\n",
        [EDITS],
    ),
    (
        "the selection is not drawn",
        "            if let (cursor, Some(anchor)) = self.caret_dots()\n",
        "            if let (cursor, Some(anchor)) = (0, None::<usize>)\n",
        [EDITS],
    ),
    (
        "a press on the dots does not place the caret",
        "                            self.press_password(field.x, mouse.x);\n",
        "                            let _ = field;\n",
        [PRESS],
    ),
    (
        "a press is measured from the box's edge, not its first dot",
        "        let offset = x - field_x - PASSWORD_DOTS_INSET + scroll;\n",
        "        let offset = x - field_x + scroll;\n",
        [PRESS],
    ),
    (
        "a press puts the caret at the end",
        "            .unwrap_or(count);\n        self.load_password();\n",
        "            .map(|_| count)\n            .unwrap_or(count);\n        self.load_password();\n",
        [PRESS],
    ),
    (
        "a shaken box is pressed where it stood",
        "            x: self.center_x() - PASSWORD_FIELD_WIDTH / 2.0 + self.shake.offset(),\n",
        "            x: self.center_x() - PASSWORD_FIELD_WIDTH / 2.0,\n",
        ["a_shaken_password_box_is_pressed_where_it_is_drawn"],
    ),
]

if __name__ == "__main__":
    only = sys.argv[1:] or None
    raise SystemExit(sweep(SRC, MUTATIONS, "lockscreen", timeout=600, only=only))
