"""Mutation test for the pomodoro timer's settings.

Breaks one piece of production code at a time and checks that the test which
claims to cover it is the one that fails.  A test that passes against a broken
program is not testing the program.

Covers what changed on 2026-09-27: "Notification Sound: On" was a row the
arrows flipped and nothing else read -- a session ends in silence either way,
because nothing here plays a sound.  The settings say so beneath their rows
now, and the rows leave room for it at any height they fit in.

Run it with no arguments to sweep everything, or with substrings of the
mutation names to run only those.
"""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "scripts"))

from mutation_harness import sweep  # noqa: E402  (path set above)

SRC = Path(__file__).parent / "src" / "main.rs"

SOUND = "the_sound_row_says_nothing_plays_it"

MUTATIONS = [
    (
        "the note is not drawn",
        "        if below + 12.0 <= layout.content.bottom() {",
        "        if false {",
        [SOUND],
    ),
    (
        "the rows leave no room for the note",
        "        ((self.content.h - 80.0) / rows).clamp(18.0, 34.0)",
        "        ((self.content.h - 60.0) / rows).clamp(18.0, 34.0)",
        [SOUND],
    ),
    (
        "the task name refuses what AltGr types",
        "                if !textline::types_into_field(key) {",
        "                if !textline::types_into_field(key) || key.modifiers.ctrl {",
        ["the_task_name_takes_altgr_letters_and_no_commands_letter"],
    ),
    (
        "the task name types a command's letter",
        "                if !textline::types_into_field(key) {",
        "                if !key.types_text() {",
        ["the_task_name_takes_altgr_letters_and_no_commands_letter"],
    ),
    (
        "a key held with Ctrl, Alt or the Windows key is a bare key",
        "        if key.modifiers.ctrl || key.modifiers.alt || key.modifiers.super_key {",
        "        if false {",
        ["a_key_held_with_ctrl_alt_or_the_windows_key_is_no_bare_key"],
    ),
    (
        "a key held with Ctrl is a bare key",
        "        if key.modifiers.ctrl || key.modifiers.alt || key.modifiers.super_key {",
        "        if key.modifiers.alt || key.modifiers.super_key {",
        ["a_key_held_with_ctrl_alt_or_the_windows_key_is_no_bare_key"],
    ),
    (
        "a key held with Alt is a bare key",
        "        if key.modifiers.ctrl || key.modifiers.alt || key.modifiers.super_key {",
        "        if key.modifiers.ctrl || key.modifiers.super_key {",
        ["a_key_held_with_ctrl_alt_or_the_windows_key_is_no_bare_key"],
    ),
    (
        "a key held with the Windows key is a bare key",
        "        if key.modifiers.ctrl || key.modifiers.alt || key.modifiers.super_key {",
        "        if key.modifiers.ctrl || key.modifiers.alt {",
        ["a_key_held_with_ctrl_alt_or_the_windows_key_is_no_bare_key"],
    ),
    (
        "Ctrl+Home and Ctrl+End are not the log's",
        "            && let Some(movement) = ListKey::of(key)",
        "            && !key.modifiers.ctrl\n            && let Some(movement) = ListKey::of(key)",
        ["ctrl_home_and_ctrl_end_are_the_ends_of_the_log"],
    ),
    (
        "End does not reach the bottom of the log",
        "                ListKey::Last => self.log_scroll = self.max_log_scroll(),",
        "                ListKey::Last => {}",
        ["ctrl_home_and_ctrl_end_are_the_ends_of_the_log", "the_page_keys_move_a_windowful"],
    ),
]

if __name__ == "__main__":
    only = sys.argv[1:] or None
    raise SystemExit(sweep(SRC, MUTATIONS, "pomodoro", timeout=900, only=only))
